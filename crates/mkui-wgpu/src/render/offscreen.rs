//! Surfaceless offscreen renderer + GPU readback harness (#106).
//!
//! Sprint 7 needs deterministic native WGPU tests without opening a window.
//! The window-bound [`Renderer`](super::Renderer) negotiates a `Surface` from
//! an `Arc<Window>`; under `HEADLESS=1` it exits before adapter/device
//! creation and so cannot validate a real GPU pipeline. This module adds a
//! reusable, **surfaceless** renderer that owns:
//!
//! - a surfaceless adapter / device / queue,
//! - an RGBA render target with `RENDER_ATTACHMENT | COPY_SRC`,
//! - a readback buffer padded to `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`
//!   (256-byte) row alignment,
//! - submission, polling, buffer mapping, and row-unpadding helpers.
//!
//! It does **not** manufacture a fake `Window` or `Surface`, and it does not
//! touch the public windowed renderer API — see [`super::Renderer`], which is
//! left exactly as is.
//!
//! # Backend + adapter contract
//!
//! The harness selects [`wgpu::Backends::VULKAN`] only and asserts the chosen
//! adapter is Vulkan + CPU (Mesa/Lavapipe on Linux CI). Adapter or device
//! unavailability is a hard error ([`OffscreenError`]) — never a silent skip.
//! Callers in tests `.expect()` the constructor so the CI test fails loudly if
//! no Lavapipe ICD was provisioned. On Linux CI the job installs
//! `mesa-vulkan-drivers vulkan-tools` and pins
//! `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`.

use std::fmt;

/// The CI harness backend selection: Vulkan only. On Linux CI this resolves to
/// Mesa/Lavapipe through the pinned `VK_ICD_FILENAMES`.
pub const CI_BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN;

/// Render-target / readback texel format. `Rgba8Unorm` (not sRGB) so a cleared
/// or drawn value reads back byte-exact — the harness asserts on raw bytes.
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Bytes per RGBA8 texel.
pub const BYTES_PER_PIXEL: u32 = 4;

/// Error raised when the surfaceless harness cannot be provisioned or a
/// readback fails. Treated as a test failure by callers — there is no silent
/// skip path (#106).
#[derive(Debug, Clone)]
pub enum OffscreenError {
    /// No adapter matched the requested backends (e.g. no Vulkan ICD present).
    AdapterUnavailable(String),
    /// An adapter was found but `request_device` failed.
    DeviceUnavailable(String),
    /// `Device::poll` reported an error while waiting on a submission/map.
    Poll(String),
    /// `Buffer::map_async` reported an error or the callback channel dropped.
    Map(String),
}

impl fmt::Display for OffscreenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OffscreenError::AdapterUnavailable(m) => {
                write!(f, "offscreen adapter unavailable: {m}")
            }
            OffscreenError::DeviceUnavailable(m) => {
                write!(f, "offscreen device unavailable: {m}")
            }
            OffscreenError::Poll(m) => write!(f, "offscreen device poll failed: {m}"),
            OffscreenError::Map(m) => write!(f, "offscreen buffer map failed: {m}"),
        }
    }
}

impl std::error::Error for OffscreenError {}

/// Round `value` up to the next multiple of `alignment` (`alignment` must be a
/// power of two and non-zero, which `COPY_BYTES_PER_ROW_ALIGNMENT` is).
fn align_up(value: u32, alignment: u32) -> u32 {
    let mask = alignment - 1;
    (value + mask) & !mask
}

/// Tightly-packed bytes for one row of `width` RGBA8 texels.
fn unpadded_bytes_per_row(width: u32) -> u32 {
    width * BYTES_PER_PIXEL
}

/// `unpadded_bytes_per_row` rounded up to `COPY_BYTES_PER_ROW_ALIGNMENT`, as
/// `copy_texture_to_buffer` requires.
fn padded_bytes_per_row(width: u32) -> u32 {
    align_up(
        unpadded_bytes_per_row(width),
        wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
    )
}

