//! Host-owned GPU and frame — the borrowed-resources entry point (#175).
//!
//! [`Renderer`] is the winit-owned "standalone" shell (ADR
//! 0006/0007): it creates its own `wgpu::Instance`, requests its own adapter
//! / device / queue, negotiates its own `Surface` from an `Arc<Window>`, and
//! owns the per-frame command encoder and submission end-to-end.
//!
//! [`EmbeddedRenderer`](crate::EmbeddedRenderer) is the opposite: a third-party `wgpu`-based host
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
//! vector-glyph lane (`slug` feature, ADR 0007), `Primitive::VectorPath`,
//! and (#187) real backdrop blur (`Primitive::Backdrop`) are not wired into
//! [`EmbeddedRenderer::render_into`](crate::EmbeddedRenderer::render_into)
//! yet; primitives of those kinds are silently skipped (the same no-op
//! [`crate::tessellate_primitives`] already gives them on an unselected
//! lane), not rejected. A glass panel still renders *something* here: it
//! also emits a translucent `Quad` (the tint) that composites normally
//! through the triangle lane — it just loses the blur, degrading to a flat
//! tint over whatever the host already drew. Follow-up: extending the
//! embedded path to the Slug/vector lane is tracked separately (#234), and
//! to the backdrop-blur lane in #187's own follow-up, so Phase 1 stays
//! scoped to the hosting contract itself.
//!
//! ## Color space
//!
//! The standalone [`Renderer`] always composites into a linear intermediate
//! target and defers the sRGB encode to a separate present pass (ADR 0006
//! §"Color space + blending"), because the final surface format is picked by
//! `Renderer::new` itself. `EmbeddedRenderer` has no such freedom — the
//! host's target format is whatever the host already uses for its own frame
//! — so it draws directly into it in one pass.
//!
//! `target_format.is_srgb()` alone cannot decide how to draw into it: it only
//! tells us whether the GPU applies a hardware sRGB encode on write. Formats
//! where that's `false` split into two groups that need *opposite* handling —
//! a plain 8-bit UNORM format (`Bgra8Unorm`) stores perceptually-encoded sRGB
//! bytes with no hardware transform and needs a manual encode, while a linear
//! float format (`Rgba16Float`, an HDR intermediate) stores already-linear
//! values and needs **no** encode at all — encoding it would be the same bug
//! class as not encoding a UNORM target. Inferring the right answer from the
//! format alone silently mishandles whichever group the heuristic didn't
//! anticipate (filed and fixed as part of #175's review — see the module
//! tests), so [`EmbeddedRenderer::new`](crate::EmbeddedRenderer::new) takes an explicit
//! [`EmbeddedColorSpace`](crate::EmbeddedColorSpace) instead of guessing:
//!
//! - [`EmbeddedColorSpace::Linear`](crate::EmbeddedColorSpace::Linear) — the target either auto-encodes sRGB on
//!   write (`target_format.is_srgb()`) or is itself a linear store that
//!   expects already-linear values (e.g. `Rgba16Float`). Either way,
//!   `render_into` writes the same linear vertex colors
//!   `super::gui_vertices` already produces, untouched, through
//!   `ui_triangles.wgsl`'s `fs_ui_triangles` entry point. Alpha blending
//!   stays physically correct, in linear light — identical in spirit to the
//!   standalone path.
//! - [`EmbeddedColorSpace::EncodedUnorm`](crate::EmbeddedColorSpace::EncodedUnorm) — the target is a plain non-sRGB
//!   UNORM format that stores perceptually-encoded sRGB bytes directly, with
//!   no hardware transform on write (e.g. `Bgra8Unorm`). `render_into` draws
//!   through `fs_ui_triangles_encode_srgb` instead, which applies the linear
//!   -> sRGB OETF before the fixed-function blend stage. Blending then
//!   happens in gamma space rather than linear light — a documented Phase 1
//!   simplification for that one target-kind, accepted because inserting a
//!   linear intermediate + present blit here would itself be the "extra
//!   texture copy" the embedding contract rules out.
//!
//! The host, which alone knows what its target format actually means, picks
//! between the two — `EmbeddedRenderer` never inspects `target_format` to
//! decide.

