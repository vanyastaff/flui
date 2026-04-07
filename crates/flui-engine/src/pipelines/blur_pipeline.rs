//! Render pipeline for Gaussian blur effects and final compositing.
//!
//! Blur pipelines are **placeholders** using the rect shader until dedicated
//! blur shaders are wired. The compositing pipeline uses a real shader for
//! offscreen render-target blending (used by SaveLayer/RestoreLayer in v2).

/// Creates the blur downsample render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated blur shader is wired.
pub fn create_blur_downsample_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(
        device,
        format,
        bind_group_layout,
        "blur_downsample",
    )
}

/// Creates the blur upsample render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated blur shader is wired.
pub fn create_blur_upsample_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(
        device,
        format,
        bind_group_layout,
        "blur_upsample",
    )
}

/// Creates the bind group layout for compositing operations (group 1).
///
/// Contains:
/// - binding 0: CompositeUniforms (uniform buffer)
/// - binding 1: source texture (texture_2d)
/// - binding 2: source sampler
pub fn create_compositing_bind_group_layout(
    device: &wgpu::Device,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("compositing_bind_group_layout"),
        entries: &[
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
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Creates the final compositing render pipeline.
///
/// Uses the real compositing shader that samples an offscreen texture and
/// blends it back with configurable opacity. This pipeline is prepared for
/// v2 SaveLayer offscreen compositing; in v1 it is registered but not
/// invoked at draw time.
pub fn create_compositing_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("compositing_shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/effects/compositing.wgsl").into(),
        ),
    });

    let compositing_bgl = create_compositing_bind_group_layout(device);

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("compositing_pipeline_layout"),
        bind_group_layouts: &[bind_group_layout, &compositing_bgl],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("compositing_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 8, // 2 x f32 for quad_pos
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                }],
            }],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}
