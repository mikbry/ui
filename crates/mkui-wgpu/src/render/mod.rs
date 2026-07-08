//! WGPU triangle renderer for mkui UI primitives.
//! Owns surface configuration, render pipelines, vertex upload, and frame
//! submission for tessellated UI triangles.
//!
//! # Kept vs dropped — port of the upstream reference
//!
//! The reference renderer this port draws from (an unrelated 3D scene
//! viewer) is a 2 854-line, multi-pass pipeline. mkui only needs the load-
//! bearing 2D UI slice; the rest is 3D-scene concerns that have no place
//! in a UI renderer.
//!
//! **Kept**
//!
//! - `Renderer::new` adapter / device / queue / surface negotiation. mkui
//!   picks the first sRGB surface format, falls back to the default, and
//!   defaults to `Fifo` present mode — same shape as the reference.
//! - MSAA capability probe (`pick_sample_count`). Both the swapchain color
//!   format **and** the depth format have to advertise the requested
//!   sample count, otherwise pipeline creation fails at runtime. mkui ships
//!   without depth (the UI pass writes directly to the swapchain), so the
//!   probe only needs the color flags — see `pick_sample_count`.
//! - UI vertex / fragment entry points. Pixel-space scene coordinates →
//!   NDC happens on the CPU in `gui_vertices`; the vertex shader passes
//!   the result through and the fragment shader writes the per-vertex
//!   color. This is the entire load-bearing GPU contract for a 2D UI.
//! - `Renderer::resize` with a zero-size guard. wgpu refuses to configure
//!   a `0×N` or `N×0` surface, so the guard is mandatory, not defensive.
//! - `RenderOutcome` enum (`Drawn` / `Skipped` / `NeedsReconfigure`) so
//!   the caller can decide whether to drive another frame or reconfigure
//!   the surface.
//!
//! **Dropped** (each with one-line rationale)
//!
//! - **3D scene pass** (`vs_scene` / `fs_scene`, lighting uniforms,
//!   per-material shading, vertex AO, tone mapping). mkui has no 3D
//!   scene — every primitive the tessellator emits is a flat colored
//!   triangle.
//! - **Shadow map pass** (`vs_shadow`, depth-only pipeline, PCF sampler,
//!   `ShadowUniform`). No light, no shadows.
//! - **Screen-space ambient occlusion** (`AmbientOcclusionPass`, geometry
//!   prepass writing view-space normals, R8 AO target, scene bind-group
//!   AO slot). UI elements are 2D — no depth or normal to derive occlusion
//!   from.
//! - **Selection outline pass** (`SelectionOutlinePass`, jump-flood ping-
//!   pong on Rg16Float). UI selection is communicated by recoloring the
//!   primitive in the scene, not by a post-process outline.
//! - **Progressive accumulator** (`accumulator.wgsl`, Rgba16Float ping-
//!   pong, running-average weight, `frames_since_input` counter). The UI
//!   has no Monte-Carlo noise to converge — every frame is deterministic
//!   from the scene description.
//! - **Camera / lighting / shadow uniforms.** Replaced by the
//!   identity-NDC mapping in `gui_vertices`. The UI pipeline takes
//!   no bind groups at all.
//! - **Depth attachment.** The UI draws back-to-front in primitive order
//!   and uses alpha blending; depth would only force us to keep a
//!   matching multisampled depth view on resize for no visual gain.

use std::ops::Range;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::render_command::{build_render_commands, classify_primitive, RenderCommand};
use crate::{tessellate_primitives, GuiTriangle, Scene};
use mkui_core::error::MkuiError;
use mkui_text::TextSystem;

#[cfg(feature = "slug")]
use mkui_vector2d_wgpu::{PlacedSlugGlyph, PreparedSlug, SlugAdapter};

/// One frame's per-command GPU draw resource, built before the render pass so
/// it outlives the borrow. The renderer walks these in `Scene::primitives`
/// order so UI/bitmap triangles and Slug glyphs composite in scene paint order
/// (#66). On the `Triangles` lane both `UiTriangles` and `BitmapText` commands
/// share the UI pipeline; the `Slug` lane exists only under the feature.
enum LaneDraw {
    Triangles {
        buffer: wgpu::Buffer,
        vertex_count: u32,
    },
    #[cfg(feature = "slug")]
    Slug(PreparedSlug),
}

/// Surfaceless offscreen renderer + readback harness (#106). Gated behind the
/// `gpu-tests` feature so the displayless default CI `test` job needs no Vulkan
/// ICD; the dedicated Lavapipe GPU job enables it. Reused by the #66 Slug and
/// #67 font GPU acceptance tests.
#[cfg(feature = "gpu-tests")]
pub mod offscreen;

/// Offscreen GPU acceptance tests for the Slug lane (#66). Compiled only in
/// test builds that enable both `gpu-tests` (the #106 surfaceless harness) and
/// `slug` (the adapter). They hand-author curve/band records — no font parser
/// (that is #67) — and run on the same Vulkan/Lavapipe contract as #106.
#[cfg(all(test, feature = "gpu-tests", feature = "slug"))]
mod slug_gpu;

