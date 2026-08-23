//! Pipeline creation for advanced effects (gradients, shadows)
//!
//! This module provides helper functions to create GPU pipelines for:
//! - Linear / radial / sweep gradients (plus their shared bind-group layout,
//!   stops buffer, and pipeline layout)
//! - Analytical shadows
//!
//! All four pipelines are specs over
//! [`super::pipelines::create_unit_quad_pipeline`], the shared unit-quad
//! instanced constructor.

use super::effects::GradientStop;
use super::pipelines::{QuadPipelineSpec, create_unit_quad_pipeline};

/// Create bind group layout for gradient stops (storage buffer)
pub fn create_gradient_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Gradient Stops Bind Group Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// Maximum number of gradient stop slots in the GPU buffer.
///
/// Each gradient consumes up to 8 slots; this cap allows 100 gradients per
/// frame. The three `*_gradient_rect` methods in `painter::gradient` enforce this
/// limit by dropping instances that would overflow it rather than writing
/// past the end of the buffer.
pub const MAX_GRADIENT_STOPS: usize = 8 * 100;

/// Create gradient stops buffer with initial capacity
pub fn create_gradient_stops_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    // Max 8 stops per gradient, support up to 100 gradients per frame
    let capacity = MAX_GRADIENT_STOPS;
    let size = (capacity * std::mem::size_of::<GradientStop>()) as u64;

    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Gradient Stops Buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Create a shared pipeline layout for all gradient pipelines.
/// Using the same PipelineLayout object ensures bind group compatibility
/// when switching pipelines within a render pass (WebGPU requirement).
pub fn create_gradient_pipeline_layout(
    device: &wgpu::Device,
    viewport_bind_group_layout: &wgpu::BindGroupLayout,
    gradient_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Shared Gradient Pipeline Layout"),
        bind_group_layouts: &[
            Some(viewport_bind_group_layout),
            Some(gradient_bind_group_layout),
        ],
        immediate_size: 0,
    })
}

/// Create linear gradient rendering pipeline
pub fn create_linear_gradient_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    pipeline_layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    create_unit_quad_pipeline(
        device,
        surface_format,
        pipeline_layout,
        &QuadPipelineSpec {
            shader_label: "Linear Gradient Shader",
            pipeline_label: "Linear Gradient Pipeline",
            shader_source: concat!(
                include_str!("shaders/common/clip.wgsl"),
                include_str!("shaders/gradients/linear.wgsl")
            ),
            instance_layout: super::instancing::LinearGradientInstance::desc(),
            blend: wgpu::BlendState::ALPHA_BLENDING,
        },
    )
}

/// Create radial gradient rendering pipeline
pub fn create_radial_gradient_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    pipeline_layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    create_unit_quad_pipeline(
        device,
        surface_format,
        pipeline_layout,
        &QuadPipelineSpec {
            shader_label: "Radial Gradient Shader",
            pipeline_label: "Radial Gradient Pipeline",
            shader_source: concat!(
                include_str!("shaders/common/clip.wgsl"),
                include_str!("shaders/gradients/radial.wgsl")
            ),
            instance_layout: super::instancing::RadialGradientInstance::desc(),
            blend: wgpu::BlendState::ALPHA_BLENDING,
        },
    )
}

/// Create sweep (angular/conic) gradient rendering pipeline
pub fn create_sweep_gradient_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    pipeline_layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    create_unit_quad_pipeline(
        device,
        surface_format,
        pipeline_layout,
        &QuadPipelineSpec {
            shader_label: "Sweep Gradient Shader",
            pipeline_label: "Sweep Gradient Pipeline",
            shader_source: concat!(
                include_str!("shaders/common/clip.wgsl"),
                include_str!("shaders/gradients/sweep.wgsl")
            ),
            instance_layout: super::instancing::SweepGradientInstance::desc(),
            blend: wgpu::BlendState::ALPHA_BLENDING,
        },
    )
}

/// Create shadow rendering pipeline
pub fn create_shadow_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    viewport_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Shadow Pipeline Layout"),
        bind_group_layouts: &[Some(viewport_bind_group_layout)],
        immediate_size: 0,
    });

    create_unit_quad_pipeline(
        device,
        surface_format,
        &pipeline_layout,
        &QuadPipelineSpec {
            shader_label: "Shadow Shader",
            pipeline_label: "Shadow Pipeline",
            shader_source: include_str!("shaders/effects/shadow.wgsl"),
            instance_layout: super::instancing::ShadowInstance::desc(),
            blend: wgpu::BlendState::ALPHA_BLENDING,
        },
    )
}
