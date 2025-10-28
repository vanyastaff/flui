//! Render pipeline setup for wgpu backend

use super::vertex::{SolidVertex, ViewportUniforms};
use wgpu::util::DeviceExt;

/// Solid color render pipeline
///
/// Renders solid-colored geometry (rectangles, circles, lines).
pub struct SolidPipeline {
    /// Render pipeline
    pipeline: wgpu::RenderPipeline,

    /// Bind group layout
    #[allow(dead_code)]
    bind_group_layout: wgpu::BindGroupLayout,

    /// Uniform buffer for viewport
    uniform_buffer: wgpu::Buffer,

    /// Bind group
    bind_group: wgpu::BindGroup,
}

impl SolidPipeline {
    /// Create a new solid color pipeline
    ///
    /// # Arguments
    /// * `device` - WGPU device
    /// * `surface_format` - Surface texture format
    /// * `viewport_size` - Initial viewport size
    #[allow(dead_code)]
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        viewport_size: (f32, f32),
    ) -> Self {
        Self::new_with_msaa(device, surface_format, viewport_size, 1)
    }

    /// Create a new solid color pipeline with MSAA
    ///
    /// # Arguments
    /// * `device` - WGPU device
    /// * `surface_format` - Surface texture format
    /// * `viewport_size` - Initial viewport size
    /// * `sample_count` - MSAA sample count (1, 2, 4, or 8)
    pub fn new_with_msaa(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        viewport_size: (f32, f32),
        sample_count: u32,
    ) -> Self {
        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Solid Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/solid.wgsl").into()),
        });

        // Create uniform buffer
        let uniforms = ViewportUniforms::new(viewport_size.0, viewport_size.1);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Viewport Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Solid Pipeline Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Solid Pipeline Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Solid Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Solid Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[SolidVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // No culling for 2D
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None, // No depth testing for 2D
            multisample: wgpu::MultisampleState {
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            bind_group,
        }
    }

    /// Update viewport size
    ///
    /// # Arguments
    /// * `queue` - WGPU queue
    /// * `width` - New width in pixels
    /// * `height` - New height in pixels
    pub fn update_viewport(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        let uniforms = ViewportUniforms::new(width, height);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    /// Get the render pipeline
    pub fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    /// Get the bind group
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}
