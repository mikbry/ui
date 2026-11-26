// Textured-quad pipeline for `Primitive::Image` (#196). Position is already
// in NDC (mapped on the CPU side by `render::image::image_vertices`, the
// same convention `ui_triangles.wgsl`'s `gui_vertices` uses). A single quad
// covers the node's whole laid-out box; `uv` already encodes the
// contain/cover fit decision (see `components::image::fit_uv`) — a sampled
// `uv` outside `[0, 1]` is this shader's own letterbox: it renders
// transparent rather than clamping to the texture's edge pixel, so
// `ImageFit::Contain`'s bars need no second primitive.
//
// The rounded-corner clip is one continuous per-fragment signed-distance
// evaluation against the box's own `local_pos`/`rect_size`/`corner_radius`
// — unlike a tessellated rounded rect (triangle fans per corner) there is
// no seam between "corner geometry" and "edge geometry" to begin with,
// because there is no separate corner geometry at all.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) local_pos: vec2<f32>,
    @location(3) rect_size: vec2<f32>,
    @location(4) corner_radius: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local_pos: vec2<f32>,
    @location(2) rect_size: vec2<f32>,
    @location(3) corner_radius: f32,
};

@group(0) @binding(0) var image_texture: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@vertex
fn vs_image(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    output.local_pos = input.local_pos;
    output.rect_size = input.rect_size;
    output.corner_radius = input.corner_radius;
    return output;
}

// Signed distance from `p` (relative to the box's center) to a rounded
// rect of half-size `half_size` and corner radius `radius`. Negative
// inside, positive outside, zero on the boundary — the standard rounded-box
// SDF. Collapses to a plain axis-aligned box SDF when `radius` is 0.
//
// `radius` is clamped to `half_size`'s shorter axis — the SDF formula below
// is only valid up to that bound; past it the `vec2<f32>(radius, radius)`
// offset overshoots the box and produces a distorted, partially transparent
// shape instead of a clamped rounded square (#331). `image_vertices` already
// clamps this on the CPU side (mirroring `rounded_rect_path`'s own
// convention); this is defense-in-depth so the shader is correct on its own
// terms too.
fn rounded_box_sdf(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = clamp(radius, 0.0, min(half_size.x, half_size.y));
    let d = abs(p) - half_size + vec2<f32>(r, r);
    return length(max(d, vec2<f32>(0.0, 0.0))) - r + min(max(d.x, d.y), 0.0);
}

@fragment
fn fs_image(input: VertexOutput) -> @location(0) vec4<f32> {
    // `textureSample`'s implicit-derivative LOD selection needs uniform
    // control flow across the quad (a per-fragment `uv`-dependent early
    // `return` before it would not be), so the letterbox/out-of-bounds
    // check below is a post-multiply on `color.a` instead of a branch
    // around the sample.
    let color = textureSample(image_texture, image_sampler, input.uv);

    // `ImageFit::Contain`'s letterbox bars: a uv outside [0, 1] on either
    // axis renders transparent — not the texture's edge pixel, which an
    // address-mode clamp would otherwise bleed in.
    let in_bounds =
        input.uv.x >= 0.0 && input.uv.x <= 1.0 && input.uv.y >= 0.0 && input.uv.y <= 1.0;
    let bounds_mask = select(0.0, 1.0, in_bounds);

    let centered = input.local_pos - input.rect_size * 0.5;
    let half_size = input.rect_size * 0.5;
    let sdf = rounded_box_sdf(centered, half_size, input.corner_radius);
    // ~1px analytic-AA feather, same shape every other coverage-based mask
    // in this crate uses (no `fwidth` dependency, so this behaves the same
    // under MSAA and on the offscreen/Lavapipe harness).
    let coverage = clamp(0.5 - sdf, 0.0, 1.0);

    return vec4<f32>(color.rgb, color.a * coverage * bounds_mask);
}
