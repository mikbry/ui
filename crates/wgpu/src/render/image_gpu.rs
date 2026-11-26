//! Cross-consumer GPU acceptance test for `Image` (#196): renders the real
//! textured-quad pipeline (`build_image_pipeline` + `ImageTextures` +
//! `image_vertex_buffer`, exactly what `Renderer::render`'s `RenderCommand::
//! Image` arm drives) on #106's Vulkan/Lavapipe harness and reads back
//! pixels. Gated on `gpu-tests` only — unlike [`super::slug_gpu`] and its
//! siblings, the `Image` pipeline has no `slug`-feature dependency at all.
//!
//! Covers the three visual acceptance criteria #196 itself lists:
//! - both source paths (a host-registered texture, and the decoded-bytes
//!   path through [`ImageTextures::resolve`]) render the same pixels for an
//!   equivalent image;
//! - `Cover` crops the overflowing axis and fills the box edge-to-edge;
//!   `Contain` letterboxes the non-matching axis transparent;
//! - the rounded-corner clip excludes a true corner pixel while showing a
//!   continuous anti-aliased boundary around the curve — a seam (a crack or
//!   hard discontinuity where separate corner/edge geometry would meet)
//!   would show up as a gap in that same boundary scan, and there is no
//!   such seam to find because there is no separate corner geometry at all
//!   (one shader, one SDF, evaluated per-fragment over the whole quad).

use super::image::{build_image_pipeline, image_vertex_buffer, ImageTextures};
use super::offscreen::OffscreenRenderer;
use crate::components::image as place_image;
use crate::theme::ImageStyle;
use crate::types::{ImageSource as WgpuImageSource, Point, Rect, Scene, Size};
use mkui_runtime::ImageFit;

const W: u32 = 64;
const H: u32 = 64;

fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "image GPU test offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_eq!(info.device_type, wgpu::DeviceType::Cpu);
    renderer
}

/// Build an `Rgba8UnormSrgb` texture view filled with a single solid color
/// (straight, unpremultiplied `[r, g, b, a]`, 0-255 per channel). Pure 0/255
/// channel values round-trip exactly through the sRGB transfer function (it
/// fixes both endpoints), so a test can assert exact readback bytes without
/// reasoning about the curve in between.
fn solid_color_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    color: [u8; 4],
) -> wgpu::TextureView {
    let pixels: Vec<u8> = color
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("image GPU test solid color texture"),
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
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Encode a tiny solid-color PNG — the decoded-bytes source path's input.
/// Uses this crate's existing `png` dev-dependency (golden-comparison tests
/// already pull it in; #196 does not add a new one here).
fn encode_solid_png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let pixels: Vec<u8> = color
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header must write");
        writer
            .write_image_data(&pixels)
            .expect("png frame must write");
    }
    bytes
}

/// Render one `PlacedImage` (via the real [`place_image`] fit/UV math) into
/// a fresh offscreen target, through the real pipeline, and read back RGBA.
fn render_one(
    renderer: &OffscreenRenderer,
    rect: Rect,
    natural: Size,
    fit: ImageFit,
    corner_radius: f32,
    source: WgpuImageSource,
    resolve: impl FnOnce(
        &mut ImageTextures,
        &wgpu::Device,
        &wgpu::Queue,
        &WgpuImageSource,
    ) -> wgpu::BindGroup,
) -> Vec<u8> {
    let mut scene = Scene::new(Size::new(W as f32, H as f32));
    place_image(
        &mut scene,
        rect,
        natural,
        fit,
        source,
        ImageStyle { corner_radius },
    );
    let crate::types::Primitive::Image(placed) = &scene.primitives[0] else {
        panic!("place_image must push exactly one Primitive::Image");
    };

    let mut textures = ImageTextures::new(renderer.device());
    let bind_group = resolve(
        &mut textures,
        renderer.device(),
        renderer.queue(),
        &placed.source,
    );
    let pipeline = build_image_pipeline(
        renderer.device(),
        renderer.format(),
        1,
        textures.bind_group_layout(),
    );
    let buffer = image_vertex_buffer(renderer.device(), placed, W as f32, H as f32);

    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("image GPU test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("image GPU test pass"),
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
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, 0..1);
    }
    renderer.queue().submit(Some(encoder.finish()));
    renderer.read_rgba().expect("readback must succeed")
}

