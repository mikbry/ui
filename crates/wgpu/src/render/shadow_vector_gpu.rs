//! Cross-consumer GPU acceptance test for `Shadow` (#279, extending the
//! #140/#141/#142 `VectorPath` mechanism to the shadow primitive).
//!
//! Renders `panel()`'s real shadow chrome on both lanes through #106's
//! offscreen Lavapipe harness and reads back pixels, proving the vector
//! lane's shadow corner is genuinely rounded — matching the paired `Quad`'s
//! own rounded corner (#219/#212's bug class) — while the triangulated
//! lane's `tessellate_shadow` stays the unfixed, `corner_radii`-blind square
//! it always was (#279's scope explicitly leaves that lane alone).
//!
//! `panel()`'s shadow fork is resolved at **compile time** by the `slug`
//! feature (see `components::shadow::emit_shadow`), so a single test
//! binary — compiled with `slug` on, as every test in this module is — can
//! only ever observe the vector shadow from a real `panel()` call. The
//! triangulated reference is therefore the same `Shadow` push
//! `components::shadow::emit_shadow`'s off-`slug` arm would make, rebuilt
//! here from `panel()`'s own field mapping, rather than obtained from a
//! second call to the real function — the same approach
//! [`super::button_vector_gpu`] and [`super::card_vector_gpu`] take for
//! their own compiled-out triangulated lane.
//!
//! Both probe points below sit outside `panel()`'s own fill/stroke `Quad`
//! (which this test does not render at all — it is never feature-forked and
//! so is identical on both lanes, irrelevant to the comparison this test
//! makes), so only the shadow's own geometry is ever on screen at the
//! probed pixels.
//!
//! Dual-gated like [`super::card_vector_gpu`] (`gpu-tests` + `slug`).

use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

use super::offscreen::OffscreenRenderer;
use super::{
    build_ui_pipeline, gui_vertices, scene_vector_paths, vector_path_paints, INTERMEDIATE_FORMAT,
};
use crate::components::panel;
use crate::tessellation::tessellate_scene;
use crate::theme::{PanelStyle, ShadowStyle};
use crate::types::{Color, CornerRadii, Insets, Point, Rect, Scene, Shadow, Size, Stroke};
use mkui_vector2d_wgpu::SlugAdapter;

const W: u32 = 64;
const H: u32 = 64;

fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "shadow-vector offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "shadow-vector GPU test must select the Vulkan backend (#106 contract)"
    );
    assert_eq!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "shadow-vector GPU test must run on a CPU (Lavapipe) adapter (#106 contract)"
    );
    assert_eq!(renderer.format(), INTERMEDIATE_FORMAT);
    renderer
}

/// A 32×32 panel centered in the 64×64 target, leaving a 16px margin on
/// every side — room for the shadow's own expansion (`spread` below) to
/// stay fully on-target.
fn test_rect() -> Rect {
    Rect::new(Point::new(16.0, 16.0), Size::new(32.0, 32.0))
}

/// `spread: 4.0`, `blur_radius: 0.0` — a pure spread expansion, so
/// `emit_shadow`'s `expand_amount` is exactly `4.0` with no blur fudge-factor
/// term to reason about. `corner_radius: 8.0` is well inside the rect's own
/// `half-shorter-side` clamp (16.0), so it survives `clamp_corner_radii`
/// untouched.
fn test_style() -> PanelStyle {
    PanelStyle {
        fill: Color::rgba(0.0, 0.0, 1.0, 1.0),
        stroke: Stroke {
            color: Color::rgba(0.0, 1.0, 0.0, 1.0),
            width: 2.0,
        },
        shadow: ShadowStyle {
            color: Color::rgba(1.0, 0.0, 0.0, 1.0),
            blur_radius: 0.0,
            spread: 4.0,
        },
        corner_radius: 8.0,
        padding: Insets::all(4.0),
    }
}

fn render_to_offscreen(
    draw: impl FnOnce(&OffscreenRenderer, &mut wgpu::CommandEncoder),
) -> Vec<u8> {
    let renderer = harness();
    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("shadow-vector-gpu test encoder"),
        });
    draw(&renderer, &mut encoder);
    renderer.queue().submit(Some(encoder.finish()));
    renderer
        .read_rgba()
        .expect("offscreen readback must succeed")
}

