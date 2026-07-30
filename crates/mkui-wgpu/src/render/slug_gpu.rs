//! Offscreen GPU acceptance tests for the Slug lane (#66).
//!
//! These run through #106's surfaceless [`OffscreenRenderer`] (Vulkan/Lavapipe,
//! CPU adapter) and contain **no TTF/font parser** — every glyph is a
//! hand-authored curve/band record (font parsing + fixtures are #67). They
//! prove:
//!
//! - the Slug pipeline builds and rasterizes hand-authored records into an
//!   offscreen target with a calibrated nonzero changed-pixel count vs a
//!   clear-only baseline,
//! - overlapping UI and Slug commands composite in `Scene::primitives` order —
//!   the later command wins the overlap region, in both orders,
//! - the adapter runs on #106's asserted Vulkan + CPU contract,
//! - and frame timing is reported diagnostically (not a pass/fail gate).

use std::sync::Arc;
use std::time::Instant;

use mkui_vector2d::Vec2;
use mkui_vector2d_wgpu::{
    BandRange, GlyphBounds, PlacedSlugGlyph, SlugAdapter, SlugCurve, SlugGlyph,
};

use super::offscreen::{OffscreenRenderer, BYTES_PER_PIXEL};

const W: u32 = 64;
const H: u32 = 64;

/// Provision the #106 harness and assert the Vulkan + CPU adapter contract.
/// Adapter/device unavailability fails the test (no silent skip).
fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "slug offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "Slug GPU tests must select the Vulkan backend (#106 contract)"
    );
    assert_eq!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "Slug GPU tests must run on a CPU (Lavapipe) adapter (#106 contract)"
    );
    renderer
}

/// A hand-authored filled square glyph, font units y-up, bounds (0,0)-(100,100).
///
/// The four edges are stored as line records (duplicated-endpoint sentinel).
/// The two horizontal edges (bottom, top) are axis-parallel to a horizontal
/// scan ray, so they are excluded from the horizontal band; it lists only the
/// two vertical edges, exactly what the encoder would emit. Symmetrically,
/// the two vertical edges (right, left) are axis-parallel to a vertical scan
/// ray and excluded from the vertical band, which lists the two horizontal
/// edges (#157 Phase 1: both bands are real inputs to the dual-ray shader,
/// so a hand-authored fixture must supply both, matching what
/// `encode_slug_glyph` would produce for this shape). A ray cast through any
/// interior sample crosses both of its axis's bounding edges → coverage 1.
fn square_glyph() -> Arc<SlugGlyph> {
    let line = |p0: Vec2, p2: Vec2| SlugCurve { p0, p1: p2, p2 };
    Arc::new(SlugGlyph {
        revision: 1,
        bounds: GlyphBounds {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        },
        curves: vec![
            line(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)), // 0: bottom (horizontal)
            line(Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)), // 1: right (vertical)
            line(Vec2::new(100.0, 100.0), Vec2::new(0.0, 100.0)), // 2: top (horizontal)
            line(Vec2::new(0.0, 100.0), Vec2::new(0.0, 0.0)), // 3: left (vertical)
        ],
        // One full-height row listing the two vertical edges, descending max-x
        // (both endpoints at x=100 and x=0 respectively — curve 1 first).
        horizontal_bands: vec![BandRange {
            lower: 0.0,
            upper: 100.0,
            first_curve: 0,
            curve_count: 2,
        }],
        horizontal_curve_indices: vec![1, 3],
        // One full-width column listing the two horizontal edges, descending
        // max-y (curve 2 at y=100 before curve 0 at y=0).
        vertical_bands: vec![BandRange {
            lower: 0.0,
            upper: 100.0,
            first_curve: 0,
            curve_count: 2,
        }],
        vertical_curve_indices: vec![2, 0],
    })
}

/// Place the square so it lands at screen x∈[7,57], y∈[7,57] in the 64×64
/// target: scale 0.5 px/unit, pen origin at the glyph's bottom-left.
fn placed_square(color: [f32; 4]) -> PlacedSlugGlyph {
    PlacedSlugGlyph {
        blob: square_glyph(),
        origin_px: [7.0, 57.0],
        scale_px_per_unit: 0.5,
        color,
        // Hand-authored fixture, not text — opts out of the Phase 3
        // small-text baseline snap.
        cap_height_px: f32::INFINITY,
    }
}

