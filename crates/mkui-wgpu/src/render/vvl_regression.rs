//! Validation-enforcement regression guard (#153).
//!
//! PR #149 shipped with 29 green CI checks and immediately panicked on macOS
//! Metal at first UI draw (reverted at `f8da740`). The `gpu-offscreen`
//! Lavapipe job had no Vulkan Validation Layers (VVL) installed, so the
//! Lavapipe adapter negotiated the bad bind-group/layout state without
//! complaint. This test builds a deliberate uniform-buffer size mismatch
//! (a 16-byte buffer bound where the shader declares a 32-byte struct) and
//! asserts `wgpu` reports a validation error instead of drawing garbage.
//!
//! # What this test does and does not prove
//!
//! Bind-group / buffer-size compatibility against shader reflection is
//! **`wgpu-core`'s own WebGPU-spec validation** — CPU-side, backend-agnostic,
//! and always on regardless of whether Vulkan Validation Layers are
//! installed. This test would already catch the mismatch it constructs even
//! on a `gpu-offscreen` job with VVL completely stripped, so it is **not** a
//! sentinel for "VVL silently went missing" — that regression is caught by
//! the log-grep step in `.github/workflows/ci.yml` (`gpu-offscreen` job),
//! which fails the CI job outright if `VK_LAYER_KHRONOS_validation` does not
//! appear active in the Vulkan loader trace.
//!
//! What this test *does* prove: the offscreen harness's device correctly
//! surfaces `wgpu-core` validation errors through an error scope instead of
//! panicking past them or drawing silently-wrong output — i.e. that the CI
//! GPU lane would catch a #149-class bind-group mismatch at all, through
//! whichever validation layer (wgpu-core or VVL) trips first. See #153 for
//! the fuller writeup of the two-layer story.
//!
//! Anchors: issue #153, revert commit `f8da740`, #149 the bug that motivated
//! this gap-fix.

use super::offscreen::OffscreenRenderer;

const W: u32 = 16;
const H: u32 = 16;

/// Provision the #106 harness and assert the Vulkan + CPU adapter contract.
/// Adapter/device unavailability fails the test (no silent skip).
fn harness() -> OffscreenRenderer {
    let renderer = OffscreenRenderer::new(W, H)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let info = renderer.adapter_info();
    eprintln!(
        "vvl_regression offscreen adapter: name={:?} backend={:?} device_type={:?}",
        info.name, info.backend, info.device_type
    );
    assert_eq!(
        info.backend,
        wgpu::Backend::Vulkan,
        "VVL regression test must select the Vulkan backend (#153)"
    );
    assert_eq!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "VVL regression test must run on a CPU (Lavapipe) adapter (#153)"
    );
    renderer
}

/// A shader declaring a 32-byte uniform struct (two `vec4<f32>`s) at
/// `@group(0) @binding(0)`. Both fields are read so naga can't dead-code the
/// binding away.
const SHADER_SRC: &str = r#"
struct Uniforms {
    a: vec4<f32>,
    b: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(p[vi], 0.0, 1.0) + u.a * 0.0;
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    return u.b;
}
"#;

#[test]
fn undersized_uniform_binding_is_rejected_by_validation() {
    let renderer = harness();
    let device = renderer.device();

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("vvl_regression shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
    });

    // The shader's `Uniforms` struct is 32 bytes. `min_binding_size: None`
    // defers the size check to draw time against the shader-reflected
    // minimum, matching the #149-class failure mode: nothing rejects the
    // mismatch at bind-group creation, only at the moment it would actually
    // be read by a draw.
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("vvl_regression bind group layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    // Deliberately undersized: 16 bytes bound where the shader requires 32.
    let undersized_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vvl_regression undersized uniform buffer"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: false,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vvl_regression bind group"),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: undersized_buffer.as_entire_binding(),
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("vvl_regression pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("vvl_regression pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: renderer.format(),
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    // Push a validation error scope before the draw so the mismatch is
    // captured here instead of hitting the device's default uncaptured-error
    // handler (which panics).
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("vvl_regression encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vvl_regression pass"),
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
        pass.draw(0..3, 0..1);
    }
    renderer.queue().submit(Some(encoder.finish()));

    let error = pollster::block_on(error_scope.pop());
    let error = error.expect(
        "binding a 16-byte buffer where the shader declares a 32-byte struct \
         must raise a wgpu validation error — if this starts passing without \
         an error, validation enforcement in the gpu-offscreen lane has \
         regressed (#153)",
    );
    eprintln!("vvl_regression captured (expected) error: {error}");
    match &error {
        wgpu::Error::Validation { description, .. } => {
            let lower = description.to_lowercase();
            assert!(
                lower.contains("size") || lower.contains("binding"),
                "expected the validation error to mention size/binding, got: {description}"
            );
        }
        other => panic!("expected wgpu::Error::Validation, got: {other:?}"),
    }
}
