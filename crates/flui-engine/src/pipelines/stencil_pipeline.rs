//! Render pipelines for stencil-based non-rectangular clipping.
//!
//! Two pipelines are provided:
//! - **Increment:** writes to the stencil buffer (increment on pass) when
//!   entering a clip region.
//! - **Decrement:** writes to the stencil buffer (decrement on pass) when
//!   leaving a clip region.
//!
//! Both use the `PathVertex` layout (position + color) but produce no visible
//! color output (write mask is empty).

/// Creates the stencil-write pipeline that **increments** the stencil value.
///
/// Used when pushing a non-rectangular clip region.
pub fn create_stencil_increment_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    create_stencil_pipeline(device, format, bind_group_layout, StencilDirection::Increment)
}

/// Creates the stencil-write pipeline that **decrements** the stencil value.
///
/// Used when popping a non-rectangular clip region.
pub fn create_stencil_decrement_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    create_stencil_pipeline(device, format, bind_group_layout, StencilDirection::Decrement)
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum StencilDirection {
    Increment,
    Decrement,
}

fn create_stencil_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
    direction: StencilDirection,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stencil_write_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/stencil_write.wgsl").into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("stencil_write_pipeline_layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    let pass_op = match direction {
        StencilDirection::Increment => wgpu::StencilOperation::IncrementClamp,
        StencilDirection::Decrement => wgpu::StencilOperation::DecrementClamp,
    };

    let label = match direction {
        StencilDirection::Increment => "stencil_write_increment_pipeline",
        StencilDirection::Decrement => "stencil_write_decrement_pipeline",
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                // Uses PathVertex layout (position + color)
                crate::vertex::PathVertex::desc(),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            // Color output with empty write mask -- we only write to stencil.
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: super::shape_pipeline::default_primitive_state(),
        depth_stencil: Some({
            let face = wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Always,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op,
            };
            wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState {
                    front: face,
                    back: face,
                    read_mask: 0xFF,
                    write_mask: 0xFF,
                },
                bias: wgpu::DepthBiasState::default(),
            }
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}
