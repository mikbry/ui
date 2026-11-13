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
//! Phase 1 (#175) covered the triangle lane only; #234 extends
//! [`EmbeddedRenderer::render_into`](crate::EmbeddedRenderer::render_into)
//! to also drive the Slug vector-glyph lane (`slug` feature, ADR 0007) and
//! `Primitive::VectorPath`, the same ordered command-classification +
//! lane-flush shape [`super::Renderer::render`] already implements (see
//! [`crate::render_command`]). #259 completes the component-text half of
//! that: under `slug`, `EmbeddedRenderer::set_slug_font` selects a
//! registered Slug face for every component `Primitive::Text` to route
//! through, mirroring `Renderer::set_slug_font` — `render_into` expands
//! the scene through `slug_text::expand_slug_text` before classification,
//! same as the standalone path. (These three names are plain code spans,
//! not doc links: all three exist only under `slug`, so a link would dangle
//! on an off-feature build.) (#187) real backdrop blur
//! (`Primitive::Backdrop`) is still not wired in; those primitives are
//! silently skipped (the same no-op [`crate::tessellate_primitives`] already
//! gives them on an unselected lane), not rejected. A glass panel still
//! renders *something* here: it also emits a translucent `Quad` (the tint)
//! that composites normally through the triangle lane — it just loses the
//! blur, degrading to a flat tint over whatever the host already drew.
//! Follow-up: extending the embedded path to the backdrop-blur lane is
//! #187's own follow-up, so this stays scoped to render lanes that compose
//! into the host's own view directly, with no sampling-the-frame-so-far
//! step.
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

use std::ops::Range;

#[cfg(feature = "slug")]
use mkui_text::FontId;
use mkui_text::TextSystem;

use crate::render_command::{build_render_commands, classify_primitive, RenderCommand};
use crate::{tessellate_primitives, Scene};

#[cfg(feature = "slug")]
use mkui_vector2d::{SlugBlobCache, SlugConfig};
#[cfg(feature = "slug")]
use mkui_vector2d_wgpu::{PreparedSlug, SlugAdapter};

