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

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::{tessellate_scene_with_text, GuiTriangle, Scene};
use mkui_core::error::MkuiError;
use mkui_text::TextSystem;

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
    /// Multisampled color attachment the UI pass renders into when
    /// `sample_count > 1`. Resolves into the swapchain texture at end-of-
    /// pass. `None` on the 1× fallback, where the UI pass writes the
    /// swapchain view directly.
    msaa_color_view: Option<wgpu::TextureView>,
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
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mkui-wgpu Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
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

        let color_flags = adapter.get_texture_format_features(format).flags;
        let sample_count = pick_sample_count(color_flags, MSAA_SAMPLE_COUNT_PREF);

        let ui_pipeline = build_ui_pipeline(&device, format, sample_count);
        let msaa_color_view = create_msaa_color_view(&device, width, height, format, sample_count);

        Ok(Self {
            _window: window,
            surface,
            device,
            queue,
            config,
            sample_count,
            ui_pipeline,
            msaa_color_view,
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
        self.msaa_color_view = create_msaa_color_view(
            &self.device,
            width,
            height,
            self.config.format,
            self.sample_count,
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
        let target_is_srgb = self.config.format.is_srgb();
        let clear = clear_color(target_is_srgb);

        let triangles = tessellate_scene_with_text(scene, text_system);
        // Project against the logical-pixel viewport, NOT the physical-pixel
        // surface config. `self.config.{width, height}` are physical pixels
        // (wgpu's `surface.configure` contract); scene primitives are authored
        // in logical pixels, so the NDC denominator must be logical too. Using
        // physical pixels here mis-projected primitives into the upper-left
        // quadrant on HiDPI displays (#97). See ADR 0006 §"Viewport units
        // contract".
        let vertices = gui_vertices(
            &triangles,
            scene.viewport.width,
            scene.viewport.height,
            target_is_srgb,
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mkui-wgpu UI Encoder"),
            });

        let vertex_buffer = (!vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mkui-wgpu UI Vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });

        {
            let color_attachment = match self.msaa_color_view.as_ref() {
                Some(msaa) => wgpu::RenderPassColorAttachment {
                    view: msaa,
                    depth_slice: None,
                    resolve_target: Some(&view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                },
                None => wgpu::RenderPassColorAttachment {
                    view: &view,
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
            if let Some(buffer) = vertex_buffer.as_ref() {
                pass.set_pipeline(&self.ui_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
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

fn create_msaa_color_view(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
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
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    Some(texture.create_view(&wgpu::TextureViewDescriptor::default()))
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

/// Map `Scene`'s pixel-space triangles into NDC, linearizing the per-vertex
/// authored sRGB color when the swapchain expects linear values.
fn gui_vertices(
    triangles: &[GuiTriangle],
    width: f32,
    height: f32,
    target_is_srgb: bool,
) -> Vec<Vertex> {
    let width = width.max(1.0);
    let height = height.max(1.0);
    triangles
        .iter()
        .flat_map(|triangle| {
            let color = authored_rgba_to_target(
                [
                    triangle.color.r,
                    triangle.color.g,
                    triangle.color.b,
                    triangle.color.a,
                ],
                target_is_srgb,
            );
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

fn clear_color(target_is_srgb: bool) -> wgpu::Color {
    let [r, g, b, a] = authored_rgba_to_target([0.09, 0.08, 0.07, 1.0], target_is_srgb);
    wgpu::Color {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a: a as f64,
    }
}

fn authored_rgba_to_target(color: [f32; 4], target_is_srgb: bool) -> [f32; 4] {
    if !target_is_srgb {
        return color;
    }
    [
        srgb_to_linear(color[0]),
        srgb_to_linear(color[1]),
        srgb_to_linear(color[2]),
        color[3],
    ]
}

fn srgb_to_linear(component: f32) -> f32 {
    let component = component.clamp(0.0, 1.0);
    if component <= 0.04045 {
        component / 12.92
    } else {
        ((component + 0.055) / 1.055).powf(2.4)
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
    target_is_srgb: bool,
) -> RenderInputCounts {
    let triangles = tessellate_scene_with_text(scene, text_system);
    let vertices = gui_vertices(&triangles, width, height, target_is_srgb);
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
        let counts = render_input_counts(&scene, &BitmapTextSystem::new(), 800.0, 600.0, true);
        assert_eq!(counts.primitives, 1);
        assert_eq!(counts.triangles, 2, "one quad → two triangles");
        assert_eq!(counts.vertices, 6, "two triangles → six vertices");
    }

    #[test]
    fn render_input_counts_empty_scene_is_zero() {
        let scene = Scene::new(crate::Size::new(800.0, 600.0));
        let counts = render_input_counts(&scene, &BitmapTextSystem::new(), 800.0, 600.0, true);
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
        let vertices = gui_vertices(&[triangle], 100.0, 100.0, false);
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
        let vertices = gui_vertices(&[triangle], 800.0, 600.0, false);
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
        let vertices = gui_vertices(&[triangle], 0.0, 0.0, false);
        for v in vertices {
            assert!(v.position[0].is_finite());
            assert!(v.position[1].is_finite());
        }
    }

    #[test]
    fn srgb_to_linear_matches_reference_for_known_components() {
        // 0 and 1 are fixed points; 0.5 should be ~0.214 in linear space.
        assert!((srgb_to_linear(0.0)).abs() < f32::EPSILON);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        let mid = srgb_to_linear(0.5);
        assert!((mid - 0.21404114).abs() < 1e-5);
    }

    #[test]
    fn authored_rgba_passthrough_when_target_is_linear() {
        let color = [0.4, 0.6, 0.8, 0.5];
        assert_eq!(authored_rgba_to_target(color, false), color);
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
