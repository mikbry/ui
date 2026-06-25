//! #67 acceptance: concrete `SfntTextSystem` integration with #62's composite
//! registry, and layout-time fallback segmentation exercised end-to-end with a
//! **real** SFNT face + the built-in bitmap fallback (not a mock provider).

use std::sync::Arc;

use mkui_text::{
    Affine2Fixed, CompositeTextSystem, FontId, FontQuery, FontSource, LayoutSpec, OutlineKey,
    TextError, TextRenderClass, TextSystem, VariationSettings,
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

    // Cluster order preserved and glyph ids are the SFNT ids.
    let scale = 24.0 / face.units_per_em() as f32;
    for (i, ch) in "Mag".chars().enumerate() {
        let gid = face.glyph_index(ch).unwrap();
        assert_eq!(run.glyphs[i].cluster, i as u32);
        assert_eq!(run.glyphs[i].glyph_id, gid as u32);
        let want_adv = face.advance_width(gid) as f32 * scale;
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