use super::{build_ui_pipeline, gui_vertices};
#[cfg(feature = "slug")]
use super::{scene_slug_glyphs, scene_vector_paths, vector_path_paints};

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

    /// Picks the Slug-lane fragment entry point for this color space (#234),
    /// mirroring [`Self::fragment_entry_point`] for the triangle lane — see
    /// the module docs' "Color space" section. `slug.wgsl`'s `fs_slug` makes
    /// the same pass-through assumption `fs_ui_triangles` does (the authored
    /// color is already linear and the target either auto-encodes on write
    /// or is itself a linear store); `fs_slug_encode_srgb` applies the same
    /// manual OETF `fs_ui_triangles_encode_srgb` does, for the one UNORM-
    /// target case that needs it.
    #[cfg(feature = "slug")]
    fn slug_fragment_entry_point(self) -> &'static str {
        match self {
            EmbeddedColorSpace::Linear => "fs_slug",
            EmbeddedColorSpace::EncodedUnorm => "fs_slug_encode_srgb",
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
    /// Native Slug glyph + vector-path lane adapter (#234), present only
    /// under the `slug` feature. Built once against the host's borrowed
    /// device, target format, and the Slug-lane fragment entry point
    /// [`EmbeddedColorSpace::slug_fragment_entry_point`] picked for
    /// `color_space` — mirroring [`super::Renderer`]'s own `slug_adapter`
    /// field — so [`render_into`](Self::render_into) can drive
    /// `Primitive::SlugGlyph` / `Primitive::VectorPath` the same way the
    /// standalone path does. Off-feature builds never construct it (Phase 1
    /// triangle-lane-only behavior, unchanged).
    #[cfg(feature = "slug")]
    slug_adapter: SlugAdapter,
    /// The registered face component text routes through the Slug lane
    /// against, or `None` to keep every component `Primitive::Text` on the
    /// bitmap lane — mirrors [`super::Renderer::slug_font_id`]/
    /// [`super::Renderer::set_slug_font`] (#171 Part B.2). Set via
    /// [`EmbeddedRenderer::set_slug_font`]; `None` by default, so a host that
    /// never opts a font in stays byte-identical to the pre-#259 bitmap-only
    /// embedded path.
    #[cfg(feature = "slug")]
    slug_font_id: Option<FontId>,
    /// Persistent Slug blob cache backing [`crate::slug_text::expand_slug_text`]
    /// — held across frames (like [`super::Renderer::slug_text_cache`]) so a
    /// warm cache never re-decodes or re-encodes an already-seen glyph.
    #[cfg(feature = "slug")]
    slug_text_cache: SlugBlobCache,
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
        #[cfg(feature = "slug")]
        let slug_adapter = SlugAdapter::with_fragment_entry_point(
            device,
            target_format,
            color_space.slug_fragment_entry_point(),
        );
        Self {
            ui_pipeline,
            target_format,
            #[cfg(feature = "slug")]
            slug_adapter,
            #[cfg(feature = "slug")]
            slug_font_id: None,
            #[cfg(feature = "slug")]
            slug_text_cache: SlugBlobCache::new(SlugConfig::new(16, 16, 1)),
        }
    }

    /// The host target format this renderer's pipeline was built against.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }

    /// Select the registered [`FontId`] component text (every widget renders
    /// through exactly one [`Primitive::Text`](crate::Primitive::Text) seam)
    /// should route through the Slug lane against, or `None` to keep the
    /// bitmap lane as the sole path — mirrors
    /// [`super::Renderer::set_slug_font`]. Per glyph/run that still can't be
    /// drawn as Slug falls back to the bitmap lane automatically; the widget
    /// as a whole always renders.
    #[cfg(feature = "slug")]
    pub fn set_slug_font(&mut self, font_id: Option<FontId>) {
        self.slug_font_id = font_id;
    }

    /// Record the UI render pass for `scene` into the host's own `encoder`
    /// and `view`, loading the existing contents (`wgpu::LoadOp::Load`) so
    /// the host's already-drawn 3D scene survives underneath — this call
    /// never clears. Does not create an encoder, does not submit `queue`,
    /// and does not present; the host does all three exactly once per frame,
    /// around this call. `queue` is only used (under the `slug` feature) to
    /// upload this frame's Slug glyph / vector-path GPU resources — see
    /// `mkui_vector2d_wgpu::SlugAdapter::prepare`.
    ///
    /// Drives the same ordered command stream [`super::Renderer::render`]
    /// does (#234, see [`crate::render_command`]): triangle-lane primitives
    /// (UI shapes, bitmap text, borders) and Slug-lane primitives
    /// (`Primitive::SlugGlyph` / `Primitive::VectorPath`, `slug` feature
    /// only) interleave in `Scene::primitives` order, each lane switch
    /// flushing the other so paint order holds. `Primitive::Backdrop` is
    /// still not wired in (see the module docs); it is silently skipped,
    /// not rejected, same as before.
    ///
    /// `target_size` is the physical-pixel size of `view`'s underlying
    /// texture, used (like [`super::Renderer::render`]) to derive the
    /// physical-pixels-per-logical-pixel ratio against `scene.viewport`
    /// (logical pixels, ADR 0006 §"Viewport units contract") for text/bitmap
    /// device-pixel snapping. The host, not this renderer, owns resize — it
    /// already knows its own target's size every frame.
    #[allow(clippy::too_many_arguments)] // every argument is load-bearing, borrowed host state (#234 added `queue`); same escape hatch as `components::dot`/`components::button`.
    pub fn render_into(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        target_size: (u32, u32),
        scene: &Scene,
        text_system: &dyn TextSystem,
    ) {
        // Only the `slug`-gated `SlugAdapter::prepare`/`prepare_path_paints`
        // calls below touch `queue`; an off-feature build never reaches
        // them, so the parameter would otherwise go unused.
        #[cfg(not(feature = "slug"))]
        let _ = queue;

        let (target_width, _) = target_size;
        let device_pixel_ratio = target_width as f32 / scene.viewport.width.max(1.0);

        // #259: when a Slug font is selected, expand every component's
        // `Primitive::Text` into Slug + (per-run/per-glyph fallback) bitmap
        // primitives *before* command classification — mirrors
        // `Renderer::render`'s own `expanded_primitives` (#171 Part B.2).
        // With no font selected (the default) or the `slug` feature off,
        // `primitives` is exactly `scene.primitives` — byte-identical to the
        // pre-#259 bitmap-only embedded path.
        #[cfg(feature = "slug")]
        let expanded_primitives = self.slug_font_id.map(|font_id| {
            crate::slug_text::expand_slug_text(
                &scene.primitives,
                font_id,
                text_system,
                &mut self.slug_text_cache,
            )
        });
        #[cfg(feature = "slug")]
        let primitives: &[crate::types::Primitive] =
            expanded_primitives.as_deref().unwrap_or(&scene.primitives);
        #[cfg(not(feature = "slug"))]
        let primitives: &[crate::types::Primitive] = &scene.primitives;

        // Tessellate a contiguous triangle-lane primitive range into one
        // vertex buffer, or `None` when it yields no geometry — mirrors
        // `Renderer::render`'s own `make_triangles` closure.
        let make_triangles = |range: Range<usize>| -> Option<LaneDraw> {
            let triangles =
                tessellate_primitives(&primitives[range], text_system, device_pixel_ratio);
            let vertices = gui_vertices(&triangles, scene.viewport.width, scene.viewport.height);
            if vertices.is_empty() {
                return None;
            }
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mkui-wgpu Embedded UI Vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            Some(LaneDraw::Triangles {
                buffer,
                vertex_count: vertices.len() as u32,
            })
        };

        // With the `slug` feature off, `classify_primitive` only ever
        // assigns `UiTriangles` / `BitmapText` / `Backdrop` (there is no
        // `Primitive::SlugGlyph` / `Primitive::VectorPath` variant to
        // classify), so this collapses to exactly the Phase 1 behavior: one
        // contiguous triangle run per `Backdrop` boundary, with `Backdrop`
        // itself still silently skipped.
        let commands = build_render_commands(primitives, classify_primitive);
        let mut lane_draws: Vec<LaneDraw> = Vec::new();
        let mut pending_triangles: Option<Range<usize>> = None;
        for command in &commands {
            let range = command.primitives();
            match command {
                RenderCommand::UiTriangles(_) | RenderCommand::BitmapText(_) => {
                    pending_triangles = Some(match pending_triangles {
                        Some(open) => open.start..range.end,
                        None => range,
                    });
                }
                RenderCommand::SlugGlyphs(_) => {
                    // Flush any open triangle run so the Slug glyphs
                    // composite over exactly the primitives that preceded
                    // them — same invariant as `Renderer::render`.
                    if let Some(open) = pending_triangles.take() {
                        lane_draws.extend(make_triangles(open));
                    }
                    // Without the `slug` feature `classify_primitive` never
                    // emits a `SlugGlyphs` command, so this arm cannot run.
                    #[cfg(feature = "slug")]
                    {
                        let glyphs = scene_slug_glyphs(&primitives[range]);
                        if let Some(prepared) = self.slug_adapter.prepare(
                            device,
                            queue,
                            [scene.viewport.width, scene.viewport.height],
                            device_pixel_ratio,
                            &glyphs,
                        ) {
                            lane_draws.push(LaneDraw::Slug(prepared));
                        }
                    }
                }
                RenderCommand::VectorPath(_) => {
                    // Same lane-flush invariant as the `SlugGlyphs` arm
                    // above.
                    if let Some(open) = pending_triangles.take() {
                        lane_draws.extend(make_triangles(open));
                    }
                    // Without the `slug` feature `classify_primitive` never
                    // emits a `VectorPath` command, so this arm cannot run.
                    #[cfg(feature = "slug")]
                    {
                        let paths = scene_vector_paths(&primitives[range]);
                        let paints = vector_path_paints(&paths);
                        if let Some(prepared) = self.slug_adapter.prepare_path_paints(
                            device,
                            queue,
                            [scene.viewport.width, scene.viewport.height],
                            device_pixel_ratio,
                            &paints,
                        ) {
                            lane_draws.push(LaneDraw::Slug(prepared));
                        }
                    }
                }
                RenderCommand::Backdrop(_) => {
                    // Real backdrop blur (#187) is still not wired into the
                    // embedded path (see the module docs) — flush any open
                    // triangle run (same lane-flush invariant as the arms
                    // above) and silently skip the backdrop primitive
                    // itself, the same no-op this renderer has always given
                    // it.
                    if let Some(open) = pending_triangles.take() {
                        lane_draws.extend(make_triangles(open));
                    }
                }
            }
        }
        if let Some(open) = pending_triangles.take() {
            lane_draws.extend(make_triangles(open));
        }

        if lane_draws.is_empty() {
            return;
        }

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
        for draw in &lane_draws {
            match draw {
                LaneDraw::Triangles {
                    buffer,
                    vertex_count,
                } => {
                    pass.set_pipeline(&self.ui_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*vertex_count, 0..1);
                }
                #[cfg(feature = "slug")]
                LaneDraw::Slug(prepared) => {
                    self.slug_adapter.draw(&mut pass, prepared);
                }
            }
        }
    }
}

