//! #67 acceptance: concrete `SfntTextSystem` integration with #62's composite
//! registry, and layout-time fallback segmentation exercised end-to-end with a
//! **real** SFNT face + the built-in bitmap fallback (not a mock provider).

use std::sync::Arc;

use mkui_text::{
    Affine2Fixed, CompositeTextSystem, FontId, FontQuery, FontSource, LayoutSpec, OutlineKey,
    TextAlign, TextEngine, TextError, TextRenderClass, TextSystem, VariationSettings,
};

const ABEL: &[u8] = include_bytes!("fixtures/abel/Abel-Regular.ttf");

fn registered() -> (CompositeTextSystem, FontId) {
    let mut sys = CompositeTextSystem::new();
    let id = sys
        .register_sfnt_face(Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
        .expect("Abel registers");
    (sys, id)
}

fn slug_spec(font_id: FontId, font_size_px: f32) -> LayoutSpec {
    LayoutSpec {
        font_id,
        font_size_px,
        ..LayoutSpec::default()
    }
}

#[test]
fn sfnt_face_registers_as_slug_beside_bitmap() {
    let (sys, id) = registered();
    // A fresh global id, never the reserved bitmap face, in the Slug lane from
    // a Bytes source — the registry (not the provider) is authoritative.
    assert_ne!(id, FontId::BITMAP);
    assert_eq!(sys.render_class(id), Some(TextRenderClass::Slug));
    assert_eq!(sys.font_source(id), Some(FontSource::Bytes));
    assert_eq!(sys.font_generation(id), Some(0));

    // Bitmap face still present and selectable beside the SFNT face.
    assert_eq!(
        sys.render_class(FontId::BITMAP),
        Some(TextRenderClass::Bitmap)
    );

    // Resolvable by the decoded family name; the default query still lands on
    // bitmap (the compatibility default).
    let q = FontQuery {
        family: Some("Abel".into()),
        ..Default::default()
    };
    assert_eq!(sys.resolve_font(&q), Some(id));
    assert_eq!(
        sys.resolve_font(&FontQuery::default()),
        Some(FontId::BITMAP)
    );

    // The decoded face is inspectable through the registry without re-parsing.
    let face = sys.sfnt_face(id).expect("SFNT face exposed");
    assert_eq!(face.units_per_em(), 2048);
    assert!(sys.sfnt_face(FontId::BITMAP).is_none());
}

#[test]
fn provider_local_id_reverse_routes_outline_requests() {
    let (sys, id) = registered();
    let gid = sys.sfnt_face(id).unwrap().glyph_index('M').unwrap();

    // glyph_outline reverse-routes the public FontId to the SFNT provider and
    // returns the decoded outline — proving the (provider, local) mapping is
    // followed, never reconstructed.
    let key = OutlineKey {
        font_id: id,
        font_generation: 0,
        glyph_id: gid as u32,
        variations: VariationSettings::empty(),
        synthesis_flags: 0,
        transform: Affine2Fixed::IDENTITY,
    };
    let outline = sys.glyph_outline(&key).expect("routed SFNT outline");
    assert_eq!(outline.units_per_em, 2048);
    assert_eq!(
        (
            outline.ink_bounds.min_x,
            outline.ink_bounds.min_y,
            outline.ink_bounds.max_x,
            outline.ink_bounds.max_y
        ),
        (164.0, 0.0, 1176.0, 1434.0)
    );

    // The bitmap face genuinely has no outline (distinct from an unknown id).
    let bmp_key = OutlineKey {
        font_id: FontId::BITMAP,
        ..key.clone()
    };
    assert_eq!(
        sys.glyph_outline(&bmp_key).unwrap_err(),
        TextError::UnsupportedOutline
    );

    // A Slug/outline face is not CPU-rasterized: typed UnsupportedRaster.
    let raster_key = mkui_text::GlyphCacheKey {
        font_id: id,
        font_generation: 0,
        glyph_id: gid as u32,
        size_px_q26_6: 24 * 64,
        variations: VariationSettings::empty(),
        format: mkui_text::GlyphFormat::Alpha,
        subpixel_variant: 0,
        synthesis_flags: 0,
        hinting: mkui_text::HintingMode::None,
        transform: Affine2Fixed::IDENTITY,
    };
    assert_eq!(
        sys.rasterize(raster_key).unwrap_err(),
        TextError::UnsupportedRaster
    );
}

#[test]
fn fully_supported_text_is_one_slug_run_in_cluster_order() {
    let (sys, id) = registered();
    let face = sys.sfnt_face(id).unwrap().clone();
    let spec = slug_spec(id, 24.0);
    let runs = sys.layout("Mag", &spec, None);

    assert_eq!(runs.len(), 1, "all-supported text stays one Slug run");
    let run = &runs[0];
    assert_eq!(run.font_id, id);
    assert_eq!(run.render_class, TextRenderClass::Slug);
    assert_eq!(run.glyphs.len(), 3);

    // Cluster order preserved and glyph ids are the SFNT ids. Each glyph's
    // reported `x_advance_px` is its own metric *plus* any kerning (#181)
    // Abel's `kern` table applies before the next glyph — the shaping
    // convention of recording a pair adjustment on the glyph it trails, so a
    // caller summing advances lands on the same position `x_px` reports.
    let text: Vec<char> = "Mag".chars().collect();
    let scale = 24.0 / face.units_per_em() as f32;
    for (i, &ch) in text.iter().enumerate() {
        let gid = face.glyph_index(ch).unwrap();
        assert_eq!(run.glyphs[i].cluster, i as u32);
        assert_eq!(run.glyphs[i].glyph_id, gid as u32);
        let mut want_adv = face.advance_width(gid) as f32 * scale;
        if let Some(&next_ch) = text.get(i + 1) {
            if let Some(next_gid) = face.glyph_index(next_ch) {
                want_adv += face.kerning(gid, next_gid) as f32 * scale;
            }
        }
        assert!((run.glyphs[i].x_advance_px - want_adv).abs() < 1e-3);
    }
}

#[test]
fn mixed_text_splits_into_positioned_slug_and_bitmap_runs() {
    let (sys, id) = registered();
    let spec = slug_spec(id, 24.0);

    // '中' is unmapped in Abel -> bitmap fallback between two Slug glyphs.
    let runs = sys.layout("M中A", &spec, None);
    assert_eq!(
        runs.len(),
        3,
        "fallback boundary splits the line into 3 runs"
    );

    // Run identities come from the registry's FaceRecord, per routing.
    assert_eq!(runs[0].render_class, TextRenderClass::Slug);
    assert_eq!(runs[0].font_id, id);
    assert_eq!(runs[1].render_class, TextRenderClass::Bitmap);
    assert_eq!(runs[1].font_id, FontId::BITMAP);
    assert_eq!(runs[2].render_class, TextRenderClass::Slug);
    assert_eq!(runs[2].font_id, id);

    // Nothing dropped: exactly the three source clusters, in order.
    let clusters: Vec<u32> = runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.cluster))
        .collect();
    assert_eq!(clusters, vec![0, 1, 2]);

    // Absolute x positions are preserved and continuous across the run
    // boundaries: each run's origin equals the previous run's origin plus its
    // advances. Positions must be strictly increasing.
    let mut expected_origin = 0.0f32;
    let mut last_abs = f32::NEG_INFINITY;
    for run in &runs {
        assert!((run.origin_x_px - expected_origin).abs() < 1e-3);
        for g in &run.glyphs {
            let abs = run.origin_x_px + g.x_px;
            assert!(abs >= last_abs, "absolute x positions must not regress");
            last_abs = abs;
            expected_origin += g.x_advance_px;
        }
    }

    // The fallback run carries the bitmap glyph id and is genuinely
    // rasterizable through the bitmap provider (proving it is renderable, not a
    // dangling reference).
    let fb_run = &runs[1];
    let img = sys.rasterize(fb_run.cache_key(&fb_run.glyphs[0])).unwrap();
    assert!(img.width_px > 0 && img.height_px > 0);

    // All runs share one baseline so the two lanes coexist on the same line.
    let baseline = runs[0].line_y_baseline_px;
    assert!(runs
        .iter()
        .all(|r| (r.line_y_baseline_px - baseline).abs() < 1e-3));
}

