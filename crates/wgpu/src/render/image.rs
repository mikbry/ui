//! GPU texture-sampling pipeline for `Primitive::Image` (#196): a textured
//! quad with a per-fragment rounded-rect SDF clip. See `image.wgsl` for the
//! shader this wires up, and [`crate::components::image`] for the CPU-side
//! fit/UV math that produces the [`PlacedImage`] this module draws.
//!
//! Unlike every other lane in [`super`], an `Image` draw needs no pass-
//! splitting dance (contrast [`super::backdrop`], which must sample the
//! frame itself) — it samples its own independent texture, so it draws
//! inside the same open UI render pass as everything else, just with its
//! own pipeline bound for the duration of each draw call.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::types::{ImageEncoding, ImageSource, PlacedImage};

/// Cap on distinct decoded-bytes cache entries, mirroring
/// `components::card`'s `ROUNDED_RECT_CACHE_CAP` — a steady-state UI has a
/// small, fixed set of images; a clear-all-on-overflow is simplest and
/// this cache is cold again within a frame or two if it ever is hit.
const DECODED_TEXTURE_CACHE_CAP: usize = 128;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct ImageVertex {
    position: [f32; 3],
    uv: [f32; 2],
    local_pos: [f32; 2],
    rect_size: [f32; 2],
    corner_radius: f32,
}

impl ImageVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x2,
        4 => Float32,
    ];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Map `placed`'s pixel-space rect/UV into NDC plus the per-vertex SDF
/// inputs `image.wgsl`'s fragment stage needs — the textured-quad analogue
/// of `render::gui_vertices`. Two triangles (6 vertices, no index buffer),
/// same convention `gui_vertices` uses; `cull_mode: None` (the pipeline's
/// default) means winding direction is not load-bearing.
pub(crate) fn image_vertices(placed: &PlacedImage, width: f32, height: f32) -> [ImageVertex; 6] {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let r = placed.rect;

    // Corner order: top-left, top-right, bottom-left, bottom-right.
    let px = [
        (r.origin.x, r.origin.y),
        (r.origin.x + r.size.width, r.origin.y),
        (r.origin.x, r.origin.y + r.size.height),
        (r.origin.x + r.size.width, r.origin.y + r.size.height),
    ];
    let uv = [
        (placed.uv_min[0], placed.uv_min[1]),
        (placed.uv_max[0], placed.uv_min[1]),
        (placed.uv_min[0], placed.uv_max[1]),
        (placed.uv_max[0], placed.uv_max[1]),
    ];
    let local = [
        (0.0, 0.0),
        (r.size.width, 0.0),
        (0.0, r.size.height),
        (r.size.width, r.size.height),
    ];
    // A single uniform radius — `PlacedImage::corner_radii` is a
    // `CornerRadii` only for type-symmetry with `Quad`; #196 never
    // constructs one with differing per-corner values (see
    // `components::image::image`). Clamped to half the shorter side,
    // mirroring `components::button::rounded_rect_path`'s own
    // `radius.max(0.0).min(w.min(h) * 0.5)` convention (#331) — otherwise a
    // theme radius larger than a small image's own box overshoots the SDF's
    // valid range in `image.wgsl` and renders a distorted, partially
    // transparent shape instead of a clamped rounded square.
    let corner_radius = placed
        .corner_radii
        .top_left
        .max(0.0)
        .min(r.size.width.min(r.size.height) * 0.5);
    let rect_size = [r.size.width, r.size.height];

    let vertex = |i: usize| ImageVertex {
        position: [
            (px[i].0 / width) * 2.0 - 1.0,
            1.0 - (px[i].1 / height) * 2.0,
            0.0,
        ],
        uv: [uv[i].0, uv[i].1],
        local_pos: [local[i].0, local[i].1],
        rect_size,
        corner_radius,
    };

    [
        vertex(0),
        vertex(1),
        vertex(2),
        vertex(1),
        vertex(3),
        vertex(2),
    ]
}

/// Bind group layout every `Image` draw shares: a filterable texture plus a
/// filtering sampler, both fragment-only.
pub(crate) fn image_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mkui-wgpu Image Bind Group Layout"),
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
        ],
    })
}