/// Font-backed Slug GPU acceptance tests (#67 Phase 2) — the licensed Abel
/// fixture rendered through the real `SfntTextSystem` → #65 encoder → #66 lane
/// on #106's Lavapipe harness. Same dual gate as [`slug_gpu`].
#[cfg(all(test, feature = "gpu-tests", feature = "slug"))]
mod sfnt_slug_gpu;

/// Linear-space blending GPU acceptance tests (#135). Render the real UI
/// pipeline into the linear intermediate format on #106's Lavapipe harness and
/// read back raw bytes to prove authored colors are linearized and alpha
/// blending composites in linear light. Gated on `gpu-tests` only (no font /
/// Slug dependency).
#[cfg(all(test, feature = "gpu-tests"))]
mod linear_blend_gpu;

/// Preferred MSAA sample count for the UI pass.
///
/// **Pinned to `1` (MSAA off) as the #93 load-bearing fix.** The 4× MSAA
/// path added in Sprint 5 has no StoneSketch parent (the upstream HUD
/// pipeline runs at `MultisampleState::default()` = `sample_count=1`) and
/// is the suspected source of two #93 symptoms on macOS Metal: the gray
/// backdrop darkening on every resize, and `atoms-on-wgpu` rendering empty
/// despite emitting 9012 valid triangles at the CPU stage — both consistent
/// with the MSAA-resolve-into-sRGB step double-applying sRGB encoding.
/// `sample_count=1` is the StoneSketch-proven, visually-correct path; it
/// writes the swapchain view directly with no resolve step.
///
/// The MSAA machinery (`pick_sample_count`, `create_msaa_color_view`, the
/// `msaa_color_view` attachment) is retained but dormant so the follow-up
/// can re-enable it with correct sRGB orchestration — see #95.
const MSAA_SAMPLE_COUNT_PREF: u32 = 1;

/// Linear-space intermediate color format. Every lane (UI triangles, bitmap
/// text, Slug curves) renders into a texture of this format, so alpha blending
/// composites in physically-correct linear light; a final [present pass](present.wgsl)
/// encodes it to the (usually sRGB) surface. `Rgba8Unorm` is a plain linear
/// UNORM — no hardware sRGB encode on write — and matches the offscreen
/// harness `offscreen::TARGET_FORMAT` so windowed and headless paths blend
/// identically. See ADR 0006 §"Color space + blending". MSAA (when #134
/// re-enables it) resolves into this same linear target; the sRGB encode stays
/// isolated in the present pass.
const INTERMEDIATE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Outcome of a single `Renderer::render` call. Mirrors the upstream
/// reference's contract so the event-loop shell knows when to reconfigure
/// the surface vs. just drive another frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderOutcome {
    /// Frame was submitted and presented.
    Drawn,
    /// Frame was skipped because the surface was not ready
    /// (timeout / occluded / validation). The next redraw should retry.
    Skipped,
    /// The surface is outdated or lost; caller must call `resize` (or
    /// reconfigure) before the next render.
    NeedsReconfigure,
}

/// 2D UI renderer. Owns the wgpu device, the surface configuration, the
/// UI pipeline, and the optional multisampled color attachment.
#[derive(Debug)]
pub struct Renderer {
    /// Held so the surface lifetime stays bound to a real window — the
    /// `Surface<'static>` we create from `Arc<Window>` borrows the window
    /// internally.
    _window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Effective MSAA sample count for the UI pass. `1` on adapters that
    /// don't support 4× MSAA on the chosen swapchain color format.
    sample_count: u32,
    ui_pipeline: wgpu::RenderPipeline,
    /// Linear-space intermediate color target ([`INTERMEDIATE_FORMAT`]) every
    /// lane composites into. Recreated on resize. The present pass samples it
    /// and encodes to the surface.
    intermediate_view: wgpu::TextureView,
    /// Full-screen pass that reads [`Self::intermediate_view`] and writes the
    /// surface, applying the linear→sRGB encode when the surface is UNORM.
    present_pipeline: wgpu::RenderPipeline,
    /// Layout for the present bind group (intermediate texture + sampler +
    /// encode-flag uniform). Held so the bind group can be rebuilt on resize.
    present_bind_group_layout: wgpu::BindGroupLayout,
    /// Sampler for the 1:1 intermediate→surface blit. Nearest/clamp — the blit
    /// is size-matched so no filtering is required.
    present_sampler: wgpu::Sampler,
    /// Uniform carrying the present pass's `encode_srgb` flag (`1` when the
    /// surface is UNORM and the shader must apply the OETF; `0` when the sRGB
    /// surface encodes on write). Fixed for the surface's lifetime.
    present_flags_buffer: wgpu::Buffer,
    /// Present bind group binding the current [`Self::intermediate_view`].
    /// Rebuilt whenever the intermediate view is recreated (resize).
    present_bind_group: wgpu::BindGroup,
    /// Multisampled color attachment the UI pass renders into when
    /// `sample_count > 1`. Resolves into the **linear intermediate** target at
    /// end-of-pass. `None` on the 1× fallback, where the UI pass writes the
    /// intermediate view directly.
    msaa_color_view: Option<wgpu::TextureView>,
    /// Native Slug glyph adapter (#66), present only when the `slug` feature is
    /// enabled. Built once against the swapchain format; invoked inside the UI
    /// render pass so Slug glyphs composite in scene paint order. Off-feature
    /// builds never construct it (v0.9.3 bitmap-only behavior).
    #[cfg(feature = "slug")]
    slug_adapter: SlugAdapter,
}

