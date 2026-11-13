//! Host-owned GPU and frame example (#175).
//!
//! This binary plays the role of a third-party `wgpu`-based game/engine host
//! embedding mkui as its UI layer — the scenario ADR 0012
//! (`docs/architecture/0012-host-owned-gpu-embedding.md`) and
//! [`mkui_wgpu::EmbeddedRenderer`] exist for. The host (this file) owns
//! everything a real engine would: the `wgpu::Instance`, the adapter, the
//! device, the queue, the window's `Surface`, and a lit 3D pass (a rotating,
//! per-face Lambertian-shaded cube). `mkui-wgpu` owns none of that — it only
//! borrows the host's device and the host's surface format once, at startup
//! (`EmbeddedRenderer::new`), and records its one UI pass into the host's
//! own command encoder and the host's own swapchain view every frame
//! (`EmbeddedRenderer::render_into`).
//!
//! Per frame: ONE `wgpu::CommandEncoder`, carrying the host's 3D pass
//! followed by mkui's UI pass into the *same* swapchain `TextureView`, then
//! ONE `queue.submit`, then present — satisfying #175's acceptance bar: one
//! frame, one queue submission, no extra texture copy.
//!
//! The UI pass itself is a believable HUD/pause-menu scene (#255), built
//! from the components `#173`'s batch 15 shipped: an always-on health
//! `progress_bar` + score `counter_chip` + a "LIVE" `badge`/status `dot`,
//! plus a toast/banner `toast_stack` firing on a timer — and, behind
//! `Escape`, a pause overlay (`glass_panel` chrome, a keyboard-navigable
//! `MenuList`, a `toggle`/`slider_with_label`/`segmented_select` settings
//! row, a `tooltip`, and a `titled_dialog` quit confirmation stacked on
//! top). #230 profiles CPU/GPU frame cost and memory against this scene,
//! not the two-quad placeholder #175 shipped it with; this issue only
//! assembles the scene, it does not measure anything itself.
//!
//! `cargo run -p host-embed-wgpu --release` opens the window.
//! `HEADLESS=1 cargo run -p host-embed-wgpu` exits before opening a window,
//! the same CI-smoke convention `native-window`/`native-showcase` use.
//!
//! ## Manual profiling
//!
//! This binary is #230's profiling target, not a profiler itself — run it
//! under the host platform's own GPU/CPU capture tool while it's open:
//!
//! - **macOS** — Instruments' Metal System Trace, or Xcode's GPU frame
//!   capture, attached to the running `host-embed-wgpu` process.
//! - **Windows** — PIX or RenderDoc, launching/attaching to the built
//!   `host-embed-wgpu.exe`.
//!
//! Press `Escape` to open the pause overlay (exercising the glass panel +
//! menu + settings + tooltip + dialog lanes together) and `Escape`/`Enter`
//! to navigate it — the richer, GPU-heavier frame #230 wants captured
//! alongside the base HUD's steady per-frame cost.

mod mat4;

use std::sync::Arc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

use mkui_core::headless::{KeyboardInteractable, MenuList};
use mkui_text::BitmapTextSystem;
use mkui_wgpu::components::{
    badge, counter_chip, dot, glass_panel, label, progress_bar, segmented_select,
    slider_with_label, titled_dialog, toast_stack, toggle, tooltip, tooltip_rect, TOOLTIP_GAP_PX,
};
use mkui_wgpu::{
    BadgeSize, BadgeVariant, Color, CornerRadii, DotAnimation, DotSize, DotVariant,
    EmbeddedColorSpace, EmbeddedRenderer, GlassMode, IconId, PanelLayout, Point, Quad, Rect, Scene,
    Size, TextVariant, ToastStack, ToastVariant, TooltipPlacement, TooltipState, WgpuTheme,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CubeVertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl CubeVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    mvp: [[f32; 4]; 4],
}

/// Fixed light direction for the host's per-face Lambertian shading
/// (`normalize([0.4, 0.8, 0.5])`, baked here to avoid a sqrt at startup).
const LIGHT_DIR: [f32; 3] = [0.379_008_25, 0.758_016_5, 0.473_760_3];

/// `(normal, base_color, 4 corners)` for one cube face.
type Face = ([f32; 3], [f32; 3], [[f32; 3]; 4]);

