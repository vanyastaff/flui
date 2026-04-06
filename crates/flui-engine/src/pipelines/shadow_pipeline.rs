//! Render pipeline for box shadow rendering.
//!
//! Uses an analytical Gaussian approximation shader with ShadowInstance
//! layout: bounds, color, offset+blur+spread packed into 3 vec4s.

/// Creates the box shadow render pipeline.
///
/// Uses the dedicated shadow shader with ShadowInstance layout.
pub fn create_shadow_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shadow_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/effects/shadow.wgsl").into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shadow_pipeline_layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                super::shape_pipeline::unit_quad_vertex_layout(),
                shadow_instance_desc(),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(super::shape_pipeline::alpha_blend_target(format))],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: super::shape_pipeline::default_primitive_state(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Vertex buffer layout for `ShadowInstance` (from `batchers::effects`).
///
/// Layout: bounds(vec4 @2), color(vec4 @3), offset+blur+spread(vec4 @4)
/// Total: 48 bytes = 3 x vec4<f32>
fn shadow_instance_desc() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
        2 => Float32x4, // bounds [x, y, w, h]
        3 => Float32x4, // color [r, g, b, a]
        4 => Float32x4, // [offset_x, offset_y, blur_radius, spread]
    ];

    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<crate::batchers::effects::ShadowInstance>()
            as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: ATTRIBUTES,
    }
}
