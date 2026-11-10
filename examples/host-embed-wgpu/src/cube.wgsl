// The host's own lit 3D pass (#175 demo). Per-face Lambertian shading is
// baked into each vertex's color on the CPU (see `build_cube_vertices` in
// main.rs) rather than computed here — the shader only projects and shades
// through; this keeps the pass's GPU side identical in shape to mkui's own
// UI pipeline (`ui_triangles.wgsl`), which also receives pre-shaded
// per-vertex color.

struct Uniforms {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_cube(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = uniforms.mvp * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_cube(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}