/// Linear-filtered, clamp-to-edge sampler shared by every `Image` draw.
/// Clamping never matters for `ImageFit::Contain`'s letterbox bars — the
/// fragment shader already treats an out-of-`[0,1]` `uv` as transparent
/// before it would sample the clamped edge pixel — but it is still the
/// correct no-surprises default for the in-bounds `[0,1]` sampling every
/// draw does.
pub(crate) fn image_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("mkui-wgpu Image Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// Build the `Image` pipeline against `format`/`sample_count` — mirrors
/// `render::build_ui_pipeline`'s shape exactly, just with `image.wgsl`'s
/// entry points and vertex layout, and one bind group (texture + sampler)
/// instead of none.
pub(crate) fn build_image_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    sample_count: u32,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mkui-wgpu Image Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mkui-wgpu Image Pipeline Layout"),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    let targets = [Some(wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mkui-wgpu Image Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_image"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(ImageVertex::layout())],
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
            entry_point: Some("fs_image"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Create an `Rgba8UnormSrgb` texture sized for `rgba` and upload it,
/// returning a view. `Rgba8UnormSrgb` (not plain `Rgba8Unorm`) so sampling
/// it in `image.wgsl` linearizes RGB on read, matching every other lane's
/// contract: composite into the linear intermediate in linear light (ADR
/// 0006 §"Color space + blending"). `mkui_core::image::decode_rgba8`'s
/// output is sRGB-perceptual RGB with linear (straight) alpha — exactly
/// what an `*Srgb` format view expects (alpha is never gamma-encoded by
/// the format, only RGB is).
pub(crate) fn upload_rgba8_srgb(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mkui-wgpu Image Texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width.max(1)),
            rows_per_image: Some(height.max(1)),
        },
        size,
    );
    // The local `texture` binding is dropped at the end of this function;
    // the returned `TextureView` keeps the underlying resource alive via
    // its own internal reference, the same pattern
    // `render::create_intermediate_view` already relies on.
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Decode `bytes` (as `encoding`) and upload the result to a texture,
/// `None` on either a decode failure or a decoded size past `device`'s
/// `max_texture_dimension_2d` limit. A free function (not an `ImageTextures`
/// method) so [`ImageTextures::resolve`] can cache this `None` itself — see
/// that method's negative-cache note (#331).
fn decode_and_upload_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoding: &ImageEncoding,
    bytes: &[u8],
) -> Option<wgpu::TextureView> {
    let format = match encoding {
        ImageEncoding::Png => mkui_runtime::ImageFormat::Png,
        ImageEncoding::WebP => mkui_runtime::ImageFormat::WebP,
    };
    let decoded = mkui_core::image::decode_rgba8(bytes, format).ok()?;
    // `create_texture` validation panics on a dimension past the device's
    // advertised limit (8192 on the default limit set) rather than
    // returning an error — guard it here instead of letting a too-large
    // decoded image panic the whole frame.
    let max_dim = device.limits().max_texture_dimension_2d;
    if decoded.width > max_dim || decoded.height > max_dim {
        return None;
    }
    Some(upload_rgba8_srgb(
        device,
        queue,
        decoded.width,
        decoded.height,
        &decoded.rgba,
    ))
}

/// Resolves [`ImageSource`]s to drawable bind groups: decodes and caches an
/// `Encoded` source's texture on first use (keyed by its `cache_key`), and
/// looks up a `Host` source's already-registered texture by handle.
///
/// Built once per [`super::Renderer`]/[`super::embedded::EmbeddedRenderer`]
/// and held for the renderer's lifetime so neither cache clears on resize.
#[derive(Debug)]
pub(crate) struct ImageTextures {
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    // `None` is a negative-cache entry: a `cache_key` whose decode already
    // failed once (bad bytes, or a decoded size past the device's
    // `max_texture_dimension_2d`). Without it, a permanently-broken image
    // re-runs its full decode attempt every single frame forever instead of
    // once (#331).
    decoded: HashMap<u64, Option<wgpu::BindGroup>>,
    host: HashMap<u64, wgpu::BindGroup>,
    next_host_handle: u64,
}