#[test]
fn abel_kern_table_is_applied_between_mapped_glyphs() {
    // Abel's real `kern` table carries a `"` -> `A` pair (glyphs 5, 36) worth
    // -123 font units — #181's kerning path applied through the full
    // composite registry + layout pipeline, not a synthetic fixture.
    let (sys, id) = registered();
    let face = sys.sfnt_face(id).unwrap().clone();
    let quote_gid = face.glyph_index('"').unwrap();
    let a_gid = face.glyph_index('A').unwrap();
    let kern = face.kerning(quote_gid, a_gid);
    assert_eq!(
        kern, -123,
        "fixture's known kerning pair changed underneath this test"
    );

    let spec = slug_spec(id, 24.0);
    let runs = sys.layout("\"A", &spec, None);
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.glyphs.len(), 2);

    let scale = 24.0 / face.units_per_em() as f32;
    let quote_advance = face.advance_width(quote_gid) as f32 * scale;
    let kern_px = kern as f32 * scale;

    // The second glyph's position includes the kerning gap...
    assert!((run.glyphs[1].x_px - (quote_advance + kern_px)).abs() < 1e-3);
    // ...and it is folded into the first glyph's reported advance too (the
    // shaping convention), so summing advances agrees with the position.
    assert!((run.glyphs[0].x_advance_px - (quote_advance + kern_px)).abs() < 1e-3);
}