/// Clear `renderer`'s target to `clear`, optionally draw a solid-red fullscreen
/// quad and the Slug glyph in the requested order, submit, and read back RGBA.
fn render_pixels(
    renderer: &OffscreenRenderer,
    adapter: &SlugAdapter,
    glyphs: &[PlacedSlugGlyph],
    draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
    clear: wgpu::Color,
) -> Vec<u8> {
    let prepared = adapter.prepare(
        renderer.device(),
        renderer.queue(),
        [W as f32, H as f32],
        1.0,
        glyphs,
    );
    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("slug test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("slug test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: renderer.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let draw_slug = |pass: &mut wgpu::RenderPass<'_>| {
            if let Some(prepared) = prepared.as_ref() {
                adapter.draw(pass, prepared);
            }
        };
        // The caller decides the draw order; `draw_slug` is the Slug command.
        draw(&mut pass);
        draw_slug(&mut pass);
    }
    renderer.queue().submit(Some(encoder.finish()));
    renderer.read_rgba().expect("readback must succeed")
}

/// Number of texels that differ between two equal-length RGBA byte buffers.
fn changed_pixels(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(BYTES_PER_PIXEL as usize)
        .zip(b.chunks_exact(BYTES_PER_PIXEL as usize))
        .filter(|(x, y)| x != y)
        .count()
}

/// Linear index of the first byte of texel (x, y).
fn texel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * BYTES_PER_PIXEL) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// A solid-red fullscreen-triangle pipeline standing in for the UI/bitmap lane.
fn solid_red_pipeline(renderer: &OffscreenRenderer) -> wgpu::RenderPipeline {
    let shader = renderer
        .device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slug test solid"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
                @vertex
                fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
                    var p = array<vec2<f32>, 3>(
                        vec2<f32>(-1.0, -1.0),
                        vec2<f32>( 3.0, -1.0),
                        vec2<f32>(-1.0,  3.0),
                    );
                    return vec4<f32>(p[vi], 0.0, 1.0);
                }
                @fragment
                fn fs() -> @location(0) vec4<f32> {
                    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
                }
                "#
                .into(),
            ),
        });
    let layout = renderer
        .device()
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("slug test solid layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
    renderer
        .device()
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("slug test solid pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: renderer.format(),
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
}

#[test]
fn slug_pipeline_renders_handauthored_glyph_with_calibrated_changed_pixels() {
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), renderer.format());

    // Baseline: clear to black, no glyph.
    let baseline = render_pixels(&renderer, &adapter, &[], |_| {}, wgpu::Color::BLACK);

    // Draw the hand-authored square in green.
    let start = Instant::now();
    let drawn = render_pixels(
        &renderer,
        &adapter,
        &[placed_square([0.0, 1.0, 0.0, 1.0])],
        |_| {},
        wgpu::Color::BLACK,
    );
    eprintln!(
        "slug frame timing (diagnostic, not a gate): {:?} for a {}x{} target",
        start.elapsed(),
        W,
        H
    );

    let changed = changed_pixels(&baseline, &drawn);
    // The square spans ~50×50 px; require a calibrated floor well below the
    // 2500-px ideal (AA-eroded edges + conservative margin) but far above zero.
    assert!(
        changed > 1500,
        "hand-authored Slug glyph must change a calibrated number of pixels; got {changed}"
    );
    assert!(
        changed < (W * H) as usize,
        "the glyph must not cover the entire target; got {changed}"
    );

    // A texel deep inside the square must be solidly green.
    assert_eq!(
        texel(&drawn, 32, 32),
        [0, 255, 0, 255],
        "interior of the Slug square must be fully covered green"
    );
    // A corner texel well outside the square must remain the black clear.
    assert_eq!(
        texel(&drawn, 1, 1),
        [0, 0, 0, 255],
        "outside the glyph quad must stay the clear color"
    );
}