/// One entry per face of the cube. Each face is flat-shaded (one normal,
/// baked into the vertex color below) — a cube has no use for interpolated
/// per-vertex normals.
const FACES: [Face; 6] = [
    (
        [1.0, 0.0, 0.0],
        [0.85, 0.25, 0.25],
        [
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, 0.5],
        ],
    ),
    (
        [-1.0, 0.0, 0.0],
        [0.25, 0.85, 0.25],
        [
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, -0.5],
        ],
    ),
    (
        [0.0, 1.0, 0.0],
        [0.25, 0.25, 0.85],
        [
            [-0.5, 0.5, -0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
        ],
    ),
    (
        [0.0, -1.0, 0.0],
        [0.85, 0.85, 0.25],
        [
            [-0.5, -0.5, 0.5],
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
        ],
    ),
    (
        [0.0, 0.0, 1.0],
        [0.85, 0.25, 0.85],
        [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ],
    ),
    (
        [0.0, 0.0, -1.0],
        [0.25, 0.85, 0.85],
        [
            [0.5, -0.5, -0.5],
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [0.5, 0.5, -0.5],
        ],
    ),
];

/// Build the host's demo mesh: a unit cube, one baked Lambertian color per
/// face. Pure, no GPU — covered by the plain unit tests below.
fn build_cube_vertices() -> Vec<CubeVertex> {
    let mut vertices = Vec::with_capacity(FACES.len() * 6);
    for (normal, base_color, corners) in FACES {
        let diffuse =
            (normal[0] * LIGHT_DIR[0] + normal[1] * LIGHT_DIR[1] + normal[2] * LIGHT_DIR[2])
                .max(0.0);
        let factor = 0.3 + 0.7 * diffuse;
        let color = [
            base_color[0] * factor,
            base_color[1] * factor,
            base_color[2] * factor,
        ];
        for [a, b, c] in [[0usize, 1, 2], [0, 2, 3]] {
            for idx in [a, b, c] {
                vertices.push(CubeVertex {
                    position: corners[idx],
                    color,
                });
            }
        }
    }
    vertices
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("host-embed-wgpu Depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Convert a physical-pixel window size into the logical-pixel viewport the
/// HUD `Scene` carries — mirrors `mkui_wgpu::app`'s own
/// `logical_viewport_from_physical_size` (the host, not mkui, owns resize
/// here, but the unit convention is the same).
fn logical_viewport(size: winit::dpi::PhysicalSize<u32>, scale_factor: f64) -> Size {
    let scale = (scale_factor as f32).max(f32::EPSILON);
    Size::new(size.width as f32 / scale, size.height as f32 / scale)
}

/// Everything the host owns: its own instance-derived device/queue/surface,
/// its own lit 3D pipeline, and the one `EmbeddedRenderer` borrowing that
/// device + the surface's format for mkui's HUD pass.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    cube_pipeline: wgpu::RenderPipeline,
    cube_vertex_buffer: wgpu::Buffer,
    cube_vertex_count: u32,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    embedded: EmbeddedRenderer,
}

impl Gpu {
    async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .expect("host-embed-wgpu: create_surface failed");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("host-embed-wgpu: request_adapter failed");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("host-embed-wgpu Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("host-embed-wgpu: request_device failed");

        let capabilities = surface.get_capabilities(&adapter);
        let default_format = capabilities
            .formats
            .first()
            .copied()
            .expect("host-embed-wgpu: surface advertised no supported formats");
        // Prefer an sRGB surface format — the common host configuration, and
        // the one `EmbeddedRenderer` draws through its physically-linear
        // fragment entry point for (see ADR 0012 §"Color space").
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
            .expect("host-embed-wgpu: surface advertised no present modes");
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .expect("host-embed-wgpu: surface advertised no alpha modes");

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, width, height);

        let cube_vertices = build_cube_vertices();
        let cube_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("host-embed-wgpu Cube Vertices"),
            contents: bytemuck::cast_slice(&cube_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("host-embed-wgpu Cube Uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("host-embed-wgpu Cube Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("host-embed-wgpu Cube Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("host-embed-wgpu Cube Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cube.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("host-embed-wgpu Cube Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let cube_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("host-embed-wgpu Cube Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_cube"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(CubeVertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_cube"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // The one borrow: mkui never sees the instance, the adapter, or the
        // surface above — only the device it already negotiated and the
        // format it already picked for its own frame target (#175). `format`
        // was chosen above to prefer an sRGB surface format, so this host
        // declares `Linear` explicitly — never infer this from `format`
        // alone; see `EmbeddedColorSpace`'s docs for why a format-based
        // heuristic is unsound for e.g. a linear HDR intermediate.
        let embedded = EmbeddedRenderer::new(&device, format, EmbeddedColorSpace::Linear);

        Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            cube_pipeline,
            cube_vertex_buffer,
            cube_vertex_count: cube_vertices.len() as u32,
            uniform_buffer,
            uniform_bind_group,
            embedded,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, width, height);
    }

    /// Record and submit exactly one frame: the host's lit 3D pass, then
    /// mkui's HUD pass into the same encoder and the same swapchain view,
    /// then one `queue.submit` and present (#175's acceptance bar).
    fn render(&mut self, elapsed_secs: f32, hud_scene: &Scene, text_system: &BitmapTextSystem) {
        let (frame, reconfigure_after_present) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let model = mat4::rotation_y(elapsed_secs * 0.6);
        let eye = mat4::look_at_rh([2.2, 1.8, 2.6], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let aspect = self.config.width as f32 / (self.config.height.max(1) as f32);
        let proj = mat4::perspective(60f32.to_radians(), aspect, 0.1, 100.0);
        let mvp = mat4::mul(proj, mat4::mul(eye, model));
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[Uniforms { mvp }]),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("host-embed-wgpu Frame Encoder"),
            });

        // Pass 1: the host's own lit 3D scene, writing into the swapchain view.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("host-embed-wgpu 3D Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.06,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.cube_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_vertex_buffer(0, self.cube_vertex_buffer.slice(..));
            pass.draw(0..self.cube_vertex_count, 0..1);
        }

        // Pass 2: mkui's HUD, recorded into the SAME encoder and the SAME
        // view, after the host's 3D scene and before present — the
        // embedding contract #175 exists for. `render_into` never clears
        // (`wgpu::LoadOp::Load`), never submits, and never presents.
        self.embedded.render_into(
            &self.device,
            &mut encoder,
            &view,
            (self.config.width, self.config.height),
            hud_scene,
            text_system,
        );

        // ONE submission, carrying both passes.
        self.queue.submit(Some(encoder.finish()));
        drop(view);
        self.queue.present(frame);
        if reconfigure_after_present {
            self.surface.configure(&self.device, &self.config);
        }
    }
}

