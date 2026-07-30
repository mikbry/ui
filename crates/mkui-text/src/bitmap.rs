//! 5×7 ASCII bitmap implementation of [`TextSystem`].
//!
//! Direct port of the predecessor renderer prototype's `tessellate_text` /
//! `bitmap_glyph` / `wrap_text_lines` / `normalize_bitmap_char` /
//! `measure_line_width` / `max_glyphs_for_width` helpers from an earlier
//! internal 2D HUD tessellation pipeline. The algorithms are unchanged; only
//! the names are domain-neutral and the call shape is the trait surface
//! rather than an inline renderer helper.
//!
//! What changed vs the predecessor source:
//!
//! - Wrapping, alignment, and line positioning moved from the renderer into
//!   [`BitmapTextSystem::layout`] so the renderer never replays the math.
//! - The per-glyph 5×7 lookup moved into [`BitmapTextSystem::rasterize`] and
//!   returns an `Alpha` [`GlyphImage`] scaled nearest-neighbor to the
//!   requested `size_px_q26_6`, instead of being inlined as a triangle
//!   emitter.
//! - Pure read-only data + helper functions only — no `unsafe`, no mutable
//!   global state, trivially `Send + Sync`.

use std::sync::Arc;

use crate::canonical::{Affine2Fixed, VariationSettings};
use crate::system::{
    FontId, FontQuery, GlyphCacheKey, GlyphFormat, GlyphImage, HintingMode, LayoutGlyph, LayoutRun,
    LayoutSpec, ShapedGlyph, ShapedText, TextAlign, TextError, TextRenderClass, TextSystem,
};

/// Family name reported by [`TextSystem::families`] for the bitmap face.
pub const BITMAP_FAMILY: &str = "mkui-bitmap-5x7";

/// Width of a bitmap glyph cell, in source pixels (before scale).
pub const GLYPH_CELL_WIDTH_PX: f32 = 5.0;

/// Height of a bitmap glyph cell, in source pixels (before scale).
pub const GLYPH_CELL_HEIGHT_PX: f32 = 7.0;

/// Horizontal advance between bitmap glyphs, in source pixels (before
/// scale). Slightly wider than the cell so adjacent glyphs do not touch.
pub const GLYPH_ADVANCE_PX: f32 = 5.8;

/// Reference font size at which scale = 1.0 (so a 10px font draws each bit
/// at 1 surface pixel and a 20px font scales 2x).
pub const REFERENCE_FONT_SIZE_PX: f32 = 10.0;

/// 5×7 ASCII bitmap text system. Single hard-coded face; no font registry,
/// no shaping, no hinting — every cache-key dimension other than `glyph_id`
/// and `size_px_q26_6` is ignored on rasterization.
#[derive(Debug, Default, Clone, Copy)]
pub struct BitmapTextSystem;

impl BitmapTextSystem {
    pub const fn new() -> Self {
        Self
    }
}

impl TextSystem for BitmapTextSystem {
    fn resolve_font(&self, _query: &FontQuery) -> Option<FontId> {
        Some(FontId::BITMAP)
    }

    fn register_font_bytes(&self, _bytes: Arc<[u8]>, _index: u32) -> Result<FontId, TextError> {
        Err(TextError::InvalidFontBytes)
    }

    fn layout(&self, text: &str, spec: &LayoutSpec, max_width_px: Option<f32>) -> Vec<LayoutRun> {
        // One-shot path: shape once, then break at the requested width. The
        // engine's cached path calls `prepare` + `wrap` directly so the shaping
        // here happens only once across many widths.
        let shaped = self.prepare(text, spec);
        self.wrap(&shaped, max_width_px)
    }

