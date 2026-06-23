//! Font-backed Slug GPU acceptance tests (#67 Phase 2).
//!
//! Unlike #66's [`slug_gpu`](super::slug_gpu) tests (hand-authored curve/band
//! records), these decode the **licensed Abel fixture** through the narrow
//! `SfntTextSystem`, encode the outline through #65's Slug encoder, place it via
//! [`place_slug_run`](crate::slug_text::place_slug_run), and render it through
//! the real #66 GPU lane on #106's surfaceless Vulkan/Lavapipe harness. They
//! prove the whole vertical slice end-to-end:
//!
//! - glyph `M` renders through the real Slug lane at 12/16/24/48 px and changes
//!   at least the calibrated number of pixels inside its outward-rounded ink
//!   rectangle (the Tier-2 baseline-diff threshold, computed from decoded
//!   bounds — same formula proven feasible in the CPU calibration test), and
//! - a real SFNT-Slug run and a real bitmap-fallback run compose, in scene
//!   paint order, through the renderer's ordered command stream — exercising
//!   #62's mixed-fallback routing with real font data and a real GPU draw.

use std::sync::Arc;

use mkui_text::{CompositeTextSystem, FontId, LayoutSpec, TextRenderClass, TextSystem};
use mkui_vector2d::{SlugBlobCache, SlugConfig};
use mkui_vector2d_wgpu::{PlacedSlugGlyph, SlugAdapter};

use super::offscreen::{OffscreenRenderer, BYTES_PER_PIXEL};
use crate::render_command::{build_render_commands, classify_primitive, RenderCommand};
use crate::slug_text::place_slug_run;
use crate::types::{Color, FontFaceId, Point, Rect, Scene, Size, Text, TextAlign, TextStyle};

const ABEL: &[u8] = include_bytes!("../../../mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

const W: u32 = 96;
const H: u32 = 96;

/// Provision the #106 harness and assert the Vulkan + CPU adapter contract
/// (no silent skip — adapter/device unavailability fails loudly).
fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "font-backed Slug GPU tests must select the Vulkan backend (#106 contract)"
    );
    assert_eq!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "font-backed Slug GPU tests must run on a CPU (Lavapipe) adapter (#106 contract)"
    );
    renderer
}

/// Register the Abel SFNT face as a Slug provider beside the built-in bitmap
/// face, returning the composite + the minted `FontId`.
fn registered() -> (CompositeTextSystem, FontId) {
    let mut sys = CompositeTextSystem::new();
    let id = sys
        .register_sfnt_face(Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
        .expect("Abel registers");
    (sys, id)
}

/// Clear to black, draw `glyphs` through the Slug adapter, and read back RGBA.
fn render_slug(
    renderer: &OffscreenRenderer,
    adapter: &SlugAdapter,
    glyphs: &[PlacedSlugGlyph],
) -> Vec<u8> {
    let prepared = adapter.prepare(
        renderer.device(),
        renderer.queue(),
        [W as f32, H as f32],
        glyphs,
    );
    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("sfnt slug test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sfnt slug test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: renderer.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(prepared) = prepared.as_ref() {
            adapter.draw(&mut pass, prepared);
        }
    }
    renderer.queue().submit(Some(encoder.finish()));
    renderer.read_rgba().expect("readback must succeed")
}

/// Count texels that differ between `baseline` and `drawn` **inside** the
/// half-open screen rectangle `[x0, x1) × [y0, y1)`, clamped to the target.
fn changed_in_rect(baseline: &[u8], drawn: &[u8], rect: (i64, i64, i64, i64)) -> usize {
    let (x0, x1, y0, y1) = rect;
    let x0 = x0.max(0) as u32;
    let y0 = y0.max(0) as u32;
    let x1 = x1.clamp(0, W as i64) as u32;
    let y1 = y1.clamp(0, H as i64) as u32;
    let bpp = BYTES_PER_PIXEL as usize;
    let mut changed = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * W + x) as usize) * bpp;
            if baseline[i..i + bpp] != drawn[i..i + bpp] {
                changed += 1;
            }
        }
    }
    changed
}

