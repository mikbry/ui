//! Cross-consumer GPU acceptance test for the `dot` atom's circle (#142,
//! Sprint 8 Wave 2 — "the killer Sprint 8 demo primitive"). Same dual gate as
//! [`super::slug_gpu`] (`gpu-tests` + `slug`, run on #106's Vulkan/Lavapipe
//! harness).
//!
//! This drives the **real** pipeline on both sides of the #142 comparison:
//!
//! - **vector**: the real `dot()`, compiled with `slug`, emits a
//!   `Primitive::VectorPath` circle. This test lowers it through the exact
//!   scene-level seam the windowed renderer uses
//!   ([`scene_vector_paths`](super::scene_vector_paths) +
//!   [`vector_path_paints`](super::vector_path_paints)) and draws it with the
//!   real [`SlugAdapter`].
//! - **triangulated**: the exact v0.10.0 quad `dot()` emits *off* the `slug`
//!   feature — a plain square `Quad` (the renderer does not round quad
//!   corners, see `components::dot`'s module docs). Hand-built here because
//!   this test binary is compiled *with* `slug`, so the real `dot()` only
//!   ever gives this test the vector arm. Tessellated with the real
//!   [`tessellate_scene`](crate::tessellate_scene) and drawn with the real
//!   [`build_ui_pipeline`](super::build_ui_pipeline).
//!
//! The acceptance criterion is that the vector circle is a true round shape
//! with per-fragment AA coverage at its boundary, while the triangulated
//! fallback is visibly polygonal (here: literally square). Concretely, a
//! point that lies inside the square's bounding box but outside the circle —
//! the corner a circle doesn't have — is covered by the triangulated
//! primitive and not by the vector one.

use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

use mkui_vector2d_wgpu::SlugAdapter;

use super::offscreen::OffscreenRenderer;
use super::{
    build_ui_pipeline, gui_vertices, scene_vector_paths, vector_path_paints, INTERMEDIATE_FORMAT,
};
use crate::components::dot;
use crate::theme::{DotSize, DotVariant, WgpuTheme};
use crate::types::{CornerRadii, DotAnimation, Point, Quad, Rect, Scene, Size};

const W: u32 = 32;
const H: u32 = 32;
/// `DotSize::Md`'s radius — issue #142's "typical dot radii".
const RADIUS: f32 = 4.0;
const CENTER: f32 = 16.0;

fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "dot vector-vs-triangulated offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "harness must select the Vulkan backend"
    );
    assert_eq!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "CI harness must run on a CPU (Lavapipe) adapter"
    );
    renderer
}

/// The real `dot()` scene (compiled with `slug`): one `Primitive::VectorPath`
/// circle, `RADIUS` px, centred at `(CENTER, CENTER)`.
fn vector_scene() -> Scene {
    let theme = WgpuTheme::default();
    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    dot(
        &mut scene,
        Point::new(CENTER, CENTER),
        DotVariant::Ok,
        DotSize::Md,
        false,
        DotAnimation::None,
        &theme,
    );
    scene
}

/// The exact v0.10.0 quad `dot()` emits off the `slug` feature — hand-built
/// here since this test binary only ever compiles the vector arm of `dot()`
/// (see the module docs).
fn triangulated_scene() -> Scene {
    let theme = WgpuTheme::default();
    let fill = theme.dot_style(DotVariant::Ok, DotSize::Md).fill;
    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    let origin = Point::new(CENTER - RADIUS, CENTER - RADIUS);
    scene.quad(Quad {
        rect: Rect::new(origin, Size::new(RADIUS * 2.0, RADIUS * 2.0)),
        fill,
        corner_radii: CornerRadii::all(RADIUS),
        stroke: None,
    });
    scene
}

/// Draw `scene`'s vector paths through the real Slug adapter (the same
/// lowering + draw path the windowed renderer's `VectorPath` render command
/// uses) and read back the full RGBA target, cleared to opaque black first.
fn render_vector(scene: &Scene) -> Vec<u8> {
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), INTERMEDIATE_FORMAT);
    let paths = scene_vector_paths(&scene.primitives);
    let paints = vector_path_paints(&paths);
    let prepared = adapter
        .prepare_path_paints(
            renderer.device(),
            renderer.queue(),
            [W as f32, H as f32],
            1.0,
            &paints,
        )
        .expect("the dot's vector circle must produce drawable geometry");

    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("dot vector test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dot vector test pass"),
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
    }
    renderer.queue().submit(Some(encoder.finish()));
    renderer.read_rgba().expect("vector readback must succeed")
}

/// Draw `scene`'s quads through the real UI pipeline and read back the full
/// RGBA target, cleared to opaque black first.
fn render_triangulated(scene: &Scene) -> Vec<u8> {
    let renderer = harness();
    let pipeline = build_ui_pipeline(renderer.device(), INTERMEDIATE_FORMAT, 1);
    let triangles = crate::tessellate_scene(scene);
    let vertices = gui_vertices(&triangles, W as f32, H as f32);
    let buffer = renderer
        .device()
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dot triangulated test vertices"),
            contents: cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("dot triangulated test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dot triangulated test pass"),
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
    renderer
        .read_rgba()
        .expect("triangulated readback must succeed")
}

fn pixel(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * W + x) * 4) as usize;
    [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]]
}

