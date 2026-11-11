//! Real backdrop blur (#187): a dedicated two-pass GPU pipeline that
//! samples whatever has already been painted into the target so far, in
//! scene order — never a static texture — and composites a tinted,
//! rounded, blurred panel over it.
//!
//! [`BackdropBlur`] is deliberately standalone, parallel to
//! `mkui_vector2d_wgpu::SlugAdapter`: it owns only its own pipelines, bind
//! group layout, sampler, and scratch texture, built against a borrowed
//! `device` and the target's own color format. That is what lets it be
//! driven directly against #106's surfaceless `super::offscreen::OffscreenRenderer`
//! in a GPU test (see the `gpu` test module below) — the same reason
//! `SlugAdapter` and `EmbeddedRenderer` are tested that way rather than
//! through the full windowed `Renderer`, which needs a real OS window and
//! so has no GPU-readback test of its own.
//!
//! [`super::Renderer`] owns one [`BackdropBlur`] and calls
//! [`BackdropBlur::composite`] once per `Primitive::Backdrop` it encounters,
//! after closing its own open UI render pass (so this pass can legally
//! sample the same texture that pass was just writing to) and before
//! reopening a fresh one with `LoadOp::Load`.
//!
//! ## Why two passes, not three
//!
//! A naive separable Gaussian blur is horizontal-pass-then-vertical-pass,
//! each a full fragment-shader sweep. This folds the vertical pass and the
//! tint composite into one: [`fs_composite`](backdrop.wgsl) samples the
//! horizontally-blurred scratch texture with vertical taps *and* mixes
//! `tint` over the result in the same fragment, drawn only across the
//! rounded-rect polygon [`composite_vertices`] builds. Pixels outside that
//! polygon are never touched by this draw — the panel's rounded corners
//! come from real geometry (the same corner-arc boundary
//! `tessellation::rounded_rect_boundary` builds for a rounded `Quad`), not
//! a shader-side distance mask.
//!
//! Both passes draw with blending disabled (`blend: None`): the fragment
//! computes the final pixel outright from the blurred sample (and, in pass
//! 2, the tint mix), so the draw must *replace* the destination, not blend
//! over the sharp backdrop still sitting there from before this primitive
//! ran.
//!
//! ## Scope decision (#187)
//!
//! This always blurs at the target's full resolution — no downsample/
//! upsample (dual Kawase) tier for large radii on a big viewport. UI blur
//! radii are modest (tens of logical px), so a fixed 9-tap kernel at full
//! res is cheap enough for a first cut; a downsampled path is a real but
//! separable perf follow-up, not a correctness gap, so it is deferred
//! rather than blocking this issue.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::tessellation::{clamp_corner_radii, rounded_rect_boundary, CORNER_ARC_SEGMENTS};
use crate::types::{Backdrop, CornerRadii, Point, Rect, Size};

