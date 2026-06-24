// Dual Kawase blur pipeline creation and render_blur execution.
//
// Moved from `offscreen.rs` into `offscreen/blur.rs` as part of the C1 LOC-cap
// concern-separation split.  Zero behaviour changes.

use std::sync::Arc;

use super::super::shader_compiler::ShaderType;
use super::super::texture_pool::PooledTexture;
use super::{BlurParams, BlurPipelines, FullscreenVertex, OffscreenRenderer};

impl OffscreenRenderer {
    /// Get or create the blur render pipelines (downsample + upsample)
    ///
    /// Lazily creates both pipelines on first call, then caches them.
    fn get_or_create_blur_pipelines(&mut self) -> &BlurPipelines {
        if self.blur_pipelines.is_none() {
            tracing::debug!("Creating Dual Kawase blur pipelines");

            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("Blur Pipeline Layout"),
                        bind_group_layouts: &[Some(&self.blur_bind_group_layout)],
                        immediate_size: 0,
                    });

            let vertex_buffer_layout = wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<FullscreenVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    // position: vec2<f32>
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    // uv: vec2<f32>
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                        shader_location: 1,
                    },
                ],
            };

            // Downsample pipeline
            let downsample_shader = self
                .shader_cache
                .get_or_compile_module(ShaderType::DualKawaseDownsample, &self.device);
            let downsample_module = downsample_shader
                .module
                .as_ref()
                .expect("DualKawaseDownsample shader module must be compiled before use");

            let downsample_pipeline =
                self.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("Dual Kawase Downsample Pipeline"),
                        layout: Some(&pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: downsample_module,
                            entry_point: Some("vs_main"),
                            buffers: std::slice::from_ref(&vertex_buffer_layout),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: downsample_module,
                            entry_point: Some("fs_main"),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: self.surface_format,
                                blend: Some(wgpu::BlendState::REPLACE),
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
                        multisample: wgpu::MultisampleState {
                            count: 1,
                            mask: !0,
                            alpha_to_coverage_enabled: false,
                        },
                        multiview_mask: None,
                        cache: None,
                    });

            // Upsample pipeline
            let upsample_shader = self
                .shader_cache
                .get_or_compile_module(ShaderType::DualKawaseUpsample, &self.device);
            let upsample_module = upsample_shader
                .module
                .as_ref()
                .expect("DualKawaseUpsample shader module must be compiled before use");

            let upsample_pipeline =
                self.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("Dual Kawase Upsample Pipeline"),
                        layout: Some(&pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: upsample_module,
                            entry_point: Some("vs_main"),
                            buffers: &[vertex_buffer_layout],
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: upsample_module,
                            entry_point: Some("fs_main"),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: self.surface_format,
                                blend: Some(wgpu::BlendState::REPLACE),
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
                        multisample: wgpu::MultisampleState {
                            count: 1,
                            mask: !0,
                            alpha_to_coverage_enabled: false,
                        },
                        multiview_mask: None,
                        cache: None,
                    });

            self.blur_pipelines = Some(BlurPipelines {
                downsample: Arc::new(downsample_pipeline),
                upsample: Arc::new(upsample_pipeline),
            });
        }

        self.blur_pipelines
            .as_ref()
            .expect("blur_pipelines was set above")
    }

    /// Apply Dual Kawase blur to an input texture
    ///
    /// Uses a downsample/upsample mip chain for fast, high-quality blur.
    /// The number of iterations is derived from `sigma` (clamped to 1..5).
    ///
    /// # Arguments
    ///
    /// * `input` - Source texture to blur
    /// * `sigma` - Blur strength (higher = more blur)
    ///
    /// # Returns
    ///
    /// A new `PooledTexture` containing the blurred result at the original resolution.
    pub fn render_blur(&mut self, input: &PooledTexture, sigma: f32) -> PooledTexture {
        // sigma ≤ 0 means "no blur" — copy the input through without running
        // any Kawase passes. Without this guard sigma=0 would produce one
        // downsample+upsample pass (iterations=1 from the clamp below), visibly
        // blurring content that should be unchanged (e.g. backdrop_filter with
        // sigma 0, identity image filters).
        if sigma <= 0.0 {
            tracing::trace!(
                sigma,
                width = input.width(),
                height = input.height(),
                "render_blur: sigma ≤ 0, copying input through without Kawase passes"
            );
            let out = self
                .texture_pool
                .acquire(input.width(), input.height(), self.surface_format);
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Blur Passthrough Encoder"),
                });
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: input.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: out.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: input.width(),
                    height: input.height(),
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(std::iter::once(encoder.finish()));
            return out;
        }

        // `sigma` flows in from public APIs (BlurFilter constructors) that do
        // not clamp non-negative, so explicitly clamp to `[0, ∞)` in float
        // space before the `as u32` cast. The `.clamp(1, 5)` then bounds the
        // result; truncation is the documented integer-iteration count and
        // sign loss is impossible after the float-space clamp.
        let sigma_nonneg = sigma.max(0.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "sigma_nonneg is ≥0 by the line above; the cast is bounded by .clamp(1, 5)"
        )]
        let iterations = ((sigma_nonneg / 2.0).ceil() as u32).clamp(1, 5);
        let offset = sigma.max(1.0);

        tracing::debug!(
            "Rendering Dual Kawase blur: sigma={}, iterations={}, offset={}, input={}x{}",
            sigma,
            iterations,
            offset,
            input.width(),
            input.height()
        );

        // Ensure blur pipelines exist
        let pipelines = self.get_or_create_blur_pipelines();
        let downsample_pipeline = Arc::clone(&pipelines.downsample);
        let upsample_pipeline = Arc::clone(&pipelines.upsample);

        // Create mip chain: mip[0] = input size, mip[i+1] = half of mip[i]
        let mut mip_chain: Vec<PooledTexture> = Vec::with_capacity(iterations as usize + 1);

        // mip[0] = copy of input at original resolution
        let mip0 = self
            .texture_pool
            .acquire(input.width(), input.height(), self.surface_format);
        mip_chain.push(mip0);

        // Create progressively smaller mip levels
        for i in 0..iterations {
            let prev_w = mip_chain[i as usize].width();
            let prev_h = mip_chain[i as usize].height();
            let w = (prev_w / 2).max(1);
            let h = (prev_h / 2).max(1);
            let mip = self.texture_pool.acquire(w, h, self.surface_format);
            mip_chain.push(mip);
        }

        // Reuse the cached fullscreen-quad VB and linear sampler — both are
        // invariant across all blur iterations (same geometry, same filter params).

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Blur Command Encoder"),
            });

        // Copy input to mip[0]
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: input.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: mip_chain[0].texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: input.width(),
                height: input.height(),
                depth_or_array_layers: 1,
            },
        );

        // === Downsample passes ===
        // Uniform buffer index `i` is written before it is consumed by the draw
        // call at iteration `i`.  All writes and draws land in the same command
        // buffer that is submitted once at the end of this method, so there is
        // no hazard: the GPU processes the commands in order within a submission.
        for i in 0..iterations {
            let src_index = i as usize;
            let dst_index = (i + 1) as usize;

            let src_w = mip_chain[src_index].width() as f32;
            let src_h = mip_chain[src_index].height() as f32;

            let params = BlurParams {
                texture_size: [src_w, src_h],
                offset,
                _padding: 0.0,
            };

            // Update the pre-allocated uniform slot instead of allocating a new buffer.
            self.queue.write_buffer(
                &self.blur_uniform_buffers[src_index],
                0,
                bytemuck::bytes_of(&params),
            );

            let src_view = mip_chain[src_index].view();
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur Downsample Bind Group"),
                layout: &self.blur_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.blur_uniform_buffers[src_index].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                    },
                ],
            });

            let dst_view = mip_chain[dst_index].view();
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Blur Downsample Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: dst_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

                render_pass.set_pipeline(&downsample_pipeline);
                render_pass.set_bind_group(0, &bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.fullscreen_quad_vb.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        // === Upsample passes ===
        // Reuse the same pre-allocated uniform buffer slots (one per mip level).
        // The downsample loop already updated slots 0..iterations-1; the upsample
        // loop processes the same `src_index` range in reverse with the same `params`
        // values (same `texture_size` and `offset`), so we can re-read the buffers
        // that were written during the downsample phase without overwriting them.
        // This is safe because both phases use the same `src_w/src_h` calculation
        // for a given `src_index`, and `offset` is constant for the whole `render_blur`
        // call.
        for i in (0..iterations).rev() {
            let src_index = (i + 1) as usize;
            let dst_index = i as usize;

            // The uniform slot for `src_index` was already written during the
            // downsample phase (same params: texture_size of mip[src_index] +
            // the same `offset`).  Re-use it directly — no write needed.
            let src_view = mip_chain[src_index].view();
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur Upsample Bind Group"),
                layout: &self.blur_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.blur_uniform_buffers[src_index].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                    },
                ],
            });

            let dst_view = mip_chain[dst_index].view();
            {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Blur Upsample Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: dst_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

                render_pass.set_pipeline(&upsample_pipeline);
                render_pass.set_bind_group(0, &bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.fullscreen_quad_vb.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        // Submit all blur passes
        self.queue.submit(std::iter::once(encoder.finish()));

        tracing::debug!("Blur rendering complete: {} iterations", iterations);

        // Return mip[0] which now contains the blurred result at original resolution.
        // Drop the rest of the mip chain (returned to pool automatically).
        let result = mip_chain.remove(0);
        drop(mip_chain);
        result
    }
}