    fn prepare(&self, text: &str, spec: &LayoutSpec) -> ShapedText {
        // Width-invariant work: per-character normalization, glyph-id mapping,
        // advance, and vertical metrics. None of this depends on wrap width.
        let scale = bitmap_scale(spec.font_size_px);
        let advance = GLYPH_ADVANCE_PX * scale;
        let glyph_height = GLYPH_CELL_HEIGHT_PX * scale;

        let glyphs = text
            .chars()
            .enumerate()
            .map(|(cluster, ch)| {
                let normalized = normalize_bitmap_char(ch);
                ShapedGlyph {
                    glyph_id: normalized as u32,
                    x_advance_px: advance,
                    cluster: cluster as u32,
                    format: GlyphFormat::Alpha,
                    ch: normalized,
                }
            })
            .collect();

        ShapedText {
            text: text.to_string(),
            spec: spec.clone(),
            glyphs,
            line_ascent_px: glyph_height,
            line_descent_px: 0.0,
            glyph_height_px: glyph_height,
        }
    }

    fn wrap(&self, shaped: &ShapedText, max_width_px: Option<f32>) -> Vec<LayoutRun> {
        // Width-dependent work only: line breaking + positioning. The shaped
        // glyphs (and their normalized chars/advances) are reused as-is — no
        // re-shaping happens here.
        let spec = &shaped.spec;
        let scale = bitmap_scale(spec.font_size_px);
        let advance = GLYPH_ADVANCE_PX * scale;
        let glyph_width = GLYPH_CELL_WIDTH_PX * scale;
        let glyph_height = shaped.glyph_height_px;
        let line_height = spec.line_height_px.max(1.0);

        let max_glyphs = match max_width_px {
            Some(width) if width.is_finite() && width > 0.0 => {
                max_glyphs_for_width(width, advance, glyph_width)
            }
            _ => usize::MAX,
        };
        // Reconstruct the (already-normalized) character stream from the
        // prepared glyphs and break it into lines. Normalization and glyph
        // mapping are not repeated.
        let normalized: String = shaped.glyphs.iter().map(|g| g.ch).collect();
        let wrapped_lines = wrap_text_lines(
            &normalized,
            max_glyphs,
            spec.max_lines.unwrap_or(usize::MAX),
        );

        let mut runs = Vec::with_capacity(wrapped_lines.len());
        for (line_index, line) in wrapped_lines.iter().enumerate() {
            let glyph_chars = line.chars().map(normalize_bitmap_char).collect::<Vec<_>>();
            let line_width = measure_line_width(&glyph_chars, advance, glyph_width);
            let origin_x_px = match spec.align {
                TextAlign::Start => 0.0,
                TextAlign::Center => {
                    max_width_px.map_or(0.0, |w| ((w - line_width) * 0.5).max(0.0))
                }
                TextAlign::End => max_width_px.map_or(0.0, |w| (w - line_width).max(0.0)),
            };
            // Centre the 7-row bitmap inside the line box, matching the
            // prototype's vertical positioning.
            let glyph_top_offset = ((line_height - glyph_height).max(0.0) * 0.5).max(0.0);
            let origin_y_px = line_index as f32 * line_height + glyph_top_offset;

            let mut glyphs = Vec::with_capacity(glyph_chars.len());
            let mut pen_x: f32 = 0.0;
            for (cluster, ch) in glyph_chars.iter().enumerate() {
                glyphs.push(LayoutGlyph {
                    glyph_id: *ch as u32,
                    x_px: pen_x,
                    y_px: 0.0,
                    x_advance_px: advance,
                    x_offset_px: 0.0,
                    y_offset_px: 0.0,
                    cluster: cluster as u32,
                    subpixel_variant: 0,
                    format: GlyphFormat::Alpha,
                });
                pen_x += advance;
            }

            runs.push(LayoutRun {
                font_id: spec.font_id,
                render_class: TextRenderClass::Bitmap,
                font_generation: spec.font_generation,
                font_size_px: spec.font_size_px,
                variations: spec.variations.clone(),
                synthesis_flags: spec.synthesis_flags,
                hinting: spec.hinting,
                origin_x_px,
                origin_y_px,
                line_y_baseline_px: origin_y_px + glyph_height,
                line_ascent_px: glyph_height,
                line_descent_px: 0.0,
                glyphs,
            });
        }

        runs
    }

