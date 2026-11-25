//! MSAA re-enable regression smoke test (#156, closes #95, supersedes #147).
//!
//! #147's original design resolved 4x MSAA directly into an sRGB swapchain
//! texture (matching-format resolve, decode/average/re-encode once). #135/#155
//! replaced that architecture: every lane now composites into a **linear**
//! `Rgba8Unorm` intermediate framebuffer, and a separate present pass
//! (`render/present.wgsl`) performs the sole sRGB encode at the surface
//! boundary. #156 re-enables MSAA *inside that linear intermediate*, so this
//! test is reshaped from #147's (which is why it lives at the same path but
//! asserts a different contract):
//!
//! 1. **Resolve is a plain linear average, not an sRGB round-trip.** The MSAA
//!    color texture and its resolve target now share the *linear*
//!    `Rgba8Unorm` format, so there is no decode/re-encode in the resolve at
//!    all — clearing an already-linear color through 4x MSAA + resolve must
//!    reproduce the single-sample reference byte-for-byte (within rounding).
//!    MSAA changes edge coverage, never color.
//! 2. **The #149/#155 linear-intermediate invariant survives MSAA.** An
//!    authored sRGB `0.5` gray (linearized the same way `gui_vertices` does)
//!    must still read back as byte ~55 in the resolved intermediate — MSAA
//!    must not reintroduce a double-encode.
//! 3. **The present pass completes the round trip.** Sampling the resolved
//!    linear intermediate through the real `present.wgsl` (included verbatim
//!    so the shader under test cannot drift from production) with
//!    `encode_srgb = 1` (simulating a plain-UNORM surface) must re-encode
//!    that ~55 byte back to ~128 — i.e. authored sRGB 0.5 survives the full
//!    MSAA-resolve-then-present round trip unchanged.
//!
//! Gated on `gpu-tests`: like #106's other GPU acceptance tests this needs a
//! real Vulkan/Lavapipe adapter and hard-fails (no silent skip) if none is
//! present. It runs on the dedicated `gpu-offscreen` CI job
//! (`cargo test -p mkui-wgpu --features gpu-tests`); the default displayless
//! matrix only compiles it (`--all-features --no-run`).

#![cfg(feature = "gpu-tests")]

use mkui_wgpu::{Color, OffscreenRenderer};

/// Square target so a 64-wide `Rgba8` row is exactly 256 bytes — already at
/// wgpu's `COPY_BYTES_PER_ROW_ALIGNMENT`, so readback needs no row-padding
/// bookkeeping.
const W: u32 = 64;
const H: u32 = 64;

/// Sample count under test — the #156 default and the wgpu-portable ceiling.
const MSAA: u32 = 4;

/// The linear intermediate's format (mirrors `render::INTERMEDIATE_FORMAT`,
/// which is not exported — the value is pinned by `linear_blend_gpu.rs` and
/// `OffscreenRenderer::format()` on the same contract).
const LINEAR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[test]
fn msaa_resolve_preserves_linear_intermediate_and_present_round_trip() {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "msaa-no-regression offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "MSAA GPU test must select the Vulkan backend (#106 contract)"
    );
    assert_eq!(renderer.format(), LINEAR_FORMAT);

    let device = renderer.device();
    let queue = renderer.queue();

    // Authored sRGB 0.5 gray, linearized exactly as `gui_vertices` linearizes
    // triangle colors at the render boundary (ADR 0006 §"Color space +
    // blending"). This is the "already on the GPU's doorstep" value a real
    // draw would clear/blend with; the test clears with it directly (like
    // #147's `clear_readback`) rather than driving a shader, since MSAA
    // resolve behavior does not depend on how the sample was produced.
    let [r, g, b, a] = Color::from_srgb(0.5, 0.5, 0.5).to_linear_rgba();
    let linear_clear = wgpu::Color {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a: a as f64,
    };

    let (single_texture, single_bytes) = clear_and_resolve(device, queue, 1, linear_clear);
    let (msaa_texture, msaa_bytes) = clear_and_resolve(device, queue, MSAA, linear_clear);

    // Invariant 2: the #149/#155 linear-intermediate contract (authored sRGB
    // 0.5 -> linear ~0.214 -> byte ~55) survives the MSAA resolve.
    for &c in &msaa_bytes[..3] {
        assert!(
            (50..=60).contains(&c),
            "sRGB 0.5 must resolve to ~55 through 4x MSAA, got {c} (128 would mean \
             sRGB-space compositing; a value far from 55 would mean the resolve \
             double-encoded or corrupted the linear average)"
        );
    }
    assert_eq!(msaa_bytes[3], 255, "opaque clear stays opaque");

    // Invariant 1: MSAA changes coverage, not color. A uniform clear has no
    // partial-coverage edges to resolve, so the 4x-resolved and single-sample
    // paths must land on the same bytes (±1 for backend rounding).
    for (chan, (m, s)) in ["R", "G", "B", "A"]
        .iter()
        .zip(msaa_bytes.iter().zip(&single_bytes))
    {
        let (m, s) = (*m as i16, *s as i16);
        assert!(
            (m - s).abs() <= 1,
            "channel {chan}: 4x-MSAA-resolved={m} vs single-sample={s} — the linear \
             resolve is not a lossless average (MSAA must not change color)"
        );
    }

    // Invariant 3: the present pass completes the round trip. Sample the
    // resolved 4x-MSAA intermediate through the real `present.wgsl` with
    // `encode_srgb = 1` (a plain-UNORM "surface") and confirm the authored
    // sRGB 0.5 comes back out at ~byte 128 (sRGB 0.5 round-trips to itself).
    let surface_bytes = run_present_pass(device, queue, &msaa_texture, true);
    for &c in &surface_bytes[..3] {
        assert!(
            (123..=132).contains(&c),
            "present pass must re-encode the resolved linear ~55 back to sRGB ~128 \
             (authored 0.5 round-trips to itself), got {c}"
        );
    }
    assert_eq!(surface_bytes[3], 255);

    drop(single_texture);
}