/// Whether the pause/settings overlay is hidden, open, or showing its
/// nested "quit to desktop?" confirmation. `QuitConfirm` stacks a second
/// [`titled_dialog`] *on top of* the already-drawn overlay — stacking falls
/// out of call order, same as [`mkui_wgpu::components::dialog`]'s own doc
/// comment describes — rather than replacing it, so Escape from
/// `QuitConfirm` can cleanly fall back to `Open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayState {
    Closed,
    Open,
    QuitConfirm,
}

/// Fresh pause menu, pre-selecting "Resume" (`MenuList::builder`'s default
/// first-item selection) — rebuilt every time the overlay transitions from
/// `Closed` to `Open` so a reopened pause menu never remembers a stale
/// selection from the last time it was open.
fn build_pause_menu() -> MenuList {
    MenuList::builder().items(["Resume", "Quit"]).build()
}

/// Every piece of per-frame HUD state the host owns across frames — the
/// renderer is immediate-mode and retains no component tree of its own
/// (see `counter_chip`'s module doc), so whatever the scene should show
/// next frame has to live here, not in the `Scene` built from it.
struct HudState {
    overlay: OverlayState,
    menu: MenuList,
    /// Toggled by `M` while the overlay is open — the one genuinely
    /// keyboard-interactive settings control; `volume`/`quality` below are
    /// rendered but held static, satisfying the issue's "at least one"
    /// toggle/slider/segmented-select bar without inventing extra input
    /// plumbing the issue doesn't ask for.
    mute: bool,
    volume: f32,
    quality: usize,
    tooltip: TooltipState,
    toasts: ToastStack,
}

impl HudState {
    fn new() -> Self {
        Self {
            overlay: OverlayState::Closed,
            menu: build_pause_menu(),
            mute: false,
            volume: 0.65,
            quality: 1,
            tooltip: TooltipState::new(),
            toasts: ToastStack::new(),
        }
    }
}

