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
        // `prepare`/`wrap` already normalize before building a cache key, but
        // callers that build a `GlyphCacheKey` straight from a raw codepoint
        // (e.g. `cache_key_for`, or another crate's bitmap-fallback lane)
        // must not bypass the NBSP/accent folding and hit `FALLBACK_GLYPH`
        // for a codepoint that has a real glyph. Idempotent on an
        // already-normalized `ch`.
        let bits = bitmap_glyph(normalize_bitmap_char(ch));

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

/// Map a few non-ASCII characters down to an existing bitmap-table glyph.
/// Two kinds of folding happen here:
///
/// - Width/space folding: the bitmap face is fixed-advance, so there is no
///   "narrow" glyph variant — NBSP and the French narrow no-break space both
///   collapse to the plain blank space.
/// - `Ç` folding: every other accented capital gets its own [`bitmap_glyph`]
///   entry (the letterform compressed into 6 rows, freeing row 0 for the
///   diacritic — see the accented-capital block in that table). A cedilla
///   sits *below* the baseline instead, and capital `C`'s bottom row
///   (`0b01111`) already uses every column the 5-bit cell has, so there is
///   no spare bit left to hang a hook off (unlike lowercase `c`'s bottom row
///   `0b01110`, which has one free column — see `ç` in [`bitmap_glyph`]).
///   Rendering the unaccented capital is the legible fallback until the
///   cell grows.
pub fn normalize_bitmap_char(character: char) -> char {
    match character {
        '²' => '2',
        '×' => 'X',
        '–' | '—' => '-',
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{00A0}' | '\u{202F}' => ' ',
        'Ç' => 'C',
        _ => character,
    }
}

/// The explicit "notdef" glyph: a hollow box, distinct from every real
/// letterform and digit in this table (including `'?'`, which keeps its own
/// question-mark glyph and is not treated as a fallback marker). Any
/// codepoint reaching [`bitmap_glyph`] with no table entry renders this —
/// legible and non-blank, never a panic and never an empty cell that could
/// be mistaken for a space.
#[rustfmt::skip]
pub const FALLBACK_GLYPH: [u8; 7] = [
    0b11111,
    0b10001,
    0b10001,
    0b10001,
    0b10001,
    0b10001,
    0b11111,
];

/// Whether `bits` is exactly the [`FALLBACK_GLYPH`] pattern.
pub fn is_fallback_glyph(bits: &[u8; 7]) -> bool {
    *bits == FALLBACK_GLYPH
}