    fn rasterize(&self, key: GlyphCacheKey) -> Result<GlyphImage, TextError> {
        if !matches!(key.format, GlyphFormat::Alpha) {
            return Err(TextError::UnknownGlyph(key.glyph_id));
        }
        let Some(ch) = char::from_u32(key.glyph_id) else {
            return Err(TextError::UnknownGlyph(key.glyph_id));
        };
        let bits = bitmap_glyph(ch);

        let font_size_px = (key.size_px_q26_6 as f32) / 64.0;
        let scale = bitmap_scale(font_size_px);
        let width_px = (GLYPH_CELL_WIDTH_PX * scale).round().max(1.0) as u32;
        let height_px = (GLYPH_CELL_HEIGHT_PX * scale).round().max(1.0) as u32;

        let mut data = vec![0u8; (width_px * height_px) as usize];
        // Nearest-neighbor sample from the 5×7 source. Each output pixel maps
        // to the source bit underneath it: src_x = floor(ox / scale_x),
        // src_y = floor(oy / scale_y).
        let scale_x = width_px as f32 / GLYPH_CELL_WIDTH_PX;
        let scale_y = height_px as f32 / GLYPH_CELL_HEIGHT_PX;
        for oy in 0..height_px {
            let src_y = ((oy as f32 / scale_y).floor() as usize).min(6);
            let row_bits = bits[src_y];
            for ox in 0..width_px {
                let src_x = ((ox as f32 / scale_x).floor() as usize).min(4);
                let mask = 1u8 << (4 - src_x);
                if row_bits & mask != 0 {
                    data[(oy * width_px + ox) as usize] = 255;
                }
            }
        }

        Ok(GlyphImage {
            width_px,
            height_px,
            left_px: 0,
            top_px: 0,
            format: GlyphFormat::Alpha,
            data: Arc::from(data.into_boxed_slice()),
        })
    }

    fn families(&self) -> Vec<String> {
        vec![BITMAP_FAMILY.to_string()]
    }
}

/// Bitmap scale derived from a requested font size. Matches the predecessor
/// prototype's `(font_size_px / 10.0).max(1.0)`, clamped to the nearest
/// **integer** scale (#157 Phase 4, Codex plan step 8 variant B): the bitmap
/// face is a fixed 5×7 pixel grid with no intermediate representation, so a
/// non-integer scale (e.g. a 16px request giving `1.6`) forces the
/// nearest-neighbor upscale in `rasterize` to duplicate source rows/columns
/// unevenly, which is exactly the kind of asymmetric, blurry scaling this
/// mission's Slug work fixes for vector text — restricting the bitmap lane
/// to integer scales keeps every source bit an exact N×N block on screen.
/// `debug_assert!` documents (and catches in debug builds) the invariant
/// this function must uphold: the return value is always a positive integer.
pub fn bitmap_scale(font_size_px: f32) -> f32 {
    let scale = (font_size_px / REFERENCE_FONT_SIZE_PX).max(1.0).round();
    debug_assert!(
        scale >= 1.0 && scale.fract() == 0.0,
        "bitmap_scale must return a positive integer, got {scale} for font_size_px={font_size_px}"
    );
    scale
}

/// How many bitmap glyphs fit within `width` given a per-glyph `advance` and
/// the cell `glyph_width`. Ported verbatim from `max_glyphs_for_width`.
pub fn max_glyphs_for_width(width: f32, advance: f32, glyph_width: f32) -> usize {
    if width <= glyph_width {
        1
    } else {
        (((width - glyph_width) / advance).floor() as usize + 1).max(1)
    }
}

/// Wrap `content` into lines, each no longer than `max_glyphs`, capped at
/// `max_lines` total (ellipsizing the last line when content overflows).
/// Ported from the predecessor prototype's `wrap_text_lines` helper.
pub fn wrap_text_lines(content: &str, max_glyphs: usize, max_lines: usize) -> Vec<String> {
    let mut wrapped = Vec::new();
    let mut truncated = false;

    if max_glyphs == 0 || max_lines == 0 {
        return wrapped;
    }

    for paragraph in content.split('\n') {
        let paragraph_lines = wrap_paragraph(paragraph, max_glyphs);
        if paragraph_lines.is_empty() {
            wrapped.push(String::new());
        } else {
            wrapped.extend(paragraph_lines);
        }
        if wrapped.len() > max_lines {
            truncated = true;
            wrapped.truncate(max_lines);
            break;
        }
    }

    if wrapped.len() > max_lines {
        wrapped.truncate(max_lines);
        truncated = true;
    }

    if truncated && !wrapped.is_empty() {
        let last = wrapped.pop().unwrap_or_default();
        wrapped.push(ellipsize_line(&last, max_glyphs));
    }

    wrapped
}

