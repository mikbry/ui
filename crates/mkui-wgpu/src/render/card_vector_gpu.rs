//! Cross-consumer integration test for `card` (#141, Sprint 8 plan v2 §4.3).
//!
//! Renders the triangulated and vector lanes of `components::card` through
//! #106's offscreen Lavapipe harness and reads back pixels, proving the two
//! lanes differ exactly where #141 says they should: the vector lane's
//! corners are genuinely rounded; the triangulated lane's are square, because
//! `tessellate_quad` never reads `Quad::corner_radii` — the pre-#141 "rounded"
//! card was in fact a square-corner approximation.
//!
//! `card()` itself is a compile-time branch on the `slug` feature, so with
//! `slug` on (required here for the `SlugAdapter`) the public entry point
//! only ever reaches the vector lane. The triangulated reference below is
//! therefore the same `Quad` push `components::card`'s private
//! `card_triangulated` does, inlined here rather than exposed across module
//! boundaries just for this test — not a second implementation, the same one
//! copied so both lanes can render in a single binary.
//!
//! Dual-gated like [`super::linear_blend_gpu`] (`gpu-tests`) and
//! [`super::slug_gpu`] (`slug`).

use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

use super::offscreen::OffscreenRenderer;
use super::{
    build_ui_pipeline, gui_vertices, scene_vector_paths, vector_path_paints, INTERMEDIATE_FORMAT,
};
use crate::theme::CardStyle;
use crate::{components, Color, CornerRadii, Point, Quad, Rect, Scene, Size, Stroke};
use mkui_vector2d_wgpu::SlugAdapter;

const W: u32 = 64;
const H: u32 = 64;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "card-vector offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "card-vector GPU test must select the Vulkan backend (#106 contract)"
    );
    assert_eq!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "card-vector GPU test must run on a CPU (Lavapipe) adapter (#106 contract)"
    );
    // The harness target is the same linear format the windowed UI pipeline
    // composites into, so a byte readback here means what it would downstream.
    assert_eq!(renderer.format(), INTERMEDIATE_FORMAT);
    renderer
}

/// Fill and stroke the identical pure-red color: `0.0`/`1.0` channels are
/// fixed points of the sRGB↔linear conversion (`srgb_to_linear(0.0) == 0.0`,
/// `srgb_to_linear(1.0) == 1.0`), so every readback byte is exactly `0` or
/// `255` regardless of which lane linearizes it — no stroke-vs-fill color
/// split to reason about, and no float-tolerance window needed on the
/// assertions below. `corner_radius: 20.0` on a 64×64 card is Sprint 8's
/// "large radius" characteristic for this component (plan v2 §2.5).
fn test_style() -> CardStyle {
    CardStyle {
        fill: Color::rgba(1.0, 0.0, 0.0, 1.0),
        stroke: Color::rgba(1.0, 0.0, 0.0, 1.0),
        stroke_width: 2.0,
        corner_radius: 20.0,
    }
}

fn test_rect() -> Rect {
    Rect::new(Point::new(0.0, 0.0), Size::new(W as f32, H as f32))
}

fn render_to_offscreen(
    draw: impl FnOnce(&OffscreenRenderer, &mut wgpu::CommandEncoder),
) -> Vec<u8> {
    let renderer = harness();
    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("card-vector-gpu test encoder"),
        });
    draw(&renderer, &mut encoder);
    renderer.queue().submit(Some(encoder.finish()));
    renderer
        .read_rgba()
        .expect("offscreen readback must succeed")
}

/// Render through the real UI triangle pipeline — the pre-#141 behavior and
/// the feature-off behavior today. Mirrors `components::card::card_triangulated`'s
/// `Quad` push by hand (see module docs for why this can't call that function
/// directly from here).
fn render_triangulated() -> Vec<u8> {
    let style = test_style();
    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    scene.quad(Quad {
        rect: test_rect(),
        fill: style.fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.stroke,
            width: style.stroke_width,
        }),
    });
    let triangles = crate::tessellation::tessellate_scene(&scene);
    let vertices = gui_vertices(&triangles, W as f32, H as f32);

    render_to_offscreen(|renderer, encoder| {
        let pipeline = build_ui_pipeline(renderer.device(), INTERMEDIATE_FORMAT, 1);
        let buffer = renderer
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("card-triangulated test vertices"),
                contents: cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("card-triangulated test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: renderer.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    })
}

/// Render through the real `components::card` public entry point + the real
/// `SlugAdapter` — with `slug` enabled this is exactly what a consumer's
/// frame draws today.
fn render_vector() -> Vec<u8> {
    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    components::card(&mut scene, test_rect(), test_style());
    let paths = scene_vector_paths(&scene.primitives);
    assert_eq!(paths.len(), 1, "card must emit exactly one vector path");
    let paints = vector_path_paints(&paths);

    render_to_offscreen(|renderer, encoder| {
        let adapter = SlugAdapter::new(renderer.device(), renderer.format());
        let prepared = adapter
            .prepare_path_paints(
                renderer.device(),
                renderer.queue(),
                [W as f32, H as f32],
                1.0,
                &paints,
            )
            .expect("card's fill + stroke must produce at least one paint");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("card-vector test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: renderer.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        adapter.draw(&mut pass, &prepared);
    })
}

fn texel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * W + x) * 4) as usize;
    [
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    ]
}

/// `cargo test -p mkui-wgpu --features 'gpu-tests slug' card_vector_vs_triangulated`
/// — the exact name Sprint 8 plan v2 §4.3 and issue #141's acceptance
/// criteria name.
#[test]
fn card_vector_vs_triangulated() {
    let triangulated = render_triangulated();
    let vector = render_vector();

    // The exact corner pixel: squared in the triangulated lane
    // (`tessellate_quad` ignores `corner_radii`), clipped to the background
    // in the vector lane (the rounded corner's arc excludes it) — the
    // precise bug #141 fixes.
    assert_eq!(
        texel(&triangulated, 0, 0),
        RED,
        "triangulated corner is square: the exact corner pixel must be filled"
    );
    assert_eq!(
        texel(&vector, 0, 0),
        BLUE,
        "vector corner is rounded: the exact corner pixel must show the background through it"
    );

    // A point inside the corner's radius (but off the exact corner pixel) is
    // filled on both lanes — the rounding clips only the true corner, not the
    // whole corner box.
    assert_eq!(texel(&triangulated, 18, 18), RED);
    assert_eq!(texel(&vector, 18, 18), RED);

    // The center stays filled on both lanes regardless of corner handling.
    assert_eq!(texel(&triangulated, W / 2, H / 2), RED);
    assert_eq!(texel(&vector, W / 2, H / 2), RED);
}