#[test]
fn leading_and_trailing_fallback_runs_are_segmented() {
    let (sys, id) = registered();
    let spec = slug_spec(id, 16.0);
    // Unsupported at both ends, supported in the middle.
    let runs = sys.layout("中M中", &spec, None);
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].render_class, TextRenderClass::Bitmap);
    assert_eq!(runs[1].render_class, TextRenderClass::Slug);
    assert_eq!(runs[2].render_class, TextRenderClass::Bitmap);
}

// ---- #182: wrap / align / max-lines / ellipsis / measurement on the Slug
// (SFNT) lane — the gap `expand_slug_text` bailed to bitmap for since #171/#172.

/// Each wrapped line's measured width (via [`TextEngine`], the public
/// measurement API) must respect `max_width_px`, and the block must contain
/// more than one line — proving `SfntProvider::layout_local` genuinely wraps
/// instead of ignoring `max_width_px` (the pre-#182 behavior).
#[test]
fn slug_lane_wraps_a_paragraph_at_an_arbitrary_max_width() {
    let (sys, id) = registered();
    let spec = slug_spec(id, 16.0);
    let text = "The quick brown fox jumps over the lazy dog";

    let engine = TextEngine::new(Arc::new(sys));
    let prepared = engine.prepare(text, spec);
    let intrinsic_width = prepared.intrinsic_width_px();

    let narrow_width = intrinsic_width / 4.0;
    let layout = prepared.layout(Some(narrow_width));

    assert!(
        layout.lines.len() > 1,
        "wrapping a narrow box must produce multiple lines, got {}",
        layout.lines.len()
    );
    for (i, line) in layout.lines.iter().enumerate() {
        assert!(
            line.width_px <= narrow_width + 1.0,
            "line {i} width {} exceeds max width {narrow_width}",
            line.width_px
        );
    }
    // Every wrap point consumes exactly the one whitespace character it
    // broke at (mirrors the bitmap lane's `wrap_paragraph`), so the glyph
    // count is less than the source length by the number of line breaks but
    // never collapses to (near-)nothing.
    let total_glyphs: usize = layout.runs.iter().map(|r| r.glyphs.len()).sum();
    assert!(total_glyphs > text.chars().count().saturating_sub(layout.lines.len()));
    assert!(total_glyphs <= text.chars().count());
}

/// A word that exactly fits the line must not be wrapped early just because
/// the *next* character (the separating space before the following word) is
/// the one that overflows. The break point must be recognized as the
/// overflowing whitespace itself, not an earlier, shorter one.
#[test]
fn slug_lane_does_not_wrap_a_word_that_exactly_fits_the_line() {
    let (sys, id) = registered();
    let spec = slug_spec(id, 16.0);
    let engine = TextEngine::new(Arc::new(sys));

    let probe = engine.prepare("hello world", spec.clone());
    let hello_world_width = probe.intrinsic_width_px();

    let prepared = engine.prepare("hello world foo", spec);
    let layout = prepared.layout(Some(hello_world_width + 0.5));

    assert_eq!(
        layout.lines[0].glyph_range.len(),
        11,
        "line 0 must keep all of \"hello world\" (11 glyphs), not wrap early"
    );
}

/// `TextAlign::Center` / `TextAlign::End` must offset the Slug run's pen
/// start, not render flush-left — the exact regression #172's round-1/2 fix
/// bailed to bitmap for instead of fixing.
#[test]
fn slug_lane_honors_center_and_end_alignment() {
    let (sys, id) = registered();
    let box_width = 400.0;

    let start_runs = sys.layout(
        "Mag",
        &LayoutSpec {
            align: TextAlign::Start,
            ..slug_spec(id, 16.0)
        },
        Some(box_width),
    );
    let center_runs = sys.layout(
        "Mag",
        &LayoutSpec {
            align: TextAlign::Center,
            ..slug_spec(id, 16.0)
        },
        Some(box_width),
    );
    let end_runs = sys.layout(
        "Mag",
        &LayoutSpec {
            align: TextAlign::End,
            ..slug_spec(id, 16.0)
        },
        Some(box_width),
    );

    let start_x = start_runs[0].origin_x_px;
    let center_x = center_runs[0].origin_x_px;
    let end_x = end_runs[0].origin_x_px;

    assert_eq!(start_x, 0.0);
    assert!(
        center_x > start_x && center_x < end_x,
        "center offset {center_x} must sit strictly between start {start_x} and end {end_x}"
    );
    // The same content, so End's offset must be exactly double Center's
    // (Start=0, Center=(box-content)/2, End=box-content).
    assert!((end_x - 2.0 * center_x).abs() < 1e-2);
}

