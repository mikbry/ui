//! Decode support for `components::Image`'s encoded-bytes source (#196).
//!
//! PNG via `png`, WebP via `image-webp` — the two formats issue #196 scopes
//! the "decoded bytes" source path to. Deliberately narrow: no general
//! multi-format decoder (the `image` crate) and no AVIF (every AVIF decoder
//! needs a full AV1 decoder underneath it, a much bigger dependency than
//! either format here).
//!
//! Decoding lives here, once, so every backend agrees on the same
//! `width`/`height` a construction-time decode validated. [`decode_rgba8`]
//! is `pub` (not `pub(crate)`) so `mkui-wgpu`'s texture-upload path can call
//! back into it when it needs real pixels, rather than depending on `png`/
//! `image-webp` itself — see that crate's `Cargo.toml`.

use std::io::Cursor;

use mkui_runtime::ImageFormat;

/// PNG's 8-byte file signature (ISO/IEC 15948 §5.2).
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Decoded, always-RGBA8 (straight alpha), row-major pixel buffer plus the
/// dimensions it was decoded at. `rgba.len() == width as usize * height as
/// usize * 4` always holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Failure decoding an `Image` source's encoded bytes (#196).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImageDecodeError {
    /// The byte buffer is too short, or its magic bytes, match neither
    /// PNG's signature nor a RIFF/WEBP container.
    #[error("not a recognized PNG or WebP byte stream")]
    UnrecognizedFormat,
    #[error("png decode: {0}")]
    Png(#[from] png::DecodingError),
    #[error("webp decode: {0}")]
    WebP(#[from] image_webp::DecodingError),
    /// `png::Transformations::EXPAND | ALPHA | STRIP_16` is expected to
    /// always normalize the decoded frame to one of the four 8-bit color
    /// types this crate's PNG decoder handles; landing anywhere else means
    /// those transform flags did not apply the way their docs describe.
    #[error("png decoder produced an unexpected post-transform format: {0}")]
    UnexpectedPngOutputFormat(String),
    /// [`image_webp::WebPDecoder::output_buffer_size`] returned `None`
    /// (the decoded size would exceed `usize::MAX`).
    #[error("webp image too large to decode")]
    WebPImageTooLarge,
}

/// Sniff `bytes`' magic header to tell PNG from WebP, with no dependency on
/// a caller-supplied hint or file extension — mirrors how every other
/// format-detecting boundary in this codebase works (e.g. `StyleClass`
/// parsing a raw string).
pub fn sniff_format(bytes: &[u8]) -> Result<ImageFormat, ImageDecodeError> {
    if bytes.len() >= 8 && bytes[0..8] == PNG_MAGIC {
        return Ok(ImageFormat::Png);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(ImageFormat::WebP);
    }
    Err(ImageDecodeError::UnrecognizedFormat)
}

/// Decode `bytes` (already known to be `format`-encoded — see
/// [`sniff_format`]) to a normalized RGBA8 buffer.
pub fn decode_rgba8(bytes: &[u8], format: ImageFormat) -> Result<DecodedImage, ImageDecodeError> {
    match format {
        ImageFormat::Png => decode_png(bytes),
        ImageFormat::WebP => decode_webp(bytes),
        // `ImageFormat` is `#[non_exhaustive]` across the crate boundary;
        // `sniff_format` is the only producer of a value passed here and it
        // never yields anything else.
        _ => unreachable!("decode_rgba8: ImageFormat has only Png/WebP variants"),
    }
}

/// Decode PNG bytes to RGBA8, normalizing every source color type (RGB,
/// RGBA, grayscale, grayscale+alpha, palette, any bit depth) to 8-bit RGBA.
/// `png::Transformations::EXPAND | ALPHA | STRIP_16` gets most of the way
/// there (palette → RGB(A), <8-bit grayscale → 8-bit, 16-bit → 8-bit); the
/// match below covers the remaining color types those flags do not touch
/// (grayscale never gains an RGB expansion or an alpha channel from them —
/// see `png`'s own `Transformations` docs).
// `slice::as_chunks` (clippy's newer-only suggestion for the two
// `chunks_exact` calls below) only stabilized in Rust 1.88, past this
// workspace's declared 1.87 MSRV floor — keep `chunks_exact` and silence
// the lint instead of taking the suggestion literally.
#[allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]
fn decode_png(bytes: &[u8]) -> Result<DecodedImage, ImageDecodeError> {
    use png::{BitDepth, ColorType, Transformations};

    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        Transformations::EXPAND | Transformations::ALPHA | Transformations::STRIP_16,
    );
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let out = reader.next_frame(&mut buf)?;
    let src = &buf[..out.buffer_size()];

    let rgba = match (out.color_type, out.bit_depth) {
        (ColorType::Rgba, BitDepth::Eight) => src.to_vec(),
        (ColorType::Rgb, BitDepth::Eight) => src
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        (ColorType::Grayscale, BitDepth::Eight) => {
            src.iter().flat_map(|&g| [g, g, g, 255]).collect()
        }
        (ColorType::GrayscaleAlpha, BitDepth::Eight) => src
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        (color_type, bit_depth) => {
            return Err(ImageDecodeError::UnexpectedPngOutputFormat(format!(
                "{color_type:?}/{bit_depth:?}"
            )));
        }
    };

    Ok(DecodedImage {
        width: out.width,
        height: out.height,
        rgba,
    })
}