/// Half-width of the discrete Gaussian kernel on each axis — must match
/// `backdrop.wgsl`'s `TAPS` constant exactly (`2*TAPS+1` taps per axis).
const TAPS: f32 = 4.0;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BlurParams {
    texel: [f32; 2],
    sigma: f32,
    step: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CompositeParams {
    texel: [f32; 2],
    sigma: f32,
    step: f32,
    tint: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CompositeVertex {
    position: [f32; 3],
    uv: [f32; 2],
}

impl CompositeVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Real backdrop-blur GPU resources (#187). See the module docs.
#[derive(Debug)]
pub struct BackdropBlur {
    format: wgpu::TextureFormat,
    blur_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    scratch_view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl BackdropBlur {
    /// Build against a borrowed `device` and the format of the target this
    /// instance will later composite into (the windowed renderer's linear
    /// intermediate, or — in a GPU test — `OffscreenRenderer::format()`).
    /// `width`/`height` are the target's physical-pixel size; the scratch
    /// texture is sized to match and recreated by [`Self::resize`].
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mkui-wgpu Backdrop Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mkui-wgpu Backdrop Bind Group Layout"),
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
            label: Some("mkui-wgpu Backdrop Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let targets = [Some(wgpu::ColorTargetState {
            format,
            // Each pass's fragment fully determines the output pixel from
            // the blurred sample (pass 2 also mixes in the tint) — this
            // must replace the destination, never blend over it. See the
            // module docs' "Why two passes" note.
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];

        let blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mkui-wgpu Backdrop Blur Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blur_h"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mkui-wgpu Backdrop Composite Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_composite"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(CompositeVertex::layout())],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_composite"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mkui-wgpu Backdrop Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let (width, height) = (width.max(1), height.max(1));
        let scratch_view = create_scratch_view(device, format, width, height);

        Self {
            format,
            blur_pipeline,
            composite_pipeline,
            bind_group_layout,
            sampler,
            scratch_view,
            width,
            height,
        }
    }

    /// Recreate the scratch texture for a new target size. No-op if the
    /// (clamped) size is unchanged.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.width && height == self.height {
            return;
        }
        self.scratch_view = create_scratch_view(device, self.format, width, height);
        self.width = width;
        self.height = height;
    }

    /// Blur whatever `target` already holds behind `backdrop.rect` and
    /// composite `backdrop.tint` over the blurred result, clipped to
    /// `backdrop.corner_radii`. `device_pixel_ratio` is the frame's
    /// physical-pixels-per-logical-pixel ratio (ADR 0006 §"Viewport units
    /// contract") — `backdrop.blur_radius` is authored in logical px like
    /// every other geometry value in the scene, so it scales with DPI here,
    /// at draw time, the same way a `Stroke` or corner radius does.
    ///
    /// The caller must not hold an open render pass on `target` when this is
    /// called — pass 1 samples it, which a still-open pass targeting the
    /// same texture cannot do. See the module docs.
    pub fn composite(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        backdrop: &Backdrop,
        device_pixel_ratio: f32,
    ) {
        let texel = [1.0 / self.width as f32, 1.0 / self.height as f32];
        let sigma = (backdrop.blur_radius.max(0.0) * device_pixel_ratio * 0.5).max(0.0001);
        let step = (sigma / TAPS).max(1.0);

        // Pass 1: horizontal blur, `target` -> `self.scratch_view`.
        let blur_params = BlurParams { texel, sigma, step };
        let blur_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mkui-wgpu Backdrop Blur Params"),
            contents: bytemuck::bytes_of(&blur_params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let blur_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mkui-wgpu Backdrop Blur Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(target),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: blur_params_buffer.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mkui-wgpu Backdrop Blur Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scratch_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &blur_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // Pass 2: vertical blur + tint composite, `self.scratch_view` ->
        // `target`, clipped to the rounded-rect polygon. An empty vertex
        // list (a degenerate zero-size rect) draws nothing — pass 1's cost
        // above is the only one a degenerate rect still pays.
        let vertices = composite_vertices(
            backdrop,
            device_pixel_ratio,
            self.width as f32,
            self.height as f32,
        );
        if vertices.is_empty() {
            return;
        }
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mkui-wgpu Backdrop Composite Vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let composite_params = CompositeParams {
            texel,
            sigma,
            step,
            tint: backdrop.tint.to_linear_rgba(),
        };
        let composite_params_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mkui-wgpu Backdrop Composite Params"),
                contents: bytemuck::bytes_of(&composite_params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mkui-wgpu Backdrop Composite Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.scratch_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: composite_params_buffer.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mkui-wgpu Backdrop Composite Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &composite_bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..vertices.len() as u32, 0..1);
        }
    }
}

