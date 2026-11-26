//! Offscreen GPU acceptance tests for `VectorPath` fill-rule and local
//! transform semantics (#304). Same dual gate as [`super::slug_gpu`]
//! (`gpu-tests` + `slug`, #106's Vulkan/Lavapipe harness).
//!
//! `mkui-vector2d`'s `encode_vector_path` carries [`VectorPath::fill`] and
//! applies [`VectorPath::transform`], but the actual non-zero-vs-even-odd
//! rasterization decision happens in `slug.wgsl`'s `calc_coverage` — nothing
//! in `mkui-vector2d` itself ever rasterizes a pixel (see that crate's own
//! `encode::tests::nested_same_winding_contours_still_encode_as_two_full_contours`,
//! which only proves the geometry and `fill_rule` tag survive encoding).
//! These tests are the actual pixel-level regression the issue asks for:
//!
//! - two same-winding nested squares produce a hole under even-odd and stay
//!   solid under non-zero;
//! - a path-local transform moves the rendered ink to the transformed
//!   position, not the authored one.

use mkui_vector2d::Affine2;
use mkui_vector2d_wgpu::{
    Bounds, FillRule, PathCommand, PlacedVectorPath, SlugAdapter, Vec2, VectorPath,
};

use super::offscreen::OffscreenRenderer;
use super::INTERMEDIATE_FORMAT;

const W: u32 = 64;
const H: u32 = 64;

const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const GREEN_BYTES: [u8; 4] = [0, 255, 0, 255];
const BLACK_BYTES: [u8; 4] = [0, 0, 0, 255];

fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "vector-path-fill-rule offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "vector-path-fill-rule GPU test must select the Vulkan backend (#106 contract)"
    );
    assert_eq!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "vector-path-fill-rule GPU test must run on a CPU (Lavapipe) adapter (#106 contract)"
    );
    assert_eq!(renderer.format(), INTERMEDIATE_FORMAT);
    renderer
}

fn square(min: f32, max: f32) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo(Vec2::new(min, min)),
        PathCommand::LineTo(Vec2::new(max, min)),
        PathCommand::LineTo(Vec2::new(max, max)),
        PathCommand::LineTo(Vec2::new(min, max)),
        PathCommand::Close,
    ]
}

/// Two same-winding (both traced bottom→right→top→left) nested squares: an
/// outer ring 0..100 and an inner one 25..75, sharing one `fill`.
fn nested_squares(fill: FillRule) -> VectorPath {
    let mut commands = square(0.0, 100.0);
    commands.extend(square(25.0, 75.0));
    VectorPath::new(commands, fill, Bounds::new(Vec2::ZERO, Vec2::ZERO))
}

/// Places the 0..100 unit square frame at screen `[7, 57] x [7, 57]` in the
/// 64x64 target — the same scale/origin `render::slug_gpu`'s hand-authored
/// fixture uses, so the mapping from unit coordinates to screen pixels below
/// is easy to cross-check against that module.
fn placed(path: VectorPath) -> PlacedVectorPath {
    PlacedVectorPath {
        path,
        origin_px: [7.0, 57.0],
        scale_px_per_unit: 0.5,
        color: GREEN,
    }
}

fn render(fills: &[PlacedVectorPath]) -> Vec<u8> {
    let renderer = harness();
    let adapter = SlugAdapter::new(renderer.device(), renderer.format(), 1);
    let prepared = adapter.prepare_paths(
        renderer.device(),
        renderer.queue(),
        [W as f32, H as f32],
        1.0,
        fills,
        &[],
    );
    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vector-path-fill-rule test encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vector-path-fill-rule test pass"),
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
        if let Some(prepared) = prepared.as_ref() {
            adapter.draw(&mut pass, prepared);
        }
    }
    renderer.queue().submit(Some(encoder.finish()));
    renderer.read_rgba().expect("readback must succeed")
}

fn texel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn even_odd_nested_same_winding_squares_punch_a_hole() {
    let pixels = render(&[placed(nested_squares(FillRule::EvenOdd))]);

    // Annulus between the two squares: inked under either rule.
    assert_eq!(
        texel(&pixels, 32, 12),
        GREEN_BYTES,
        "the annulus between the two same-winding squares must be inked"
    );
    // Dead center, inside the inner square: a hole under even-odd.
    assert_eq!(
        texel(&pixels, 32, 32),
        BLACK_BYTES,
        "even-odd must punch a hole where two same-winding contours overlap"
    );
    // Well outside the outer square: untouched background.
    assert_eq!(
        texel(&pixels, 2, 2),
        BLACK_BYTES,
        "outside the outer square must stay the clear color"
    );
}

#[test]
fn non_zero_nested_same_winding_squares_stay_solid() {
    let pixels = render(&[placed(nested_squares(FillRule::NonZero))]);

    assert_eq!(
        texel(&pixels, 32, 12),
        GREEN_BYTES,
        "the annulus must be inked"
    );
    // Same center sample that is a hole under even-odd: non-zero winding
    // from the two same-winding contours (magnitude 2) still reads as filled.
    assert_eq!(
        texel(&pixels, 32, 32),
        GREEN_BYTES,
        "non-zero must stay filled where two same-winding contours overlap, unlike even-odd"
    );
    assert_eq!(texel(&pixels, 2, 2), BLACK_BYTES);
}

#[test]
fn local_transform_translates_the_rendered_fill() {
    // A single 0..100 unit square placed exactly as the baseline above, but
    // this time the translation is carried by `VectorPath::transform` itself
    // (not `origin_px`) — proving `encode_vector_path` actually applies the
    // path-local transform rather than silently ignoring it. +20 local units
    // at this placement's 0.5 scale is +10 screen px.
    let commands = square(0.0, 100.0);
    let mut path = VectorPath::new(
        commands,
        FillRule::NonZero,
        Bounds::new(Vec2::ZERO, Vec2::ZERO),
    );
    path.transform = Affine2 {
        tx: 20.0,
        ty: 0.0,
        ..Affine2::IDENTITY
    };
    let pixels = render(&[placed(path)]);

    // (10, 32) sits inside the untransformed square's screen box
    // ([7,57]x[7,57]) but is now past the translated square's left edge
    // (screen x >= 7 + 20*0.5 = 17), so it must be background.
    assert_eq!(
        texel(&pixels, 10, 32),
        BLACK_BYTES,
        "the untransformed position must no longer be inked"
    );
    // (40, 32) is inside the translated square's screen box ([17,67]x[7,57],
    // clipped to the 64-wide target) and outside the untransformed one.
    assert_eq!(
        texel(&pixels, 40, 32),
        GREEN_BYTES,
        "the transformed position must be inked"
    );
}