#[test]
fn font_backed_glyph_m_meets_calibrated_threshold_at_all_sizes() {
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), renderer.format());
    let (sys, id) = registered();

    let face = sys.sfnt_face(id).unwrap();
    let gid = face.glyph_index('M').unwrap();
    let ink = face.glyph_outline(gid).unwrap().ink_bounds;
    let ink_area = (ink.max_x - ink.min_x) as f64 * (ink.max_y - ink.min_y) as f64;

    // Box top near the origin: `place_slug_run` puts the baseline an ascent
    // below this, so a small top margin keeps even the 48 px glyph on-target.
    // Integer placement so the outward-rounded rectangle is unambiguous.
    let box_origin = [8.0f32, 8.0f32];

    // Clear-only baseline frame (no glyphs).
    let baseline = render_slug(&renderer, &adapter, &[]);

    for px in [12.0f32, 16.0, 24.0, 48.0] {
        let spec = LayoutSpec {
            font_id: id,
            font_size_px: px,
            ..Default::default()
        };
        let runs = sys.layout("M", &spec, None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].render_class, TextRenderClass::Slug);

        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let glyphs = place_slug_run(
            &sys,
            &mut cache,
            &runs[0],
            box_origin,
            Color::rgb(0.0, 1.0, 0.0),
        );
        assert_eq!(
            glyphs.len(),
            1,
            "M is a single drawable Slug glyph at {px}px"
        );

        let drawn = render_slug(&renderer, &adapter, &glyphs);

        // Outward-rounded ink rectangle in screen space. Derived from the glyph's
        // ACTUAL placed pen origin (`origin_px`) + per-unit scale, so the rect
        // tracks exactly where the adapter drew it (y-down: font y_max maps to
        // the smaller/top screen y).
        let placed = &glyphs[0];
        let scale = placed.scale_px_per_unit as f64;
        let pen_x = placed.origin_px[0] as f64;
        let baseline_y = placed.origin_px[1] as f64;
        let x0 = (pen_x + ink.min_x as f64 * scale).floor() as i64;
        let x1 = (pen_x + ink.max_x as f64 * scale).ceil() as i64;
        let y_top = (baseline_y - ink.max_y as f64 * scale).floor() as i64;
        let y_bot = (baseline_y - ink.min_y as f64 * scale).ceil() as i64;

        let changed = changed_in_rect(&baseline, &drawn, (x0, x1, y_top, y_bot));

        // Tier-2 baseline-diff threshold from decoded bounds.
        let scaled_ink_area = ink_area * scale * scale;
        let threshold = scaled_ink_area
            .min((10.0f64).max(scaled_ink_area * 0.10))
            .ceil() as usize;

        assert!(
            changed >= threshold,
            "M at {px}px: {changed} changed pixels inside the ink rect < calibrated threshold {threshold}"
        );
        // The glyph must not flood the whole target — it is one letter.
        assert!(
            changed < (W * H) as usize,
            "M at {px}px must not cover the entire target ({changed} px)"
        );
    }
}

#[test]
fn cross_provider_slug_and_bitmap_fallback_compose_in_order() {
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), renderer.format());
    let (sys, id) = registered();

    // "M中": M maps to Abel (Slug), 中 is unmapped → bitmap fallback. Real
    // registry routing (#62 ValidatedFallback) splits this into two runs.
    let spec = LayoutSpec {
        font_id: id,
        font_size_px: 32.0,
        ..Default::default()
    };
    let runs = sys.layout("M中", &spec, None);
    assert_eq!(runs.len(), 2, "mixed text splits into Slug + bitmap runs");
    assert_eq!(runs[0].render_class, TextRenderClass::Slug);
    assert_eq!(runs[0].font_id, id);
    assert_eq!(runs[1].render_class, TextRenderClass::Bitmap);
    assert_eq!(runs[1].font_id, FontId::BITMAP);
    // The two lanes are laid out side by side: the bitmap run starts to the
    // right of the Slug glyph's advance.
    assert!(
        runs[1].origin_x_px > runs[0].origin_x_px,
        "bitmap fallback run must sit to the right of the Slug run"
    );

    // Build a real Scene with both lanes in paint order: the Slug glyph (M) and
    // a bitmap text primitive (中) at the fallback run's position.
    let box_origin = [8.0f32, 70.0f32];
    let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
    let slug_glyphs = place_slug_run(
        &sys,
        &mut cache,
        &runs[0],
        box_origin,
        Color::rgb(0.0, 1.0, 0.0),
    );
    assert_eq!(slug_glyphs.len(), 1);

    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    for glyph in &slug_glyphs {
        scene.slug_glyph(glyph.clone());
    }
    scene.text(Text {
        rect: Rect::new(
            Point::new(box_origin[0] + runs[1].origin_x_px, box_origin[1] - 20.0),
            Size::new(24.0, 24.0),
        ),
        content: "中".into(),
        style: TextStyle {
            font: FontFaceId(FontId::BITMAP.raw()),
            font_size_px: 32.0,
            line_height_px: 36.0,
            color: Color::rgb(1.0, 1.0, 1.0),
            align: TextAlign::Start,
        },
    });

    // The renderer's real ordered command stream must keep both lanes, in scene
    // order: Slug glyphs first, then bitmap text — never coalesced across lanes.
    let commands = build_render_commands(&scene.primitives, classify_primitive);
    assert_eq!(commands.len(), 2, "two lanes => two ordered commands");
    assert!(
        matches!(commands[0], RenderCommand::SlugGlyphs(_)),
        "first command is the Slug lane (paint order preserved)"
    );
    assert!(
        matches!(commands[1], RenderCommand::BitmapText(_)),
        "second command is the bitmap lane (paint order preserved)"
    );

    // Drive the Slug lane through the renderer's real collection seam
    // (`scene_slug_glyphs`) + the GPU adapter, and confirm M draws pixels.
    let baseline = render_slug(&renderer, &adapter, &[]);
    let mut drawn = baseline.clone();
    for command in &commands {
        if let RenderCommand::SlugGlyphs(range) = command {
            let collected = super::scene_slug_glyphs(&scene.primitives[range.clone()]);
            assert_eq!(
                collected, slug_glyphs,
                "scene_slug_glyphs round-trips the placed glyphs"
            );
            drawn = render_slug(&renderer, &adapter, &collected);
        }
    }
    let changed = changed_in_rect(&drawn, &baseline, (0, W as i64, 0, H as i64));
    assert!(
        changed > 0,
        "the SFNT-backed Slug glyph must draw visible pixels alongside the bitmap lane"
    );
}
