//! Linear-space blending GPU acceptance tests (#135).
//!
//! These run through #106's surfaceless [`OffscreenRenderer`] (Vulkan/Lavapipe,
//! CPU adapter), whose target is `Rgba8Unorm` — the same linear
//! [`INTERMEDIATE_FORMAT`](super::INTERMEDIATE_FORMAT) the windowed renderer
//! composites into. They drive the **real** UI pipeline
//! ([`build_ui_pipeline`](super::build_ui_pipeline)) + the real per-vertex
//! linearization ([`gui_vertices`](super::gui_vertices)) and read back raw
//! bytes, proving two halves of the gamma-correct contract:
//!
//! 1. authored sRGB-perceptual colors are linearized before they reach the
//!    linear target (an sRGB `0.5` gray stores as ~`0.214`, byte ~55 — NOT the
//!    byte 128 that un-linearized/sRGB-space compositing produced pre-Sprint-8),
//! 2. alpha blending composites in linear light (white at 50% over black is the
//!    linear midpoint, byte ~128).
//!
//! The windowed present pass (linear→sRGB encode to the swapchain) has no
//! offscreen surface to target, so it is covered by naga validation
//! (`present_wgsl_parses_and_validates`) + the deferred operator smoke, not here.

use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

use super::offscreen::OffscreenRenderer;
use super::{build_ui_pipeline, gui_vertices, GuiTriangle, INTERMEDIATE_FORMAT};
use crate::{Color, Point};

const W: u32 = 32;
const H: u32 = 32;

/// Provision the #106 harness and assert the Vulkan + CPU adapter contract.
fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "linear-blend offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_eq!(info.device_type, wgpu::DeviceType::Cpu);
    // The harness target must be the linear format the windowed renderer blends
    // in, or the readback bytes wouldn't reflect the linear pipeline.
    assert_eq!(renderer.format(), INTERMEDIATE_FORMAT);
    renderer
}

/// Draw one full-viewport quad of `fill` over `clear` through the real UI
/// pipeline, then read back the center pixel's RGBA bytes.
fn render_center_pixel(clear: wgpu::Color, fill: Color) -> [u8; 4] {
    let renderer = harness();
    let pipeline = build_ui_pipeline(renderer.device(), INTERMEDIATE_FORMAT, 1);

    // Two triangles covering the whole 32×32 target, authored in logical pixels.
    let quad = [
        GuiTriangle {
            points: [
                Point::new(0.0, 0.0),
                Point::new(W as f32, 0.0),
                Point::new(0.0, H as f32),
            ],
            color: fill,
        },
        GuiTriangle {
            points: [
                Point::new(W as f32, 0.0),
                Point::new(W as f32, H as f32),
                Point::new(0.0, H as f32),
            ],
            color: fill,
        },
    ];
    let vertices = gui_vertices(&quad, W as f32, H as f32);
    let buffer = renderer
        .device()
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("linear-blend test vertices"),
            contents: cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("linear-blend test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("linear-blend test pass"),
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
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
    renderer.queue().submit(Some(encoder.finish()));

    let pixels = renderer.read_rgba().expect("readback must succeed");
    let idx = (((H / 2) * W + W / 2) * 4) as usize;
    [
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    ]
}

#[test]
fn authored_srgb_gray_is_linearized_into_the_target() {
    // sRGB 0.5 → linear ~0.214 → byte ~55. The pre-Sprint-8 bug (compositing
    // the un-linearized 0.5 straight into the target) would have produced ~128.
    let px = render_center_pixel(wgpu::Color::BLACK, Color::from_srgb(0.5, 0.5, 0.5));
    for &c in &px[..3] {
        assert!(
            (50..=60).contains(&c),
            "sRGB 0.5 must linearize to ~55, got {c} (128 would mean sRGB-space compositing)"
        );
    }
    assert_eq!(px[3], 255, "opaque quad stays opaque");
}

#[test]
fn alpha_blend_composites_in_linear_light() {
    // White (linear 1.0) at 50% alpha over black: linear midpoint 0.5 → byte
    // ~128. Proves the blend equation runs on linear values in the linear
    // intermediate, not on sRGB-encoded framebuffer bytes.
    let px = render_center_pixel(wgpu::Color::BLACK, Color::from_srgb_a(1.0, 1.0, 1.0, 0.5));
    for &c in &px[..3] {
        assert!(
            (123..=132).contains(&c),
            "white@50% over black must be the linear midpoint ~128, got {c}"
        );
    }
}