use mkui_text::TextSystem;

use crate::{tessellate_primitives, Scene};

use super::{build_ui_pipeline, gui_vertices};

use wgpu::util::DeviceExt;

/// How [`EmbeddedRenderer`] should treat the host's target format for color
/// output. See the module docs' "Color space" section for why this must be
/// declared explicitly rather than inferred from `wgpu::TextureFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedColorSpace {
    /// The target auto-encodes sRGB on write, or is itself a linear store
    /// (e.g. an `Rgba16Float` HDR intermediate) that expects already-linear
    /// values with no encoding at all. Draws through `fs_ui_triangles`.
    Linear,
    /// The target is a plain non-sRGB UNORM format that stores
    /// perceptually-encoded sRGB bytes directly, with no hardware transform
    /// on write. Draws through `fs_ui_triangles_encode_srgb`.
    EncodedUnorm,
}

impl EmbeddedColorSpace {
    /// Picks the UI fragment entry point for this color space. See the
    /// module docs' "Color space" section.
    fn fragment_entry_point(self) -> &'static str {
        match self {
            EmbeddedColorSpace::Linear => "fs_ui_triangles",
            EmbeddedColorSpace::EncodedUnorm => "fs_ui_triangles_encode_srgb",
        }
    }
}

/// Host-owned GPU and frame entry point (#175). Owns only the UI render
/// pipeline — no instance, no adapter, no device, no queue, no surface, no
/// window. Built once against the host's borrowed device, target format, and
/// declared color space; [`EmbeddedRenderer::render_into`] is then called
/// once per frame, recording into the host's own encoder and view.
#[derive(Debug)]
pub struct EmbeddedRenderer {
    ui_pipeline: wgpu::RenderPipeline,
    target_format: wgpu::TextureFormat,
}

