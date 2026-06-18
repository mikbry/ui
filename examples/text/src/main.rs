//! Minimal Slug + bitmap text smoke binary (#67 examples/text).
//!
//! Phase 1 is CPU-only: it shapes and lays out an Abel Slug line beside a
//! bitmap label through the #62 composite registry and prints the resulting
//! `LayoutRun`s, so a Sprint 8+ regression in shaping/layout/routing is visible
//! without a GPU. `cargo run -p text` is bitmap-only; `--features slug` adds the
//! Slug lane + #65 encode. Phase 2 (post-#66) upgrades the Slug path to real
//! GPU rendering via `mkui-wgpu`.

use mkui_text::{CompositeTextSystem, FontId, LayoutSpec, TextSystem};

const ABEL: &[u8] =
    include_bytes!("../../../crates/mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

fn main() {
    let mut sys = CompositeTextSystem::new();

    // The bitmap label is always available (the compatibility-default lane).
    let bitmap_spec = LayoutSpec {
        font_id: FontId::BITMAP,
        font_size_px: 16.0,
        ..Default::default()
    };
    println!("== bitmap label (FontId::BITMAP) ==");
    for run in sys.layout("abel slug demo", &bitmap_spec, None) {
        println!("  {:?} x{} glyphs", run.render_class, run.glyphs.len());
    }

    run_slug(&mut sys);
}

#[cfg(feature = "slug")]
fn run_slug(sys: &mut CompositeTextSystem) {
    use mkui_vector2d::{
        glyph_outline_to_path, Affine2Fixed, OutlineKey, SlugBlobCache, SlugConfig, SlugGlyphKey,
    };

    let id = sys
        .register_sfnt_face(std::sync::Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
        .expect("Abel registers");
    let spec = LayoutSpec {
        font_id: id,
        font_size_px: 24.0,
        ..Default::default()
    };

    // "Mag" is fully supported by Abel; "Mag☃" forces a bitmap-fallback run.
    println!("== Abel Slug line + bitmap fallback (registry routing) ==");
    let runs = sys.layout("Mag☃", &spec, None);
    let mut cache = SlugBlobCache::new(SlugConfig::new(8, 8, 1));
    for run in &runs {
        println!(
            "  {:?} font_id={} x{} glyphs @origin_x={:.1}",
            run.render_class,
            run.font_id.raw(),
            run.glyphs.len(),
            run.origin_x_px
        );
        if run.render_class != mkui_text::TextRenderClass::Slug {
            continue;
        }
        for g in &run.glyphs {
            let key = OutlineKey {
                font_id: run.font_id,
                font_generation: run.font_generation,
                glyph_id: g.glyph_id,
                variations: run.variations.clone(),
                synthesis_flags: run.synthesis_flags,
                transform: Affine2Fixed::IDENTITY,
            };
            let path = glyph_outline_to_path(&sys.glyph_outline(&key).expect("outline"));
            let slug_key = SlugGlyphKey::from_run(run, g.glyph_id, Affine2Fixed::IDENTITY);
            match cache.encode(slug_key, &path) {
                Ok(blob) => println!(
                    "    glyph {} -> {} Slug curves",
                    g.glyph_id,
                    blob.curves.len()
                ),
                Err(e) => println!("    glyph {} -> (no curves: {e})", g.glyph_id),
            }
        }
    }
    println!(
        "  cache: {} blob(s), {} miss / {} hit",
        cache.len(),
        cache.misses(),
        cache.hits()
    );
}

#[cfg(not(feature = "slug"))]
fn run_slug(_sys: &mut CompositeTextSystem) {
    let _ = ABEL; // Slug lane is compiled out; bitmap-only.
    println!("== slug feature off: bitmap-only (build with --features slug) ==");
}