fn wrap_paragraph(paragraph: &str, max_glyphs: usize) -> Vec<String> {
    let chars = paragraph.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return vec![];
    }

    let mut lines = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        if start >= chars.len() {
            break;
        }

        let remaining = chars.len() - start;
        if remaining <= max_glyphs {
            lines.push(chars[start..].iter().collect::<String>());
            break;
        }

        let end = start + max_glyphs;
        let mut break_at = None;
        for index in (start..end).rev() {
            if chars[index].is_whitespace() {
                break_at = Some(index);
                break;
            }
        }

        match break_at {
            Some(index) if index > start => {
                lines.push(chars[start..index].iter().collect::<String>());
                start = index + 1;
            }
            _ => {
                lines.push(chars[start..end].iter().collect::<String>());
                start = end;
            }
        }
    }

    lines
}

fn ellipsize_line(line: &str, max_glyphs: usize) -> String {
    let mut glyphs = line.trim_end().chars().collect::<Vec<_>>();
    if max_glyphs <= 3 {
        return ".".repeat(max_glyphs);
    }

    if glyphs.len() > max_glyphs - 3 {
        glyphs.truncate(max_glyphs - 3);
    }

    let mut result = glyphs.into_iter().collect::<String>();
    result.push_str("...");
    result
}

/// Width of a line of bitmap glyphs in source pixels. Ported verbatim from
/// `measure_line_width`.
pub fn measure_line_width(glyphs: &[char], advance: f32, glyph_width: f32) -> f32 {
    match glyphs.len() {
        0 => 0.0,
        count => (count.saturating_sub(1)) as f32 * advance + glyph_width,
    }
}

/// Map a few common non-ASCII characters down to their ASCII bitmap-table
/// equivalents. Ported verbatim from `normalize_bitmap_char` — the same
/// short table the predecessor prototype ships.
pub fn normalize_bitmap_char(character: char) -> char {
    match character {
        '²' => '2',
        '×' => 'X',
        '–' | '—' => '-',
        '\u{2018}' | '\u{2019}' => '\'',
        _ => character,
    }
}