impl EmbeddedRenderer {
    /// Build the UI pipeline against the host's borrowed `device`,
    /// `target_format` — the format of the texture view the host will later
    /// pass to [`render_into`](Self::render_into) — and `color_space`, which
    /// the host (not this renderer) decides; see the module docs' "Color
    /// space" section for why `target_format` alone is not enough. Always
    /// single-sampled (`sample_count = 1`): the embedded renderer has no
    /// owned MSAA attachment to resolve into, matching how the host's own
    /// frame target is whatever the host configured (ADR 0007's "borrowed,
    /// never owned" discipline — see the module docs).
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        color_space: EmbeddedColorSpace,
    ) -> Self {
        let ui_pipeline =
            build_ui_pipeline(device, target_format, 1, color_space.fragment_entry_point());
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
    fn linear_color_space_always_passes_through_regardless_of_format() {
        // The regression this guards: the fragment entry point must be a
        // function of the explicit `EmbeddedColorSpace`, never of
        // `wgpu::TextureFormat::is_srgb()`. A linear float format like
        // `Rgba16Float` is NOT sRGB, but a host declaring it `Linear` (the
        // correct choice — see the module docs) must still get the
        // pass-through entry point, not the encode one a format-based
        // heuristic would have picked.
        assert_eq!(
            EmbeddedColorSpace::Linear.fragment_entry_point(),
            "fs_ui_triangles"
        );
    }

    #[test]
    fn encoded_unorm_color_space_always_applies_the_oetf() {
        assert_eq!(
            EmbeddedColorSpace::EncodedUnorm.fragment_entry_point(),
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

    // ---- GPU acceptance tests (#175): the host-embedding contract end to end.
    //
    // Reuses the #106 surfaceless offscreen harness to stand in for "the
    // host's device/queue/target" — `OffscreenRenderer` already exposes
    // exactly the borrowed `device()` / `queue()` / `view()` / `format()`
    // triple a real host would hand `EmbeddedRenderer`. Gated on `gpu-tests`
    // only (no `slug` dependency — Phase 1 is the triangle lane).
    #[cfg(feature = "gpu-tests")]
    mod gpu {
        use crate::render::embedded::{EmbeddedColorSpace, EmbeddedRenderer};
        use crate::render::offscreen::OffscreenRenderer;
        use crate::Scene;
        use mkui_text::BitmapTextSystem;

        fn host(width: u32, height: u32) -> OffscreenRenderer {
            OffscreenRenderer::new(width, height).expect(
                "offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner",
            )
        }

        /// Read the RGBA8 texel at `(x, y)` out of a tightly-packed
        /// `read_rgba()` buffer of the given `width`.
        fn pixel_at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
            let idx = ((y * width + x) * 4) as usize;
            [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
        }

        fn assert_close(actual: [u8; 4], expected: [u8; 4], tolerance: u8, what: &str) {
            for channel in 0..4 {
                let diff = actual[channel].abs_diff(expected[channel]);
                assert!(
                    diff <= tolerance,
                    "{what}: channel {channel} was {} (expected ~{}, tolerance {tolerance})",
                    actual[channel],
                    expected[channel]
                );
            }
        }

        #[test]
        fn render_into_leaves_untouched_pixels_alone_and_alpha_blends_covered_ones() {
            // `OffscreenRenderer::format()` is `Rgba8Unorm` — a plain non-sRGB
            // UNORM store, so this is `EmbeddedColorSpace::EncodedUnorm`.
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

            // A partial, translucent quad covering only the left half of the
            // target — proves two things at once: pixels the quad never
            // touches must survive `LoadOp::Load` byte-for-byte (the
            // preservation contract), and pixels it does cover must show a
            // real alpha-blended composite, not a full overwrite (the
            // draw-on-top contract) — an opaque, full-viewport quad (the
            // original version of this test) could not distinguish "loads
            // then blends" from "clears then overwrites".
            let mut scene = Scene::new(crate::Size::new(size as f32, size as f32));
            scene.push(crate::Primitive::Quad(crate::Quad {
                rect: crate::Rect::new(
                    crate::Point::new(0.0, 0.0),
                    crate::Size::new(size as f32 / 2.0, size as f32),
                ),
                fill: crate::Color::rgba(0.0, 1.0, 0.0, 0.5),
                corner_radii: crate::CornerRadii::all(0.0),
                stroke: None,
            }));

            // One encoder, one submission — the host's — carrying both the
            // (already-recorded) host content and mkui's UI pass.
            let embedded = EmbeddedRenderer::new(
                host.device(),
                host.format(),
                EmbeddedColorSpace::EncodedUnorm,
            );
            let mut encoder =
                host.device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("host+mkui embedded test encoder"),
                    });
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

            // Untouched: a pixel well inside the right (uncovered) half must
            // be byte-identical to the host's pristine clear — `LoadOp::Load`
            // preserved it, nothing re-cleared or overwrote it.
            let untouched = pixel_at(&after, size, (size * 3) / 4, size / 2);
            assert_eq!(
                untouched,
                [255, 0, 0, 255],
                "a pixel outside the UI quad must survive untouched"
            );

            // Blended: a pixel inside the covered (left) half must be the
            // 50%-alpha "over" composite of the UI's green onto the host's
            // red — half red, half green, full coverage alpha — not a flat
            // overwrite and not the untouched red.
            let covered = pixel_at(&after, size, size / 4, size / 2);
            assert_close(
                covered,
                [128, 128, 0, 255],
                2,
                "blended pixel under the UI quad",
            );
        }

        #[test]
        fn render_into_leaves_content_untouched_on_an_empty_scene() {
            let size = 32u32;
            let host = host(size, size);
            host.clear(wgpu::Color::BLACK);
            let before = host.read_rgba().expect("baseline readback");

            let embedded = EmbeddedRenderer::new(
                host.device(),
                host.format(),
                EmbeddedColorSpace::EncodedUnorm,
            );
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
