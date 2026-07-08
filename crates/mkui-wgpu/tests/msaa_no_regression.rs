//! MSAA re-enable + sRGB-resolve regression smoke test (#95, closes #93's
//! Sprint 6 v0.9.1 MSAA deferral).
//!
//! Sprint 6 v0.9.1 pinned the UI pass to `sample_count = 1` on the theory that
//! the MSAA-resolve-into-sRGB step double-applied sRGB encoding on macOS Metal.
//! #95 re-enables 4× MSAA with the canonical matching-sRGB resolve: the
//! intermediate multisampled color texture is created in the **same** (sRGB)
//! format as its resolve target — wgpu requires the two to match
//! (`wgpu-core` `MismatchedResolveTextureFormat`) — the fragment/ROP stage
//! encodes linear→sRGB once per sample on store, and the resolve is
//! spec-defined to decode → average → re-encode, i.e. a **single** logical
//! sRGB encode.
//!
//! This test proves that on a real GPU (Vulkan/Lavapipe on CI, #106's
//! surfaceless harness) by writing a solid color two ways into an sRGB target —
//! once through a 4× MSAA texture + resolve, once single-sample directly — and
//! asserting the read-back bytes are identical. A resolve that double-encoded
//! (or skipped the sRGB decode) would shift the MSAA path away from the
//! single-sample reference; byte-equality is the "no double-encoding" proof.
//!
//! Gated on `gpu-tests`: it needs the real Lavapipe adapter and, like the other
//! GPU acceptance tests, hard-fails (no silent skip) if none is present. It runs
//! on the dedicated CI job (`cargo test -p mkui-wgpu --features gpu-tests`);
//! the default displayless matrix only compiles it (`--all-features --no-run`).

#![cfg(feature = "gpu-tests")]

use mkui_wgpu::OffscreenRenderer;

/// Square target so a 64-wide `Rgba8` row is exactly 256 bytes — already at
/// wgpu's `COPY_BYTES_PER_ROW_ALIGNMENT`, so the readback needs no row padding.
const W: u32 = 64;
const H: u32 = 64;

/// sRGB render/resolve target format. sRGB is the whole point: the resolve must
/// do the sRGB round-trip correctly. `Rgba8UnormSrgb` gives predictable R,G,B,A
/// channel order in the readback.
const SRGB_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Sample count under test — the #95 default and the wgpu-portable ceiling.
const MSAA: u32 = 4;

/// A mid-gray **linear** clear. `LoadOp::Clear` values are linear; the sRGB
/// attachment encodes them on store. 0.5 linear ≈ 188/255 sRGB — safely away
/// from both 0/255 (which can't reveal an encode bug) and 128 (the value a
/// missing sRGB encode would leave), so the readback is diagnostic.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.5,
    g: 0.25,
    b: 0.75,
    a: 1.0,
};

#[test]
fn msaa_srgb_resolve_matches_single_sample_no_double_encoding() {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "msaa offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "MSAA GPU test must select the Vulkan backend (#106 contract)"
    );

    let device = renderer.device();
    let queue = renderer.queue();

    // Reference: clear straight into a single-sample sRGB target, no resolve.
    let single = clear_readback(device, queue, 1, CLEAR);
    // Under test: clear into a 4× MSAA sRGB texture, resolve into a single-
    // sample sRGB target, read that back.
    let resolved = clear_readback(device, queue, MSAA, CLEAR);

    // Sanity: the sRGB encode actually happened (a raw/linear write of 0.5 would
    // land at ~128; a correct sRGB encode lands at ~188).
    assert!(
        (180..=196).contains(&single[0]),
        "single-sample red channel {} not in the sRGB-encoded 0.5 range — sRGB \
         encode did not happen as expected",
        single[0]
    );

    // The core assertion: 4× MSAA + resolve must reproduce the single-sample
    // value exactly (±1 for backend rounding). Any double sRGB encode in the
    // resolve step would push `resolved` away from `single`.
    for (chan, (r, s)) in ["R", "G", "B", "A"]
        .iter()
        .zip(resolved.iter().zip(&single))
    {
        let (r, s) = (*r as i16, *s as i16);
        assert!(
            (r - s).abs() <= 1,
            "channel {chan}: MSAA-resolved={r} vs single-sample={s} — resolve is \
             not a single, lossless sRGB round-trip (double-encoding regression)"
        );
    }
}

/// Clear a solid `color` into a fresh single-sample sRGB target at `sample_count`
/// (`1` = direct, `>1` = render into an MSAA texture and resolve), copy it back
/// to the CPU, and return the center texel's RGBA bytes.
fn clear_readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sample_count: u32,
    color: wgpu::Color,
) -> [u8; 4] {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa-test resolve/target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SRGB_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // The multisampled color texture, only when testing the MSAA path. Its
    // format matches `target` so the resolve is legal under wgpu's format-match
    // rule; that is exactly the #95 contract under test.
    let msaa_texture = (sample_count > 1).then(|| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa-test multisampled color"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: SRGB_FORMAT,
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
        // 1× path: render straight into `target_view`, no resolve.
        let (view, resolve_target) = match &msaa_view {
            Some(msaa) => (msaa, Some(&target_view)),
            None => (&target_view, None),
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msaa-test clear pass"),
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

    // 64 * 4 = 256 bytes/row = already `COPY_BYTES_PER_ROW_ALIGNMENT`-aligned.
    let bytes_per_row = W * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("msaa-test readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
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

    let data = slice.get_mapped_range();
    // Center texel: row H/2, column W/2.
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
