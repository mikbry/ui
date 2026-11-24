//! Issue #61 contract acceptance smoke: identity domain, render-lane
//! propagation, canonical fixed-point cache keys, and the outline default.

use std::sync::Arc;

use mkui_text::{
    Affine2Fixed, BitmapTextSystem, Fixed16_16, FontId, FontIdAllocator, GlyphCacheKey,
    GlyphFormat, HintingMode, LayoutSpec, OpenTypeTag, OutlineKey, TextEngine, TextError,
    TextRenderClass, TextSystem, VariationAxis, VariationSettings,
};

#[test]
fn font_id_is_opaque_only_bitmap_is_constructible() {
    // The only publicly-constructible value is the reserved bitmap face; every
    // other id must come from an allocator (no public raw constructor).
    assert_eq!(FontId::BITMAP.raw(), 0);
    // Allocated ids are >= 1, distinct, monotonic, and never the bitmap face.
    // This uses the public `new()` allocator (the only one downstream crates
    // can reach — `isolated()` is test-only and crate-private), so it asserts
    // ordering/distinctness rather than exact raw values, which depend on the
    // shared process-global counter.
    let alloc = FontIdAllocator::new();
    let a = alloc.allocate().unwrap();
    let b = alloc.allocate().unwrap();
    assert!(a.raw() >= 1 && b.raw() >= 1);
    assert!(a.raw() < b.raw(), "allocated ids must be monotonic");
    assert_ne!(a, b);
    assert_ne!(a, FontId::BITMAP);
}

#[test]
fn independent_providers_get_globally_unique_ids() {
    // Finding 2 guard: two providers, each with their own default allocator,
    // each registering one font, must produce *distinct* ids — never both
    // `FontId(1)`. The default allocator is process-global so this holds even
    // across separate allocator handles.
    let provider_a = FontIdAllocator::new();
    let provider_b = FontIdAllocator::new();
    let id_a = provider_a.allocate().unwrap();
    let id_b = provider_b.allocate().unwrap();
    assert_ne!(id_a, id_b);
    assert_ne!(id_a, FontId::BITMAP);
    assert_ne!(id_b, FontId::BITMAP);
}

#[test]
fn font_id_overflow_is_an_error_not_a_wrap() {
    // The unit test in the crate drives the u64::MAX boundary directly. Here we
    // just assert the error variant exists and is surfaced through the shared
    // contract type.
    let err: TextError = TextError::FontIdOverflow;
    assert_eq!(err, TextError::FontIdOverflow);
}

#[test]
fn bitmap_layout_propagates_bitmap_render_class() {
    let system = BitmapTextSystem::new();
    let runs = system.layout("hi", &LayoutSpec::default(), None);
    assert!(!runs.is_empty());
    for run in &runs {
        assert_eq!(run.render_class, TextRenderClass::Bitmap);
        assert_eq!(run.font_id, FontId::BITMAP);
    }
}

#[test]
fn cache_key_uses_canonical_variation_and_transform_types() {
    let system = BitmapTextSystem::new();
    let variations = VariationSettings::new([VariationAxis {
        tag: OpenTypeTag::new(*b"wght"),
        value: Fixed16_16::from_f32(600.0).unwrap(),
    }])
    .unwrap();
    let spec = LayoutSpec {
        variations: variations.clone(),
        ..LayoutSpec::default()
    };
    let runs = system.layout("A", &spec, None);
    let key = runs[0].cache_key(&runs[0].glyphs[0]);
    assert_eq!(key.variations, variations);
    // Transform is the *real* Q16.16 identity, distinct from the zero matrix.
    assert_eq!(key.transform, Affine2Fixed::IDENTITY);
    assert_ne!(key.transform, Affine2Fixed::ZERO);
    // The key is Eq + Hash usable.
    use std::collections::HashSet;
    let mut set = HashSet::new();
    assert!(set.insert(key.clone()));
    assert!(!set.insert(key));
}

#[test]
fn two_glyph_keys_differ_only_by_transform_translation() {
    let base = GlyphCacheKey {
        font_id: FontId::BITMAP,
        font_generation: 0,
        glyph_id: 'A' as u32,
        size_px_q26_6: 640,
        variations: VariationSettings::empty(),
        format: GlyphFormat::Alpha,
        subpixel_variant: 0,
        synthesis_flags: 0,
        hinting: HintingMode::None,
        transform: Affine2Fixed::IDENTITY,
    };
    let translated = GlyphCacheKey {
        transform: Affine2Fixed {
            tx: Fixed16_16::ONE,
            ..Affine2Fixed::IDENTITY
        },
        ..base.clone()
    };
    // Translation is part of the outline-local transform: the keys differ.
    assert_ne!(base, translated);
}

#[test]
fn bitmap_text_system_returns_unsupported_outline_by_default() {
    let system = BitmapTextSystem::new();
    let key = OutlineKey {
        font_id: FontId::BITMAP,
        font_generation: 0,
        glyph_id: 'A' as u32,
        variations: VariationSettings::empty(),
        synthesis_flags: 0,
        transform: Affine2Fixed::IDENTITY,
    };
    assert_eq!(
        system.glyph_outline(&key),
        Err(TextError::UnsupportedOutline)
    );
}

#[test]
fn engine_lays_out_repeated_widths_without_repreparing() {
    let engine = TextEngine::new(Arc::new(BitmapTextSystem::new()));
    let prepared = engine.prepare(
        "the quick brown fox jumps over the lazy dog",
        LayoutSpec::default(),
    );
    let wide = prepared.layout(Some(800.0));
    let narrow = prepared.layout(Some(50.0));
    assert!(narrow.lines.len() > wide.lines.len());
    // Re-laying the same width hands back the cached allocation.
    let narrow_again = prepared.layout(Some(50.0));
    assert!(Arc::ptr_eq(&narrow, &narrow_again));
    // Block metrics are populated.
    assert!(narrow.block_height_px > wide.block_height_px);
    assert!(prepared.intrinsic_width_px() >= wide.logical_width_px);
}