/// Toast/banner firing cadence (#255's "firing periodically" acceptance
/// bullet) and the small rotation of messages it cycles through.
const TOAST_INTERVAL_SECS: f32 = 4.0;
const TOAST_MESSAGES: [(ToastVariant, &str); 3] = [
    (ToastVariant::Info, "Checkpoint saved"),
    (ToastVariant::Success, "Objective complete"),
    (ToastVariant::Warning, "Low ammo"),
];

/// Push the next toast once `elapsed_secs` has advanced `TOAST_INTERVAL_SECS`
/// past `*last_toast_at`, cycling through `TOAST_MESSAGES`. Pure state
/// mutation, no `Scene` — kept separate from rendering so the firing
/// cadence is testable without a GPU.
fn maybe_fire_toast(
    toasts: &mut ToastStack,
    last_toast_at: &mut f32,
    cycle: &mut usize,
    elapsed_secs: f32,
) {
    if elapsed_secs - *last_toast_at < TOAST_INTERVAL_SECS {
        return;
    }
    *last_toast_at = elapsed_secs;
    let (variant, message) = TOAST_MESSAGES[*cycle % TOAST_MESSAGES.len()];
    *cycle += 1;
    toasts.push(variant, message);
}

/// The always-on-screen base HUD: health `progress_bar`, score
/// `counter_chip`, a "LIVE" `badge` + status `dot`, and the periodically
/// firing `toast_stack` — present every frame regardless of the pause
/// overlay, so #230 always has a believable minimum scene to profile.
fn draw_base_hud(
    scene: &mut Scene,
    viewport: Size,
    elapsed_secs: f32,
    hud: &HudState,
    theme: &WgpuTheme,
) {
    // Health bar — a slow bob so the base HUD isn't perfectly static under
    // profiling, same spirit as the host's own rotating cube.
    let health_fraction = 0.55 + 0.1 * (elapsed_secs * 0.5).sin();
    let bar_rect = Rect::new(Point::new(24.0, 24.0), Size::new(220.0, 18.0));
    progress_bar(scene, bar_rect, health_fraction, &theme.progress_bar());

    let chip_rect = Rect::new(
        Point::new(bar_rect.width_end() + 12.0, 20.0),
        Size::new(96.0, 26.0),
    );
    counter_chip(
        scene,
        chip_rect,
        IconId(1),
        2480,
        "Score",
        false,
        false,
        &theme.counter_chip(),
    );

    let dot_point = Point::new(viewport.width - 96.0, 36.0);
    dot(
        scene,
        dot_point,
        DotVariant::Ok,
        DotSize::Sm,
        true,
        DotAnimation::Pulse,
        theme,
    );
    let status_badge_rect = Rect::new(
        Point::new(viewport.width - 80.0, 24.0),
        Size::new(56.0, 24.0),
    );
    badge(
        scene,
        status_badge_rect,
        "LIVE",
        BadgeVariant::Secondary,
        BadgeSize::Sm,
        theme,
    );

    let toast_region = Rect::new(
        Point::new(
            (viewport.width - 260.0).max(0.0),
            (viewport.height - 200.0).max(0.0),
        ),
        Size::new(240.0, 180.0),
    );
    let mut layout = PanelLayout::<()>::default();
    toast_stack(scene, &mut layout, toast_region, &hud.toasts, theme, |_| ());
}

/// The pause menu's rows: a highlight behind [`MenuList::is_selected`]'s
/// current row, then each item's label — [`MenuList`] itself carries no
/// wgpu rendering (#255 only assembles existing batch-15 render
/// components; `MenuList` isn't one), so this is the thin per-row paint
/// loop a host wires up itself.
fn render_menu(scene: &mut Scene, rect: Rect, menu: &MenuList, theme: &WgpuTheme) {
    const ROW_HEIGHT: f32 = 36.0;
    const ROW_GAP: f32 = 8.0;

    for (index, item) in menu.items().iter().enumerate() {
        let row_rect = Rect::new(
            Point::new(
                rect.origin.x,
                rect.origin.y + index as f32 * (ROW_HEIGHT + ROW_GAP),
            ),
            Size::new(rect.size.width, ROW_HEIGHT),
        );
        if menu.is_selected(index) {
            scene.quad(Quad {
                rect: row_rect,
                fill: Color::rgba(0.4, 0.6, 0.9, 0.35),
                corner_radii: CornerRadii::all(8.0),
                stroke: None,
            });
        }
        let label_style = theme.body_style();
        let label_rect = Rect::new(
            Point::new(
                row_rect.origin.x + 12.0,
                row_rect.origin.y + (row_rect.size.height - label_style.line_height_px) * 0.5,
            ),
            Size::new(
                (row_rect.size.width - 24.0).max(0.0),
                label_style.line_height_px,
            ),
        );
        label(scene, label_rect, item.label(), label_style);
    }
}

