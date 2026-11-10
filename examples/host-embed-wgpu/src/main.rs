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
//! `cargo run -p host-embed-wgpu --release` opens the window.
//! `HEADLESS=1 cargo run -p host-embed-wgpu` exits before opening a window,
//! the same CI-smoke convention `native-window`/`native-showcase` use.

mod mat4;

use std::sync::Arc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

use mkui_text::BitmapTextSystem;
use mkui_wgpu::{
    Color, CornerRadii, EmbeddedColorSpace, EmbeddedRenderer, Point, Primitive, Quad, Rect, Scene,
    Size,
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

/// Build the HUD scene: a small translucent badge + status dot in the
/// corner. Deliberately minimal (mirrors `native-window`'s single-quad
/// smoke) — proving mkui draws *on top of* the host's already-rendered
/// cube, in the same frame, is the point; an elaborate HUD is not.
fn build_hud_scene(viewport: Size) -> Scene {
    let mut scene = Scene::new(viewport);
    scene.push(Primitive::Quad(Quad {
        rect: Rect::new(Point::new(24.0, 24.0), Size::new(220.0, 64.0)),
        fill: Color::rgba(0.07, 0.09, 0.12, 0.82),
        corner_radii: CornerRadii::all(12.0),
        stroke: None,
    }));
    scene.push(Primitive::Quad(Quad {
        rect: Rect::new(Point::new(36.0, 36.0), Size::new(28.0, 28.0)),
        fill: Color::rgba(0.35, 0.85, 0.55, 1.0),
        corner_radii: CornerRadii::all(6.0),
        stroke: None,
    }));
    scene
}

struct HostApp {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    hud_scene: Scene,
    text_system: BitmapTextSystem,
    start: Instant,
}

impl HostApp {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
            hud_scene: build_hud_scene(Size::new(960.0, 720.0)),
            text_system: BitmapTextSystem::new(),
            start: Instant::now(),
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
                self.hud_scene.viewport = logical_viewport(size, scale);
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = self.gpu.as_mut() {
                    let elapsed = self.start.elapsed().as_secs_f32();
                    gpu.render(elapsed, &self.hud_scene, &self.text_system);
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
        let _ = build_hud_scene(Size::new(960.0, 720.0));
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

    #[test]
    fn build_hud_scene_carries_the_badge_and_status_dot() {
        let scene = build_hud_scene(Size::new(960.0, 720.0));
        assert_eq!(scene.primitives.len(), 2, "badge panel + status dot");
    }

    #[test]
    fn logical_viewport_divides_by_scale_factor() {
        let viewport = logical_viewport(winit::dpi::PhysicalSize::new(1920, 1440), 2.0);
        assert_eq!(viewport.width, 960.0);
        assert_eq!(viewport.height, 720.0);
    }
}
