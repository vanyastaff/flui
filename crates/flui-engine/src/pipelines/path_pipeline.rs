//! Render pipeline for tessellated vector paths.
//!
//! Handles both fill and stroke rendering of lyon-tessellated geometry.
//! Non-instanced: uses PathVertex with per-vertex position and color.

/// Creates the path fill render pipeline.
///
/// Uses the dedicated fill shader with PathVertex layout (position + color).
pub fn create_path_fill_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("path_fill_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/fill.wgsl").into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("path_fill_pipeline_layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("path_fill_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                // Single vertex buffer: PathVertex (no instance buffer)
                crate::vertex::PathVertex::desc(),
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
        depth_stencil: Some(super::shape_pipeline::default_depth_stencil_state()),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Creates the path stroke render pipeline.
///
/// Uses the same fill shader as path fill (stroke tessellation is handled
/// on the CPU side by lyon).
pub fn create_path_stroke_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    // Stroke uses the same shader and vertex layout as fill;
    // the tessellation difference is handled on the CPU side.
    create_path_fill_pipeline(device, format, bind_group_layout)
}
