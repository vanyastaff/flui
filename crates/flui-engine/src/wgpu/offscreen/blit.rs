// Fullscreen intermediate→surface blit pipeline creation and blit_to_surface execution.
//
// Moved from `offscreen.rs` into `offscreen/blit.rs` as part of the C1 LOC-cap
// concern-separation split.  Zero behaviour changes.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use super::{BlitPipeline, FullscreenVertex, OffscreenRenderer};

impl OffscreenRenderer {
    /// Lazily create (and cache) the fullscreen blit pipeline.
    ///
    /// The pipeline samples the intermediate texture with nearest-neighbour
    /// filtering and overwrites the surface with no blend equation.  Calling
    /// this more than once is a no-op after the first build.
    fn get_or_create_blit_pipeline(
        &mut self,
        surface_format: wgpu::TextureFormat,
    ) -> &BlitPipeline {
        if self.blit_pipeline.is_none() {
            tracing::debug!("Creating intermediate-blit pipeline");

            let bind_group_layout =
                self.device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("Blit Bind Group Layout"),
                        entries: &[
                            // @group(0) @binding(0) — intermediate texture.
                            // `filterable: false` matches the NonFiltering sampler
                            // at binding 1; the blit uses Nearest and must not
                            // over-declare the sampling contract.
                            wgpu::BindGroupLayoutEntry {
                                binding: 0,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Texture {
                                    sample_type: wgpu::TextureSampleType::Float {
                                        filterable: false,
                                    },
                                    view_dimension: wgpu::TextureViewDimension::D2,
                                    multisampled: false,
                                },
                                count: None,
                            },
                            // @group(0) @binding(1) — nearest-neighbour sampler.
                            // NonFiltering matches the Nearest FilterMode used in
                            // `blit_to_surface` and mirrors `advanced_blend/pipeline.rs`.
                            wgpu::BindGroupLayoutEntry {
                                binding: 1,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Sampler(
                                    wgpu::SamplerBindingType::NonFiltering,
                                ),
                                count: None,
                            },
                        ],
                    });

            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("Blit Pipeline Layout"),
                        bind_group_layouts: &[Some(&bind_group_layout)],
                        immediate_size: 0,
                    });

            let shader_module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Blit Shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        include_str!("../shaders/effects/blit.wgsl").into(),
                    ),
                });

            let vertex_buffer_layout = wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<FullscreenVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    // @location(0) position: vec2<f32>
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    // @location(1) uv: vec2<f32>
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                        shader_location: 1,
                    },
                ],
            };

            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Intermediate Blit Pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader_module,
                        entry_point: Some("vs_main"),
                        buffers: &[vertex_buffer_layout],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader_module,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: surface_format,
                            // No blend: every surface texel is replaced by the
                            // intermediate.  This is intentional — any blend
                            // equation would composite over the cleared surface
                            // and would not be pixel-identical to a direct render.
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        polygon_mode: wgpu::PolygonMode::Fill,
                        unclipped_depth: false,
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                });

            // Cache the sampler and vertex buffer: both are frame-invariant
            // (Nearest parameters + static fullscreen-quad geometry) so
            // allocating them once here avoids per-frame GPU allocations on
            // COPY_SRC-less adapters.
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Blit Nearest Sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                // Nearest: 1:1 blit must not interpolate — same texel every pixel.
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });

            let vertices = FullscreenVertex::fullscreen_quad();
            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Blit Vertex Buffer"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

            self.blit_pipeline = Some(BlitPipeline {
                pipeline: Arc::new(pipeline),
                bind_group_layout: Arc::new(bind_group_layout),
                sampler: Arc::new(sampler),
                vertex_buffer: Arc::new(vertex_buffer),
            });
        }
        self.blit_pipeline
            .as_ref()
            .expect("blit_pipeline was set above")
    }

    /// Blit the intermediate texture 1:1 onto the swapchain surface view.
    ///
    /// Used by the COPY_SRC-less present path: after all frame passes have
    /// rendered into `intermediate_texture`, this method copies it onto the
    /// real surface without any blend equation (Replace/Copy semantics).
    ///
    /// # Arguments
    ///
    /// * `intermediate_texture` — the pooled offscreen texture holding the
    ///   fully-rendered frame.
    /// * `surface_view` — the swapchain view to write into.
    /// * `surface_format` — the swapchain surface format.
    pub fn blit_to_surface(
        &mut self,
        intermediate_texture: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        surface_format: wgpu::TextureFormat,
    ) {
        // Clone all Arc handles out so the `&mut self` borrow ends before we
        // use `self.device`/`self.queue` below (wgpu handle types are not
        // `Clone`; Arc makes this borrow-check-safe without unsafe).
        // The sampler and vertex buffer are frame-invariant and were cached in
        // `get_or_create_blit_pipeline` — no per-frame GPU allocation needed.
        let blit = self.get_or_create_blit_pipeline(surface_format);
        let pipeline = Arc::clone(&blit.pipeline);
        let bind_group_layout = Arc::clone(&blit.bind_group_layout);
        let sampler = Arc::clone(&blit.sampler);
        let vertex_buffer = Arc::clone(&blit.vertex_buffer);

        // Only the bind group is per-frame: it references the intermediate view,
        // which is different for every frame (the pooled texture changes).
        let intermediate_view =
            intermediate_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blit Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&intermediate_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let mut blit_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Intermediate Blit Encoder"),
                });
        {
            let mut blit_pass = blit_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Intermediate Blit Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // Clear the swapchain surface before writing.  Without
                        // this, pixels outside the intermediate's viewport
                        // (e.g., after a resize race) would show stale content.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            blit_pass.set_pipeline(&pipeline);
            blit_pass.set_bind_group(0, &bind_group, &[]);
            blit_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            blit_pass.draw(0..6, 0..1);
        }
        self.queue.submit(std::iter::once(blit_encoder.finish()));

        tracing::trace!("Intermediate blit submitted to swapchain");
    }
}