/// 5×7 bit pattern for a single ASCII glyph. Rows are MSB-first; bit 4 is the
/// leftmost column. Unknown characters fall back to `'?'`. Ported verbatim
/// from the predecessor prototype's `bitmap_glyph` table.
#[rustfmt::skip]
pub fn bitmap_glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'J' => [0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        'a' => [0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101],
        'b' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b11110],
        'c' => [0b00000, 0b01110, 0b10001, 0b10000, 0b10000, 0b10001, 0b01110],
        'd' => [0b00001, 0b00001, 0b01101, 0b10011, 0b10001, 0b10001, 0b01111],
        'e' => [0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b10001, 0b01110],
        'f' => [0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000],
        'g' => [0b00000, 0b01111, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        'h' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
        'i' => [0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        'j' => [0b00010, 0b00000, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100],
        'k' => [0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010],
        'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'm' => [0b00000, 0b11010, 0b10101, 0b10101, 0b10101, 0b10101, 0b10101],
        'n' => [0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001, 0b10001],
        'o' => [0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'p' => [0b00000, 0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000],
        'q' => [0b00000, 0b01101, 0b10011, 0b10001, 0b01111, 0b00001, 0b00001],
        'r' => [0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000, 0b10000],
        's' => [0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        't' => [0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110],
        'u' => [0b00000, 0b10001, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'v' => [0b00000, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'w' => [0b00000, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'x' => [0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'y' => [0b00000, 0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        'z' => [0b00000, 0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110],
        '6' => [0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110, 0b00100],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '/' => [0b00001, 0b00010, 0b00100, 0b00100, 0b01000, 0b10000, 0b00000],
        '|' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        '\'' => [0b00100, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000],
        '=' => [0b00000, 0b11111, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000],
        '?' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100],
        '#' => [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010],
        '·' => [0b00000, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000, 0b00000],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => bitmap_glyph('?'),
    }
}

/// Build a [`GlyphCacheKey`] for the bitmap face directly from a character.
/// Convenience for callers (renderers, tests) that already know the codepoint
/// and font size but do not have a [`LayoutRun`] handy.
pub fn cache_key_for(ch: char, font_size_px: f32) -> GlyphCacheKey {
    GlyphCacheKey {
        font_id: FontId::BITMAP,
        font_generation: 0,
        glyph_id: ch as u32,
        size_px_q26_6: (font_size_px * 64.0).round() as i32,
        variations: VariationSettings::empty(),
        format: GlyphFormat::Alpha,
        subpixel_variant: 0,
        synthesis_flags: 0,
        hinting: HintingMode::None,
        transform: Affine2Fixed::IDENTITY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_text_lines_truncates_and_ellipsizes() {
        let lines = wrap_text_lines("selected terrain patch with a very long action hint", 12, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with("..."));
    }

    #[test]
    fn bitmap_scale_is_always_a_positive_integer() {
        // #157 Phase 4 variant B: the bitmap face is restricted to integer
        // scales — no font_size_px should ever produce a fractional scale.
        for tenths in 1..500 {
            let font_size_px = tenths as f32 / 10.0; // sweeps 0.1..50.0px
            let scale = bitmap_scale(font_size_px);
            assert!(
                scale >= 1.0,
                "font_size_px {font_size_px}: scale {scale} < 1.0"
            );
            assert_eq!(
                scale.fract(),
                0.0,
                "font_size_px {font_size_px}: scale {scale} is not an integer"
            );
        }
    }

    #[test]
    fn bitmap_scale_rounds_to_the_nearest_integer() {
        // 16px / 10.0 = 1.6 -> rounds to 2, not truncated to 1.
        assert_eq!(bitmap_scale(16.0), 2.0);
        // 14px / 10.0 = 1.4 -> rounds down to 1.
        assert_eq!(bitmap_scale(14.0), 1.0);
        // Exact multiples of REFERENCE_FONT_SIZE_PX are unaffected.
        assert_eq!(bitmap_scale(10.0), 1.0);
        assert_eq!(bitmap_scale(20.0), 2.0);
        assert_eq!(bitmap_scale(30.0), 3.0);
    }

    #[test]
    fn bitmap_font_has_own_glyphs_for_hash_and_middle_dot() {
        let hash = bitmap_glyph('#');
        let dot = bitmap_glyph('·');
        let fallback = bitmap_glyph('?');
        assert_ne!(hash, fallback, "'#' should have its own glyph");
        assert_ne!(dot, fallback, "'·' should have its own glyph");
        assert!(hash.iter().any(|row| *row != 0));
        assert!(dot.iter().any(|row| *row != 0));
    }

    #[test]
    fn families_lists_only_bitmap_face() {
        let system = BitmapTextSystem::new();
        assert_eq!(system.families(), vec![BITMAP_FAMILY.to_string()]);
    }

    #[test]
    fn register_font_bytes_rejects_external_fonts() {
        let system = BitmapTextSystem::new();
        let bytes: Arc<[u8]> = Arc::from(vec![0u8; 8].into_boxed_slice());
        assert_eq!(
            system.register_font_bytes(bytes, 0),
            Err(TextError::InvalidFontBytes)
        );
    }

    #[test]
    fn resolve_font_always_returns_face_zero() {
        let system = BitmapTextSystem::new();
        assert_eq!(
            system.resolve_font(&FontQuery::default()),
            Some(FontId::BITMAP)
        );
    }

    #[test]
    fn layout_run_cache_key_matches_direct_construction() {
        let system = BitmapTextSystem::new();
        let spec = LayoutSpec::default();
        let runs = system.layout("A", &spec, None);
        let run = &runs[0];
        let glyph = &run.glyphs[0];
        assert_eq!(run.cache_key(glyph), cache_key_for('A', spec.font_size_px));
    }
}
