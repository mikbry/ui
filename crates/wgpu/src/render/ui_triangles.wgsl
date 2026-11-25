// 2D UI pipeline. Position is already in NDC (mapped on the CPU side from
// pixel-space scene coordinates by `gui_vertices`); the vertex shader passes
// it through unmodified. The fragment stage writes the per-vertex linear
// RGBA the host uploaded — no lighting, no AO, no tone-mapping. This is the
// load-bearing 2D slice ported from the upstream reference's overlay entry
// points, stripped of the unused normal / overlay / material varyings that
// only a lit 3D scene pass cares about.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_ui_triangles(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(input.position, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_ui_triangles(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}

// linear -> sRGB OETF, matching `present.wgsl`'s `linear_to_srgb` and
// `types::linear_to_srgb` component-wise.
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c <= vec3<f32>(0.0031308);
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, cutoff);
}

// Host-embedded direct-draw entry point (#175): `input.color` is already
// linear (the same `gui_vertices` output `fs_ui_triangles` passes through
// unchanged), but this draw writes straight into the host's target with no
// present pass to apply the OETF afterward. Used only when the host's target
// format has no automatic sRGB encode on write (a plain UNORM format) — see
// `render::embedded::EmbeddedRenderer`. Note alpha blending then composites in
// gamma space rather than `fs_ui_triangles`'s physically-linear blend, a
// known Phase 1 simplification for that one target-format case (most hosts
// present through an sRGB-format view, where `fs_ui_triangles` is used
// instead and blending stays linear).
@fragment
fn fs_ui_triangles_encode_srgb(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(linear_to_srgb(input.color.rgb), input.color.a);
}
