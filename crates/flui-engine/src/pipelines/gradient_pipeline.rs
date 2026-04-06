//! Render pipelines for linear and radial gradients.
//!
//! Gradient pipelines use two bind groups:
//! - Group 0: viewport uniform (shared with all pipelines)
//! - Group 1: gradient uniforms + stops storage buffer (per-gradient)
//!
//! Each gradient is drawn individually (one draw call per gradient) because
//! gradient stops are variable-length and stored in a storage buffer.

use super::shape_pipeline::{alpha_blend_target, default_primitive_state, unit_quad_vertex_layout};

/// Creates the bind group layout for gradient-specific data (group 1).
///
/// Contains:
/// - Binding 0: Gradient uniform buffer (bounds, start/end or center/radius, corner radii, stop count)
/// - Binding 1: Gradient stops storage buffer (dynamic array of color stops)
pub fn create_gradient_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("gradient_bind_group_layout"),
        entries: &[
            // Gradient uniforms (bounds, geometry params, stop count)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // Gradient stops (storage buffer for variable count)
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

/// Creates the linear gradient render pipeline.
pub fn create_linear_gradient_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    viewport_bind_group_layout: &wgpu::BindGroupLayout,
    gradient_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("linear_gradient_shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/gradients/linear.wgsl").into(),
        ),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("linear_gradient_pipeline_layout"),
        bind_group_layouts: &[viewport_bind_group_layout, gradient_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("linear_gradient_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[unit_quad_vertex_layout()],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(alpha_blend_target(format))],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: default_primitive_state(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Creates the radial gradient render pipeline.
pub fn create_radial_gradient_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    viewport_bind_group_layout: &wgpu::BindGroupLayout,
    gradient_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("radial_gradient_shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/gradients/radial.wgsl").into(),
        ),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("radial_gradient_pipeline_layout"),
        bind_group_layouts: &[viewport_bind_group_layout, gradient_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("radial_gradient_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[unit_quad_vertex_layout()],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(alpha_blend_target(format))],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: default_primitive_state(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}