/// The triangulated reference: exactly the `Shadow` push
/// `components::shadow::emit_shadow`'s off-`slug` arm makes from `panel()`'s
/// own field mapping (see the module doc for why this is rebuilt by hand
/// rather than called — this binary only ever compiles the vector arm).
fn render_triangulated(rect: Rect, style: &PanelStyle) -> Vec<u8> {
    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    scene.shadow(Shadow {
        rect,
        blur_radius: style.shadow.blur_radius,
        spread: style.shadow.spread,
        color: style.shadow.color,
        corner_radii: CornerRadii::all(style.corner_radius),
    });
    let triangles = tessellate_scene(&scene);
    let vertices = gui_vertices(&triangles, W as f32, H as f32);

    render_to_offscreen(|renderer, encoder| {
        let pipeline =
            build_ui_pipeline(renderer.device(), INTERMEDIATE_FORMAT, 1, "fs_ui_triangles");
        let buffer = renderer
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shadow-triangulated test vertices"),
                contents: cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow-triangulated test pass"),
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
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    })
}

/// The vector actual: the real production `panel()` call (this binary is
/// compiled with `slug` on, so its shadow fork emits a `Primitive::VectorPath`)
/// — only the vector paths are drawn; `panel()`'s own fill/stroke `Quad`
/// (never feature-forked, identical on both lanes) is deliberately left out,
/// since neither probe point below falls inside it.
fn render_vector(rect: Rect, style: &PanelStyle) -> Vec<u8> {
    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    panel(&mut scene, rect, *style);
    let paths = scene_vector_paths(&scene.primitives);
    assert_eq!(
        paths.len(),
        1,
        "panel's shadow must emit exactly one vector path"
    );
    let paints = vector_path_paints(&paths);

    render_to_offscreen(|renderer, encoder| {
        let adapter = SlugAdapter::new(renderer.device(), renderer.format(), 1);
        let prepared = adapter
            .prepare_path_paints(
                renderer.device(),
                renderer.queue(),
                [W as f32, H as f32],
                1.0,
                &paints,
            )
            .expect("the shadow's vector path must produce drawable geometry");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow-vector test pass"),
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

/// True once a channel has visibly moved off the opaque-black clear —
/// tolerant of the shadow's own `multiply_alpha(0.6)` blend rather than
/// requiring an exact color match.
fn is_drawn(px: [u8; 4]) -> bool {
    px[0] > 2 || px[1] > 2 || px[2] > 2
}

/// `test_rect()` spans `16..48`; `test_style()`'s shadow expands that by
/// `spread: 4.0` to `12..52`, with its radius-8 corner grown to `8 + 4 = 12`
/// (`emit_shadow`'s own concentric-offset contract — see that function's
/// doc). The exact corner pixel of the *expanded* square, `(12, 12)`, sits
/// `sqrt(12² + 12²) ≈ 16.97`px from that corner's arc center (`24, 24`) —
/// well past the grown radius of `12` — so the vector lane's rounded corner
/// must exclude it, while the triangulated lane (still `corner_radii`-blind,
/// #279's scope) paints its full square there regardless.
#[test]
fn shadow_vector_vs_triangulated_corner() {
    let rect = test_rect();
    let style = test_style();

    let triangulated = render_triangulated(rect, &style);
    let vector = render_vector(rect, &style);

    // Sanity: a straight-edge point in the shadow's halo — outside the
    // panel's own `16..48` rect, but well inside the expanded `12..52`
    // square, away from any corner — must be painted on *both* lanes. This
    // rules out a trivial pass from the vector lane drawing nothing at all.
    let edge_probe = (32, 13);
    assert!(
        is_drawn(texel(&triangulated, edge_probe.0, edge_probe.1)),
        "triangulated shadow must paint its straight-edge halo"
    );
    assert!(
        is_drawn(texel(&vector, edge_probe.0, edge_probe.1)),
        "vector shadow must paint its straight-edge halo"
    );

    let corner_probe = (12, 12);
    assert!(
        is_drawn(texel(&triangulated, corner_probe.0, corner_probe.1)),
        "triangulated shadow's unfixed square corner (#279 scope) must still paint this pixel"
    );
    assert!(
        !is_drawn(texel(&vector, corner_probe.0, corner_probe.1)),
        "vector shadow's rounded corner must leave this pixel the untouched clear color — \
         the #219/#212 bug class this issue fixes for the vector lane"
    );
}