/// The settings column: an audio status `dot` + mute `toggle`, a volume
/// `slider_with_label`, and a quality `segmented_select` + `badge` — plus a
/// keyboard-focus-driven `tooltip` anchored above the quality control.
fn render_settings(scene: &mut Scene, rect: Rect, hud: &HudState, theme: &WgpuTheme) {
    let mut y = rect.origin.y;

    let dot_point = Point::new(rect.origin.x + 6.0, y + 10.0);
    dot(
        scene,
        dot_point,
        if hud.mute {
            DotVariant::Warn
        } else {
            DotVariant::Ok
        },
        DotSize::Md,
        false,
        DotAnimation::None,
        theme,
    );
    label(
        scene,
        Rect::new(Point::new(rect.origin.x + 20.0, y), Size::new(80.0, 20.0)),
        "Audio (M)",
        theme.body_style(),
    );
    let toggle_rect = Rect::new(
        Point::new(rect.width_end() - 44.0, y - 2.0),
        Size::new(44.0, 24.0),
    );
    toggle(scene, toggle_rect, !hud.mute, &theme.toggle());
    y += 20.0 + 16.0;

    label(
        scene,
        Rect::new(
            Point::new(rect.origin.x, y),
            Size::new(rect.size.width, 18.0),
        ),
        "Volume",
        theme.text_style(TextVariant::Muted),
    );
    y += 20.0;
    let slider_rect = Rect::new(
        Point::new(rect.origin.x, y),
        Size::new(rect.size.width, 8.0),
    );
    let volume_label = format!("{:.0}%", hud.volume * 100.0);
    slider_with_label(
        scene,
        slider_rect,
        hud.volume,
        Some(&volume_label),
        &theme.slider(),
    );
    y += 36.0 + 16.0;

    label(
        scene,
        Rect::new(
            Point::new(rect.origin.x, y),
            Size::new(rect.size.width, 18.0),
        ),
        "Quality",
        theme.text_style(TextVariant::Muted),
    );
    y += 20.0;
    const QUALITY_LABELS: [&str; 3] = ["Low", "Medium", "High"];
    let select_rect = Rect::new(
        Point::new(rect.origin.x, y),
        Size::new((rect.size.width - 64.0).max(0.0), 28.0),
    );
    let mut layout = PanelLayout::<()>::default();
    segmented_select(
        scene,
        &mut layout,
        select_rect,
        &QUALITY_LABELS,
        hud.quality,
        &theme.segmented_select(),
        |_| (),
    );
    let quality_badge_rect = Rect::new(
        Point::new(select_rect.width_end() + 8.0, y + 2.0),
        Size::new(56.0, 24.0),
    );
    badge(
        scene,
        quality_badge_rect,
        QUALITY_LABELS[hud.quality].to_uppercase(),
        BadgeVariant::Secondary,
        BadgeSize::Sm,
        theme,
    );

    // Tooltip last, per its own doc comment, so the bubble always draws
    // over everything else this frame — visible on keyboard *focus*
    // (`TooltipState::set_focused`, driven by the overlay being open),
    // not hover; this host has no pointer at all.
    if hud.tooltip.is_visible() {
        let bubble_size = Size::new(220.0, 36.0);
        let bubble_rect = tooltip_rect(
            select_rect,
            bubble_size,
            TooltipPlacement::Top,
            TOOLTIP_GAP_PX,
            scene.viewport,
        );
        tooltip(scene, bubble_rect, "Lower quality reduces GPU load.", theme);
    }
}

