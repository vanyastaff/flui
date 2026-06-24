// Shader-mask pipeline creation and render_masked execution.
//
// Moved from `offscreen.rs` into `offscreen/mask.rs` as part of the C1 LOC-cap
// concern-separation split.  Zero behaviour changes.

use std::sync::Arc;

use flui_types::{
    Size,
    geometry::{Pixels, Rect},
    painting::Shader,
};
use wgpu::util::DeviceExt;

use super::super::shader_compiler::ShaderType;
use super::{MaskedRenderResult, OffscreenRenderer};

impl OffscreenRenderer {
    /// Get or create render pipeline for shader type
    ///
    /// Creates a wgpu::RenderPipeline from the WGSL shader source.
    /// Pipelines are cached to avoid recreation.
    ///
    /// Visibility is `pub(super)` because `warmup` in `mod.rs` calls this
    /// method — a parent-to-child cross-module call.
    pub(super) fn get_or_create_pipeline(
        &mut self,
        shader_type: ShaderType,
    ) -> Arc<wgpu::RenderPipeline> {
        if !self.pipelines.contains_key(&shader_type) {
            tracing::trace!("Creating render pipeline for {:?}", shader_type);

            // Get compiled shader from cache
            let compiled_shader = self.shader_cache.get_or_compile(shader_type);

            // Create shader module from WGSL source
            let shader_module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(shader_type.label()),
                    source: wgpu::ShaderSource::Wgsl(compiled_shader.source.as_str().into()),
                });

            // Create pipeline layout
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some(&format!("{} Pipeline Layout", shader_type.label())),
                        bind_group_layouts: &[Some(&self.bind_group_layout)],
                        immediate_size: 0,
                    });

            // Create render pipeline
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&format!("{} Pipeline", shader_type.label())),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader_module,
                        entry_point: Some("vs_main"),
                        buffers: &[
                            // Fullscreen quad vertex buffer layout
                            wgpu::VertexBufferLayout {
                                array_stride: std::mem::size_of::<[f32; 4]>()
                                    as wgpu::BufferAddress,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &[
                                    // position: vec2<f32>
                                    wgpu::VertexAttribute {
                                        format: wgpu::VertexFormat::Float32x2,
                                        offset: 0,
                                        shader_location: 0,
                                    },
                                    // tex_coords: vec2<f32>
                                    wgpu::VertexAttribute {
                                        format: wgpu::VertexFormat::Float32x2,
                                        offset: std::mem::size_of::<[f32; 2]>()
                                            as wgpu::BufferAddress,
                                        shader_location: 1,
                                    },
                                ],
                            },
                        ],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader_module,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: self.surface_format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

            self.pipelines.insert(shader_type, Arc::new(pipeline));
        }

        // The key was either already present or just inserted above.
        Arc::clone(
            self.pipelines
                .get(&shader_type)
                .expect("pipeline was inserted above"),
        )
    }

    /// Render child with shader mask applied
    ///
    /// # Arguments
    ///
    /// * `child_bounds` - Bounding rectangle of child content, in **logical**
    ///   pixels. Used only to normalize the shader's gradient endpoints into the
    ///   0..1 range (`(endpoint - origin) / extent`); that normalization is
    ///   scale-invariant, so the shader's endpoints and these bounds must share
    ///   one coordinate space (both logical) for the gradient to land correctly.
    /// * `result_size` - Size of the masked result texture, in **device**
    ///   pixels (`child_bounds` extent × device-pixel-ratio). The fullscreen
    ///   quad covers the whole result regardless of its pixel dimensions, so
    ///   sizing it at device resolution keeps a HiDPI masked layer crisp instead
    ///   of allocating it at half resolution and upscaling on composite.
    /// * `shader` - Shader (gradient, solid, etc.)
    /// * `blend_mode` - Blend mode for compositing
    /// * `child_texture` - Pre-rendered child content texture (device-sized)
    ///
    /// # Returns
    ///
    /// Render commands or texture handle for the masked result
    ///
    /// # Implementation Steps
    ///
    /// 1. Acquire offscreen texture from pool
    /// 2. Create GPU resources (buffers, bind groups)
    /// 3. Setup render pass targeting offscreen texture
    /// 4. Execute shader mask pipeline
    /// 5. Return masked texture
    pub fn render_masked(
        &mut self,
        child_bounds: Rect<Pixels>,
        result_size: Size<Pixels>,
        shader: &Shader,
        blend_mode: flui_types::painting::BlendMode,
        child_texture: &wgpu::Texture,
    ) -> MaskedRenderResult {
        // Get shader type for this shader
        let shader_type = ShaderType::from_shader(shader);

        tracing::trace!(
            "Rendering shader mask: {:?}, bounds: {:?}, result_size: {:?}",
            shader_type,
            child_bounds,
            result_size
        );

        // Acquire offscreen texture for masked result, sized at device
        // resolution so a HiDPI mask is not allocated at half resolution.
        let texture = self
            .texture_pool
            .acquire_from_size(result_size, self.surface_format);

        // Ensure pipeline exists (Arc allows using after mutable borrow ends)
        let _ = self.get_or_create_pipeline(shader_type);

        // Reuse the cached fullscreen-quad vertex buffer (content is invariant).
        // The uniform buffer is per-call (depends on child_bounds + shader type).
        let uniform_data = shader.to_mask_uniform_data(child_bounds);
        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Shader Mask Uniform Buffer"),
                contents: &uniform_data,
                usage: wgpu::BufferUsages::UNIFORM,
            });

        // Create texture view for child content
        let child_view = child_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Reuse the cached linear sampler (ClampToEdge × Linear — invariant).
        // Create bind group (references per-call child_view + uniform_buffer).
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader Mask Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&child_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        // Use the pooled texture as the offscreen render target
        let output_view = texture.view();

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Shader Mask Render Encoder"),
            });

        // Get pipeline reference (exists from earlier get_or_create call)
        let pipeline = self
            .pipelines
            .get(&shader_type)
            .expect("pipeline was ensured by get_or_create_pipeline above");

        // Create render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader Mask Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
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

            // Set pipeline and bind group
            render_pass.set_pipeline(pipeline);
            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.fullscreen_quad_vb.slice(..));

            // Draw fullscreen quad (6 vertices = 2 triangles)
            render_pass.draw(0..6, 0..1);
        }

        // Submit commands
        self.queue.submit(std::iter::once(encoder.finish()));

        tracing::trace!(
            "Shader mask rendering complete: {:?}, size: {}x{}",
            shader_type,
            result_size.width,
            result_size.height
        );

        MaskedRenderResult {
            texture,
            shader_type,
            blend_mode,
        }
    }
}
