//! Cross-consumer integration test for `card` (#141, Sprint 8 plan v2 §4.3).
//!
//! Renders the triangulated and vector lanes of `components::card` through
//! #106's offscreen Lavapipe harness and reads back pixels, proving the two
//! lanes agree everywhere they should and differ exactly where #141 says
//! they should:
//!
//! - the vector lane's corners are genuinely rounded; the triangulated
//!   lane's are square, because `tessellate_quad` never reads
//!   `Quad::corner_radii` — the pre-#141 "rounded" card was in fact a
//!   square-corner approximation;
//! - the vector lane's border stays inset within the card's own rect, same
//!   as the triangulated lane's border rects — a regression Codex round-1
//!   review of #141 caught (a stroke centered on the fill's outer boundary
//!   painted half the border outside the rect) and that the original
//!   version of this test, using one color for fill+stroke on a
//!   full-viewport card, could not have caught.
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
const GREEN: [u8; 4] = [0, 255, 0, 255];
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

/// Fill (red), stroke (green), and background (blue) are three *distinct*
/// pure primary colors — `0.0`/`1.0` channels are fixed points of the
/// sRGB↔linear conversion (`srgb_to_linear(0.0) == 0.0`,
/// `srgb_to_linear(1.0) == 1.0`), so every readback byte is exactly `0` or
/// `255` regardless of which lane linearizes it, and no float-tolerance
/// window is needed on the assertions below.
///
/// Codex round-1 review of #141: the original version of this test used the
/// *same* color for fill and stroke, and a card sized to the whole
/// viewport — both choices hid the border-bleeds-outside-the-rect
/// regression the review caught (identical colors make the border
/// invisible against the fill; a full-viewport card leaves no background
/// margin to show the border painting past the rect). Distinct colors plus
/// [`test_rect`]'s margin make both the corner-rounding and the border-inset
/// behavior directly observable in the readback.
fn test_style() -> CardStyle {
    CardStyle {
        fill: Color::rgba(1.0, 0.0, 0.0, 1.0),
        stroke: Color::rgba(0.0, 1.0, 0.0, 1.0),
        stroke_width: 4.0,
        corner_radius: 8.0,
    }
}

/// A 32×32 card centered in the 64×64 target, leaving a 16px background
/// margin on every side — room enough to see the border if it bleeds past
/// the card's own bounds, and room enough for the radius-8 corners to clip a
/// multi-pixel region.
fn test_rect() -> Rect {
    Rect::new(Point::new(16.0, 16.0), Size::new(32.0, 32.0))
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
    assert_eq!(
        paths.len(),
        2,
        "card must emit a fill path and a separate inset-stroke border path"
    );
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
///
/// [`test_rect`] spans x/y `16..48`; [`test_style`]'s border is 4px wide, so
/// the border band is `16..20` (top/left) and `44..48` (bottom/right) on
/// each axis, and the radius-8 corners clip a multi-pixel region at each
/// corner. Every point below is checked on *both* lanes: the triangulated
/// lane (square corners, border rects fully inside the rect — see
/// `tessellate_quad`) is the independent reference the vector lane must
/// match everywhere except the corners themselves.
#[test]
fn card_vector_vs_triangulated() {
    let triangulated = render_triangulated();
    let vector = render_vector();

    // The exact corner pixel: squared (and border-colored, since both
    // adjacent border rects overlap there) in the triangulated lane;
    // clipped to the background in the vector lane (the rounded corner's
    // arc excludes it) — the precise corner-squareness bug #141 fixes.
    assert_eq!(
        texel(&triangulated, 16, 16),
        GREEN,
        "triangulated corner is square: the exact corner pixel must be border-colored"
    );
    assert_eq!(
        texel(&vector, 16, 16),
        BLUE,
        "vector corner is rounded: the exact corner pixel must show the background through it"
    );

    // Well inside the corner's radius (but off the exact corner pixel, and
    // inside the border's inner edge too): fill on both lanes.
    assert_eq!(texel(&triangulated, 23, 23), RED);
    assert_eq!(texel(&vector, 23, 23), RED);

    // Border-inset regression (Codex round-1 review of #141): a pixel just
    // *outside* the card's rect, on each of the four sides, must be
    // background on both lanes — the border must never paint past the
    // rect it was given. A stroke centered on the fill's own outer boundary
    // (the bug) would paint `stroke_width / 2` = 2px past this point.
    for (x, y, side) in [
        (32, 13, "top"),
        (32, 51, "bottom"),
        (13, 32, "left"),
        (51, 32, "right"),
    ] {
        assert_eq!(
            texel(&triangulated, x, y),
            BLUE,
            "triangulated {side}: 3px outside the rect must be background"
        );
        assert_eq!(
            texel(&vector, x, y),
            BLUE,
            "vector {side}: 3px outside the rect must be background — the border must not bleed past the card's bounds"
        );
    }

    // Inside the 4px border band, away from any corner: border color on both
    // lanes, proving the border is actually drawn (not shrunk away from the
    // edge either).
    for (x, y, side) in [
        (32, 18, "top"),
        (32, 46, "bottom"),
        (18, 32, "left"),
        (46, 32, "right"),
    ] {
        assert_eq!(
            texel(&triangulated, x, y),
            GREEN,
            "triangulated {side} border band must be border-colored"
        );
        assert_eq!(
            texel(&vector, x, y),
            GREEN,
            "vector {side} border band must be border-colored"
        );
    }

    // The center, well past the border band, is fill on both lanes.
    assert_eq!(texel(&triangulated, 32, 32), RED);
    assert_eq!(texel(&vector, 32, 32), RED);
}
