//! #67 CPU-side vertical slice: a decoded SFNT glyph flows through #62's
//! registry, #61's outline contract, and #65's Slug encoder + size-independent
//! blob cache — stopping cleanly at the GPU boundary (no wgpu).
//!
//! Proves the two cache-reuse invariants the issue calls out:
//! - the same glyph/key reuses one cached blob across 12/16/24/48 px,
//! - generation / variation / transform differences do **not** reuse it.

use std::sync::Arc;

use mkui_text::{CompositeTextSystem, LayoutSpec, TextSystem};
use mkui_vector2d::{
    glyph_outline_to_path, Affine2Fixed, Fixed16_16, FontId, OpenTypeTag, OutlineKey,
    SlugBlobCache, SlugConfig, SlugGlyphKey, TextRenderClass, VariationAxis, VariationSettings,
    VectorPath,
};

// The Abel fixture is owned by the `mkui-text` crate's test tree; this slice
// consumes it through `mkui-vector2d`'s public re-exports.
const ABEL: &[u8] = include_bytes!("../../mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

fn registered() -> (CompositeTextSystem, FontId, u32) {
    let mut sys = CompositeTextSystem::new();
    let id = sys
        .register_sfnt_face(Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
        .expect("Abel registers");
    let gid = sys.sfnt_face(id).unwrap().glyph_index('M').unwrap() as u32;
    (sys, id, gid)
}

fn config() -> SlugConfig {
    SlugConfig::new(8, 8, 1)
}

/// Lay out a single "M" at `px`, route its outline through the registry, and
/// return the (size-independent) Slug key + the encoded path.
fn slice_at(
    sys: &CompositeTextSystem,
    id: FontId,
    gid: u32,
    px: f32,
) -> (SlugGlyphKey, VectorPath) {
    let spec = LayoutSpec {
        font_id: id,
        font_size_px: px,
        ..Default::default()
    };
    let runs = sys.layout("M", &spec, None);
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.render_class, TextRenderClass::Slug);

    let okey = OutlineKey {
        font_id: run.font_id,
        font_generation: run.font_generation,
        glyph_id: gid,
        variations: run.variations.clone(),
        synthesis_flags: run.synthesis_flags,
        transform: Affine2Fixed::IDENTITY,
    };
    let outline = sys.glyph_outline(&okey).expect("routed outline");
    let path = glyph_outline_to_path(&outline);
    let key = SlugGlyphKey::from_run(run, gid, Affine2Fixed::IDENTITY);
    (key, path)
}

#[test]
fn glyph_m_encodes_to_a_nonempty_slug_blob() {
    let (sys, id, gid) = registered();
    let (key, path) = slice_at(&sys, id, gid, 24.0);
    let mut cache = SlugBlobCache::new(config());
    let blob = cache.encode(key, &path).expect("M encodes");
    assert!(!blob.curves.is_empty(), "M produces drawable curves");
    // Encoded bounds carry the decoded font-unit ink bounds through unchanged.
    assert_eq!(
        (
            blob.bounds.x_min,
            blob.bounds.y_min,
            blob.bounds.x_max,
            blob.bounds.y_max
        ),
        (164.0, 0.0, 1176.0, 1434.0)
    );
}

#[test]
fn same_glyph_reuses_one_blob_across_all_four_sizes() {
    let (sys, id, gid) = registered();
    let mut cache = SlugBlobCache::new(config());

    let mut first_blob = None;
    for px in [12.0, 16.0, 24.0, 48.0] {
        let (key, path) = slice_at(&sys, id, gid, px);
        // The key is identical at every size (size is not part of it).
        let blob = cache.encode(key, &path).unwrap();
        match &first_blob {
            None => first_blob = Some(blob),
            Some(f) => assert!(
                Arc::ptr_eq(f, &blob),
                "every size must reuse the one cached blob"
            ),
        }
    }

    // One miss (first encode), three hits (12/16/24/48 share the blob).
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.misses(), 1);
    assert_eq!(cache.hits(), 3);
}

#[test]
fn generation_variation_and_transform_differences_do_not_reuse() {
    let (sys, id, gid) = registered();
    let (base, path) = slice_at(&sys, id, gid, 24.0);
    let mut cache = SlugBlobCache::new(config());
    cache.encode(base.clone(), &path).unwrap();

    // Each variant differs from `base` in exactly one identity field; each must
    // be a fresh miss (no aliasing onto the base blob).
    let variants = [
        SlugGlyphKey {
            font_generation: 1,
            ..base.clone()
        },
        SlugGlyphKey {
            variation_axes: VariationSettings::new([VariationAxis {
                tag: OpenTypeTag::new(*b"wght"),
                value: Fixed16_16::from_f32(700.0).unwrap(),
            }])
            .unwrap(),
            ..base.clone()
        },
        SlugGlyphKey {
            outline_transform: Affine2Fixed {
                tx: Fixed16_16::from_f32(1.0).unwrap(),
                ..Affine2Fixed::IDENTITY
            },
            ..base.clone()
        },
    ];

    let mut expected = 1;
    for v in variants {
        cache.encode(v, &path).unwrap();
        expected += 1;
        assert_eq!(cache.len(), expected);
    }
    assert_eq!(cache.hits(), 0, "distinct identities never reuse a blob");
}