impl Renderer {
    /// Async constructor — requests an adapter, creates the device + queue,
    /// configures the surface, builds the UI pipeline.
    pub async fn new(window: Arc<Window>) -> Result<Self, MkuiError> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| MkuiError::initialization(format!("create_surface failed: {e}")))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| MkuiError::initialization(format!("request_adapter failed: {e}")))?;

        let (device, queue) = adapter
            .request_device(&device_descriptor())
            .await
            .map_err(|e| MkuiError::initialization(format!("request_device failed: {e}")))?;

        let capabilities = surface.get_capabilities(&adapter);
        let default_format =
            capabilities.formats.first().copied().ok_or_else(|| {
                MkuiError::initialization("surface advertised no supported formats")
            })?;
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(default_format);
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .or_else(|| capabilities.present_modes.first().copied())
            .ok_or_else(|| MkuiError::initialization("surface advertised no present modes"))?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .ok_or_else(|| MkuiError::initialization("surface advertised no alpha modes"))?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // #101 root-cause fix (macOS only): switch the CAMetalLayer backing
        // this surface into synchronous-present mode so the fast-resize jerk
        // is eliminated at the presentation layer. See
        // [`enable_presents_with_transaction`].
        #[cfg(target_os = "macos")]
        enable_presents_with_transaction(&surface);

        // The MSAA probe runs against the linear intermediate format the UI
        // pass actually renders into, not the swapchain format.
        let color_flags = adapter
            .get_texture_format_features(INTERMEDIATE_FORMAT)
            .flags;
        let sample_count = pick_sample_count(color_flags, MSAA_SAMPLE_COUNT_PREF);

        // UI + Slug lanes target the linear intermediate; the surface format is
        // only seen by the present pass.
        let ui_pipeline = build_ui_pipeline(&device, INTERMEDIATE_FORMAT, sample_count);
        let intermediate_view = create_intermediate_view(&device, width, height);
        let msaa_color_view = create_msaa_color_view(&device, width, height, sample_count);

        let (present_pipeline, present_bind_group_layout) = build_present_pipeline(&device, format);
        let present_sampler = create_present_sampler(&device);
        let present_flags_buffer = create_present_flags_buffer(&device, format.is_srgb());
        let present_bind_group = create_present_bind_group(
            &device,
            &present_bind_group_layout,
            &intermediate_view,
            &present_sampler,
            &present_flags_buffer,
        );

        #[cfg(feature = "slug")]
        let slug_adapter = SlugAdapter::new(&device, INTERMEDIATE_FORMAT);

        Ok(Self {
            _window: window,
            surface,
            device,
            queue,
            config,
            sample_count,
            ui_pipeline,
            intermediate_view,
            present_pipeline,
            present_bind_group_layout,
            present_sampler,
            present_flags_buffer,
            present_bind_group,
            msaa_color_view,
            #[cfg(feature = "slug")]
            slug_adapter,
        })
    }

    /// Effective MSAA sample count picked at adapter probe time. `1` when
    /// the renderer fell back to no anti-aliasing on this device.
    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// Current surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Reconfigure the surface (and the MSAA color attachment when
    /// applicable) for a new window size. No-op on `0×N` or `N×0` — wgpu
    /// refuses those.
    pub fn resize(&mut self, width: u32, height: u32) {
        let Some((width, height)) = clamp_resize(width, height) else {
            return;
        };
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.intermediate_view = create_intermediate_view(&self.device, width, height);
        self.msaa_color_view =
            create_msaa_color_view(&self.device, width, height, self.sample_count);
        // The present bind group references the intermediate view by handle, so
        // it must be rebuilt whenever that view is recreated.
        self.present_bind_group = create_present_bind_group(
            &self.device,
            &self.present_bind_group_layout,
            &self.intermediate_view,
            &self.present_sampler,
            &self.present_flags_buffer,
        );
    }

    /// Tessellate `scene` against the supplied text system, upload the
    /// resulting triangles, issue the UI draw call and present.
    pub fn render(
        &mut self,
        scene: &Scene,
        text_system: &dyn TextSystem,
    ) -> Result<RenderOutcome, MkuiError> {
        let (frame, reconfigure_after_present) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return Ok(RenderOutcome::Skipped),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return Ok(RenderOutcome::NeedsReconfigure);
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // The UI pass now clears + composites into the linear intermediate, so
        // the authored backdrop is linearized unconditionally (the target is
        // always linear, never the sRGB surface). The present pass handles the
        // sRGB encode.
        let clear = clear_color();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mkui-wgpu UI Encoder"),
            });

        // Derive the ordered command stream from `Scene::primitives` and build
        // each command's GPU resources up front so they outlive the pass. The
        // renderer always consumes this stream — there is no separate
        // "all UI then all Slug" path — and lane switches happen exactly at
        // scene-order boundaries (#66). Consecutive triangle-lane commands
        // (`UiTriangles` + `BitmapText`, which share the UI pipeline) coalesce
        // into one buffer, since only same-pipeline adjacency may merge; a Slug
        // command flushes the pending triangle run first so paint order holds.
        // With the feature off every command is triangle-lane, so this collapses
        // to a single buffer + draw — byte-identical to the v0.9.3 path.
        // Triangles are projected against the logical-pixel viewport, NOT the
        // physical surface config (#97, ADR 0006 §"Viewport units").
        let commands = build_render_commands(&scene.primitives, classify_primitive);
        let mut lane_draws: Vec<LaneDraw> = Vec::new();

        // Tessellate a contiguous triangle-lane primitive range into one vertex
        // buffer, or `None` when it yields no geometry.
        let make_triangles = |range: Range<usize>| -> Option<LaneDraw> {
            let triangles = tessellate_primitives(&scene.primitives[range], text_system);
            let vertices = gui_vertices(&triangles, scene.viewport.width, scene.viewport.height);
            if vertices.is_empty() {
                return None;
            }
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mkui-wgpu UI Vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            Some(LaneDraw::Triangles {
                buffer,
                vertex_count: vertices.len() as u32,
            })
        };

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
                    // Flush any open triangle run so the Slug glyphs composite
                    // over exactly the primitives that preceded them.
                    if let Some(open) = pending_triangles.take() {
                        lane_draws.extend(make_triangles(open));
                    }
                    // Without the `slug` feature `classify_primitive` never
                    // emits a SlugGlyphs command, so this arm cannot run.
                    #[cfg(feature = "slug")]
                    {
                        let glyphs = scene_slug_glyphs(&scene.primitives[range]);
                        if let Some(prepared) = self.slug_adapter.prepare(
                            &self.device,
                            &self.queue,
                            [scene.viewport.width, scene.viewport.height],
                            &glyphs,
                        ) {
                            lane_draws.push(LaneDraw::Slug(prepared));
                        }
                    }
                }
            }
        }
        if let Some(open) = pending_triangles.take() {
            lane_draws.extend(make_triangles(open));
        }

        {
            // UI/bitmap/Slug lanes composite into the linear intermediate. With
            // MSAA active the multisampled attachment resolves into the linear
            // intermediate (a correct linear→linear average); with MSAA off the
            // pass writes the intermediate view directly. The sRGB encode is the
            // separate present pass below — never here — so there is no
            // double-encode (the Sprint 6 v0.9.1 bug, closed by construction).
            let color_attachment = match self.msaa_color_view.as_ref() {
                Some(msaa) => wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: Some(&self.intermediate_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                },
                None => wgpu::RenderPassColorAttachment {
                    view: &self.intermediate_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                },
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mkui-wgpu UI Pass"),
                color_attachments: &[Some(color_attachment)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Draw each command in scene order within the single pass, so a
            // later command composites over an earlier one (alpha blending is
            // load/store compatible across lanes). UI/bitmap runs use the UI
            // pipeline; Slug runs draw through the adapter (#66).
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

        // Present pass: encode the linear intermediate to the surface. A single
        // full-screen triangle samples the intermediate and (when the surface
        // is UNORM) applies the linear→sRGB OETF; an sRGB surface encodes on
        // write instead. The triangle covers every pixel, so the clear is only
        // to keep the freshly-acquired surface texture defined for validation.
        // See `present.wgsl` + ADR 0006 §"Color space + blending".
        {
            let mut present = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mkui-wgpu Present Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            present.set_pipeline(&self.present_pipeline);
            present.set_bind_group(0, &self.present_bind_group, &[]);
            present.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        drop(view);
        frame.present();
        if reconfigure_after_present {
            self.surface.configure(&self.device, &self.config);
        }
        Ok(RenderOutcome::Drawn)
    }
}

/// Collect the [`PlacedSlugGlyph`]s carried by a primitive slice, in order.
///
/// This is the scene-level seam between the renderer and the
/// `mkui-vector2d-wgpu` adapter (#66): scenes carry Slug glyphs as
/// [`Primitive::SlugGlyph`] variants, and the ordered render path hands each
/// `SlugGlyphs` command's slice here to recover the glyphs for that lane run.
/// #67's outline text system emits these primitives directly, so its glyphs
/// flow through with no further renderer change. Glyphs are cloned (an
/// `Arc<SlugGlyph>` clone is cheap) so the returned owned `Vec` can back a GPU
/// upload outliving the borrowed scene.
///
/// The fill color is **linearized here**, at the render boundary, exactly as
/// [`gui_vertices`] linearizes triangle colors — the Slug lane composites into
/// the linear intermediate, so its authored sRGB-perceptual color must become
/// linear before the coverage shader multiplies it by alpha. Before Sprint 8
/// this lane skipped linearization (it drew straight into an sRGB surface),
/// which composited Slug text in a different space than the UI lane. The
/// size-independent blob (curves/bands) is untouched.
#[cfg(feature = "slug")]
fn scene_slug_glyphs(primitives: &[crate::Primitive]) -> Vec<PlacedSlugGlyph> {
    primitives
        .iter()
        .filter_map(|p| match p {
            crate::Primitive::SlugGlyph(glyph) => {
                use crate::types::srgb_to_linear;
                let [r, g, b, a] = glyph.color;
                Some(PlacedSlugGlyph {
                    color: [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a],
                    ..glyph.clone()
                })
            }
            _ => None,
        })
        .collect()
}

/// Switch the `CAMetalLayer` backing `surface` into synchronous-present mode
/// (`presentsWithTransaction = true`) — the **root-cause** fix for the macOS
/// fast-resize jerk (#101).
///
/// The jerk is a presentation-layer race, not an event-scheduling one (the
/// v0.9.3 `about_to_wait` pump and the #117 `CursorMoved` M2 bridge both
/// operate at the event layer and could not close it). AppKit commits a new
/// window's bounds inside a `CATransaction`, but by default
/// (`presentsWithTransaction = false`) `CAMetalLayer` presents its drawable on
/// an independent, asynchronous timeline. During a live-resize the window rect
/// jumps to the new size before the matching drawable is scheduled, so Core
/// Animation stretches the previous (stale-size) drawable into the new rect for
/// one or more frames — the visible vibration.
///
/// Flipping the layer to `presentsWithTransaction = true` makes presentation
/// synchronous: wgpu-hal's Metal `present` then commits the command buffer,
/// `waitUntilScheduled()`, and calls `drawable.present()` *inside the current
/// `CATransaction`* (see wgpu-hal `metal::Queue::present`). The drawable resize
/// and the window-rect resize commit atomically — no stretch frame. The flag is
/// re-read by wgpu-hal on every `acquire_texture`, so setting it once after
/// `configure` (the layer property persists across reconfigures) is sufficient.
///
/// macOS-only: other backends have no `CAMetalLayer` and no such race, so the
/// call is compile-time gated by the `#[cfg(target_os = "macos")]` at the call
/// site. No public API change; the surface keeps the standard wgpu present
/// path on every other platform.
///
/// References: Tristan Hume, "Glitchless Metal Window Resizing"
/// (<https://thume.ca/2019/06/19/glitchless-metal-window-resizing/>); Raph
/// Levien, "The smooth resize test"
/// (<https://raphlinus.github.io/rust/gui/2019/06/21/smooth-resize-test.html>);
/// wgpu#1168; winit#3644.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // sole FFI seam in the crate — see crate-root `deny` rationale
fn enable_presents_with_transaction(surface: &wgpu::Surface<'static>) {
    // SAFETY: `as_hal` yields the live HAL surface for the current backend; we
    // only read its `CAMetalLayer` and flip one boolean property on it. We do
    // not destroy or alias any wgpu-owned resource, and the returned guard is
    // dropped at the end of this scope (well before the surface is used by the
    // GPU), satisfying `as_hal`'s safety contract. The `None` arm (a non-Metal
    // surface, impossible under this `cfg`) is a silent no-op.
    unsafe {
        if let Some(hal_surface) = surface.as_hal::<wgpu::hal::api::Metal>() {
            hal_surface
                .render_layer()
                .lock()
                .setPresentsWithTransaction(true);
        }
    }
}

/// Shared `DeviceDescriptor` for the windowed [`Renderer`] and the surfaceless
/// `offscreen::OffscreenRenderer`. Both request the same default-limits,
/// no-extra-features device; factoring it here keeps the reusable device work
/// out of the window-bound path without changing its behavior (#106).
pub(crate) fn device_descriptor() -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        label: Some("mkui-wgpu Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }
}

/// Filter out resize requests that wgpu would reject. wgpu refuses to
/// configure a surface with width or height of 0, so the renderer treats
/// those as no-ops rather than calling `surface.configure` and panicking
/// on validation. Exposed at module scope so unit tests can cover the
/// resize zero-guard without standing up a real adapter.
fn clamp_resize(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        None
    } else {
        Some((width, height))
    }
}

/// MSAA capability probe. Returns `preferred` when the swapchain color
/// format advertises support for it, otherwise falls back to `1`. The UI
/// pass has no depth attachment so only the color format matters — the
/// reference also probes the depth format because its scene pass binds
/// depth in the same pipeline.
fn pick_sample_count(color_flags: wgpu::TextureFormatFeatureFlags, preferred: u32) -> u32 {
    if preferred <= 1 {
        return 1;
    }
    if color_flags.sample_count_supported(preferred) {
        preferred
    } else {
        1
    }
}

fn build_ui_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mkui-wgpu UI Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("ui_triangles.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mkui-wgpu UI Pipeline Layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    let targets = [Some(wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mkui-wgpu UI Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_ui_triangles"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Vertex::layout()],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_ui_triangles"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Create the linear-space intermediate color target (`INTERMEDIATE_FORMAT`)
/// every lane composites into. `RENDER_ATTACHMENT` so it can be drawn into and
/// `TEXTURE_BINDING` so the present pass can sample it.
fn create_intermediate_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mkui-wgpu Linear Intermediate"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: INTERMEDIATE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_msaa_color_view(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    sample_count: u32,
) -> Option<wgpu::TextureView> {
    if sample_count <= 1 {
        return None;
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mkui-wgpu UI MSAA Color"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        // The multisampled attachment resolves into the linear intermediate, so
        // it carries the same linear format.
        format: INTERMEDIATE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    Some(texture.create_view(&wgpu::TextureViewDescriptor::default()))
}

/// Build the present pipeline (`present.wgsl`) that encodes the linear
/// intermediate onto the surface, returning it with its bind-group layout
/// (intermediate texture + sampler + `encode_srgb` uniform). The pass is always
/// single-sampled — MSAA lives in the intermediate, already resolved by the
/// time the present pass runs.
fn build_present_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mkui-wgpu Present Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
    });
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mkui-wgpu Present Bind Group Layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mkui-wgpu Present Pipeline Layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let targets = [Some(wgpu::ColorTargetState {
        format: surface_format,
        // Opaque overwrite — the present pass fully replaces the surface.
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mkui-wgpu Present Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_present"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_present"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, bind_group_layout)
}

fn create_present_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("mkui-wgpu Present Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        // 1:1 size-matched blit — nearest is exact and needs no filtering.
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// The present pass encode flag as a 16-byte uniform (`encode_srgb: u32` +
/// padding, matching `present.wgsl`'s `PresentFlags`). `1` when the surface is
/// a plain UNORM format and the shader must apply the sRGB OETF; `0` when the
/// surface is sRGB and encodes on write. Fixed for the surface's lifetime.
fn create_present_flags_buffer(device: &wgpu::Device, surface_is_srgb: bool) -> wgpu::Buffer {
    let encode_srgb: u32 = if surface_is_srgb { 0 } else { 1 };
    let data: [u32; 4] = [encode_srgb, 0, 0, 0];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mkui-wgpu Present Flags"),
        contents: bytemuck::cast_slice(&data),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn create_present_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    intermediate_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    flags_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mkui-wgpu Present Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(intermediate_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: flags_buffer.as_entire_binding(),
            },
        ],
    })
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Map `Scene`'s pixel-space triangles into NDC, linearizing each authored
/// sRGB-perceptual color to the linear space the intermediate framebuffer
/// composites in. Linearization is now unconditional — the render target is
/// always the linear intermediate ([`INTERMEDIATE_FORMAT`]), never the sRGB
/// surface — so there is no longer a target-format branch. See
/// [`Color::to_linear_rgba`](crate::types::Color::to_linear_rgba).
fn gui_vertices(triangles: &[GuiTriangle], width: f32, height: f32) -> Vec<Vertex> {
    let width = width.max(1.0);
    let height = height.max(1.0);
    triangles
        .iter()
        .flat_map(|triangle| {
            let color = triangle.color.to_linear_rgba();
            triangle.points.into_iter().map(move |point| Vertex {
                position: [
                    (point.x / width) * 2.0 - 1.0,
                    1.0 - (point.y / height) * 2.0,
                    0.0,
                ],
                color,
            })
        })
        .collect()
}