/// `max_lines` caps the line count, and the final line is ellipsized rather
/// than silently dropped or left overflowing.
#[test]
fn slug_lane_caps_at_max_lines_and_ellipsizes_the_tail() {
    let (sys, id) = registered();
    let face = sys.sfnt_face(id).unwrap().clone();
    let text = "one two three four five six seven eight nine ten";
    let spec = LayoutSpec {
        max_lines: Some(2),
        ..slug_spec(id, 16.0)
    };
    // Narrow enough that the unbounded text needs far more than 2 lines.
    let runs = sys.layout(text, &spec, Some(60.0));

    // Exactly 2 distinct lines (grouped by shared baseline).
    let mut baselines: Vec<f32> = runs.iter().map(|r| r.line_y_baseline_px).collect();
    baselines.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    assert_eq!(baselines.len(), 2, "max_lines=2 must cap at 2 lines");

    // The last run's final glyph is the ellipsis marker: either '…' (if the
    // face maps it) or the last of three '.' glyphs.
    let last = runs.last().unwrap();
    let last_glyph = last.glyphs.last().unwrap();
    let is_ellipsis_glyph = match face.glyph_index('…') {
        Some(gid) => last_glyph.glyph_id == gid as u32,
        None => {
            let dot_slug = face.glyph_index('.').map(|g| g as u32);
            let dot_bitmap = mkui_text::normalize_bitmap_char('.') as u32;
            Some(last_glyph.glyph_id) == dot_slug.map(Some).unwrap_or(Some(dot_bitmap))
        }
    };
    assert!(
        is_ellipsis_glyph,
        "truncated tail must end in an ellipsis glyph"
    );
}

/// No panic across the degenerate inputs an autonomous layout caller might
/// legitimately hit: a box narrower than a single glyph, non-finite/zero
/// width, `max_lines = Some(0)`, and a run of blank lines.
#[test]
fn slug_lane_does_not_panic_on_degenerate_inputs() {
    let (sys, id) = registered();

    for width in [0.0_f32, -1.0, f32::NAN, f32::INFINITY, 0.001] {
        let _ = sys.layout("Mag", &slug_spec(id, 16.0), Some(width));
    }

    let zero_lines_spec = LayoutSpec {
        max_lines: Some(0),
        ..slug_spec(id, 16.0)
    };
    let runs = sys.layout("Mag", &zero_lines_spec, Some(100.0));
    assert!(runs.is_empty(), "max_lines=0 must yield no runs");

    // Multiple blank paragraphs: every line is empty, must not panic and
    // must still produce one run per line.
    let blank_runs = sys.layout("\n\n\n", &slug_spec(id, 16.0), None);
    assert_eq!(blank_runs.len(), 4);
    assert!(blank_runs.iter().all(|r| r.glyphs.is_empty()));

    // A single unbreakable word far wider than the box must still produce
    // exactly one line (hard-broken at the glyph boundary), not loop forever.
    let runs = sys.layout(
        "supercalifragilisticexpialidocious",
        &slug_spec(id, 16.0),
        Some(5.0),
    );
    assert!(!runs.is_empty());
}

/// The public measurement API ([`TextEngine`]/[`TextLayout`]) must report the
/// true line count and block height for Slug-routed mixed-lane text — the
/// bug `engine.rs::build_layout` had before #182, which assumed one run per
/// line and so miscounted a Slug line that splits into a Slug + bitmap-
/// fallback run pair.
#[test]
fn measurement_reports_one_line_for_a_mixed_lane_single_line() {
    let (sys, id) = registered();
    let spec = slug_spec(id, 24.0);
    let engine = TextEngine::new(Arc::new(sys));

    // "M中A": Abel maps 'M'/'A' but not '中' — three runs (Slug, Bitmap,
    // Slug), all on what must measure as exactly one line.
    let prepared = engine.prepare("M中A", spec);
    let layout = prepared.layout(None);

    assert_eq!(
        layout.lines.len(),
        1,
        "a mixed Slug+bitmap-fallback single line must measure as one line, not {}",
        layout.lines.len()
    );
    assert_eq!(layout.block_height_px, layout.line_height_px);
    // The line's width spans the full mixed run, not just its first/last run.
    let total_glyphs: usize = layout.runs.iter().map(|r| r.glyphs.len()).sum();
    assert_eq!(layout.lines[0].glyph_range.len(), total_glyphs);
}