/// Clear `color` into a fresh single-sample [`LINEAR_FORMAT`] target at
/// `sample_count` (`1` = direct, `>1` = render into an MSAA texture and
/// resolve), read back the center texel, and return the resolved/target
/// texture (kept alive so callers can chain it into a further pass) alongside
/// the readback bytes.
fn clear_and_resolve(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sample_count: u32,
    color: wgpu::Color,
) -> (wgpu::Texture, [u8; 4]) {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa-no-regression resolve/target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LINEAR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // The multisampled color texture, only when testing the MSAA path. Its
    // format matches `target` (both LINEAR_FORMAT) — the #156 contract: the
    // resolve is a linear->linear average, never a format-crossing sRGB
    // decode/encode.
    let msaa_texture = (sample_count > 1).then(|| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa-no-regression multisampled color"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: LINEAR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    });
    let msaa_view = msaa_texture
        .as_ref()
        .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        // MSAA path: render into `msaa_view`, resolve into `target_view`.
        // 1x path: render straight into `target_view`, no resolve.
        let (view, resolve_target) = match &msaa_view {
            Some(msaa) => (msaa, Some(&target_view)),
            None => (&target_view, None),
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msaa-no-regression clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
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
    queue.submit(Some(encoder.finish()));

    let bytes = read_center_texel(device, queue, &target);
    (target, bytes)
}

/// Run the real `present.wgsl` (included verbatim from the production source
/// so this test cannot drift from what actually ships) against `source`,
/// writing to a fresh [`wgpu::TextureFormat::Rgba8Unorm`] "surface" target and
/// reading back its center texel. `encode_srgb` mirrors `PresentFlags` — `true`
/// simulates a plain-UNORM surface that needs the shader-side OETF.
fn run_present_pass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Texture,
    encode_srgb: bool,
) -> [u8; 4] {
    let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());

    let surface_target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa-no-regression present surface"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let surface_view = surface_target.create_view(&wgpu::TextureViewDescriptor::default());

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("msaa-no-regression present shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../src/render/present.wgsl").into()),
    });
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("msaa-no-regression present bind group layout"),
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
        label: Some("msaa-no-regression present pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("msaa-no-regression present pipeline"),
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
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("msaa-no-regression present sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    // `PresentFlags`: `encode_srgb: u32` + three scalar `u32` pad fields — a
    // flat 16 bytes (see present.wgsl's own doc comment on why NOT `vec3<u32>`).
    use wgpu::util::DeviceExt;
    let flags_data: [u32; 4] = [u32::from(encode_srgb), 0, 0, 0];
    let flags_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("msaa-no-regression present flags"),
        contents: bytemuck::cast_slice(&flags_data),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("msaa-no-regression present bind group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: flags_buffer.as_entire_binding(),
            },
        ],
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msaa-no-regression present pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &surface_view,
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
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit(Some(encoder.finish()));

    read_center_texel(device, queue, &surface_target)
}

/// Copy `texture`'s center texel back to the CPU. `W`/`H` = 64 keeps
/// `bytes_per_row` (256) already `COPY_BYTES_PER_ROW_ALIGNMENT`-aligned, so no
/// row-unpadding bookkeeping is needed.
fn read_center_texel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> [u8; 4] {
    let bytes_per_row = W * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("msaa-no-regression readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll must drive the map callback to completion");
    rx.recv()
        .expect("map callback channel must not drop")
        .expect("readback buffer must map");

    let data = slice
        .get_mapped_range()
        .expect("mapped readback buffer must yield a view");
    let row = (H / 2) as usize;
    let col = (W / 2) as usize;
    let offset = row * bytes_per_row as usize + col * 4;
    let texel = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];
    drop(data);
    readback.unmap();
    texel
}
