// Presentation blit that converts working-space pixels to the surface's colour
// space (`surface_color::OutputTransform`). The identity blit in `blit.rs`
// stays the path for plain sRGB surfaces.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use super::{FullscreenVertex, OffscreenRenderer, PreparedCost};
use crate::error::EngineResult;
use crate::surface_color::{OutputParams, OutputTransform};

/// One pipeline per swapchain format: the target format is baked into it, and
/// a recreated surface can land on another one.
pub(super) struct TransformPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,
    params: wgpu::Buffer,
}

impl OffscreenRenderer {
    fn transform_pipeline(&mut self, format: wgpu::TextureFormat) -> &TransformPipeline {
        let device = &self.device;
        self.transform_pipelines.entry(format).or_insert_with(|| {
            tracing::debug!(?format, "Creating colour-converting presentation pipeline");
            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Present Transform Bind Group Layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Present Transform Pipeline Layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Present Transform Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../shaders/effects/present_transform.wgsl").into(),
                ),
            });
            let vertex_layout = wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<FullscreenVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                        shader_location: 1,
                    },
                ],
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Present Transform Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(vertex_layout)],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // Replace, as the identity blit: every surface texel
                        // is the converted working texel.
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Present Transform Nearest Sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Present Transform Vertex Buffer"),
                contents: bytemuck::cast_slice(&FullscreenVertex::fullscreen_quad()),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Present Transform Params"),
                size: std::mem::size_of::<OutputParams>() as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            TransformPipeline {
                pipeline,
                bind_group_layout,
                sampler,
                vertex_buffer,
                params,
            }
        })
    }

    pub(super) fn encode_transform_blit(
        &mut self,
        working: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        surface_format: wgpu::TextureFormat,
        transform: OutputTransform,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<Arc<crate::device_domain::PreparedPermit>> {
        self.domain.poll()?;
        let permit = self.domain.reserve(PreparedCost {
            objects: 1,
            ..PreparedCost::default()
        })?;
        let queue = Arc::clone(self.domain.queue());
        let device = Arc::clone(&self.device);
        let blit = self.transform_pipeline(surface_format);
        // Written ahead of the frame's submission, and a frame has one blit.
        queue.write_buffer(
            &blit.params,
            0,
            bytemuck::bytes_of(&OutputParams::new(transform)),
        );
        let working_view = working.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Present Transform Bind Group"),
            layout: &blit.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&working_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&blit.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: blit.params.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Present Transform Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                depth_slice: None,
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
        pass.set_pipeline(&blit.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, blit.vertex_buffer.slice(..));
        pass.draw(0..6, 0..1);
        drop(pass);
        Ok(permit)
    }
}