#[test]
fn overlapping_ui_and_slug_respect_command_order_both_ways() {
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), renderer.format());
    let solid = solid_red_pipeline(&renderer);

    // Order A: UI (red, fills target) first, then the green Slug glyph → the
    // glyph wins its region.
    let ui_then_slug = render_pixels(
        &renderer,
        &adapter,
        &[placed_square([0.0, 1.0, 0.0, 1.0])],
        |pass| {
            pass.set_pipeline(&solid);
            pass.draw(0..3, 0..1);
        },
        wgpu::Color::BLACK,
    );

    // Order B: the green Slug glyph first, then UI red fills the target → the
    // UI fill wins the same region. `render_pixels` always draws the Slug
    // command last, so to put UI last we draw it *inside* a second pass run
    // with no Slug glyph and a red clear-equivalent fill ordering swap.
    let slug_then_ui = {
        let prepared = adapter
            .prepare(
                renderer.device(),
                renderer.queue(),
                [W as f32, H as f32],
                1.0,
                &[placed_square([0.0, 1.0, 0.0, 1.0])],
            )
            .expect("glyph prepares");
        let mut encoder =
            renderer
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("slug-then-ui encoder"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("slug-then-ui pass"),
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
            // Slug first, UI red second.
            adapter.draw(&mut pass, &prepared);
            pass.set_pipeline(&solid);
            pass.draw(0..3, 0..1);
        }
        renderer.queue().submit(Some(encoder.finish()));
        renderer.read_rgba().expect("readback")
    };

    // Inside the glyph (texel 32,32) the later command must win in each order.
    assert_eq!(
        texel(&ui_then_slug, 32, 32),
        [0, 255, 0, 255],
        "UI-then-Slug: the Slug glyph (drawn later) must win the overlap"
    );
    assert_eq!(
        texel(&slug_then_ui, 32, 32),
        [255, 0, 0, 255],
        "Slug-then-UI: the UI fill (drawn later) must win the overlap"
    );
    assert_ne!(
        texel(&ui_then_slug, 32, 32),
        texel(&slug_then_ui, 32, 32),
        "command order must change the overlap winner"
    );
}

#[test]
fn slug_adapter_builds_on_the_vulkan_cpu_contract() {
    // The harness asserts the #106 Vulkan + CPU contract; building the Slug
    // pipeline against that device proves the adapter provisions there.
    let renderer = harness();
    let _adapter = SlugAdapter::new(renderer.device(), renderer.format());
}

/// Render `placed_square` with `cap_height_px: 12.0` (below
/// `SMALL_TEXT_CAP_HEIGHT_PX`) at the given pen Y and `device_pixel_ratio`,
/// through the real `SlugAdapter::prepare`/`draw` path — no test-local math,
/// only the production `pack` codepath.
fn render_small_text_at(
    renderer: &OffscreenRenderer,
    adapter: &SlugAdapter,
    origin_y: f32,
    device_pixel_ratio: f32,
) -> Vec<u8> {
    let placed = PlacedSlugGlyph {
        blob: square_glyph(),
        origin_px: [7.0, origin_y],
        scale_px_per_unit: 0.5,
        color: [0.0, 1.0, 0.0, 1.0],
        cap_height_px: 12.0,
    };
    let prepared = adapter.prepare(
        renderer.device(),
        renderer.queue(),
        [W as f32, H as f32],
        device_pixel_ratio,
        &[placed],
    );
    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("small-text snap test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("small-text snap test pass"),
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

#[test]
fn small_text_snap_reaches_real_pixels_at_a_fractional_device_pixel_ratio() {
    // dame-rubric.md § Phase 3 (N), and Codex round 1 of the Phase 3 PR
    // review: the piecewise-constancy unit tests on `snap_to_physical_pixel`
    // and on `pack`'s `GpuGlyph` output prove the *math*; this proves the
    // snap actually reaches rendered pixels through the real
    // `SlugAdapter::prepare`/`draw` path, at a fractional DPR (1.5x) —
    // exactly the case Codex flagged (an earlier revision baked a stale DPR
    // into the baseline before the real per-frame value was known).
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), renderer.format());
    let device_pixel_ratio = 1.5f32;

    // At 1.5x DPR, snap_to_physical_pixel(v, 1.5) = round(v*1.5)/1.5:
    // 57.0 and 57.4 both round to physical row 86 (57.333...); 57.7 rounds to
    // physical row 87 (58.0) — a real cell boundary crossing.
    let same_cell_a = render_small_text_at(&renderer, &adapter, 57.0, device_pixel_ratio);
    let same_cell_b = render_small_text_at(&renderer, &adapter, 57.4, device_pixel_ratio);
    assert_eq!(
        same_cell_a, same_cell_b,
        "two pen positions in the same physical-pixel cell must render byte-identical \
         once the small-text snap is applied"
    );

    let other_cell = render_small_text_at(&renderer, &adapter, 57.7, device_pixel_ratio);
    assert_ne!(
        same_cell_a, other_cell,
        "crossing the physical-pixel cell boundary must change the render"
    );
}
