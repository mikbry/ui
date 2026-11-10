//! Host-owned GPU and frame — the borrowed-resources entry point (#175).
//!
//! [`Renderer`](super::Renderer) is the winit-owned "standalone" shell (ADR
//! 0006/0007): it creates its own `wgpu::Instance`, requests its own adapter
//! / device / queue, negotiates its own `Surface` from an `Arc<Window>`, and
//! owns the per-frame command encoder and submission end-to-end.
//!
//! [`EmbeddedRenderer`] is the opposite: a third-party `wgpu`-based host
//! (a game or interactive engine) already owns an instance, adapter, device,
//! queue, and a frame target — its swapchain view, drawn into by its own 3D
//! scene. `EmbeddedRenderer` creates **none** of those. It borrows the host's
//! `Device` at construction (exactly the `mkui-vector2d-wgpu` precedent — ADR
//! 0007 — of a GPU-resource crate taking a borrowed device/queue/format
//! rather than owning or re-deriving them) and, per frame, records its one UI
//! render pass into the host's own [`wgpu::CommandEncoder`] and
//! [`wgpu::TextureView`] — after the host's 3D scene, before the host
//! presents. There is one command buffer and one `queue.submit` per frame:
//! the host's. `EmbeddedRenderer` never calls `submit`, never acquires a
//! surface texture, and never presents.
//!
//! ## What it does not (yet) support
//!
//! Phase 1 (#175) covers the triangle lane only — the UI shapes, bitmap
//! text, and borders every component renders through today. The Slug
//! vector-glyph lane (`slug` feature, ADR 0007) and `Primitive::VectorPath`
//! are not wired into [`EmbeddedRenderer::render_into`] yet; primitives of
//! those kinds are silently skipped (the same no-op [`crate::tessellate_primitives`]
//! already gives them on an unselected lane), not rejected. Follow-up:
//! extending the embedded path to the Slug/vector lane is tracked
//! separately so Phase 1 stays scoped to the hosting contract itself.
//!
//! ## Color space
//!
//! The standalone [`Renderer`] always composites into a linear intermediate
//! target and defers the sRGB encode to a separate present pass (ADR 0006
//! §"Color space + blending"), because the final surface format is picked by
//! `Renderer::new` itself. `EmbeddedRenderer` has no such freedom — the
//! host's target format is whatever the host already uses for its own frame
//! — so it draws directly into it in one pass:
//!
//! - **Host target is an sRGB format** (the common case — most engines
//!   present through an sRGB-format view): the UI pipeline writes the same
//!   linear vertex colors [`super::gui_vertices`] already produces, and the
//!   hardware's sRGB encode-on-write does the rest. Alpha blending stays
//!   physically correct, in linear light, identical in spirit to the
//!   standalone path.
//! - **Host target is a plain UNORM format** (no automatic encode): the
//!   pipeline draws through `ui_triangles.wgsl`'s `fs_ui_triangles_encode_srgb`
//!   entry point instead, which applies the linear -> sRGB OETF before the
//!   fixed-function blend stage. Blending then happens in gamma space rather
//!   than linear light — a documented Phase 1 simplification for that one
//!   target-format case, accepted because inserting a linear intermediate +
//!   present blit here would itself be the "extra texture copy" the
//!   embedding contract rules out.

use mkui_text::TextSystem;

use crate::{tessellate_primitives, Scene};

use super::{build_ui_pipeline, gui_vertices};

use wgpu::util::DeviceExt;

/// Picks the UI fragment entry point for a host target format. See the
/// module docs' "Color space" section.
fn fragment_entry_point_for(format: wgpu::TextureFormat) -> &'static str {
    if format.is_srgb() {
        "fs_ui_triangles"
    } else {
        "fs_ui_triangles_encode_srgb"
    }
}