/// Strip the per-row alignment padding from a mapped readback buffer, returning
/// a tightly-packed `height * unpadded_bpr` byte vector. Pure (no GPU) so the
/// row-unpadding contract is unit-testable on its own.
fn unpad_rows(padded: &[u8], unpadded_bpr: u32, padded_bpr: u32, height: u32) -> Vec<u8> {
    let unpadded_bpr = unpadded_bpr as usize;
    let padded_bpr = padded_bpr as usize;
    let mut out = Vec::with_capacity(unpadded_bpr * height as usize);
    for row in 0..height as usize {
        let start = row * padded_bpr;
        out.extend_from_slice(&padded[start..start + unpadded_bpr]);
    }
    out
}

/// A surfaceless renderer that draws into an offscreen RGBA texture and reads
/// the result back to the CPU. See the module docs for the backend/adapter
/// contract.
pub struct OffscreenRenderer {
    adapter_info: wgpu::AdapterInfo,
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_bytes_per_row: u32,
    unpadded_bytes_per_row: u32,
}

impl OffscreenRenderer {
    /// Provision the harness on the CI backend ([`CI_BACKENDS`] = Vulkan) at
    /// the given size. Blocks on adapter/device creation (the harness is
    /// driven from synchronous `#[test]` bodies).
    pub fn new(width: u32, height: u32) -> Result<Self, OffscreenError> {
        Self::with_backends(CI_BACKENDS, width, height)
    }

    /// Provision the harness on an explicit backend set. Used by `new` and
    /// available to downstream GPU tests (#66/#67) that need a non-default
    /// backend locally.
    pub fn with_backends(
        backends: wgpu::Backends,
        width: u32,
        height: u32,
    ) -> Result<Self, OffscreenError> {
        pollster::block_on(Self::new_async(backends, width, height))
    }

    async fn new_async(
        backends: wgpu::Backends,
        width: u32,
        height: u32,
    ) -> Result<Self, OffscreenError> {
        let width = width.max(1);
        let height = height.max(1);

        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends;
        let instance = wgpu::Instance::new(descriptor);

        // Surfaceless: `compatible_surface: None`. Adapter unavailability is a
        // hard error, never a skip.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| OffscreenError::AdapterUnavailable(e.to_string()))?;

        let adapter_info = adapter.get_info();

        let (device, queue) = adapter
            .request_device(&super::device_descriptor())
            .await
            .map_err(|e| OffscreenError::DeviceUnavailable(e.to_string()))?;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mkui-wgpu Offscreen Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded = unpadded_bytes_per_row(width);
        let padded = padded_bytes_per_row(width);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mkui-wgpu Offscreen Readback"),
            size: (padded as u64) * (height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(Self {
            adapter_info,
            device,
            queue,
            width,
            height,
            texture,
            view,
            readback,
            padded_bytes_per_row: padded,
            unpadded_bytes_per_row: unpadded,
        })
    }

    /// Adapter metadata for the negotiated device. Tests assert
    /// `backend == Vulkan` + `device_type == Cpu` and log `name`.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// The wgpu device, for building test-only pipelines and shaders.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The wgpu queue, for submitting caller-built encoders.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The offscreen render-target view to attach as a render pass color
    /// target.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// `(width, height)` of the offscreen target, in texels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The render-target / readback texel format ([`TARGET_FORMAT`]).
    pub fn format(&self) -> wgpu::TextureFormat {
        TARGET_FORMAT
    }

    /// 256-byte-aligned bytes-per-row used by the readback copy.
    pub fn padded_bytes_per_row(&self) -> u32 {
        self.padded_bytes_per_row
    }

    /// Tightly-packed bytes-per-row of the returned readback (`width * 4`).
    pub fn unpadded_bytes_per_row(&self) -> u32 {
        self.unpadded_bytes_per_row
    }

    /// Record + submit a render pass that clears the target to `color` and
    /// stores it. A convenience for the clear/readback baseline test.
    pub fn clear(&self, color: wgpu::Color) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mkui-wgpu Offscreen Clear Encoder"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mkui-wgpu Offscreen Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(Some(encoder.finish()));
    }