fn pixel(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * W + x) * 4) as usize;
    [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]]
}

const RED: [u8; 4] = [255, 0, 0, 255];

#[test]
fn host_texture_source_renders_the_registered_texture() {
    let renderer = harness();
    let view = solid_color_view(renderer.device(), renderer.queue(), 8, 8, RED);
    let buf = render_one(
        &renderer,
        Rect::new(Point::new(0.0, 0.0), Size::new(W as f32, H as f32)),
        Size::new(8.0, 8.0),
        ImageFit::Cover,
        0.0,
        WgpuImageSource::Host(0), // overwritten by `resolve` below's registration
        |textures, device, _queue, _source| {
            let handle = textures.register_host_texture(device, &view);
            // The handle `register_host_texture` returns is what a real
            // `ImageSource::Host` would carry; resolve through the same
            // public path `Renderer::render` uses.
            let source = WgpuImageSource::Host(handle);
            textures
                .resolve(device, renderer.queue(), &source)
                .expect("a freshly registered host texture must resolve")
                .clone()
        },
    );
    assert_eq!(
        pixel(&buf, W / 2, H / 2),
        RED,
        "the box's center must show the host-registered texture's color"
    );
}

#[test]
fn encoded_png_source_renders_the_same_color_as_the_equivalent_host_texture() {
    let renderer = harness();
    let bytes = encode_solid_png(8, 8, RED);
    let buf = render_one(
        &renderer,
        Rect::new(Point::new(0.0, 0.0), Size::new(W as f32, H as f32)),
        Size::new(8.0, 8.0),
        ImageFit::Cover,
        0.0,
        WgpuImageSource::Encoded {
            encoding: crate::types::ImageEncoding::Png,
            bytes: std::sync::Arc::from(bytes.as_slice()),
            cache_key: 1,
        },
        |textures, device, queue, source| {
            textures
                .resolve(device, queue, source)
                .expect("a valid encoded PNG source must decode and resolve")
                .clone()
        },
    );
    assert_eq!(
        pixel(&buf, W / 2, H / 2),
        RED,
        "the decoded-bytes source path must render the same color the \
         host-texture path does for an equivalent image (#196 acceptance: \
         both source paths render correctly)"
    );
}

#[test]
fn cover_fills_the_box_edge_to_edge_cropping_the_overflowing_axis() {
    // A 128x64 (2:1) image covering a 64x64 (1:1) box: Cover crops the
    // overflowing width, so every point in the box — including right up to
    // its edges — must show the image's color, not background.
    let renderer = harness();
    let view = solid_color_view(renderer.device(), renderer.queue(), 128, 64, RED);
    let buf = render_one(
        &renderer,
        Rect::new(Point::new(0.0, 0.0), Size::new(W as f32, H as f32)),
        Size::new(128.0, 64.0),
        ImageFit::Cover,
        0.0,
        WgpuImageSource::Host(0),
        |textures, device, queue, _source| {
            let handle = textures.register_host_texture(device, &view);
            textures
                .resolve(device, queue, &WgpuImageSource::Host(handle))
                .unwrap()
                .clone()
        },
    );
    for (x, y) in [
        (1, 1),
        (W - 2, 1),
        (1, H - 2),
        (W - 2, H - 2),
        (W / 2, H / 2),
    ] {
        assert_eq!(
            pixel(&buf, x, y),
            RED,
            "Cover must fill the box edge-to-edge at ({x},{y})"
        );
    }
}

#[test]
fn contain_letterboxes_the_non_matching_axis_transparent() {
    // Same 128x64 image, but Contain: the box's full width is used (scale
    // 1.0 on the limiting axis) and the height letterboxes — centered
    // 64x32 image with 16px transparent bars above and below.
    let renderer = harness();
    let view = solid_color_view(renderer.device(), renderer.queue(), 128, 64, RED);
    let buf = render_one(
        &renderer,
        Rect::new(Point::new(0.0, 0.0), Size::new(W as f32, H as f32)),
        Size::new(128.0, 64.0),
        ImageFit::Contain,
        0.0,
        WgpuImageSource::Host(0),
        |textures, device, queue, _source| {
            let handle = textures.register_host_texture(device, &view);
            textures
                .resolve(device, queue, &WgpuImageSource::Host(handle))
                .unwrap()
                .clone()
        },
    );
    // Center: inside the drawn image.
    assert_eq!(pixel(&buf, W / 2, H / 2), RED);
    // Within the top/bottom letterbox bars (y=4, well inside the 16px
    // bar): background (the clear color), not the image.
    assert_ne!(pixel(&buf, W / 2, 4)[0..3], RED[0..3]);
    assert_ne!(pixel(&buf, W / 2, H - 5)[0..3], RED[0..3]);
}

