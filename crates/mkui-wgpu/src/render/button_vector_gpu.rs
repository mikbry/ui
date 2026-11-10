//! Cross-consumer GPU acceptance test for the `button` component (#140).
//!
//! Sprint 8 plan v2 §4.3 Tier 2B: "renders the `button` component in both
//! modes on Lavapipe; asserts vector mode's readback shows smooth corner
//! pixels where triangulated shows polygonal." Runs through #106's
//! surfaceless offscreen harness, same dual gate (`gpu-tests` + `slug`) as
//! the sibling Slug GPU test modules in this directory.
//!
//! `button`'s chrome fork (an internal, non-`pub` function private to
//! `components::button`) is resolved at **compile time** by the `slug`
//! feature, so a single test binary — compiled with `slug` on, as every test
//! in this module is — can only ever observe the vector output from the real
//! `button_with` call. The triangulated reference is therefore rebuilt by
//! hand from the exact same resolved `ButtonStyle` fields `button_with` would
//! have fed its (compiled-out, in this binary) triangulated path — i.e.
//! exactly the pre-#140 `Quad` emission — rather than obtained from a second
//! call to the real function.
//!
//! Since #212, the triangle tessellator facets `Quad::corner_radii` into a
//! many-sided polygon instead of ignoring it, so the bounding rect's outer
//! corner is now excluded on *both* lanes (the probe point sits ~2.6px
//! outside the radius-8 arc, far past the faceted polygon's sub-pixel
//! deviation from a true arc) — the remaining, still-genuine difference is
//! that the triangulated lane's edges stay hard/binary while the vector
//! lane's curved boundary is anti-aliased.

use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

use crate::components::button_with;
use crate::tessellation::tessellate_scene;
use crate::theme::{ButtonSize, ButtonState, ButtonVariant, WgpuTheme};
use crate::{CornerRadii, Point, Quad, Rect, Scene, Size, SlugAdapter, Stroke};

use super::offscreen::{OffscreenRenderer, BYTES_PER_PIXEL};
use super::{
    build_ui_pipeline, gui_vertices, scene_vector_paths, vector_path_paints, INTERMEDIATE_FORMAT,
};

const W: u32 = 64;
const H: u32 = 64;

fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "button-vector offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_eq!(info.device_type, wgpu::DeviceType::Cpu);
    renderer
}

/// Render `primitives` through the real triangulated UI pipeline and read
/// back RGBA bytes.
fn render_triangulated(renderer: &OffscreenRenderer, scene: &Scene) -> Vec<u8> {
    let pipeline = build_ui_pipeline(renderer.device(), INTERMEDIATE_FORMAT, 1);
    let triangles = tessellate_scene(scene);
    let vertices = gui_vertices(&triangles, W as f32, H as f32);
    let buffer = renderer
        .device()
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("button-vector-gpu triangulated vertices"),
            contents: cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("button-vector-gpu triangulated encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("button-vector-gpu triangulated pass"),
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
    }
    renderer.queue().submit(Some(encoder.finish()));
    renderer.read_rgba().expect("readback must succeed")
}

/// Render `scene`'s `Primitive::VectorPath`s through the real Slug coverage
/// pipeline (the same seam `Renderer::render` drives in production — #139's
/// `scene_vector_paths` + `vector_path_paints`) and read back RGBA bytes.
fn render_vector(renderer: &OffscreenRenderer, adapter: &SlugAdapter, scene: &Scene) -> Vec<u8> {
    let paths = scene_vector_paths(&scene.primitives);
    let paints = vector_path_paints(&paths);
    let prepared = adapter.prepare_path_paints(
        renderer.device(),
        renderer.queue(),
        [W as f32, H as f32],
        1.0,
        &paints,
    );

    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("button-vector-gpu vector encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("button-vector-gpu vector pass"),
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

fn texel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * BYTES_PER_PIXEL) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn button_vector_vs_triangulated() {
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), renderer.format());

    let theme = WgpuTheme::default();
    let style = theme.button_style(ButtonVariant::Default, ButtonSize::Default);
    let rect = Rect::new(Point::new(8.0, 8.0), Size::new(48.0, 48.0));

    // Triangulated reference: exactly the pre-#140 `Quad` emission (see the
    // module doc for why this is rebuilt by hand rather than called).
    let mut tri_scene = Scene::new(Size::new(W as f32, H as f32));
    tri_scene.quad(Quad {
        rect,
        fill: style.idle_fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.idle_stroke,
            width: style.stroke_width,
        }),
    });
    let tri_pixels = render_triangulated(&renderer, &tri_scene);

    // Vector actual: the real production `button_with` call (this binary is
    // compiled with `slug` on, so its chrome fork emits `Primitive::VectorPath`).
    let mut vec_scene = Scene::new(Size::new(W as f32, H as f32));
    button_with(&mut vec_scene, rect, "", ButtonState::Idle, &style);
    let vec_pixels = render_vector(&renderer, &adapter, &vec_scene);

    // Sanity: both modes actually paint the button's interior.
    let clear = [0u8, 0, 0, 255];
    assert_ne!(
        texel(&tri_pixels, 32, 32),
        clear,
        "triangulated button must paint its interior"
    );
    assert_ne!(
        texel(&vec_pixels, 32, 32),
        clear,
        "vector button must paint its interior"
    );

    // The outer corner of the bounding rect (screen (8, 8), the button's
    // top-left-most pixel): its center (8.5, 8.5) is ~10.61px from the
    // radius-8 arc's own center (16, 16) — 2.6px outside the radius, far
    // past the faceted polygon's sub-pixel deviation from a true arc — so
    // since #212 the triangulated lane's rounded corner excludes this point
    // too, the same as the vector lane's.
    assert_eq!(
        texel(&tri_pixels, 8, 8),
        clear,
        "triangulated button's rounded corner (#212) must leave this pixel \
         the untouched clear color"
    );
    assert_eq!(
        texel(&vec_pixels, 8, 8),
        clear,
        "vector button's rounded corner must leave this pixel the untouched clear color"
    );
}
