//! Acceptance-criterion smoke for the Sprint 2 bitmap implementation.
//!
//! Asserts the three things called out in issue #19:
//!   (a) glyph count matches printable-ASCII count of the input,
//!   (b) line count matches expected wrap behaviour at a fixed `max_width_px`,
//!   (c) the rasterized output for 'A' matches the checked-in expected
//!       bitmap (full per-bit table — every other glyph would gate the test
//!       on visual review, 'A' is the canonical inspection target per the
//!       issue body).

use mkui_text::{
    bitmap_glyph, bitmap_scale, cache_key_for, BitmapTextSystem, GlyphFormat, LayoutSpec,
    TextAlign, TextSystem, GLYPH_ADVANCE_PX, GLYPH_CELL_WIDTH_PX,
};

const PANGRAM: &str = "The quick brown fox 0123456789";

/// Acceptance criterion (a): every printable-ASCII source character —
/// including the spaces that the prototype intentionally keeps as glyph
/// slots — round-trips through `layout()` as a `LayoutGlyph`.
#[test]
fn pangram_glyph_count_matches_input_chars() {
    let system = BitmapTextSystem::new();
    let spec = LayoutSpec {
        align: TextAlign::Start,
        max_lines: Some(8),
        ..LayoutSpec::default()
    };
    let runs = system.layout(PANGRAM, &spec, Some(1_000.0));
    let total_glyphs: usize = runs.iter().map(|r| r.glyphs.len()).sum();
    assert_eq!(
        total_glyphs,
        PANGRAM.chars().count(),
        "every input character must produce a LayoutGlyph (spaces included)"
    );
}

/// Acceptance criterion (b): at a fixed `max_width_px` chosen to fit ~10
/// glyphs, the pangram wraps into multiple lines and never exceeds
/// `max_lines`.
#[test]
fn pangram_wraps_to_multiple_lines_under_fixed_width() {
    let system = BitmapTextSystem::new();
    let spec = LayoutSpec {
        max_lines: Some(4),
        ..LayoutSpec::default()
    };
    // 10 glyphs at the default 10px font (scale = 1): advance 5.8px × 9
    // gaps + 5.0px tail cell = 57.2px box.
    let max_width = 57.2;
    let runs = system.layout(PANGRAM, &spec, Some(max_width));
    assert!(runs.len() >= 2, "pangram must wrap under 60px max width");
    assert!(runs.len() <= 4, "max_lines must clamp the output");
    let glyph_count_per_run = runs.iter().map(|r| r.glyphs.len()).collect::<Vec<_>>();
    let expected_max =
        mkui_text::max_glyphs_for_width(max_width, GLYPH_ADVANCE_PX, GLYPH_CELL_WIDTH_PX);
    for count in &glyph_count_per_run {
        assert!(
            *count <= expected_max,
            "line of {count} glyphs exceeds wrap budget {expected_max}"
        );
    }
}

/// Acceptance criterion (c): the rasterized output for 'A' matches the
/// checked-in expected bitmap, verifying the 5×7 bit pattern survives the
/// trait round-trip into `Alpha`-format `GlyphImage` bytes.
#[test]
fn rasterized_a_matches_expected_5x7_bitmap() {
    let system = BitmapTextSystem::new();
    let key = cache_key_for('A', 10.0);
    let image = system.rasterize(key).expect("'A' must rasterize");

    assert_eq!(image.format, GlyphFormat::Alpha);
    assert_eq!(image.width_px, 5);
    assert_eq!(image.height_px, 7);
    assert_eq!(image.left_px, 0);
    assert_eq!(image.top_px, 0);

    // Source bit pattern for 'A':
    //   .###.
    //   #...#
    //   #...#
    //   #####
    //   #...#
    //   #...#
    //   #...#
    let expected: [[u8; 5]; 7] = [
        [0, 255, 255, 255, 0],
        [255, 0, 0, 0, 255],
        [255, 0, 0, 0, 255],
        [255, 255, 255, 255, 255],
        [255, 0, 0, 0, 255],
        [255, 0, 0, 0, 255],
        [255, 0, 0, 0, 255],
    ];
    for (row_index, row) in expected.iter().enumerate() {
        for (col_index, expected_alpha) in row.iter().enumerate() {
            let actual = image.data[row_index * image.width_px as usize + col_index];
            assert_eq!(
                actual, *expected_alpha,
                "mismatch at row {row_index}, col {col_index}: got {actual}, expected {expected_alpha}"
            );
        }
    }
}

/// Sanity: at 2× the reference font size the raster scales 2x and stays a
/// faithful nearest-neighbor replication of the 5×7 source.
#[test]
fn raster_scales_with_font_size() {
    let system = BitmapTextSystem::new();
    let key = cache_key_for('A', 20.0);
    let image = system.rasterize(key).expect("'A' must rasterize");
    assert_eq!(image.width_px, 10);
    assert_eq!(image.height_px, 14);
    assert!((bitmap_scale(20.0) - 2.0).abs() < 1e-6);
    // The top-left 2×2 block should mirror the source bit at (0, 0), which
    // is the off-bit of the 'A' top row.
    assert_eq!(image.data[0], 0);
    assert_eq!(image.data[1], 0);
    assert_eq!(image.data[image.width_px as usize], 0);
    assert_eq!(image.data[image.width_px as usize + 1], 0);
    // Source bit at (1, 0) is on; the 2×2 block at output (2..4, 0..2)
    // should be fully filled.
    assert_eq!(image.data[2], 255);
    assert_eq!(image.data[3], 255);
}

/// Alignment math runs entirely inside layout(); a centred line places the
/// run origin at `(max_width - line_width) / 2`.
#[test]
fn layout_centers_lines_under_explicit_max_width() {
    let system = BitmapTextSystem::new();
    let spec = LayoutSpec {
        align: TextAlign::Center,
        ..LayoutSpec::default()
    };
    let runs = system.layout("A", &spec, Some(100.0));
    assert_eq!(runs.len(), 1);
    let line_width = GLYPH_CELL_WIDTH_PX; // single-glyph line
    let expected_origin = (100.0 - line_width) * 0.5;
    assert!(
        (runs[0].origin_x_px - expected_origin).abs() < 1e-4,
        "expected centred origin {expected_origin}, got {}",
        runs[0].origin_x_px
    );
}

/// 'A' bitmap is non-empty — guards against accidental deletion of the
/// glyph table.
#[test]
fn glyph_table_contains_a() {
    let bits = bitmap_glyph('A');
    assert!(bits.iter().any(|row| *row != 0));
}