/// The pause/settings overlay's outer chrome (a `glass_panel`) plus its
/// menu + settings columns, and — only in [`OverlayState::QuitConfirm`] —
/// a second `titled_dialog` stacked on top asking to confirm quitting.
///
/// `GlassMode::Solid`, not `Blur`: [`EmbeddedRenderer::render_into`]
/// doesn't wire up the `Primitive::Backdrop` blur lane yet (#175's
/// `render/embedded.rs` module doc — tracked as its own #187 follow-up), so
/// `Blur` would paint a `Color::TRANSPARENT`-filled panel here, i.e. an
/// outline with no visible chrome at all. `Solid` is the documented
/// fallback for exactly this case — "no blur cost," by construction.
fn draw_pause_overlay(scene: &mut Scene, viewport: Size, hud: &HudState, theme: &WgpuTheme) {
    let panel_rect = Rect::new(
        Point::new(
            (viewport.width - 560.0).max(0.0) * 0.5,
            (viewport.height - 420.0).max(0.0) * 0.5,
        ),
        Size::new(
            560.0_f32.min(viewport.width),
            420.0_f32.min(viewport.height),
        ),
    );
    let content = glass_panel(
        scene,
        panel_rect,
        theme.glass_panel_style(),
        GlassMode::Solid,
    );

    let title_style = theme.title_style();
    label(
        scene,
        Rect::new(
            content.origin,
            Size::new(content.size.width, title_style.line_height_px),
        ),
        "Paused",
        title_style,
    );
    let body = Rect::new(
        Point::new(
            content.origin.x,
            content.origin.y + title_style.line_height_px + 12.0,
        ),
        Size::new(
            content.size.width,
            (content.size.height - title_style.line_height_px - 12.0).max(0.0),
        ),
    );
    const MENU_WIDTH: f32 = 160.0;
    const COLUMN_GAP: f32 = 24.0;
    let menu_rect = Rect::new(body.origin, Size::new(MENU_WIDTH, body.size.height));
    let settings_rect = Rect::new(
        Point::new(body.origin.x + MENU_WIDTH + COLUMN_GAP, body.origin.y),
        Size::new(
            (body.size.width - MENU_WIDTH - COLUMN_GAP).max(0.0),
            body.size.height,
        ),
    );

    render_menu(scene, menu_rect, &hud.menu, theme);
    render_settings(scene, settings_rect, hud, theme);

    if hud.overlay == OverlayState::QuitConfirm {
        let confirm_rect = Rect::new(
            Point::new(viewport.width * 0.5 - 160.0, viewport.height * 0.5 - 70.0),
            Size::new(320.0, 140.0),
        );
        let confirm_content = titled_dialog(
            scene,
            confirm_rect,
            "Quit to desktop?",
            theme.dialog(),
            theme.title_style(),
        );
        label(
            scene,
            confirm_content,
            "Enter to confirm, Esc to cancel",
            theme.text_style(TextVariant::Muted),
        );
    }
}

/// Build this frame's full HUD scene: the always-on base HUD, plus the
/// pause overlay (and its nested quit-confirm dialog) whenever
/// `hud.overlay` isn't [`OverlayState::Closed`]. Rebuilt fresh every frame
/// — like every other `Scene` producer in this crate, nothing here is
/// retained between frames; `hud` (owned by [`HostApp`] across frames) is
/// what makes two consecutive calls differ.
fn build_hud_scene(viewport: Size, elapsed_secs: f32, hud: &HudState, theme: &WgpuTheme) -> Scene {
    let mut scene = Scene::new(viewport);
    draw_base_hud(&mut scene, viewport, elapsed_secs, hud, theme);
    if hud.overlay != OverlayState::Closed {
        draw_pause_overlay(&mut scene, viewport, hud, theme);
    }
    scene
}

struct HostApp {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    viewport: Size,
    theme: WgpuTheme,
    hud: HudState,
    text_system: BitmapTextSystem,
    start: Instant,
    last_toast_at: f32,
    toast_cycle: usize,
}