/// Decode WebP bytes to RGBA8. `image_webp::WebPDecoder` outputs RGB8 when
/// the source has no alpha channel ([`image_webp::WebPDecoder::has_alpha`])
/// and RGBA8 otherwise; the RGB8 case is expanded with an opaque alpha
/// channel below.
// `slice::as_chunks` (clippy's newer-only suggestion for the `chunks_exact`
// call below) only stabilized in Rust 1.88, past this workspace's declared
// 1.87 MSRV floor — keep `chunks_exact` and silence the lint instead of
// taking the suggestion literally.
#[allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]
fn decode_webp(bytes: &[u8]) -> Result<DecodedImage, ImageDecodeError> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))?;
    let (width, height) = decoder.dimensions();
    let has_alpha = decoder.has_alpha();
    let size = decoder
        .output_buffer_size()
        .ok_or(ImageDecodeError::WebPImageTooLarge)?;
    let mut buf = vec![0u8; size];
    decoder.read_image(&mut buf)?;

    let rgba = if has_alpha {
        buf
    } else {
        buf.chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    };

    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_png(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        bytes
    }

    #[test]
    fn sniff_format_recognizes_png_magic() {
        let bytes = encode_png(1, 1, png::ColorType::Rgba, &[1, 2, 3, 4]);
        assert_eq!(sniff_format(&bytes).unwrap(), ImageFormat::Png);
    }

    #[test]
    fn sniff_format_recognizes_webp_riff_container() {
        // Minimal RIFF/WEBP header is enough for the magic-byte sniff —
        // it never looks past byte 12.
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0]); // chunk size, unchecked here
        bytes.extend_from_slice(b"WEBP");
        assert_eq!(sniff_format(&bytes).unwrap(), ImageFormat::WebP);
    }

    #[test]
    fn sniff_format_rejects_garbage() {
        assert!(matches!(
            sniff_format(b"not an image"),
            Err(ImageDecodeError::UnrecognizedFormat)
        ));
    }

    #[test]
    fn sniff_format_rejects_too_short_buffers() {
        assert!(matches!(
            sniff_format(&[0x89, 0x50]),
            Err(ImageDecodeError::UnrecognizedFormat)
        ));
    }

    #[test]
    fn decode_rgba8_round_trips_an_rgba_png() {
        // 2x2 RGBA8 = 4 pixels * 4 bytes.
        let pixels = [
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ];
        let bytes = encode_png(2, 2, png::ColorType::Rgba, &pixels);
        let decoded = decode_rgba8(&bytes, ImageFormat::Png).unwrap();
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba, pixels);
    }

    #[test]
    fn decode_rgba8_expands_an_rgb_png_to_opaque_rgba() {
        let pixels = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let bytes = encode_png(2, 2, png::ColorType::Rgb, &pixels);
        let decoded = decode_rgba8(&bytes, ImageFormat::Png).unwrap();
        assert_eq!(decoded.rgba.len(), 2 * 2 * 4);
        assert_eq!(&decoded.rgba[0..4], &[10, 20, 30, 255]);
        assert_eq!(&decoded.rgba[4..8], &[40, 50, 60, 255]);
    }

    #[test]
    fn decode_rgba8_expands_grayscale_to_rgba() {
        let pixels = [0u8, 128, 255, 64];
        let bytes = encode_png(2, 2, png::ColorType::Grayscale, &pixels);
        let decoded = decode_rgba8(&bytes, ImageFormat::Png).unwrap();
        assert_eq!(&decoded.rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&decoded.rgba[4..8], &[128, 128, 128, 255]);
    }

    #[test]
    fn decode_rgba8_expands_grayscale_alpha_to_rgba() {
        let pixels = [10u8, 200, 20, 100, 30, 50, 40, 0];
        let bytes = encode_png(2, 2, png::ColorType::GrayscaleAlpha, &pixels);
        let decoded = decode_rgba8(&bytes, ImageFormat::Png).unwrap();
        assert_eq!(&decoded.rgba[0..4], &[10, 10, 10, 200]);
        assert_eq!(&decoded.rgba[4..8], &[20, 20, 20, 100]);
    }

    #[test]
    fn decode_rgba8_rejects_corrupt_png_bytes() {
        let bytes = PNG_MAGIC.to_vec();
        assert!(decode_rgba8(&bytes, ImageFormat::Png).is_err());
    }

    fn encode_webp(
        width: u32,
        height: u32,
        color: image_webp::ColorType,
        pixels: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        image_webp::WebPEncoder::new(&mut bytes)
            .encode(pixels, width, height, color)
            .unwrap();
        bytes
    }

    #[test]
    fn sniff_format_recognizes_a_real_encoded_webp_stream() {
        let bytes = encode_webp(1, 1, image_webp::ColorType::Rgba8, &[1, 2, 3, 4]);
        assert_eq!(sniff_format(&bytes).unwrap(), ImageFormat::WebP);
    }

    #[test]
    fn decode_rgba8_round_trips_an_rgba_webp() {
        let pixels = [
            10, 20, 30, 255, 40, 50, 60, 200, 70, 80, 90, 100, 100, 110, 120, 0,
        ];
        let bytes = encode_webp(2, 2, image_webp::ColorType::Rgba8, &pixels);
        let decoded = decode_rgba8(&bytes, ImageFormat::WebP).unwrap();
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba, pixels);
    }

    #[test]
    fn decode_rgba8_expands_an_rgb_webp_to_opaque_rgba() {
        let pixels = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let bytes = encode_webp(2, 2, image_webp::ColorType::Rgb8, &pixels);
        let decoded = decode_rgba8(&bytes, ImageFormat::WebP).unwrap();
        assert_eq!(decoded.rgba.len(), 2 * 2 * 4);
        assert_eq!(&decoded.rgba[0..4], &[10, 20, 30, 255]);
        assert_eq!(&decoded.rgba[4..8], &[40, 50, 60, 255]);
    }

    #[test]
    fn decode_rgba8_rejects_corrupt_webp_bytes() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(b"WEBP");
        assert!(decode_rgba8(&bytes, ImageFormat::WebP).is_err());
    }
}
