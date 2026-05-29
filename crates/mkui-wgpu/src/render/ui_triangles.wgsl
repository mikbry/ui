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