    /// Copy the offscreen target into the padded readback buffer, submit, block
    /// on the device until the submission completes, map the buffer, and return
    /// a tightly-packed `height * width * 4` RGBA8 byte vector with the
    /// per-row alignment padding stripped.
    ///
    /// Mapping completion is proven by the explicit `device.poll(Wait)` — the
    /// `map_async` callback only fires once the device is polled.
    pub fn read_rgba(&self) -> Result<Vec<u8>, OffscreenError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mkui-wgpu Offscreen Readback Encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            // Ignore send errors: the receiver is dropped only if we already
            // returned early on a poll failure.
            let _ = tx.send(result);
        });

        // Explicit poll drives the map callback to completion. Without it the
        // callback never fires on the native backends.
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| OffscreenError::Poll(e.to_string()))?;

        rx.recv()
            .map_err(|e| OffscreenError::Map(format!("map callback channel dropped: {e}")))?
            .map_err(|e| OffscreenError::Map(e.to_string()))?;

        let data = slice.get_mapped_range();
        let pixels = unpad_rows(
            &data,
            self.unpadded_bytes_per_row,
            self.padded_bytes_per_row,
            self.height,
        );
        drop(data);
        self.readback.unmap();
        Ok(pixels)
    }
}

impl fmt::Debug for OffscreenRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OffscreenRenderer")
            .field("adapter", &self.adapter_info.name)
            .field("backend", &self.adapter_info.backend)
            .field("device_type", &self.adapter_info.device_type)
            .field("size", &(self.width, self.height))
            .field("padded_bytes_per_row", &self.padded_bytes_per_row)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Pure row-alignment math (no GPU required) ------------------------

    #[test]
    fn align_up_rounds_to_next_multiple() {
        assert_eq!(align_up(0, 256), 0);
        assert_eq!(align_up(1, 256), 256);
        assert_eq!(align_up(256, 256), 256);
        assert_eq!(align_up(257, 256), 512);
    }

    #[test]
    fn padded_row_is_256_aligned_and_at_least_unpadded() {
        // width 64 → 256 bytes, already aligned (the lucky case).
        assert_eq!(unpadded_bytes_per_row(64), 256);
        assert_eq!(padded_bytes_per_row(64), 256);
        // width 100 → 400 unpadded → padded up to 512.
        assert_eq!(unpadded_bytes_per_row(100), 400);
        assert_eq!(padded_bytes_per_row(100), 512);
        // width 1 → 4 unpadded → padded up to 256.
        assert_eq!(unpadded_bytes_per_row(1), 4);
        assert_eq!(padded_bytes_per_row(1), 256);
    }

    #[test]
    fn unpad_rows_strips_alignment_padding() {
        // 2 rows of 2 RGBA texels (8 unpadded bytes), padded to 16 bytes/row.
        let unpadded_bpr = 8;
        let padded_bpr = 16;
        let height = 2;
        let mut padded = Vec::new();
        // Row 0: 8 real bytes + 8 padding.
        padded.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        padded.extend_from_slice(&[0; 8]);
        // Row 1: 8 real bytes + 8 padding.
        padded.extend_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        padded.extend_from_slice(&[0; 8]);

        let out = unpad_rows(&padded, unpadded_bpr, padded_bpr, height);
        assert_eq!(
            out,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn unpad_rows_is_identity_when_already_aligned() {
        // unpadded == padded → output is the input verbatim.
        let row: Vec<u8> = (0..8).collect();
        let out = unpad_rows(&row, 8, 8, 1);
        assert_eq!(out, row);
    }

    // ---- GPU harness tests (require a Vulkan + CPU adapter / Lavapipe) -----

    /// Provision the harness, asserting the Vulkan + CPU adapter contract.
    /// Adapter/device unavailability fails the test — no silent skip (#106).
    fn harness(width: u32, height: u32) -> OffscreenRenderer {
        let renderer = OffscreenRenderer::new(width, height)
            .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
        let info = renderer.adapter_info();
        // Log the adapter name for diagnostics; do NOT gate on a name substring.
        eprintln!(
            "offscreen adapter: name={:?} backend={:?} device_type={:?}",
            info.name, info.backend, info.device_type
        );
        assert_eq!(
            info.backend,
            wgpu::Backend::Vulkan,
            "harness must select the Vulkan backend"
        );
        assert_eq!(
            info.device_type,
            wgpu::DeviceType::Cpu,
            "CI harness must run on a CPU (Lavapipe) adapter"
        );
        renderer
    }

    #[test]
    fn clear_to_known_color_reads_back_exactly() {
        let renderer = harness(64, 64);
        // 0.0/1.0 components map to exact 0/255 bytes under Rgba8Unorm.
        renderer.clear(wgpu::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        let pixels = renderer.read_rgba().expect("readback must succeed");
        let (w, h) = renderer.size();
        assert_eq!(pixels.len(), (w * h * BYTES_PER_PIXEL) as usize);
        for px in pixels.chunks_exact(4) {
            assert_eq!(
                px,
                [255, 0, 0, 255],
                "every texel must equal the clear color"
            );
        }
    }

    #[test]
    fn draw_solid_triangle_changes_pixels_vs_clear_baseline() {
        let renderer = harness(64, 64);

        // Baseline: clear to black and read back.
        renderer.clear(wgpu::Color::BLACK);
        let baseline = renderer.read_rgba().expect("baseline readback");

        // Tiny test-only shader: a fullscreen-ish solid green triangle.
        let shader = renderer
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("offscreen test triangle"),
                source: wgpu::ShaderSource::Wgsl(
                    r#"
                    @vertex
                    fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
                        var p = array<vec2<f32>, 3>(
                            vec2<f32>(-1.0, -1.0),
                            vec2<f32>( 3.0, -1.0),
                            vec2<f32>(-1.0,  3.0),
                        );
                        return vec4<f32>(p[vi], 0.0, 1.0);
                    }
                    @fragment
                    fn fs() -> @location(0) vec4<f32> {
                        return vec4<f32>(0.0, 1.0, 0.0, 1.0);
                    }
                    "#
                    .into(),
                ),
            });
        let layout = renderer
            .device()
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("offscreen test layout"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
        let pipeline = renderer
            .device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("offscreen test pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: renderer.format(),
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });

        let mut encoder =
            renderer
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("offscreen test draw"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("offscreen test pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: renderer.view(),
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
            pass.set_pipeline(&pipeline);
            pass.draw(0..3, 0..1);
        }
        renderer.queue().submit(Some(encoder.finish()));

        let drawn = renderer.read_rgba().expect("drawn readback");
        assert_eq!(drawn.len(), baseline.len());
        assert_ne!(drawn, baseline, "the triangle must change pixels");
        // The triangle covers the whole 64×64 target → all texels green.
        assert!(
            drawn.chunks_exact(4).all(|px| px == [0, 255, 0, 255]),
            "every covered texel must be the solid green draw color"
        );
    }

    #[test]
    fn readback_handles_non_256_aligned_row_width() {
        // width 100 → 400 unpadded bytes/row, not a multiple of 256 → padded
        // to 512. Proves row-unpadding through a real GPU readback.
        let renderer = harness(100, 7);
        assert_eq!(renderer.unpadded_bytes_per_row(), 400);
        assert_eq!(renderer.padded_bytes_per_row(), 512);

        renderer.clear(wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        });
        let pixels = renderer.read_rgba().expect("readback must succeed");
        let (w, h) = renderer.size();
        assert_eq!(
            pixels.len(),
            (w * h * BYTES_PER_PIXEL) as usize,
            "unpadded readback length must be width*height*4, no stride bytes"
        );
        for px in pixels.chunks_exact(4) {
            assert_eq!(px, [0, 0, 255, 255]);
        }
    }
}
