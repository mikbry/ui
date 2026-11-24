// Present pass: linear intermediate framebuffer -> swapchain surface.
//
// The UI, bitmap-text, and Slug lanes all composite into a linear-space
// `Rgba8Unorm` intermediate target (ADR 0006 §"Color space + blending"), so
// alpha blending happens in physically-correct linear light. This full-screen
// pass reads that linear result and writes it to the surface, applying the
// linear -> sRGB OETF only when the surface is a plain UNORM format with no
// hardware sRGB encode. When the surface IS sRGB (the preferred, common case)
// the fragment returns the linear value unchanged and the swapchain's own
// view performs the single encode — never a double encode (which was the
// Sprint 6 v0.9.1 macOS-Metal symptom this architecture closes by construction).

struct PresentFlags {
    // 1 = apply the sRGB OETF here (surface is UNORM); 0 = pass linear through
    // and let an sRGB surface encode. Three scalar `u32` pad fields (NOT
    // `vec3<u32>`) keep the struct a flat 16 bytes: WGSL host-shareable layout
    // aligns `vec3<u32>` to 16 bytes, which would push this struct to 32 bytes
    // total (offset 16 + size 16, rounded to the struct's own 16-byte align) —
    // exactly the bind-group-0/binding-2 "size 16 where the shader expects 32"
    // mismatch that panicked on macOS Metal (#135 reintroduce, revert `f8da740`).
    // Scalar `u32` fields stay 4-byte aligned, so four of them pack to 16 bytes
    // with no hidden padding.
    encode_srgb: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> flags: PresentFlags;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_present(@builtin(vertex_index) vi: u32) -> VsOut {
    // Oversized full-screen triangle covering the whole viewport in one draw.
    var clip = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = clip[vi];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    // NDC (y-up) -> texture uv (y-down): u = x*0.5+0.5, v = 0.5 - y*0.5.
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

// linear -> sRGB OETF, matching `types::linear_to_srgb` component-wise.
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c <= vec3<f32>(0.0031308);
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, cutoff);
}

@fragment
fn fs_present(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_sampler, in.uv);
    if (flags.encode_srgb == 1u) {
        return vec4<f32>(linear_to_srgb(c.rgb), c.a);
    }
    return c;
}