#[test]
fn rounded_clip_excludes_the_true_corner_pixel() {
    let renderer = harness();
    let view = solid_color_view(renderer.device(), renderer.queue(), 64, 64, RED);
    let radius = 12.0;
    let buf = render_one(
        &renderer,
        Rect::new(Point::new(0.0, 0.0), Size::new(W as f32, H as f32)),
        Size::new(64.0, 64.0),
        ImageFit::Cover,
        radius,
        WgpuImageSource::Host(0),
        |textures, device, queue, _source| {
            let handle = textures.register_host_texture(device, &view);
            textures
                .resolve(device, queue, &WgpuImageSource::Host(handle))
                .unwrap()
                .clone()
        },
    );
    // Center: solidly inside the rounded rect.
    assert_eq!(pixel(&buf, W / 2, H / 2), RED);
    // The box's own true corner pixel sits well outside the radius-12 arc —
    // excluded.
    assert_ne!(
        pixel(&buf, 0, 0)[0..3],
        RED[0..3],
        "a pixel at the box's true corner must be clipped by the rounded radius"
    );
    assert_ne!(pixel(&buf, W - 1, H - 1)[0..3], RED[0..3]);
}

#[test]
fn rounded_clip_boundary_has_no_seam_around_the_curve() {
    // "No visible seams" (#196 acceptance): scan the whole target for
    // partial-coverage (anti-aliased) texels. A seam — a crack where
    // separate corner-arc and straight-edge geometry would meet, the
    // failure mode a tessellated rounded rect is exposed to — would show
    // up as a gap (zero partial-coverage texels) somewhere along the
    // curve's quarter-circle arc. This shader has no separate corner
    // geometry to seam against (one SDF, evaluated per-fragment across the
    // whole quad), so the arc's AA ring is expected to be continuous.
    let renderer = harness();
    let view = solid_color_view(renderer.device(), renderer.queue(), 64, 64, RED);
    let radius = 16.0;
    let buf = render_one(
        &renderer,
        Rect::new(Point::new(0.0, 0.0), Size::new(W as f32, H as f32)),
        Size::new(64.0, 64.0),
        ImageFit::Cover,
        radius,
        WgpuImageSource::Host(0),
        |textures, device, queue, _source| {
            let handle = textures.register_host_texture(device, &view);
            textures
                .resolve(device, queue, &WgpuImageSource::Host(handle))
                .unwrap()
                .clone()
        },
    );

    // The corner-circle's SDF center sits at local `(radius, radius)` (see
    // `rounded_box_sdf`'s docs): walk its quarter-circle arc in 5-degree
    // steps *at* the nominal radius — where `sdf == 0` and the shader's
    // `clamp(0.5 - sdf, 0, 1)` AA feather sits right at ~0.5 coverage — and
    // assert every sampled texel along it shows partial (neither fully
    // background nor fully opaque-red) coverage: a continuous ring, no
    // gap.
    let mut saw_partial = 0;
    let mut gap_found = false;
    for step in 0..=18 {
        let angle = std::f32::consts::PI + (step as f32 / 18.0) * (std::f32::consts::PI / 2.0);
        let x = (radius + radius * angle.cos()).round() as u32;
        let y = (radius + radius * angle.sin()).round() as u32;
        let px = pixel(&buf, x.min(W - 1), y.min(H - 1));
        let is_full = px[0] > 250;
        let is_background = px[0] < 5;
        if !is_full && !is_background {
            saw_partial += 1;
        } else if is_background {
            // A background sample right on the nominal radius is the
            // "gap" failure mode — the AA fringe should sit here, not
            // vanish outright.
            gap_found = true;
        }
    }
    assert!(
        saw_partial >= 4,
        "expected several partial-coverage AA texels around the curved \
         corner, got {saw_partial}"
    );
    assert!(
        !gap_found,
        "found a background gap along the rounded corner's arc — a seam"
    );
}
