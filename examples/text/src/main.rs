//! Minimal Slug + bitmap text smoke binary (#67).
//!
//! - `cargo run -p text` — Phase 1 CPU smoke: shapes a bitmap label + an Abel
//!   line through the #62 composite registry and prints the resulting
//!   `LayoutRun`s (shaping/layout/routing visible, no GPU). A feature-off build
//!   contains no Slug execution path and never pulls in wgpu.
//! - `cargo run -p text --features slug` — Phase 2 GPU smoke: decodes Abel,
//!   encodes its outlines through #65, and renders "Mag" through #66's Slug lane
//!   in a window beside a bitmap label.

use mkui_text::{CompositeTextSystem, FontId, LayoutSpec, TextSystem};

const ABEL: &[u8] =
    include_bytes!("../../../crates/mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "slug")]
    {
        gpu_demo()
    }
    #[cfg(not(feature = "slug"))]
    {
        cpu_smoke();
        Ok(())
    }
}

/// Phase 1 CPU smoke: layout through the composite registry, print the runs.
#[cfg(not(feature = "slug"))]
fn cpu_smoke() {
    let sys = CompositeTextSystem::new();
    let bitmap_spec = LayoutSpec {
        font_id: FontId::BITMAP,
        font_size_px: 16.0,
        ..Default::default()
    };
    println!("== bitmap label (FontId::BITMAP) ==");
    for run in sys.layout("abel slug demo", &bitmap_spec, None) {
        println!("  {:?} x{} glyphs", run.render_class, run.glyphs.len());
    }
    let _ = ABEL; // Slug lane is compiled out; bitmap-only.
    println!("== slug feature off: bitmap-only (build with --features slug) ==");
}

/// Phase 2 GPU smoke: build a Slug + bitmap scene and render it in a window.
#[cfg(feature = "slug")]
fn gpu_demo() -> Result<(), Box<dyn std::error::Error>> {
    let scene = build_scene();
    println!(
        "rendering {} scene primitive(s) (Abel Slug + bitmap) — close the window to exit",
        scene.primitives.len()
    );
    mkui_wgpu::Mkui::with_scene(scene).run()?;
    Ok(())
}

/// Compose one Abel "Mag" line on the Slug lane plus a bitmap label, in a
/// `Scene` the renderer draws in `Scene::primitives` paint order.
#[cfg(feature = "slug")]
fn build_scene() -> mkui_wgpu::Scene {
    use mkui_vector2d::{SlugBlobCache, SlugConfig};
    use mkui_wgpu::{
        place_slug_run, Color, FontFaceId, Point, Rect, Scene, Size, Text, TextAlign, TextStyle,
    };

    let mut sys = CompositeTextSystem::new();
    let id = sys
        .register_sfnt_face(std::sync::Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
        .expect("Abel registers");

    let spec = LayoutSpec {
        font_id: id,
        font_size_px: 48.0,
        ..Default::default()
    };
    let runs = sys.layout("Mag", &spec, None);

    let mut scene = Scene::new(Size::new(640.0, 360.0));
    let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
    // Place the Slug line with its baseline near the top of the window.
    for run in &runs {
        for glyph in place_slug_run(
            &sys,
            &mut cache,
            run,
            [48.0, 140.0],
            Color::rgb(0.2, 0.9, 0.5),
        ) {
            scene.slug_glyph(glyph);
        }
    }
    // A bitmap-class label below it (different lane, same scene).
    scene.text(Text {
        rect: Rect::new(Point::new(48.0, 200.0), Size::new(540.0, 28.0)),
        content: "Abel via Slug, label via bitmap".into(),
        style: TextStyle {
            font: FontFaceId(FontId::BITMAP.raw()),
            font_size_px: 16.0,
            line_height_px: 22.0,
            color: Color::rgb(0.85, 0.85, 0.85),
            align: TextAlign::Start,
        },
    });
    scene
}
