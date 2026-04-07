//! Render pipeline for textured image quads.
//!
//! Uses GPU instancing with ImageQuadInstance for efficient batched
//! texture rendering. Requires an additional bind group for texture + sampler.

/// Creates the textured image quad render pipeline.
///
/// Uses the texture_instanced shader with ImageQuadInstance layout.
/// Requires two bind groups:
///   group(0) = Viewport uniform
///   group(1) = sampler + texture_2d
pub fn create_image_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("image_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/texture_instanced.wgsl").into()),
    });

    // Create texture bind group layout for group(1)
    let texture_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image_texture_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("image_pipeline_layout"),
        bind_group_layouts: &[bind_group_layout, &texture_bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("image_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                super::shape_pipeline::unit_quad_vertex_layout(),
                crate::vertex::ImageQuadInstance::desc(),
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