impl ImageTextures {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            bind_group_layout: image_bind_group_layout(device),
            sampler: image_sampler(device),
            decoded: HashMap::new(),
            host: HashMap::new(),
            // 0 is never handed out, so a caller that forgets to check a
            // `register_host_texture` result against a sentinel still
            // can't accidentally collide with a real handle at 0.
            next_host_handle: 1,
        }
    }

    pub(crate) fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    fn build_bind_group(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mkui-wgpu Image Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Register a host-owned texture view (already in VRAM — this crate
    /// never decodes or uploads anything for it) and return the opaque
    /// handle an `mkui_runtime::TextureHandle` embeds. `view`'s format
    /// should be (or be an `*Srgb` reinterpretation of) an sRGB format, the
    /// same linearize-on-sample contract [`upload_rgba8_srgb`] gives a
    /// decoded-bytes texture — otherwise the host's image looks washed out
    /// next to everything else this renderer draws.
    pub(crate) fn register_host_texture(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
    ) -> u64 {
        let handle = self.next_host_handle;
        self.next_host_handle += 1;
        let bind_group = self.build_bind_group(device, view);
        self.host.insert(handle, bind_group);
        handle
    }

    /// Drop a previously-registered host texture's bind group. A no-op if
    /// `handle` was never registered or was already unregistered.
    pub(crate) fn unregister_host_texture(&mut self, handle: u64) {
        self.host.remove(&handle);
    }

    /// Resolve `source` to a bind group to draw with. `None` means: a
    /// `Host` handle nothing (or no longer) registered, or an `Encoded`
    /// decode failure — bytes that passed
    /// `mkui_core::components::Image::from_encoded`'s construction-time
    /// validation but somehow fail here regardless. Either way the caller
    /// skips drawing that one primitive rather than panicking the frame.
    ///
    /// An `Encoded` decode failure is itself cached (as `None`, keyed by
    /// `cache_key`) rather than retried: without this, a permanently-broken
    /// image (bad bytes, or a decoded size past the device's texture
    /// dimension limit) would re-run its full decode attempt every single
    /// frame forever instead of once (#331).
    pub(crate) fn resolve(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &ImageSource,
    ) -> Option<&wgpu::BindGroup> {
        match source {
            ImageSource::Host(handle) => self.host.get(handle),
            ImageSource::Encoded {
                encoding,
                bytes,
                cache_key,
            } => {
                if !self.decoded.contains_key(cache_key) {
                    let bind_group = decode_and_upload_image(device, queue, encoding, bytes)
                        .map(|view| self.build_bind_group(device, &view));
                    if self.decoded.len() >= DECODED_TEXTURE_CACHE_CAP {
                        self.decoded.clear();
                    }
                    self.decoded.insert(*cache_key, bind_group);
                }
                self.decoded.get(cache_key).and_then(Option::as_ref)
            }
        }
    }
}

/// Build a GPU vertex buffer for one `Image` draw. Small (6 vertices) and
/// created fresh per draw per frame — the same per-frame-buffer-per-lane
/// shape `render::Renderer::render`'s own `make_triangles` closure already
/// uses for the whole triangle lane; the *expensive* GPU work (decode +
/// texture upload) is what [`ImageTextures`] caches, not this.
pub(crate) fn image_vertex_buffer(
    device: &wgpu::Device,
    placed: &PlacedImage,
    viewport_width: f32,
    viewport_height: f32,
) -> wgpu::Buffer {
    let vertices = image_vertices(placed, viewport_width, viewport_height);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mkui-wgpu Image Vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CornerRadii, Point, Rect, Size};

    fn placed(rect: Rect, uv_min: [f32; 2], uv_max: [f32; 2], radius: f32) -> PlacedImage {
        PlacedImage {
            rect,
            uv_min,
            uv_max,
            corner_radii: CornerRadii::all(radius),
            source: ImageSource::Host(1),
        }
    }

    #[test]
    fn image_vertices_map_pixel_rect_to_ndc() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 100.0));
        let verts = image_vertices(&placed(rect, [0.0, 0.0], [1.0, 1.0], 0.0), 100.0, 100.0);
        // Top-left corner (vertex 0): pixel (0,0) -> NDC (-1, 1).
        assert_eq!(verts[0].position, [-1.0, 1.0, 0.0]);
        // Top-right corner (vertex 1): pixel (100,0) -> NDC (1, 1).
        assert_eq!(verts[1].position, [1.0, 1.0, 0.0]);
        // Bottom-left corner (vertex 2): pixel (0,100) -> NDC (-1, -1).
        assert_eq!(verts[2].position, [-1.0, -1.0, 0.0]);
    }

    #[test]
    fn image_vertices_carry_the_resolved_uv_rect_per_corner() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 100.0));
        let verts = image_vertices(&placed(rect, [0.25, 0.0], [0.75, 1.0], 0.0), 200.0, 200.0);
        assert_eq!(verts[0].uv, [0.25, 0.0]); // top-left
        assert_eq!(verts[1].uv, [0.75, 0.0]); // top-right
        assert_eq!(verts[2].uv, [0.25, 1.0]); // bottom-left
    }

    #[test]
    fn image_vertices_carry_rect_size_and_corner_radius_on_every_vertex() {
        let rect = Rect::new(Point::new(10.0, 20.0), Size::new(64.0, 32.0));
        let verts = image_vertices(&placed(rect, [0.0, 0.0], [1.0, 1.0], 8.0), 200.0, 200.0);
        for v in &verts {
            assert_eq!(v.rect_size, [64.0, 32.0]);
            assert_eq!(v.corner_radius, 8.0);
        }
    }

    #[test]
    fn image_vertices_clamps_corner_radius_to_half_the_shorter_side() {
        // Shorter side is 8.0 (height); an oversized 100.0 radius clamps to
        // 4.0 — the `rounded_rect_path_clamps_radius_to_half_the_shorter_side`
        // regression test's counterpart for Image (#331).
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(20.0, 8.0));
        let verts = image_vertices(&placed(rect, [0.0, 0.0], [1.0, 1.0], 100.0), 200.0, 200.0);
        for v in &verts {
            assert_eq!(v.corner_radius, 4.0);
        }
    }

    #[test]
    fn image_vertices_local_pos_spans_zero_to_rect_size() {
        let rect = Rect::new(Point::new(5.0, 5.0), Size::new(40.0, 20.0));
        let verts = image_vertices(&placed(rect, [0.0, 0.0], [1.0, 1.0], 0.0), 200.0, 200.0);
        // Corner order (see `image_vertices`' docs): top-left, top-right,
        // bottom-left, bottom-right, emitted as two triangles
        // (0,1,2),(1,3,2) — vertex 4 is the second triangle's own
        // bottom-right corner.
        assert_eq!(verts[0].local_pos, [0.0, 0.0]);
        assert_eq!(verts[1].local_pos, [40.0, 0.0]);
        assert_eq!(verts[2].local_pos, [0.0, 20.0]);
        assert_eq!(verts[4].local_pos, [40.0, 20.0]);
        // All six vertices collectively cover exactly the four corners.
        let corners: std::collections::HashSet<[u32; 2]> = verts
            .iter()
            .map(|v| [v.local_pos[0].to_bits(), v.local_pos[1].to_bits()])
            .collect();
        assert_eq!(
            corners.len(),
            4,
            "two triangles must share exactly 4 distinct corners"
        );
    }

    // ---- GPU acceptance test (#331): decode-failure negative cache --------
    //
    // `resolve` needs a real `wgpu::Device`/`Queue` to exercise, so this
    // reuses the #106 surfaceless `OffscreenRenderer` harness the same way
    // `backdrop::tests::gpu` and `embedded::tests::gpu` do.
    #[cfg(feature = "gpu-tests")]
    mod gpu {
        use super::super::*;
        use crate::render::offscreen::OffscreenRenderer;

        fn host(width: u32, height: u32) -> OffscreenRenderer {
            OffscreenRenderer::new(width, height).expect(
                "offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner",
            )
        }

        #[test]
        fn resolve_caches_a_decode_failure_instead_of_retrying_it_every_frame() {
            let renderer = host(8, 8);
            let mut textures = ImageTextures::new(renderer.device());
            let source = ImageSource::Encoded {
                encoding: ImageEncoding::Png,
                bytes: std::sync::Arc::from(b"not a png".as_slice()),
                cache_key: 42,
            };

            assert!(
                textures
                    .resolve(renderer.device(), renderer.queue(), &source)
                    .is_none(),
                "invalid bytes must fail to resolve"
            );
            assert_eq!(
                textures.decoded.len(),
                1,
                "the failed decode must be cached, not left unrecorded"
            );
            assert!(
                matches!(textures.decoded.get(&42), Some(None)),
                "a decode failure's cache entry is `None` (negative cache), not absent"
            );

            // A second resolve on the same `cache_key` must hit that
            // negative-cache entry rather than re-running the decode
            // attempt: the cache must not grow, and the result stays
            // `None`.
            assert!(textures
                .resolve(renderer.device(), renderer.queue(), &source)
                .is_none());
            assert_eq!(textures.decoded.len(), 1);
        }
    }
}