/// One frame's per-command GPU draw resource for [`EmbeddedRenderer`]
/// (#234), built before the render pass so it outlives the borrow — the
/// embedded parallel to [`super::LaneDraw`]. There is no `Backdrop` variant:
/// unlike the standalone path, `EmbeddedRenderer` has no backdrop-blur pass
/// to sample into, so `RenderCommand::Backdrop` never produces a draw (see
/// [`EmbeddedRenderer::render_into`]).
enum LaneDraw {
    Triangles {
        buffer: wgpu::Buffer,
        vertex_count: u32,
    },
    #[cfg(feature = "slug")]
    Slug(PreparedSlug),
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
    // only — the triangle lane has no `slug` dependency. The Slug/vector-path
    // lane's own acceptance tests (#234) live in the nested `slug_lane`
    // module below, dual-gated on `gpu-tests` + `slug` the same way
    // `render::slug_gpu` and friends are.
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
            let mut embedded = EmbeddedRenderer::new(
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
                host.queue(),
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

            let mut embedded = EmbeddedRenderer::new(
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
                host.queue(),
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

        // ---- GPU acceptance tests (#234): the Slug glyph + vector-path
        // lanes, dual-gated on `gpu-tests` + `slug` the same way
        // `render::slug_gpu` / `render::dot_vector_gpu` are. Reuses this
        // module's `host` / `pixel_at` / `assert_close` helpers.
        #[cfg(feature = "slug")]
        mod slug_lane {
            use std::sync::Arc;

            use mkui_vector2d::Vec2;
            use mkui_vector2d_wgpu::{
                BandRange, GlyphBounds, PlacedSlugGlyph, SlugCurve, SlugGlyph,
            };

            use super::{assert_close, host, pixel_at};
            use crate::components::dot;
            use crate::render::embedded::{EmbeddedColorSpace, EmbeddedRenderer};
            use crate::theme::{DotSize, DotVariant, WgpuTheme};
            use crate::types::{DotAnimation, Point, Scene};
            use mkui_text::BitmapTextSystem;

            /// A hand-authored filled square glyph, font units y-up, bounds
            /// (0,0)-(100,100) — same shape as `render::slug_gpu`'s fixture,
            /// duplicated locally since that module is private to its own
            /// file (test-only, not reusable across files).
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
                        line(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)),
                        line(Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)),
                        line(Vec2::new(100.0, 100.0), Vec2::new(0.0, 100.0)),
                        line(Vec2::new(0.0, 100.0), Vec2::new(0.0, 0.0)),
                    ],
                    horizontal_bands: vec![BandRange {
                        lower: 0.0,
                        upper: 100.0,
                        first_curve: 0,
                        curve_count: 2,
                    }],
                    horizontal_curve_indices: vec![1, 3],
                    vertical_bands: vec![BandRange {
                        lower: 0.0,
                        upper: 100.0,
                        first_curve: 0,
                        curve_count: 2,
                    }],
                    vertical_curve_indices: vec![2, 0],
                })
            }

            #[test]
            fn render_into_drives_the_slug_glyph_lane_with_the_host_color_space() {
                // `OffscreenRenderer::format()` is `Rgba8Unorm`, so this is
                // `EmbeddedColorSpace::EncodedUnorm` — exercising
                // `fs_slug_encode_srgb`, not the pass-through `fs_slug` the
                // standalone `Renderer` always uses. A mid-gray fill (`0.5`)
                // is deliberately not `0.0`/`1.0`: those two are fixed points
                // of the sRGB OETF, so a mistaken pass-through couldn't be
                // told apart from a correct encode at full coverage/alpha.
                //
                // `scene_slug_glyphs` (the scene-level seam `render_into`
                // always goes through) linearizes this *authored*
                // sRGB-perceptual `0.5` to `≈0.214` before it ever reaches
                // the GPU — same as every other Slug/vector-path color in
                // the tree. `fs_slug_encode_srgb` then re-encodes `≈0.214`
                // back to `≈0.5` (the two are exact inverses), so with full
                // coverage and `alpha=1.0` (no background to blend against)
                // the correctly-encoding pipeline's final byte is `≈128` —
                // the *same* byte the authored `0.5` started as. A pipeline
                // that regressed to the pass-through `fs_slug` would instead
                // write the un-encoded linear `0.214` straight through, i.e.
                // `≈55`.
                let size = 64u32;
                let host = host(size, size);
                host.clear(wgpu::Color::BLACK);
                let before = host.read_rgba().expect("baseline readback");

                let mut scene = Scene::new(crate::Size::new(size as f32, size as f32));
                scene.slug_glyph(PlacedSlugGlyph {
                    blob: square_glyph(),
                    // Scale 0.5 px/unit, pen origin bottom-left — lands the
                    // square at screen x,y in [7,57], same placement as
                    // `render::slug_gpu`'s fixture.
                    origin_px: [7.0, 57.0],
                    scale_px_per_unit: 0.5,
                    color: [0.5, 0.5, 0.5, 1.0],
                    cap_height_px: f32::INFINITY,
                });

                let mut embedded = EmbeddedRenderer::new(
                    host.device(),
                    host.format(),
                    EmbeddedColorSpace::EncodedUnorm,
                );
                let mut encoder =
                    host.device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("host+mkui embedded slug lane test encoder"),
                        });
                embedded.render_into(
                    host.device(),
                    host.queue(),
                    &mut encoder,
                    host.view(),
                    (size, size),
                    &scene,
                    &BitmapTextSystem::new(),
                );
                host.queue().submit(Some(encoder.finish()));

                let after = host.read_rgba().expect("post-UI readback");

                // Well inside the square, away from its anti-aliased edges:
                // full coverage, so (per the module doc above) the correctly
                // round-tripped byte is `≈128` — not the un-encoded `≈55` a
                // pass-through `fs_slug` would have written.
                let inside = pixel_at(&after, size, 32, 32);
                assert_close(
                    inside,
                    [128, 128, 128, 255],
                    3,
                    "Slug glyph interior pixel must be sRGB-encoded for an EncodedUnorm host",
                );

                // Untouched: a pixel outside the glyph's bounds must survive
                // `LoadOp::Load` byte-for-byte.
                let outside = pixel_at(&after, size, 2, 2);
                assert_eq!(
                    outside,
                    pixel_at(&before, size, 2, 2),
                    "a pixel outside the Slug glyph must survive untouched"
                );
            }

            #[test]
            fn render_into_drives_the_vector_path_lane() {
                // The real `dot()` component, compiled with `slug`, emits a
                // `Primitive::VectorPath` circle — the same scene-level seam
                // `render::dot_vector_gpu` exercises against the standalone
                // path, driven here through `EmbeddedRenderer::render_into`
                // instead.
                let size = 32u32;
                let host = host(size, size);
                host.clear(wgpu::Color::BLACK);
                let before = host.read_rgba().expect("baseline readback");

                let theme = WgpuTheme::default();
                let center = Point::new(16.0, 16.0);
                let mut scene = Scene::new(crate::Size::new(size as f32, size as f32));
                dot(
                    &mut scene,
                    center,
                    DotVariant::Ok,
                    DotSize::Md,
                    false,
                    DotAnimation::None,
                    &theme,
                );

                let mut embedded = EmbeddedRenderer::new(
                    host.device(),
                    host.format(),
                    EmbeddedColorSpace::EncodedUnorm,
                );
                let mut encoder =
                    host.device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("host+mkui embedded vector path test encoder"),
                        });
                embedded.render_into(
                    host.device(),
                    host.queue(),
                    &mut encoder,
                    host.view(),
                    (size, size),
                    &scene,
                    &BitmapTextSystem::new(),
                );
                host.queue().submit(Some(encoder.finish()));

                let after = host.read_rgba().expect("post-UI readback");

                // `fill_color.to_linear_rgba()` (the scene-level seam) and
                // `fs_slug_encode_srgb`'s OETF are exact inverses, so with
                // full coverage the shader's output color round-trips back
                // to the theme's authored sRGB fill — but the theme's
                // `success` token is *not* fully opaque (`a = 0.96`), so
                // that round-tripped color still blends against the black
                // clear per `wgpu::BlendState::ALPHA_BLENDING`
                // (`src * src_a + dst * (1 - src_a)`, `dst = 0` here): the
                // expected byte is `fill.{r,g,b} * fill.a * 255`, not the
                // fill color alone. A pipeline that regressed to the
                // pass-through `fs_slug` would instead blend the
                // *un-encoded* linear fill (visibly darker).
                let fill = theme.dot_style(DotVariant::Ok, DotSize::Md).fill;
                let expected = [
                    (fill.r * fill.a * 255.0).round() as u8,
                    (fill.g * fill.a * 255.0).round() as u8,
                    (fill.b * fill.a * 255.0).round() as u8,
                    255,
                ];
                let covered = pixel_at(&after, size, 16, 16);
                assert_close(
                    covered,
                    expected,
                    3,
                    "the dot's own centre must round-trip to the authored sRGB fill color blended over black",
                );

                // Untouched: a corner well outside the dot's radius must
                // survive `LoadOp::Load` byte-for-byte.
                let outside = pixel_at(&after, size, 1, 1);
                assert_eq!(
                    outside,
                    pixel_at(&before, size, 1, 1),
                    "a pixel outside the vector path must survive untouched"
                );
            }

            // ---- #259: component `Primitive::Text` routed through
            // `set_slug_font` + `expand_slug_text`, mirroring the standalone
            // `Renderer`'s own #171 Part B.2 wiring.

            const ABEL: &[u8] =
                include_bytes!("../../../mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

            /// Register the licensed Abel fixture as a Slug provider beside
            /// the built-in bitmap face.
            fn registered() -> (mkui_text::CompositeTextSystem, mkui_text::FontId) {
                let mut sys = mkui_text::CompositeTextSystem::new();
                let id = sys
                    .register_sfnt_face(Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
                    .expect("Abel registers");
                (sys, id)
            }

            /// One `Primitive::Text` scene, identical across both tests below
            /// — only `EmbeddedRenderer::set_slug_font` differs between runs.
            fn text_scene(size: u32) -> Scene {
                use crate::types::{FontFaceId, Text, TextAlign, TextStyle};

                let mut scene = Scene::new(crate::Size::new(size as f32, size as f32));
                scene.text(Text {
                    rect: crate::Rect::new(Point::new(4.0, 32.0), crate::Size::new(88.0, 32.0)),
                    content: "Mag".to_string(),
                    style: TextStyle {
                        font: FontFaceId(mkui_text::FontId::BITMAP.raw()),
                        font_size_px: 24.0,
                        line_height_px: 28.0,
                        color: crate::Color::rgb(1.0, 1.0, 1.0),
                        align: TextAlign::Start,
                    },
                });
                scene
            }

            /// Render `scene` into a fresh offscreen host through `text_system`,
            /// building the `EmbeddedRenderer` against that *same* host's
            /// device/format (a pipeline built against one `OffscreenRenderer`'s
            /// device cannot draw into another's), optionally selecting
            /// `slug_font_id`. Returns the readback RGBA bytes.
            fn render_text(
                text_system: &dyn mkui_text::TextSystem,
                slug_font_id: Option<mkui_text::FontId>,
                size: u32,
                scene: &Scene,
            ) -> Vec<u8> {
                let host = host(size, size);
                host.clear(wgpu::Color::BLACK);
                let mut embedded = EmbeddedRenderer::new(
                    host.device(),
                    host.format(),
                    EmbeddedColorSpace::EncodedUnorm,
                );
                embedded.set_slug_font(slug_font_id);
                let mut encoder =
                    host.device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("host+mkui embedded component-text test encoder"),
                        });
                embedded.render_into(
                    host.device(),
                    host.queue(),
                    &mut encoder,
                    host.view(),
                    (size, size),
                    scene,
                    text_system,
                );
                host.queue().submit(Some(encoder.finish()));
                host.read_rgba().expect("component-text readback")
            }

            #[test]
            fn render_into_keeps_component_text_on_the_bitmap_lane_by_default() {
                // A `CompositeTextSystem` with a Slug face registered, but no
                // `set_slug_font` call, must render component text exactly
                // like the plain `BitmapTextSystem` every other test in this
                // file uses — the "default OFF" half of #171 Part B.2's
                // contract, now proven for the embedded path too.
                let size = 96u32;
                let (sys, _abel_id) = registered();
                let scene = text_scene(size);

                let composite_bytes = render_text(&sys, None, size, &scene);
                let bitmap_bytes = render_text(&BitmapTextSystem::new(), None, size, &scene);

                assert_eq!(
                    composite_bytes, bitmap_bytes,
                    "an embedded renderer with no `set_slug_font` call must stay byte-identical \
                     to the bitmap-only path, even when the text system has a Slug face registered"
                );
            }

            #[test]
            fn render_into_routes_component_text_through_the_slug_lane_once_a_font_is_selected() {
                // The regression #259 exists to fix: before `EmbeddedRenderer`
                // learned `set_slug_font`/`expand_slug_text`, swapping in a
                // `CompositeTextSystem` with a registered real face compiled
                // and ran, but the registered face was never looked up —
                // `tessellate_text` always asks for `FontId::BITMAP`. This
                // proves selecting a Slug font actually changes the rendered
                // output.
                let size = 96u32;
                let (sys, abel_id) = registered();
                let scene = text_scene(size);

                let bitmap_bytes = render_text(&sys, None, size, &scene);
                let slug_bytes = render_text(&sys, Some(abel_id), size, &scene);

                assert_ne!(
                    bitmap_bytes, slug_bytes,
                    "selecting a Slug font via `set_slug_font` must change the rendered \
                     component text (bitmap lane vs. real anti-aliased Slug glyphs) — an \
                     unchanged readback means `render_into` never reached `expand_slug_text`"
                );
            }
        }
    }
}