/// The authored backdrop color, linearized for the linear intermediate. Kept
/// as an `sRGB` literal (a designer-picked dark warm gray) and converted once
/// here — the present pass re-encodes it to the surface.
fn clear_color() -> wgpu::Color {
    let [r, g, b, a] = crate::types::Color::from_srgb(0.09, 0.08, 0.07).to_linear_rgba();
    wgpu::Color {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a: a as f64,
    }
}

/// CPU-stage render-input counts for a scene: how many `Primitive`s it
/// holds, how many `GuiTriangle`s they tessellate into, and how many GPU
/// `Vertex`es those triangles map to. This is the displayless proxy the
/// #93 regression tests assert on — "render input would draw" without a
/// GPU surface or display server. It deliberately re-runs the same
/// `tessellate_scene_with_text` → `gui_vertices` pipeline `render` drives,
/// so a future tessellation/vertex regression trips the count assertions.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderInputCounts {
    primitives: usize,
    triangles: usize,
    vertices: usize,
}

#[cfg(test)]
fn render_input_counts(
    scene: &Scene,
    text_system: &dyn TextSystem,
    width: f32,
    height: f32,
) -> RenderInputCounts {
    let triangles = crate::tessellate_scene_with_text(scene, text_system);
    let vertices = gui_vertices(&triangles, width, height);
    RenderInputCounts {
        primitives: scene.primitives.len(),
        triangles: triangles.len(),
        vertices: vertices.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_text::BitmapTextSystem;

    #[cfg(feature = "slug")]
    #[test]
    fn scene_slug_glyphs_collects_only_slug_primitives_in_order() {
        use mkui_vector2d::Vec2;
        use mkui_vector2d_wgpu::{GlyphBounds, PlacedSlugGlyph, SlugCurve, SlugGlyph};
        use std::sync::Arc;

        let make = |color: [f32; 4]| PlacedSlugGlyph {
            blob: Arc::new(SlugGlyph {
                revision: 1,
                bounds: GlyphBounds {
                    x_min: 0.0,
                    y_min: 0.0,
                    x_max: 1.0,
                    y_max: 1.0,
                },
                curves: vec![SlugCurve {
                    p0: Vec2::new(0.0, 0.0),
                    p1: Vec2::new(1.0, 1.0),
                    p2: Vec2::new(1.0, 1.0),
                }],
                horizontal_bands: Vec::new(),
                horizontal_curve_indices: Vec::new(),
                vertical_bands: Vec::new(),
                vertical_curve_indices: Vec::new(),
            }),
            origin_px: [0.0, 0.0],
            scale_px_per_unit: 1.0,
            color,
        };
        let g0 = make([1.0, 0.0, 0.0, 1.0]);
        let g1 = make([0.0, 1.0, 0.0, 1.0]);
        let quad = crate::Primitive::Quad(crate::Quad {
            rect: crate::Rect::new(crate::Point::new(0.0, 0.0), crate::Size::new(1.0, 1.0)),
            fill: crate::Color::rgba(1.0, 1.0, 1.0, 1.0),
            corner_radii: crate::CornerRadii::all(0.0),
            stroke: None,
        });
        // Interleaved with a non-Slug primitive: the seam returns exactly the
        // Slug glyphs, in scene order, and nothing else (regression guard
        // against the lane being silently re-stubbed to empty).
        let prims = vec![
            crate::Primitive::SlugGlyph(g0.clone()),
            quad,
            crate::Primitive::SlugGlyph(g1.clone()),
        ];
        assert_eq!(scene_slug_glyphs(&prims), vec![g0, g1]);
    }

    #[test]
    fn msaa_pref_is_off_pending_srgb_orchestration() {
        // #93: MSAA is pinned off (sample_count=1) — the StoneSketch-proven
        // path — until #95 re-introduces it with correct sRGB resolve.
        assert_eq!(MSAA_SAMPLE_COUNT_PREF, 1);
        assert_eq!(
            pick_sample_count(
                wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4,
                MSAA_SAMPLE_COUNT_PREF
            ),
            1
        );
    }

    #[test]
    fn render_input_counts_native_window_quad_is_non_empty() {
        // native-window's `with_scene` quad must reach the GPU stage as
        // non-empty triangles + vertices (#93 — empty here would mean a
        // tessellation regression, not the resize clobber).
        let mut scene = Scene::new(crate::Size::new(800.0, 600.0));
        scene.push(crate::Primitive::Quad(crate::Quad {
            rect: crate::Rect::new(
                crate::Point::new(200.0, 150.0),
                crate::Size::new(400.0, 300.0),
            ),
            fill: crate::Color::rgba(0.42, 0.66, 0.84, 1.0),
            corner_radii: crate::CornerRadii::all(0.0),
            stroke: None,
        }));
        let counts = render_input_counts(&scene, &BitmapTextSystem::new(), 800.0, 600.0);
        assert_eq!(counts.primitives, 1);
        assert_eq!(counts.triangles, 2, "one quad → two triangles");
        assert_eq!(counts.vertices, 6, "two triangles → six vertices");
    }

    #[test]
    fn render_input_counts_empty_scene_is_zero() {
        let scene = Scene::new(crate::Size::new(800.0, 600.0));
        let counts = render_input_counts(&scene, &BitmapTextSystem::new(), 800.0, 600.0);
        assert_eq!(
            counts,
            RenderInputCounts {
                primitives: 0,
                triangles: 0,
                vertices: 0
            }
        );
    }

    #[test]
    fn sample_count_picks_preferred_when_color_format_supports_it() {
        let flags = wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4;
        assert_eq!(pick_sample_count(flags, 4), 4);
    }

    #[test]
    fn sample_count_falls_back_to_one_when_color_format_lacks_support() {
        let flags = wgpu::TextureFormatFeatureFlags::empty();
        assert_eq!(pick_sample_count(flags, 4), 1);
    }

    #[test]
    fn sample_count_returns_one_when_preferred_is_one_or_zero() {
        let flags = wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4;
        assert_eq!(pick_sample_count(flags, 1), 1);
        assert_eq!(pick_sample_count(flags, 0), 1);
    }

    #[test]
    fn sample_count_falls_back_when_preferred_is_unsupported_higher_count() {
        let flags = wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4;
        assert_eq!(pick_sample_count(flags, 8), 1);
    }

    #[test]
    fn gui_vertices_map_pixel_to_ndc() {
        let triangle = GuiTriangle {
            points: [
                crate::Point::new(0.0, 0.0),
                crate::Point::new(100.0, 0.0),
                crate::Point::new(0.0, 100.0),
            ],
            color: crate::Color::rgba(1.0, 0.0, 0.0, 1.0),
        };
        let vertices = gui_vertices(&[triangle], 100.0, 100.0);
        assert_eq!(vertices.len(), 3);
        // (0, 0) → (-1, 1, 0); (100, 0) → (1, 1, 0); (0, 100) → (-1, -1, 0).
        assert_eq!(vertices[0].position, [-1.0, 1.0, 0.0]);
        assert_eq!(vertices[1].position, [1.0, 1.0, 0.0]);
        assert_eq!(vertices[2].position, [-1.0, -1.0, 0.0]);
    }

    #[test]
    fn gui_vertices_project_against_logical_viewport_not_physical_surface() {
        // #97: project against the logical viewport (800×600), NOT the
        // physical surface (which on a 2× Retina display would be 1600×1200).
        // A logical x=200 must map to NDC -0.5 — i.e. (200/800)*2 - 1 — so the
        // primitive lands centered, not in the upper-left quadrant. The render
        // path now feeds `scene.viewport.{width,height}` (logical) here; this
        // test pins the math the call site depends on.
        let triangle = GuiTriangle {
            points: [
                crate::Point::new(200.0, 0.0),
                crate::Point::new(200.0, 100.0),
                crate::Point::new(300.0, 0.0),
            ],
            color: crate::Color::rgba(1.0, 1.0, 1.0, 1.0),
        };
        let vertices = gui_vertices(&[triangle], 800.0, 600.0);
        // x=200 in a 800-wide logical viewport → (200/800)*2 - 1 = -0.5,
        // independent of the physical surface size.
        assert!((vertices[0].position[0] - (-0.5)).abs() < f32::EPSILON);
    }

    #[test]
    fn gui_vertices_guard_against_zero_dimensions() {
        // Real `Renderer::resize` already drops zero-size requests, but the
        // helper has to stay safe in case it's invoked with a stale config
        // mid-frame — division by zero would NaN every position.
        let triangle = GuiTriangle {
            points: [
                crate::Point::new(10.0, 10.0),
                crate::Point::new(20.0, 10.0),
                crate::Point::new(10.0, 20.0),
            ],
            color: crate::Color::rgba(0.5, 0.5, 0.5, 1.0),
        };
        let vertices = gui_vertices(&[triangle], 0.0, 0.0);
        for v in vertices {
            assert!(v.position[0].is_finite());
            assert!(v.position[1].is_finite());
        }
    }

    #[test]
    fn srgb_to_linear_matches_reference_for_known_components() {
        // 0 and 1 are fixed points; 0.5 should be ~0.214 in linear space.
        use crate::types::srgb_to_linear;
        assert!((srgb_to_linear(0.0)).abs() < f32::EPSILON);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        let mid = srgb_to_linear(0.5);
        assert!((mid - 0.21404114).abs() < 1e-5);
    }

    #[test]
    fn gui_vertices_linearizes_authored_color_but_keeps_alpha() {
        // The intermediate framebuffer is linear, so `gui_vertices` must
        // linearize the authored sRGB color unconditionally (the old
        // target-format branch is gone). A 50%-alpha mid-gray discriminates:
        // sRGB-space blending (the pre-Sprint-8 bug) would leave 0.5 in the
        // color channels; correct linearization yields ~0.214, alpha stays 0.5.
        let triangle = GuiTriangle {
            points: [
                crate::Point::new(0.0, 0.0),
                crate::Point::new(1.0, 0.0),
                crate::Point::new(0.0, 1.0),
            ],
            color: crate::Color::from_srgb_a(0.5, 0.5, 0.5, 0.5),
        };
        let vertices = gui_vertices(&[triangle], 1.0, 1.0);
        for v in &vertices {
            assert!((v.color[0] - 0.21404114).abs() < 1e-5);
            assert!((v.color[1] - 0.21404114).abs() < 1e-5);
            assert!((v.color[2] - 0.21404114).abs() < 1e-5);
            assert_eq!(v.color[3], 0.5, "alpha is a linear coverage weight");
        }
    }

    #[test]
    fn clear_color_is_linearized_backdrop() {
        // The backdrop literal is authored in sRGB and linearized for the
        // linear intermediate; the linear red channel must be below the
        // authored 0.09 (sRGB compresses the dark end).
        let clear = clear_color();
        let expected = crate::types::srgb_to_linear(0.09) as f64;
        assert!((clear.r - expected).abs() < 1e-6);
        assert!(clear.r < 0.09, "linear value sits below the sRGB literal");
        assert_eq!(clear.a, 1.0);
    }

    #[test]
    fn present_wgsl_parses_and_validates() {
        // Static naga validation catches a present-shader typo in the default
        // `test` job rather than only on the windowed path (which no offscreen
        // GPU test exercises, since the harness has no surface).
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("present.wgsl"))
            .expect("present WGSL must parse");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("present WGSL must pass naga validation");
    }

    #[test]
    fn render_outcome_is_copy_eq() {
        let drawn = RenderOutcome::Drawn;
        let copy = drawn;
        assert_eq!(drawn, copy);
    }

    #[test]
    fn resize_zero_width_is_skipped() {
        assert_eq!(clamp_resize(0, 720), None);
    }

    #[test]
    fn resize_zero_height_is_skipped() {
        assert_eq!(clamp_resize(1280, 0), None);
    }

    #[test]
    fn resize_passes_through_non_zero_dimensions() {
        assert_eq!(clamp_resize(1280, 720), Some((1280, 720)));
    }
}