fn create_scratch_view(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mkui-wgpu Backdrop Blur Scratch"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Triangle-fan the same rounded-rect boundary a rounded `Quad` fill uses
/// (`tessellation::rounded_rect_boundary`), emitting NDC position + a
/// pixel-space UV (the backdrop textures are sized to the physical target,
/// and every `Scene` primitive's pixel coordinates are already top-down —
/// the same space the texture's rows run in — so `uv = position / size`
/// needs no y-flip, unlike `present.wgsl`'s NDC-derived UV).
///
/// `backdrop.rect`/`corner_radii` are authored in **logical** px (ADR 0006
/// §"Viewport units contract"), like every other geometry value in the
/// scene, while `width`/`height` are the target's **physical**-pixel size —
/// so the rect and radii are scaled by `device_pixel_ratio` here, before
/// building the boundary, exactly as `tessellate_quad`'s caller scales a
/// `Stroke` width (this function is the one geometry path in the backdrop
/// pipeline that previously skipped that scaling, which left the panel
/// undersized and mispositioned at any ratio other than 1x).
fn composite_vertices(
    backdrop: &Backdrop,
    device_pixel_ratio: f32,
    width: f32,
    height: f32,
) -> Vec<CompositeVertex> {
    if backdrop.rect.size.width <= 0.0 || backdrop.rect.size.height <= 0.0 {
        return Vec::new();
    }
    let width = width.max(1.0);
    let height = height.max(1.0);
    let dpr = device_pixel_ratio.max(f32::EPSILON);
    let rect = Rect::new(
        Point::new(backdrop.rect.origin.x * dpr, backdrop.rect.origin.y * dpr),
        Size::new(
            backdrop.rect.size.width * dpr,
            backdrop.rect.size.height * dpr,
        ),
    );
    let corner_radii = CornerRadii {
        top_left: backdrop.corner_radii.top_left * dpr,
        top_right: backdrop.corner_radii.top_right * dpr,
        bottom_right: backdrop.corner_radii.bottom_right * dpr,
        bottom_left: backdrop.corner_radii.bottom_left * dpr,
    };
    let radii = clamp_corner_radii(rect, corner_radii);
    let boundary = rounded_rect_boundary(rect, radii, CORNER_ARC_SEGMENTS);
    if boundary.len() < 3 {
        return Vec::new();
    }
    let center = Point::new(
        rect.origin.x + rect.size.width * 0.5,
        rect.origin.y + rect.size.height * 0.5,
    );
    let to_vertex = |p: Point| CompositeVertex {
        position: [(p.x / width) * 2.0 - 1.0, 1.0 - (p.y / height) * 2.0, 0.0],
        uv: [p.x / width, p.y / height],
    };
    let n = boundary.len();
    let mut vertices = Vec::with_capacity(n * 3);
    for i in 0..n {
        vertices.push(to_vertex(center));
        vertices.push(to_vertex(boundary[i]));
        vertices.push(to_vertex(boundary[(i + 1) % n]));
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_wgsl_parses_and_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("backdrop.wgsl"))
            .expect("backdrop.wgsl must parse");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .expect("backdrop.wgsl must pass naga validation");
    }

    #[test]
    fn zero_size_rect_yields_no_composite_vertices() {
        let backdrop = Backdrop {
            rect: crate::types::Rect::new(
                crate::types::Point::new(0.0, 0.0),
                crate::types::Size::new(0.0, 0.0),
            ),
            corner_radii: crate::types::CornerRadii::all(8.0),
            blur_radius: 24.0,
            tint: crate::types::Color::rgba(0.0, 0.0, 0.0, 0.4),
        };
        assert!(composite_vertices(&backdrop, 1.0, 400.0, 300.0).is_empty());
    }

    #[test]
    fn composite_vertices_stay_within_the_target_and_map_uv_to_position() {
        let backdrop = Backdrop {
            rect: crate::types::Rect::new(
                crate::types::Point::new(20.0, 30.0),
                crate::types::Size::new(100.0, 60.0),
            ),
            corner_radii: crate::types::CornerRadii::all(12.0),
            blur_radius: 24.0,
            tint: crate::types::Color::rgba(0.0, 0.0, 0.0, 0.4),
        };
        let vertices = composite_vertices(&backdrop, 1.0, 400.0, 300.0);
        assert!(!vertices.is_empty());
        for v in &vertices {
            assert!((-1.0..=1.0).contains(&v.position[0]));
            assert!((-1.0..=1.0).contains(&v.position[1]));
            assert!((0.0..=1.0).contains(&v.uv[0]));
            assert!((0.0..=1.0).contains(&v.uv[1]));
            // uv is exactly the NDC-to-pixel inverse of position (modulo the
            // y-flip NDC applies and uv does not) — pin that relationship so
            // a future edit to one mapping can't silently desync the other.
            let expected_u = (v.position[0] + 1.0) * 0.5;
            let expected_v = (1.0 - v.position[1]) * 0.5;
            assert!((v.uv[0] - expected_u).abs() < 1e-5);
            assert!((v.uv[1] - expected_v).abs() < 1e-5);
        }
    }

    #[test]
    fn composite_vertices_scale_with_device_pixel_ratio() {
        // Regression (#251 review): `backdrop.rect` is authored in logical
        // px but `width`/`height` are the physical target size, so a
        // logical-px rect must occupy the *same* relative portion of the
        // physical target at every DPI — a HiDPI (2x) window doubles both
        // the rect's physical size and the target's physical size, so the
        // NDC/uv coverage here must come out identical to the 1x case, not
        // half of it. (The bug this guards: `composite_vertices` used to
        // skip the `device_pixel_ratio` scale entirely, leaving the panel
        // undersized and mispositioned at any ratio other than 1x, even
        // though the panel's border — drawn through the ordinary `Quad`
        // path, which already scales correctly — looked fine.)
        let backdrop = Backdrop {
            rect: crate::types::Rect::new(
                crate::types::Point::new(20.0, 30.0),
                crate::types::Size::new(100.0, 60.0),
            ),
            corner_radii: crate::types::CornerRadii::all(12.0),
            blur_radius: 24.0,
            tint: crate::types::Color::rgba(0.0, 0.0, 0.0, 0.4),
        };
        let at_1x = composite_vertices(&backdrop, 1.0, 400.0, 300.0);
        let at_2x = composite_vertices(&backdrop, 2.0, 800.0, 600.0);
        assert_eq!(at_1x.len(), at_2x.len());
        assert!(!at_1x.is_empty());
        for (a, b) in at_1x.iter().zip(at_2x.iter()) {
            for i in 0..3 {
                assert!(
                    (a.position[i] - b.position[i]).abs() < 1e-4,
                    "position[{i}] differs between 1x and 2x: {:?} vs {:?}",
                    a.position,
                    b.position
                );
            }
            for i in 0..2 {
                assert!(
                    (a.uv[i] - b.uv[i]).abs() < 1e-4,
                    "uv[{i}] differs between 1x and 2x: {:?} vs {:?}",
                    a.uv,
                    b.uv
                );
            }
        }
    }

    // ---- GPU acceptance tests (#187): real-content sampling + blur ---------
    //
    // Reuses the #106 surfaceless `OffscreenRenderer` harness exactly as
    // `embedded::tests::gpu` and `slug_gpu` do: it stands in for "the
    // already-rendered frame behind the panel" with its own
    // RENDER_ATTACHMENT | COPY_SRC | TEXTURE_BINDING-less target — actually
    // reused directly as the `target` this module composites into and
    // reads back from, since its format (`Rgba8Unorm`) matches
    // `super::INTERMEDIATE_FORMAT`.
    #[cfg(feature = "gpu-tests")]
    mod gpu {
        use super::super::*;
        use crate::render::offscreen::OffscreenRenderer;
        use crate::types::{Color, CornerRadii, Point, Rect, Size};

        fn host(width: u32, height: u32) -> OffscreenRenderer {
            OffscreenRenderer::new(width, height).expect(
                "offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner",
            )
        }

        fn pixel_at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
            let idx = ((y * width + x) * 4) as usize;
            [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
        }

        fn panel(size: u32) -> Backdrop {
            Backdrop {
                rect: Rect::new(
                    Point::new(size as f32 * 0.25, size as f32 * 0.25),
                    Size::new(size as f32 * 0.5, size as f32 * 0.5),
                ),
                corner_radii: CornerRadii::all(0.0),
                blur_radius: 16.0,
                // No tint — isolate the blur/sampling behavior from the mix.
                tint: Color::rgba(0.0, 0.0, 0.0, 0.0),
            }
        }

        #[test]
        fn interior_tracks_the_live_backdrop_not_a_static_texture() {
            let size = 64u32;
            let host = host(size, size);
            let blur = BackdropBlur::new(host.device(), host.format(), size, size);
            let backdrop = panel(size);
            let center = size / 2;

            host.clear(wgpu::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            {
                let mut encoder =
                    host.device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("backdrop red test encoder"),
                        });
                blur.composite(host.device(), &mut encoder, host.view(), &backdrop, 1.0);
                host.queue().submit(Some(encoder.finish()));
            }
            let red_interior = pixel_at(
                &host.read_rgba().expect("red readback"),
                size,
                center,
                center,
            );

            host.clear(wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            });
            {
                let mut encoder =
                    host.device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("backdrop blue test encoder"),
                        });
                blur.composite(host.device(), &mut encoder, host.view(), &backdrop, 1.0);
                host.queue().submit(Some(encoder.finish()));
            }
            let blue_interior = pixel_at(
                &host.read_rgba().expect("blue readback"),
                size,
                center,
                center,
            );

            assert_ne!(
                red_interior, blue_interior,
                "the panel's interior must track whatever was actually behind it \
                 this frame, not a texture captured once — red backdrop gave \
                 {red_interior:?}, blue backdrop gave {blue_interior:?}"
            );
            // It should still read as "mostly that color" (no tint, and a
            // uniform backdrop blurs to itself).
            assert!(red_interior[0] > 200 && red_interior[2] < 50);
            assert!(blue_interior[2] > 200 && blue_interior[0] < 50);
        }

        #[test]
        fn a_sharp_edge_behind_the_panel_blurs_into_a_gradient() {
            let size = 64u32;
            let host = host(size, size);
            let blur = BackdropBlur::new(host.device(), host.format(), size, size);
            let backdrop = panel(size);

            // Clear red, then overwrite the right half with blue via a raw
            // copy-free approach isn't available on this harness, so instead
            // render a UI quad for the right half through the UI pipeline —
            // reuse the same technique `embedded`'s tests use (a scene +
            // `tessellate_primitives`) to paint a hard vertical edge straight
            // down the middle of the panel before compositing the blur.
            host.clear(wgpu::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            {
                let ui_pipeline = crate::render::build_ui_pipeline(
                    host.device(),
                    host.format(),
                    1,
                    "fs_ui_triangles_encode_srgb",
                );
                let mut scene = crate::Scene::new(crate::Size::new(size as f32, size as f32));
                scene.push(crate::Primitive::Quad(crate::Quad {
                    rect: crate::Rect::new(
                        crate::Point::new(size as f32 * 0.5, 0.0),
                        crate::Size::new(size as f32 * 0.5, size as f32),
                    ),
                    fill: crate::Color::rgba(0.0, 0.0, 1.0, 1.0),
                    corner_radii: crate::CornerRadii::all(0.0),
                    stroke: None,
                }));
                let triangles = crate::tessellate_primitives(
                    &scene.primitives,
                    &mkui_text::BitmapTextSystem::new(),
                    1.0,
                );
                let vertices = crate::render::gui_vertices(&triangles, size as f32, size as f32);
                let vertex_buffer =
                    host.device()
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("edge test vertices"),
                            contents: bytemuck::cast_slice(&vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                let mut encoder =
                    host.device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("edge paint encoder"),
                        });
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("edge paint pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: host.view(),
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
                    pass.set_pipeline(&ui_pipeline);
                    pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    pass.draw(0..vertices.len() as u32, 0..1);
                }
                host.queue().submit(Some(encoder.finish()));
            }

            {
                let mut encoder =
                    host.device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("backdrop edge-blur encoder"),
                        });
                blur.composite(host.device(), &mut encoder, host.view(), &backdrop, 1.0);
                host.queue().submit(Some(encoder.finish()));
            }

            let after = host.read_rgba().expect("edge-blur readback");
            let mid_y = size / 2;
            let center = pixel_at(&after, size, size / 2, mid_y);
            // The exact seam must be a blend, not pure red or pure blue —
            // proof this is a multi-tap blur, not a single-tap copy.
            assert!(
                center[0] > 20 && center[0] < 235,
                "center red channel {} should sit between pure red and pure blue",
                center[0]
            );
            assert!(
                center[2] > 20 && center[2] < 235,
                "center blue channel {} should sit between pure red and pure blue",
                center[2]
            );
        }

        #[test]
        fn pixels_outside_the_rounded_rect_are_left_untouched() {
            let size = 64u32;
            let host = host(size, size);
            let blur = BackdropBlur::new(host.device(), host.format(), size, size);
            // A fully rounded panel (radius = half the shorter side) so its
            // extreme corner pixel sits well outside the rounded polygon
            // while still inside the bounding rect.
            let backdrop = Backdrop {
                rect: Rect::new(
                    Point::new(size as f32 * 0.25, size as f32 * 0.25),
                    Size::new(size as f32 * 0.5, size as f32 * 0.5),
                ),
                corner_radii: CornerRadii::all(size as f32 * 0.25),
                blur_radius: 16.0,
                tint: Color::rgba(0.0, 1.0, 0.0, 1.0),
            };

            host.clear(wgpu::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            let before = host.read_rgba().expect("baseline readback");
            let corner_x = (size as f32 * 0.25) as u32 + 1;
            let corner_y = (size as f32 * 0.25) as u32 + 1;
            let before_corner = pixel_at(&before, size, corner_x, corner_y);
            assert_eq!(
                before_corner,
                [255, 0, 0, 255],
                "precondition: solid red clear"
            );

            let mut encoder =
                host.device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("rounded-corner test encoder"),
                    });
            blur.composite(host.device(), &mut encoder, host.view(), &backdrop, 1.0);
            host.queue().submit(Some(encoder.finish()));

            let after = host.read_rgba().expect("rounded-corner readback");
            let after_corner = pixel_at(&after, size, corner_x, corner_y);
            assert_eq!(
                after_corner, before_corner,
                "a pixel just inside the bounding rect but outside the rounded \
                 polygon must be untouched by the backdrop draw"
            );
        }
    }
}