impl HostApp {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
            viewport: Size::new(960.0, 720.0),
            theme: WgpuTheme::default(),
            hud: HudState::new(),
            text_system: BitmapTextSystem::new(),
            start: Instant::now(),
            // Negative, not zero: the first `RedrawRequested` (elapsed ≈ 0)
            // must already satisfy `elapsed - last_toast_at >=
            // TOAST_INTERVAL_SECS`, so the first toast fires promptly
            // rather than only after one full interval of silence.
            last_toast_at: -TOAST_INTERVAL_SECS,
            toast_cycle: 0,
        }
    }

    /// Host-owned keyboard routing (ADR 0012: the host, not mkui, owns
    /// input here) — no backend maps winit's `Key`/`PhysicalKey` into
    /// [`mkui_core::input::Key`] yet, so this talks to [`MenuList`] through
    /// the same literal key-string vocabulary its own tests use
    /// (`"ArrowUp"`/`"ArrowDown"`), rather than inventing that mapping
    /// layer for a single example (out of this issue's scope — "only
    /// assembling what batch 15 already shipped").
    fn handle_key(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        match self.hud.overlay {
            OverlayState::Closed => {
                if matches!(&event.logical_key, WinitKey::Named(NamedKey::Escape)) {
                    self.hud.menu = build_pause_menu();
                    self.hud.overlay = OverlayState::Open;
                }
            }
            OverlayState::Open => match &event.logical_key {
                WinitKey::Named(NamedKey::Escape) => self.hud.overlay = OverlayState::Closed,
                WinitKey::Named(NamedKey::ArrowUp) => self.hud.menu.handle_key_down("ArrowUp"),
                WinitKey::Named(NamedKey::ArrowDown) => self.hud.menu.handle_key_down("ArrowDown"),
                WinitKey::Named(NamedKey::Enter) => {
                    let selected = self
                        .hud
                        .menu
                        .selected_item()
                        .map(|item| item.label().to_string());
                    match selected.as_deref() {
                        Some("Resume") => self.hud.overlay = OverlayState::Closed,
                        Some("Quit") => self.hud.overlay = OverlayState::QuitConfirm,
                        _ => {}
                    }
                }
                _ => {
                    if event.physical_key == PhysicalKey::Code(KeyCode::KeyM) {
                        self.hud.mute = !self.hud.mute;
                    }
                }
            },
            OverlayState::QuitConfirm => match &event.logical_key {
                WinitKey::Named(NamedKey::Enter) => event_loop.exit(),
                WinitKey::Named(NamedKey::Escape) => self.hud.overlay = OverlayState::Open,
                _ => {}
            },
        }
    }
}