/// True once a channel has visibly moved off the opaque-black clear.
fn is_drawn(px: [u8; 4]) -> bool {
    px[0] > 2 || px[1] > 2 || px[2] > 2
}

/// The green channel at the shape's own centre pixel — the "fully covered"
/// reference value each test below normalizes against. Asserted `> 10` at
/// every call site so a pipeline that silently drew nothing (empty geometry,
/// a `None` lane) fails loudly instead of making every other check in this
/// file vacuously pass.
fn full_coverage_green(buf: &[u8]) -> f32 {
    let g = pixel(buf, CENTER as u32, CENTER as u32)[1];
    assert!(
        g > 10,
        "the shape's own centre must be solidly filled, got green={g}"
    );
    g as f32
}

#[test]
fn dot_vector_vs_triangulated_diagonal_corner() {
    // A point inside the square's bounding box (|dx|, |dy| < RADIUS) but
    // outside the circle (dx^2 + dy^2 > RADIUS^2): the corner a circle
    // doesn't have. `0.875 * RADIUS` on both axes sits comfortably inside the
    // square while its Euclidean distance (~1.237 * RADIUS) clears the
    // circle boundary well beyond the ~0.5px AA dilation margin.
    let offset = (RADIUS * 0.875) as u32;
    let cx = CENTER as u32 + offset;
    let cy = CENTER as u32 + offset;

    let triangulated = render_triangulated(&triangulated_scene());
    let vector = render_vector(&vector_scene());

    // Rule out a trivial pass from nothing having rendered at all.
    full_coverage_green(&triangulated);
    full_coverage_green(&vector);

    assert!(
        is_drawn(pixel(&triangulated, cx, cy)),
        "the square's corner must be filled (it has no curved boundary to exclude it)"
    );
    assert!(
        !is_drawn(pixel(&vector, cx, cy)),
        "the circle must NOT cover its bounding square's corner — a vector \
         circle has no visible facets/vertices there"
    );
}

#[test]
fn dot_vector_vs_triangulated_boundary_antialiasing() {
    // Whole-image scan rather than one hand-picked boundary pixel: at
    // RADIUS=4 and an integer centre, any single axis-aligned sample point
    // risks landing exactly on a dilated-quad texel edge (coverage 0 or 1,
    // no partial fragment) — fragile. Scanning the full target instead counts
    // every texel with *partial* coverage (strictly between background and
    // the shape's own full-coverage value), which must be nonzero somewhere
    // around a curved boundary and exactly zero around the triangulated
    // quad's hard, binary edges (`push_rect` emits flat per-vertex color, no
    // AA at all).
    let vector = render_vector(&vector_scene());
    let triangulated = render_triangulated(&triangulated_scene());

    let vector_full = full_coverage_green(&vector);
    let triangulated_full = full_coverage_green(&triangulated);

    let partial_count = |buf: &[u8], full: f32| -> u32 {
        (0..W)
            .flat_map(|x| (0..H).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let g = pixel(buf, x, y)[1] as f32;
                g > 2.0 && g < full - 2.0
            })
            .count() as u32
    };

    let vector_partial = partial_count(&vector, vector_full);
    let triangulated_partial = partial_count(&triangulated, triangulated_full);

    assert!(
        vector_partial >= 4,
        "the circle boundary must show several partial-coverage \
         (anti-aliased) texels around its curved edge, got {vector_partial}"
    );
    assert_eq!(
        triangulated_partial, 0,
        "the triangulated quad has a flat, binary edge — no partial-coverage \
         texels, got {triangulated_partial}"
    );
}

#[test]
fn dot_vector_vs_triangulated_coverage_area() {
    // Integrated coverage (each texel's green channel as a fraction of the
    // shape's own full-coverage value), not a binary "is it drawn" pixel
    // count — a binary count folds the whole AA fringe ring into the
    // circle's area, which overshoots the analytic pi/4 (~0.785) this
    // integrates to, and more so the smaller the radius (the fringe is a
    // larger share of a small circle's area).
    let vector = render_vector(&vector_scene());
    let triangulated = render_triangulated(&triangulated_scene());

    let vector_full = full_coverage_green(&vector);
    let triangulated_full = full_coverage_green(&triangulated);

    let coverage_area = |buf: &[u8], full: f32| -> f32 {
        (0..W)
            .flat_map(|x| (0..H).map(move |y| (x, y)))
            .map(|(x, y)| (pixel(buf, x, y)[1] as f32 / full).min(1.0))
            .sum()
    };

    let vector_area = coverage_area(&vector, vector_full);
    let triangulated_area = coverage_area(&triangulated, triangulated_full);

    // Square area is exactly (2*RADIUS)^2 = 64 px^2 (grid-aligned, no AA);
    // circle area is pi*RADIUS^2 ~= 50.3 px^2 — a ratio of pi/4 ~= 0.785.
    let ratio = vector_area / triangulated_area;
    assert!(
        (0.65..=0.90).contains(&ratio),
        "integrated-coverage area ratio (vector/triangulated) should track \
         pi/4 (~0.785) within a generous band (pixel-grid quantization at \
         this small a radius), got {ratio} (vector_area={vector_area}, \
         triangulated_area={triangulated_area})"
    );
}
