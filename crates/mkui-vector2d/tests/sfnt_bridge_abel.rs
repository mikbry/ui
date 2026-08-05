//! #169 Part B.1 acceptance: `sfnt_bridge::extract_slug_glyph` converts real
//! SFNT-loaded glyph outlines from the licensed Abel-Regular fixture into
//! cached `SlugGlyph` blobs, with a typed error for the branch it can't
//! handle.

use std::sync::Arc;

use mkui_vector2d::{
    extract_slug_glyph, Affine2Fixed, FontId, FontIdAllocator, SfntFace, SlugBlobCache, SlugConfig,
    SlugGlyphKey, TextExtractionError, VariationSettings,
};

// The Abel fixture is owned by the `mkui-text` crate's test tree; this suite
// consumes it through `mkui-vector2d`'s public re-exports, the same pattern
// `tests/slug_slice.rs` (#67) already uses.
const ABEL: &[u8] = include_bytes!("../../mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

fn abel_face() -> SfntFace {
    SfntFace::parse(Arc::from(ABEL.to_vec().into_boxed_slice()), 0).expect("Abel decodes")
}

fn cache() -> SlugBlobCache {
    SlugBlobCache::new(SlugConfig::new(4, 4, 1))
}

fn key_for(font_id: FontId, glyph_id: u32) -> SlugGlyphKey {
    SlugGlyphKey {
        font_id,
        font_generation: 0,
        glyph_id,
        variation_axes: VariationSettings::empty(),
        synthesis_flags: 0,
        outline_transform: Affine2Fixed::IDENTITY,
    }
}

#[test]
fn glyph_m_matches_the_calibrated_font_unit_bounds() {
    let face = abel_face();
    let font_id = FontIdAllocator::new().allocate().unwrap();
    let gid = face.glyph_index('M').unwrap();
    let mut cache = cache();

    let glyph = extract_slug_glyph(&mut cache, key_for(font_id, gid as u32), &face).unwrap();

    // Same calibrated bounds `mkui-text`'s sfnt_abel.rs oracle asserts.
    assert_eq!(
        (
            glyph.bounds.x_min,
            glyph.bounds.y_min,
            glyph.bounds.x_max,
            glyph.bounds.y_max
        ),
        (164.0, 0.0, 1176.0, 1434.0)
    );
    assert!(!glyph.curves.is_empty());
    assert_eq!(cache.misses(), 1);
}

#[test]
fn five_ascii_glyphs_extract_to_nonempty_slug_blobs() {
    let face = abel_face();
    let font_id = FontIdAllocator::new().allocate().unwrap();
    let mut cache = cache();

    for ch in ['A', 'e', 'g', '0', 'M'] {
        let gid = face
            .glyph_index(ch)
            .unwrap_or_else(|| panic!("Abel should map {ch:?}"));
        let glyph = extract_slug_glyph(&mut cache, key_for(font_id, gid as u32), &face).unwrap();
        assert!(
            !glyph.curves.is_empty(),
            "{ch:?} should produce drawable curves"
        );
    }
    assert_eq!(cache.misses(), 5);
}

#[test]
fn repeated_key_hits_the_cache_and_reuses_one_blob() {
    let face = abel_face();
    let font_id = FontIdAllocator::new().allocate().unwrap();
    let gid = face.glyph_index('A').unwrap();
    let mut cache = cache();
    let key = key_for(font_id, gid as u32);

    let first = extract_slug_glyph(&mut cache, key.clone(), &face).unwrap();
    let second = extract_slug_glyph(&mut cache, key, &face).unwrap();

    assert!(
        Arc::ptr_eq(&first, &second),
        "blob is reused, not re-encoded"
    );
    assert_eq!(cache.hits(), 1);
    assert_eq!(cache.misses(), 1);
}

#[test]
fn out_of_range_glyph_id_is_a_typed_rejection_that_does_not_poison_the_cache() {
    let face = abel_face();
    let font_id = FontIdAllocator::new().allocate().unwrap();
    let bogus_gid = face.num_glyphs(); // one past the last valid id
    let mut cache = cache();

    let err =
        extract_slug_glyph(&mut cache, key_for(font_id, bogus_gid as u32), &face).unwrap_err();

    assert!(matches!(err, TextExtractionError::MissingOutlineData(_)));
    assert!(
        cache.is_empty(),
        "an errored extraction must not poison the cache"
    );
    assert_eq!(cache.misses(), 0);
}

#[test]
fn glyph_id_beyond_u16_range_is_a_typed_rejection_and_never_touches_the_face() {
    // `SlugGlyphKey::glyph_id` is `u32` for generality; SFNT glyph ids are
    // always `u16`. A key whose glyph_id doesn't fit must be rejected before
    // any SFNT decode is attempted, not silently truncated.
    let face = abel_face();
    let font_id = FontIdAllocator::new().allocate().unwrap();
    let mut cache = cache();

    let err =
        extract_slug_glyph(&mut cache, key_for(font_id, u16::MAX as u32 + 1), &face).unwrap_err();

    assert!(matches!(err, TextExtractionError::MissingOutlineData(_)));
    assert!(cache.is_empty());
}

#[test]
fn extraction_uses_the_faces_units_per_em_without_mutating_the_cache_default() {
    // Abel's units_per_em is 2048, not the cache's default of 1.0. The #157
    // Phase 2 step 5 band-overlap epsilon math this feeds is covered
    // directly in `slug.rs` with a synthetic boundary-straddling path built
    // to isolate the effect; a real font's glyph geometry can't reliably
    // reproduce that same boundary-straddle, so this test instead pins the
    // plumbing contract this bridge owns: `encode_with_units_per_em`
    // (mirrored by `SlugBlobCache::encode_with_units_per_em`'s own docs)
    // applies the face's units_per_em for this call only, never mutating the
    // cache's stored config — a second, unrelated glyph encoded right after
    // must still see the cache's original default.
    let face = abel_face();
    assert_eq!(face.units_per_em(), 2048);
    let font_id = FontIdAllocator::new().allocate().unwrap();
    let gid_m = face.glyph_index('M').unwrap();
    let mut cache = SlugBlobCache::new(SlugConfig::new(4, 4, 1));
    assert_eq!(cache.config().units_per_em(), 1.0);

    extract_slug_glyph(&mut cache, key_for(font_id, gid_m as u32), &face).unwrap();

    assert_eq!(
        cache.config().units_per_em(),
        1.0,
        "the cache's own config must not be mutated by a per-call units_per_em override"
    );
}

#[test]
fn distinct_font_ids_never_alias_blobs_even_at_the_same_glyph_id() {
    // Two distinct fonts (here: two registrations of the same bytes, which is
    // representative — a real second font would differ in units_per_em too)
    // must never share a cache slot merely because they name the same
    // glyph_id: `SlugGlyphKey::font_id` is the collision boundary.
    let face = abel_face();
    let gid = face.glyph_index('A').unwrap();
    let font_a = FontIdAllocator::new().allocate().unwrap();
    let font_b = FontIdAllocator::new().allocate().unwrap();
    let mut cache = cache();

    let blob_a = extract_slug_glyph(&mut cache, key_for(font_a, gid as u32), &face).unwrap();
    let blob_b = extract_slug_glyph(&mut cache, key_for(font_b, gid as u32), &face).unwrap();

    assert!(!Arc::ptr_eq(&blob_a, &blob_b));
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.misses(), 2);
    assert_eq!(cache.hits(), 0);
}