/// 5×7 bit pattern for a single glyph. Rows are MSB-first; bit 4 is the
/// leftmost column. Covers ASCII (including `!`/`;`) plus the French-accented
/// letters (both cases), the acute/tilde/ring vowels needed for Spanish,
/// Portuguese, and Scandinavian text (á í ó ú ã å õ, both cases), the œ/Œ and
/// æ/Æ ligatures, ñ/Ñ, ø/Ø, ß, guillemets, curly quotes, and ellipsis;
/// [`normalize_bitmap_char`] folds a few other codepoints (NBSP, narrow
/// NBSP, `Ç`) onto entries here before this table is consulted. Characters
/// with no entry below render the explicit [`FALLBACK_GLYPH`], never `'?'`
/// and never a blank cell.
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
        // French-accented lowercase letters: row 0 is the free ascender row
        // (see 'a'/'e'/'o'/'u'/'y' above, all of which start with 0b00000),
        // so the diacritic is drawn there and rows 1..=6 are the base
        // letter's rows unchanged. 'i' normally puts its tittle in row 0
        // instead; for î/ï the diacritic replaces that dot, as it does in
        // real Latin typography.
        'à' => [0b01000, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101],
        'â' => [0b01010, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101],
        'ä' => [0b10001, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101],
        'é' => [0b00010, 0b01110, 0b10001, 0b11111, 0b10000, 0b10001, 0b01110],
        'è' => [0b01000, 0b01110, 0b10001, 0b11111, 0b10000, 0b10001, 0b01110],
        'ê' => [0b01010, 0b01110, 0b10001, 0b11111, 0b10000, 0b10001, 0b01110],
        'ë' => [0b10001, 0b01110, 0b10001, 0b11111, 0b10000, 0b10001, 0b01110],
        'î' => [0b01010, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        'ï' => [0b10001, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        'ô' => [0b01010, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'ö' => [0b10001, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'ù' => [0b01000, 0b10001, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'û' => [0b01010, 0b10001, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'ü' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'ÿ' => [0b10001, 0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        // Acute, tilde, and ring diacritics complete Latin vowel coverage
        // beyond French: á/í/ó/ú (Spanish, Portuguese), ã/õ (Portuguese), å
        // (Scandinavian). Same free-ascender-row placement as the
        // grave/circumflex/diaeresis accents above; 'í' replaces the
        // tittle the way î/ï do.
        'á' => [0b00010, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101],
        'í' => [0b00010, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        'ó' => [0b00010, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'ú' => [0b00010, 0b10001, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'ã' => [0b01101, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101],
        'å' => [0b01110, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101],
        'õ' => [0b01101, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        // French-accented capital letters: unlike lowercase, capitals have
        // no free ascender row (every row is already load-bearing for the
        // letterform — see 'A'/'E'/'I'/'O'/'U'/'Y' above). Row 0 is freed
        // for the diacritic instead by dropping one row from a run of
        // duplicate rows in the base capital's shape (e.g. 'A' has three
        // identical 0b10001 leg rows after the crossbar; this keeps two),
        // which shortens the letterform by one row but does not change its
        // recognizable shape. 'Ç' has no equivalent treatment and still
        // folds to 'C' in `normalize_bitmap_char` — a cedilla sits below
        // the baseline, and 'C''s bottom row already uses every column the
        // 5-bit cell has (see `normalize_bitmap_char`'s doc comment).
        'À' => [0b01000, 0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001],
        'Â' => [0b01010, 0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001],
        'Ä' => [0b10001, 0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001],
        'É' => [0b00010, 0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'È' => [0b01000, 0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'Ê' => [0b01010, 0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'Ë' => [0b10001, 0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'Î' => [0b01010, 0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'Ï' => [0b10001, 0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        // 'Ô'/'Ö' need their own bottom row, not plain 'O''s 0b01110: 'O''s
        // compressed body (top curve + 4 identical 0b10001 stem rows +
        // closing curve) is bit-for-bit the same 6 rows as lowercase 'o''s
        // own non-blank body, so stacking an accent on top of each
        // reproduces the *same* array for the capital and lowercase
        // version (`Ô` == `ô`, `Ö` == `ö`) — unlike 'À'/'É'/'Î', whose
        // capitals have a unique non-repeating top row that the accent
        // necessarily overwrites, breaking any such reuse. Widening the
        // closing curve by one bit on the right (same trick as 'Ü' below)
        // disambiguates the capital from the lowercase glyph.
        'Ô' => [0b01010, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01111],
        'Ö' => [0b10001, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01111],
        'Ù' => [0b01000, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'Û' => [0b01010, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        // 'Ü' needs its own bottom row, not plain 'U''s 0b01110: 'U''s
        // compressed body is five identical 0b10001 stem rows followed by
        // the closing curve, so a diaeresis accent row (also 0b10001)
        // stacked on top exactly reconstructs the seven rows of plain 'U'
        // bit-for-bit. Widening the closing curve by one bit on the right breaks that
        // collision while keeping the shape recognizable as 'U'.
        'Ü' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01111],
        'Ÿ' => [0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100],
        // Á/Í/Ú/Ã/Å/Õ: acute, tilde, and ring applied to the same compressed
        // capital bodies as À/É/Ù/Ñ above. 'Ó'/'Õ' need their bottom row
        // widened to 0b01111 (the same trick used for Ô/Ö) because their
        // accent rows (0b00010 acute, 0b01101 tilde) stacked on the
        // capital-O body would otherwise exactly reproduce lowercase
        // 'ó'/'õ''s bits; Á/Í/Ã/Å/Ú share no such collision with their
        // lowercase counterparts (their letterform bodies differ from
        // a/i/u's), so they keep the unwidened O/U closing-row value
        // instead.
        'Á' => [0b00010, 0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001],
        'Í' => [0b00010, 0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'Ó' => [0b00010, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01111],
        'Ú' => [0b00010, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'Ã' => [0b01101, 0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001],
        'Å' => [0b01110, 0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001],
        'Õ' => [0b01101, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01111],
        // Cedilla has no row below the baseline to sit in (row 6 is already
        // the bottom of the letter), so it is approximated as a small hook
        // extending the bottom-right of the 'c' shape rather than a true
        // descending tail.
        'ç' => [0b00000, 0b01110, 0b10001, 0b10000, 0b10000, 0b10001, 0b01111],
        // œ: an 'o' shape with a single-row center tick suggesting the seam
        // between the two joined letters (three full rows of the tick read
        // as a fence rather than a rounded ligature).
        'œ' => [0b00000, 0b01110, 0b10001, 0b10101, 0b10001, 0b10001, 0b01110],
        // Œ: a 'C' shape (open on the right) with a middle bar extending
        // further right, like the crossbar of a fused 'E'.
        'Œ' => [0b01111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b01111],
        // æ/Æ: the same single-row-tick ligature trick as œ/Œ, applied to
        // 'a'/'A' instead of 'o'/'C' — one of the base letter's rows gains
        // a center bit to suggest the seam between the two joined letters.
        'æ' => [0b00000, 0b01110, 0b00001, 0b01111, 0b10101, 0b10011, 0b01101],
        'Æ' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10101, 0b10001, 0b10001],
        // ñ/Ñ: 'n' with a tilde. Lowercase 'n' has the usual free ascender
        // row 0; capital 'N' does not, so its body is compressed into 6
        // rows the same way the other accented capitals are (dropping one
        // of its three identical bottom 0b10001 rows) — using a tilde bit
        // pattern (0b01101) not shared with any other diacritic row in
        // this table, so it can't reconstruct plain 'N' the way a reused
        // row value could (see the 'Ü' comment above).
        'ñ' => [0b01101, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001, 0b10001],
        'Ñ' => [0b01101, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001],
        // ß (sharp s): a tall figure-eight-like stem with an open tail at
        // the bottom right instead of closing the lower loop — the
        // distinguishing feature from '8' (which closes both loops), at
        // the cost of a true descender (the 5×7 cell has none to spare).
        'ß' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b10010],
        // ø/Ø: 'o'/'O' with a diagonal stroke added through the inner
        // columns (bits 1-3, which the circle's sides at bits 0/4 leave
        // free) of the middle rows. The diagonal runs top-left to
        // bottom-right — the opposite direction from digit '0''s own
        // built-in slash (top-right to bottom-left, see '0' below) — so
        // 'Ø' and '0' don't collide despite both being a slashed circle.
        'ø' => [0b00000, 0b01110, 0b11001, 0b10101, 0b10011, 0b10001, 0b01110],
        'Ø' => [0b01110, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b01110],
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
        // '!': the dot reuses '.''s bottom-row shape so the two punctuation
        // marks share a baseline footprint; row 4 is blank to separate the
        // bar from the dot, matching ':''s gap convention.
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00110, 0b00110],
        // ';': ':''s upper dot (rows 1-2) over ','s descending tail
        // (rows 4-6), the same composition French typography uses to build
        // ';' from ':' + ','.
        ';' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00110, 0b00110, 0b00100],
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
        // Guillemets sit at mid-x-height, like a hyphen, rather than the
        // full cell height parentheses use.
        '«' => [0b00000, 0b00000, 0b01010, 0b10100, 0b01010, 0b00000, 0b00000],
        '»' => [0b00000, 0b00000, 0b01010, 0b00101, 0b01010, 0b00000, 0b00000],
        // Curly double quotes: two strokes near the top of the cell, mirror
        // images of each other, distinct from the single curly quotes
        // folded onto `'` by `normalize_bitmap_char`.
        '\u{201C}' => [0b01010, 0b01010, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000],
        '\u{201D}' => [0b01010, 0b01010, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000],
        // Ellipsis: three single-pixel dots on the baseline, each one
        // column apart — as wide a spread as the 5-bit cell allows.
        '…' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b10101, 0b10101],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => FALLBACK_GLYPH,
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
        let question_mark = bitmap_glyph('?');
        assert_ne!(hash, question_mark, "'#' should have its own glyph");
        assert_ne!(dot, question_mark, "'·' should have its own glyph");
        assert!(hash.iter().any(|row| *row != 0));
        assert!(dot.iter().any(|row| *row != 0));
    }

    #[test]
    fn unmapped_codepoint_renders_the_defined_fallback_glyph() {
        // U+E000 is in the Private Use Area: guaranteed to have no entry in
        // `bitmap_glyph`, now or in any future addition to this table.
        let bits = bitmap_glyph('\u{E000}');
        assert!(
            is_fallback_glyph(&bits),
            "unmapped codepoint must render FALLBACK_GLYPH"
        );
        assert!(
            bits.iter().any(|row| *row != 0),
            "fallback glyph must not be blank"
        );
        // '?' is a real character with its own glyph, not a reused fallback.
        assert_ne!(bits, bitmap_glyph('?'));
    }

    #[test]
    fn fallback_glyph_is_distinct_from_every_mapped_character() {
        let mapped_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789\
            :.,;!+-/|()'=?#· «»“”…àâäéèêëîïôöùûüÿçœŒÀÂÄÉÈÊËÎÏÔÖÙÛÜŸæÆñÑßøØáíóúãåõÁÍÓÚÃÅÕ";
        for ch in mapped_chars.chars() {
            let bits = bitmap_glyph(ch);
            assert!(
                !is_fallback_glyph(&bits),
                "mapped character {ch:?} must not collide with FALLBACK_GLYPH"
            );
        }
    }

    #[test]
    fn every_mapped_character_has_a_pairwise_distinct_glyph() {
        // Hand-authoring compressed/ligature-derived bit patterns (the
        // accented capitals, œ/Œ, æ/Æ, ñ/Ñ, ø/Ø, ß) risks accidentally
        // reconstructing another entry's exact bits, as happened twice
        // during review: 'Ü''s first attempt was bit-identical to plain
        // 'U' (diaeresis row == U's own duplicated stem row), and 'Ø''s
        // first attempt was bit-identical to digit '0' (same diagonal
        // direction already baked into '0' to distinguish it from 'O').
        // This check is exhaustive over every mapped character, not just
        // those two pairs, so a future addition can't reintroduce the
        // same class of bug silently.
        let mapped_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789\
            :.,;!+-/|()'=?#· «»“”…àâäéèêëîïôöùûüÿçœŒÀÂÄÉÈÊËÎÏÔÖÙÛÜŸæÆñÑßøØáíóúãåõÁÍÓÚÃÅÕ";
        let mut seen: Vec<(char, [u8; 7])> = Vec::new();
        for ch in mapped_chars.chars() {
            let bits = bitmap_glyph(ch);
            if let Some((other, _)) = seen.iter().find(|(_, other_bits)| *other_bits == bits) {
                panic!("{ch:?} and {other:?} render the identical glyph {bits:?}");
            }
            seen.push((ch, bits));
        }
    }

    #[test]
    fn accented_capitals_render_distinctly_from_their_unaccented_base_letter() {
        // Each accented capital has its own `bitmap_glyph` entry distinct
        // from the base letter — unlike `Ç`, which still folds to `C` in
        // `normalize_bitmap_char` (see its doc comment) and is excluded
        // here. All-caps UI text (menus, headings) must keep its accents,
        // not silently drop them.
        let pairs = [
            ('À', 'A'),
            ('Â', 'A'),
            ('Ä', 'A'),
            ('É', 'E'),
            ('È', 'E'),
            ('Ê', 'E'),
            ('Ë', 'E'),
            ('Î', 'I'),
            ('Ï', 'I'),
            ('Ô', 'O'),
            ('Ö', 'O'),
            ('Ù', 'U'),
            ('Û', 'U'),
            ('Ü', 'U'),
            ('Ÿ', 'Y'),
            ('Á', 'A'),
            ('Í', 'I'),
            ('Ó', 'O'),
            ('Ú', 'U'),
            ('Ã', 'A'),
            ('Õ', 'O'),
            ('Å', 'A'),
        ];
        for (accented, base) in pairs {
            assert_eq!(
                normalize_bitmap_char(accented),
                accented,
                "{accented:?} must not be folded away by normalize_bitmap_char"
            );
            assert_ne!(
                bitmap_glyph(accented),
                bitmap_glyph(base),
                "{accented:?} must render distinctly from {base:?}"
            );
            assert!(
                !is_fallback_glyph(&bitmap_glyph(accented)),
                "{accented:?} must not render the fallback glyph"
            );
        }
    }

    #[test]
    fn pan_latin_letters_render_distinctly_from_their_lookalikes() {
        // æ/Æ, ñ/Ñ, ø/Ø, and ß each risk rendering as (or colliding with) an
        // unrelated already-mapped glyph — the base letter the ligature is
        // built from, the plain letter a diacritic was dropped from, or
        // (for ø/Ø) the digit '0', which this font already draws with its
        // own diagonal slash. Each pair below must differ, and neither
        // half may fall back.
        let pairs = [
            ('æ', 'a'),
            ('Æ', 'A'),
            ('ñ', 'n'),
            ('Ñ', 'N'),
            ('ø', 'o'),
            ('Ø', 'O'),
            ('Ø', '0'),
            ('ß', '8'),
            ('á', 'a'),
            ('Á', 'A'),
            ('í', 'i'),
            ('Í', 'I'),
            ('ó', 'o'),
            ('Ó', 'O'),
            ('Ó', 'ó'),
            ('ú', 'u'),
            ('Ú', 'U'),
            ('ã', 'a'),
            ('Ã', 'A'),
            ('å', 'a'),
            ('Å', 'A'),
            ('õ', 'o'),
            ('Õ', 'O'),
            ('Õ', 'õ'),
        ];
        for (ch, lookalike) in pairs {
            assert_ne!(
                bitmap_glyph(ch),
                bitmap_glyph(lookalike),
                "{ch:?} must render distinctly from {lookalike:?}"
            );
            assert!(
                !is_fallback_glyph(&bitmap_glyph(ch)),
                "{ch:?} must not render the fallback glyph"
            );
        }
    }

    #[test]
    fn unmapped_codepoint_rasterizes_without_panicking_and_is_not_blank() {
        let system = BitmapTextSystem::new();
        let key = cache_key_for('\u{E000}', REFERENCE_FONT_SIZE_PX);
        let image = system
            .rasterize(key)
            .expect("fallback glyph must rasterize");
        assert!(
            image.data.iter().any(|&byte| byte != 0),
            "fallback glyph image must not be blank"
        );
    }

    #[test]
    fn french_sample_paragraph_has_no_missing_glyphs() {
        // Covers every character class called for by Latin + French glyph
        // coverage: lowercase accented letters, an accented capital, the
        // œ/Œ ligature, guillemets with the narrow no-break space inside
        // them, a plain NBSP, curly single and double quotes, an em dash,
        // and an ellipsis.
        let paragraph = "Été, où çà et là, l'œuvre d'un ami : à âme, édentée, \
            crème brûlée, île, hôtel, dû, mûr, sûr, gruyère, Œuf, Noël\u{00A0}: \
            «\u{202F}vraiment\u{202F}» \u{2018}simple\u{2019} \u{201C}double\u{201D} \
            — c'est tout\u{2026}";

        for ch in paragraph.chars() {
            let normalized = normalize_bitmap_char(ch);
            let bits = bitmap_glyph(normalized);
            assert!(
                !is_fallback_glyph(&bits),
                "character {ch:?} (normalized to {normalized:?}) rendered the fallback glyph"
            );
        }
    }

    #[test]
    fn french_sample_paragraph_rasterizes_without_missing_glyphs_via_real_pipeline() {
        // Exercises the same paragraph end to end through the real
        // `TextSystem::layout` -> `cache_key` -> `rasterize` path, not the
        // `bitmap_glyph`/`normalize_bitmap_char` helpers directly — `layout`
        // already normalizes before building each glyph's cache key, so
        // this does not by itself guard against `rasterize` skipping
        // normalization on a cache key built some other way; that path is
        // covered separately by
        // `accented_capitals_and_french_punctuation_rasterize_via_cache_key_for`.
        let paragraph = "Été, où çà et là, l'œuvre d'un ami : à âme, édentée, \
            crème brûlée, île, hôtel, dû, mûr, sûr, gruyère, Œuf, Noël\u{00A0}: \
            «\u{202F}vraiment\u{202F}» \u{2018}simple\u{2019} \u{201C}double\u{201D} \
            — c'est tout\u{2026}";
        let system = BitmapTextSystem::new();
        let spec = LayoutSpec::default();
        let fallback_image = system
            .rasterize(cache_key_for('\u{E000}', spec.font_size_px))
            .expect("fallback glyph must rasterize");

        let runs = system.layout(paragraph, &spec, None);
        for run in &runs {
            for glyph in &run.glyphs {
                let ch = char::from_u32(glyph.glyph_id).expect("glyph_id must be a valid char");
                let image = system
                    .rasterize(run.cache_key(glyph))
                    .unwrap_or_else(|e| panic!("char {ch:?} failed to rasterize: {e:?}"));
                if ch == ' ' {
                    continue; // Blank space is a legitimate non-fallback result.
                }
                assert_ne!(
                    image.data, fallback_image.data,
                    "char {ch:?} rasterized as the fallback glyph via the real pipeline"
                );
            }
        }
    }

    #[test]
    fn review_reported_sentences_rasterize_without_missing_glyphs_via_real_pipeline() {
        // Regression test for a review finding on #184's original PR: these
        // exact three sentences, run through the real `layout` -> `rasterize`
        // pipeline, rendered the fallback box for '!', ';', and 'æ' (and,
        // separately, for the unaccented Latin letters later folded into
        // #227's scope). '!' and ';' are plain ASCII that should have had
        // their own `bitmap_glyph` entries from the start; 'æ' is covered
        // directly here rather than deferred.
        let sentences = ["« Bonjour ! »", "café ; thé", "ex æquo"];
        let system = BitmapTextSystem::new();
        let spec = LayoutSpec::default();
        let fallback_image = system
            .rasterize(cache_key_for('\u{E000}', spec.font_size_px))
            .expect("fallback glyph must rasterize");

        for sentence in sentences {
            let runs = system.layout(sentence, &spec, None);
            for run in &runs {
                for glyph in &run.glyphs {
                    let ch = char::from_u32(glyph.glyph_id).expect("glyph_id must be a valid char");
                    let image = system
                        .rasterize(run.cache_key(glyph))
                        .unwrap_or_else(|e| panic!("char {ch:?} failed to rasterize: {e:?}"));
                    if ch == ' ' {
                        continue;
                    }
                    assert_ne!(
                        image.data, fallback_image.data,
                        "char {ch:?} in {sentence:?} rasterized as the fallback glyph"
                    );
                }
            }
        }
    }

    #[test]
    fn round_2_review_reported_words_rasterize_without_missing_glyphs_via_real_pipeline() {
        // Regression test for the round-2 review finding on #184's PR: "São
        // Paulo", "smörgås", and "Olá", run through the real `layout` ->
        // `rasterize` pipeline, rendered the fallback box for 'ã', 'å', and
        // 'á'. These letters (plus í/ó/ú/Ã/Å and their capitals) are now
        // covered directly rather than deferred to #227.
        let sentences = ["São Paulo", "smörgås", "Olá"];
        let system = BitmapTextSystem::new();
        let spec = LayoutSpec::default();
        let fallback_image = system
            .rasterize(cache_key_for('\u{E000}', spec.font_size_px))
            .expect("fallback glyph must rasterize");

        for sentence in sentences {
            let runs = system.layout(sentence, &spec, None);
            for run in &runs {
                for glyph in &run.glyphs {
                    let ch = char::from_u32(glyph.glyph_id).expect("glyph_id must be a valid char");
                    let image = system
                        .rasterize(run.cache_key(glyph))
                        .unwrap_or_else(|e| panic!("char {ch:?} failed to rasterize: {e:?}"));
                    if ch == ' ' {
                        continue;
                    }
                    assert_ne!(
                        image.data, fallback_image.data,
                        "char {ch:?} in {sentence:?} rasterized as the fallback glyph"
                    );
                }
            }
        }
    }

    #[test]
    fn accented_capitals_and_french_punctuation_rasterize_via_cache_key_for() {
        // `cache_key_for` builds a `GlyphCacheKey` directly from a raw
        // codepoint, bypassing `prepare`/`wrap`'s normalization — exactly
        // the path that must still resolve correctly through `rasterize`.
        let system = BitmapTextSystem::new();
        let fallback_image = system
            .rasterize(cache_key_for('\u{E000}', REFERENCE_FONT_SIZE_PX))
            .expect("fallback glyph must rasterize");

        for ch in ['É', 'À', 'Ç', '\u{00A0}', '\u{202F}', '—', '\u{2019}'] {
            let image = system
                .rasterize(cache_key_for(ch, REFERENCE_FONT_SIZE_PX))
                .unwrap_or_else(|e| panic!("char {ch:?} failed to rasterize: {e:?}"));
            assert_ne!(
                image.data, fallback_image.data,
                "char {ch:?} rasterized as the fallback glyph"
            );
        }
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