/// Host-owned GPU and frame entry point (#175). Owns only the UI render
/// pipeline — no instance, no adapter, no device, no queue, no surface, no
/// window. Built once against the host's borrowed device and target format;
/// [`EmbeddedRenderer::render_into`] is then called once per frame, recording
/// into the host's own encoder and view.
#[derive(Debug)]
pub struct EmbeddedRenderer {
    ui_pipeline: wgpu::RenderPipeline,
    target_format: wgpu::TextureFormat,
}

impl EmbeddedRenderer {
    /// Build the UI pipeline against the host's borrowed `device` and
    /// `target_format` — the format of the texture view the host will later
    /// pass to [`render_into`](Self::render_into). Always single-sampled
    /// (`sample_count = 1`): the embedded renderer has no owned MSAA
    /// attachment to resolve into, matching how the host's own frame target
    /// is whatever the host configured (ADR 0007's "borrowed, never owned"
    /// discipline — see the module docs).
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let ui_pipeline = build_ui_pipeline(
            device,
            target_format,
            1,
            fragment_entry_point_for(target_format),
        );
        Self {
            ui_pipeline,
            target_format,
        }
    }

    /// The host target format this renderer's pipeline was built against.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }

    /// Record the UI render pass for `scene` into the host's own `encoder`
    /// and `view`, loading the existing contents (`wgpu::LoadOp::Load`) so
    /// the host's already-drawn 3D scene survives underneath — this call
    /// never clears. Does not create an encoder, does not submit to `queue`,
    /// and does not present; the host does all three exactly once per frame,
    /// around this call.
    ///
    /// `target_size` is the physical-pixel size of `view`'s underlying
    /// texture, used (like [`super::Renderer::render`]) to derive the
    /// physical-pixels-per-logical-pixel ratio against `scene.viewport`
    /// (logical pixels, ADR 0006 §"Viewport units contract") for text/bitmap
    /// device-pixel snapping. The host, not this renderer, owns resize — it
    /// already knows its own target's size every frame.
    pub fn render_into(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        target_size: (u32, u32),
        scene: &Scene,
        text_system: &dyn TextSystem,
    ) {
        let (target_width, _) = target_size;
        let device_pixel_ratio = target_width as f32 / scene.viewport.width.max(1.0);

        // Phase 1 (#175): the triangle lane only. `tessellate_primitives`
        // already no-ops `Primitive::SlugGlyph` / `Primitive::VectorPath`
        // (see `crate::tessellation`), so running every primitive straight
        // through it, with no lane splitting, is byte-identical to the
        // standalone path's triangle lane and simply skips any Slug/vector
        // path primitives for now (see the module docs).
        let triangles = tessellate_primitives(&scene.primitives, text_system, device_pixel_ratio);
        let vertices = gui_vertices(&triangles, scene.viewport.width, scene.viewport.height);
        if vertices.is_empty() {
            return;
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mkui-wgpu Embedded UI Vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mkui-wgpu Embedded UI Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.ui_pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_entry_picks_pass_through_for_srgb_targets() {
        assert_eq!(
            fragment_entry_point_for(wgpu::TextureFormat::Bgra8UnormSrgb),
            "fs_ui_triangles"
        );
        assert_eq!(
            fragment_entry_point_for(wgpu::TextureFormat::Rgba8UnormSrgb),
            "fs_ui_triangles"
        );
    }

    #[test]
    fn fragment_entry_picks_encode_for_unorm_targets() {
        assert_eq!(
            fragment_entry_point_for(wgpu::TextureFormat::Bgra8Unorm),
            "fs_ui_triangles_encode_srgb"
        );
        assert_eq!(
            fragment_entry_point_for(wgpu::TextureFormat::Rgba8Unorm),
            "fs_ui_triangles_encode_srgb"
        );
    }

    #[test]
    fn ui_triangles_wgsl_parses_and_validates() {
        // Static naga validation catches a typo in either fragment entry
        // point (mirrors `present_wgsl_parses_and_validates` in
        // `render::mod`), independent of the `gpu-tests` adapter gate.
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("ui_triangles.wgsl"))
            .expect("ui_triangles.wgsl must parse");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("ui_triangles.wgsl must pass naga validation");
    }

    // ---- GPU acceptance test (#175): the host-embedding contract end to end.
    //
    // Reuses the #106 surfaceless offscreen harness to stand in for "the
    // host's device/queue/target" — `OffscreenRenderer` already exposes
    // exactly the borrowed `device()` / `queue()` / `view()` / `format()`
    // triple a real host would hand `EmbeddedRenderer`. Gated on `gpu-tests`
    // only (no `slug` dependency — Phase 1 is the triangle lane).
    #[cfg(feature = "gpu-tests")]
    mod gpu {
        use crate::render::embedded::EmbeddedRenderer;
        use crate::render::offscreen::OffscreenRenderer;
        use crate::Scene;
        use mkui_text::BitmapTextSystem;

        fn host(width: u32, height: u32) -> OffscreenRenderer {
            OffscreenRenderer::new(width, height).expect(
                "offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner",
            )
        }

        fn hud_scene(viewport: crate::Size) -> Scene {
            let mut scene = Scene::new(viewport);
            scene.push(crate::Primitive::Quad(crate::Quad {
                rect: crate::Rect::new(
                    crate::Point::new(0.0, 0.0),
                    crate::Size::new(viewport.width, viewport.height),
                ),
                fill: crate::Color::rgba(0.0, 1.0, 0.0, 1.0),
                corner_radii: crate::CornerRadii::all(0.0),
                stroke: None,
            }));
            scene
        }

        #[test]
        fn render_into_preserves_host_content_outside_the_ui_and_draws_on_top() {
            let size = 64u32;
            let host = host(size, size);

            // Simulate "the host already drew its 3D scene into this frame":
            // clear to a known non-UI color first.
            host.clear(wgpu::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            let before = host.read_rgba().expect("baseline readback");
            assert!(
                before.chunks_exact(4).all(|px| px == [255, 0, 0, 255]),
                "precondition: host content is the solid red clear"
            );

            // One encoder, one submission — the host's — carrying both the
            // (already-recorded) host content and mkui's UI pass.
            let embedded = EmbeddedRenderer::new(host.device(), host.format());
            let mut encoder =
                host.device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("host+mkui embedded test encoder"),
                    });
            let scene = hud_scene(crate::Size::new(size as f32, size as f32));
            embedded.render_into(
                host.device(),
                &mut encoder,
                host.view(),
                (size, size),
                &scene,
                &BitmapTextSystem::new(),
            );
            host.queue().submit(Some(encoder.finish()));

            let after = host.read_rgba().expect("post-UI readback");
            assert_ne!(
                after, before,
                "the UI pass must have changed pixels (LoadOp::Load, not a no-op)"
            );
            // `OffscreenRenderer::format()` is `Rgba8Unorm` (non-sRGB), so
            // `EmbeddedRenderer` draws through `fs_ui_triangles_encode_srgb`.
            // A fully-covering opaque green(0,1,0,1) quad over the red clear
            // replaces every texel with sRGB-encoded green.
            assert!(
                after.chunks_exact(4).all(|px| px == [0, 255, 0, 255]),
                "a full-coverage opaque quad must fully replace the host's clear color"
            );
        }

        #[test]
        fn render_into_is_a_single_pass_no_op_on_an_empty_scene() {
            let size = 32u32;
            let host = host(size, size);
            host.clear(wgpu::Color::BLACK);
            let before = host.read_rgba().expect("baseline readback");

            let embedded = EmbeddedRenderer::new(host.device(), host.format());
            let mut encoder =
                host.device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("host+mkui embedded empty-scene test encoder"),
                    });
            let scene = Scene::new(crate::Size::new(size as f32, size as f32));
            embedded.render_into(
                host.device(),
                &mut encoder,
                host.view(),
                (size, size),
                &scene,
                &BitmapTextSystem::new(),
            );
            host.queue().submit(Some(encoder.finish()));

            let after = host.read_rgba().expect("post-UI readback");
            assert_eq!(
                after, before,
                "an empty scene must leave the host's already-drawn content untouched"
            );
        }
    }
}
