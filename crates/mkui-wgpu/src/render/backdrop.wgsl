// Real backdrop blur (#187): two passes that sample whatever has already
// been painted into the target up to this point in scene order — never a
// static texture.
//
// Pass 1 (`fs_blur_h`) reads the live target and writes a horizontally
// blurred copy into a same-size scratch texture, via the oversized
// full-screen triangle trick `present.wgsl` already uses.
//
// Pass 2 (`fs_composite`) reads that scratch texture with *vertical* taps
// (completing the separable Gaussian) and mixes `tint` over the result,
// but only across the rounded-rect polygon `vs_composite`'s vertex buffer
// describes (built on the CPU from the same corner-arc boundary
// `tessellation::rounded_rect_boundary` uses for `Quad`) — pixels outside
// that polygon are never touched by this draw, so the panel's rounded
// corners come from real geometry, not a shader-side distance mask, exactly
// like every other rounded `Quad` in this renderer.
//
// Both passes write with blending disabled: the fragment itself computes
// the final color from the blurred sample (and, in pass 2, the tint mix),
// so this draw must *replace* the destination pixel, not blend over the
// sharp backdrop still sitting there from before this primitive ran.

const TAPS: i32 = 4; // 2*TAPS+1 = 9 taps per axis.

struct BlurParams {
    texel: vec2<f32>,
    sigma: f32,
    step: f32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> blur: BlurParams;

struct FullscreenVsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Oversized full-screen triangle, identical to `present.wgsl`'s `vs_present`.
@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> FullscreenVsOut {
    var clip = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = clip[vi];
    var out: FullscreenVsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

fn gaussian_tap(fi: f32, sigma: f32) -> f32 {
    let s = max(sigma, 0.0001);
    return exp(-0.5 * (fi * fi) / (s * s));
}

@fragment
fn fs_blur_h(in: FullscreenVsOut) -> @location(0) vec4<f32> {
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var i = -TAPS; i <= TAPS; i = i + 1) {
        let fi = f32(i);
        let w = gaussian_tap(fi, blur.sigma);
        let offset = vec2<f32>(blur.texel.x * fi * blur.step, 0.0);
        sum += textureSample(src_tex, src_sampler, in.uv + offset) * w;
        wsum += w;
    }
    return sum / max(wsum, 0.0001);
}

struct CompositeVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct CompositeVsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_composite(input: CompositeVertexInput) -> CompositeVsOut {
    var out: CompositeVsOut;
    out.pos = vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    return out;
}

struct CompositeParams {
    texel: vec2<f32>,
    sigma: f32,
    step: f32,
    tint: vec4<f32>,
};

@group(0) @binding(0) var blur_h_tex: texture_2d<f32>;
@group(0) @binding(1) var blur_h_sampler: sampler;
@group(0) @binding(2) var<uniform> composite: CompositeParams;

@fragment
fn fs_composite(in: CompositeVsOut) -> @location(0) vec4<f32> {
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var i = -TAPS; i <= TAPS; i = i + 1) {
        let fi = f32(i);
        let w = gaussian_tap(fi, composite.sigma);
        let offset = vec2<f32>(0.0, composite.texel.y * fi * composite.step);
        sum += textureSample(blur_h_tex, blur_h_sampler, in.uv + offset) * w;
        wsum += w;
    }
    let blurred = sum / max(wsum, 0.0001);
    let tint = composite.tint;
    let rgb = mix(blurred.rgb, tint.rgb, tint.a);
    return vec4<f32>(rgb, 1.0);
}