impl ApplicationHandler for HostApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("host-embed-wgpu — host-owned 3D + mkui HUD (#175)")
                .with_inner_size(winit::dpi::LogicalSize::new(960.0, 720.0)),
        ) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("host-embed-wgpu: failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };
        let gpu = pollster::block_on(Gpu::new(window.clone()));
        self.gpu = Some(gpu);
        self.window = Some(window.clone());
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let window_matches = self
            .window
            .as_ref()
            .is_some_and(|window| window.id() == window_id);
        if !window_matches {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
                let scale = self
                    .window
                    .as_ref()
                    .map(|window| window.scale_factor())
                    .unwrap_or(1.0);
                self.viewport = logical_viewport(size, scale);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat {
                    self.handle_key(&event, event_loop);
                }
            }
            WindowEvent::RedrawRequested => {
                let elapsed = self.start.elapsed().as_secs_f32();
                maybe_fire_toast(
                    &mut self.hud.toasts,
                    &mut self.last_toast_at,
                    &mut self.toast_cycle,
                    elapsed,
                );
                self.hud
                    .tooltip
                    .set_focused(self.hud.overlay == OverlayState::Open);
                let scene = build_hud_scene(self.viewport, elapsed, &self.hud, &self.theme);
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.render(elapsed, &scene, &self.text_system);
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CI smoke (no display server): build the HUD scene once, then return
    // *before* `EventLoop::new()` — not inside `resumed`. On a displayless
    // Linux host (no X11, no Wayland) `EventLoop::new()` itself fails, so
    // `resumed` never runs to reach a headless check placed there; the
    // check has to happen before the event loop is constructed at all.
    if std::env::var_os("HEADLESS").is_some() {
        let _ = build_hud_scene(
            Size::new(960.0, 720.0),
            0.0,
            &HudState::new(),
            &WgpuTheme::default(),
        );
        return Ok(());
    }

    let event_loop = EventLoop::new()?;
    let mut app = HostApp::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_wgpu::Primitive;

    #[test]
    fn build_cube_vertices_emits_six_faces_of_two_triangles() {
        let vertices = build_cube_vertices();
        assert_eq!(vertices.len(), 36, "6 faces x 2 triangles x 3 vertices");
    }

    #[test]
    fn build_cube_vertices_shades_the_lit_face_brighter_than_its_opposite() {
        // The +Y face (index 2 -> vertices[12]) points mostly toward the
        // fixed light; its opposite, -Y (index 3 -> vertices[18]), points
        // away from it and must come out darker despite a lighter base
        // color — proving the per-face Lambertian term, not just the base
        // color, drives the baked vertex color.
        let vertices = build_cube_vertices();
        let top: f32 = vertices[12].color.iter().sum();
        let bottom: f32 = vertices[18].color.iter().sum();
        assert!(
            top > bottom,
            "lit +Y face ({top}) must be brighter than its opposite -Y face ({bottom})"
        );
    }

    fn viewport() -> Size {
        Size::new(960.0, 720.0)
    }

    fn has_text(scene: &Scene, content: &str) -> bool {
        scene
            .primitives
            .iter()
            .any(|p| matches!(p, Primitive::Text(t) if t.content == content))
    }

    #[test]
    fn build_hud_scene_always_paints_the_base_hud_without_a_pause_overlay() {
        let theme = WgpuTheme::default();
        let hud = HudState::new();
        let scene = build_hud_scene(viewport(), 0.0, &hud, &theme);
        assert!(
            !scene.primitives.is_empty(),
            "base HUD must paint something"
        );
        assert!(
            !has_text(&scene, "Paused"),
            "pause overlay must stay hidden until Escape is pressed"
        );
    }

    #[test]
    fn build_hud_scene_shows_the_pause_overlay_once_open() {
        let theme = WgpuTheme::default();
        let mut hud = HudState::new();
        hud.overlay = OverlayState::Open;
        let scene = build_hud_scene(viewport(), 0.0, &hud, &theme);
        assert!(has_text(&scene, "Paused"));
        assert!(has_text(&scene, "Resume"));
        assert!(has_text(&scene, "Quit"));
    }

    #[test]
    fn build_hud_scene_stacks_the_quit_confirm_dialog_over_the_pause_menu() {
        let theme = WgpuTheme::default();
        let mut hud = HudState::new();
        hud.overlay = OverlayState::QuitConfirm;
        let scene = build_hud_scene(viewport(), 0.0, &hud, &theme);
        assert!(has_text(&scene, "Quit to desktop?"));
        let backdrop_quads = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(q) if q.fill == theme.dialog().backdrop))
            .count();
        assert_eq!(
            backdrop_quads, 1,
            "only the stacked quit-confirm dialog paints a full-viewport dialog backdrop"
        );
    }

    #[test]
    fn build_pause_menu_preselects_resume() {
        let menu = build_pause_menu();
        assert_eq!(
            menu.selected_item().map(|item| item.label()),
            Some("Resume")
        );
    }

    #[test]
    fn maybe_fire_toast_pushes_one_toast_per_interval_and_cycles_messages() {
        let mut toasts = ToastStack::new();
        let mut last_toast_at = -TOAST_INTERVAL_SECS;
        let mut cycle = 0usize;

        maybe_fire_toast(&mut toasts, &mut last_toast_at, &mut cycle, 0.0);
        assert_eq!(
            toasts.active().len(),
            1,
            "first tick fires promptly at startup"
        );

        maybe_fire_toast(&mut toasts, &mut last_toast_at, &mut cycle, 1.0);
        assert_eq!(
            toasts.active().len(),
            1,
            "no new toast before the interval elapses"
        );

        maybe_fire_toast(
            &mut toasts,
            &mut last_toast_at,
            &mut cycle,
            TOAST_INTERVAL_SECS + 1.1,
        );
        assert_eq!(
            toasts.active().len(),
            2,
            "a new toast fires once the interval elapses"
        );
    }

    #[test]
    fn logical_viewport_divides_by_scale_factor() {
        let viewport = logical_viewport(winit::dpi::PhysicalSize::new(1920, 1440), 2.0);
        assert_eq!(viewport.width, 960.0);
        assert_eq!(viewport.height, 720.0);
    }
}
