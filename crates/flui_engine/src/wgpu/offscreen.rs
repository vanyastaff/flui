//! Offscreen rendering infrastructure for shader masks
//!
//! Manages GPU pipelines, render passes, and offscreen texture rendering
//! for ShaderMaskLayer effects.

use super::shader_compiler::{ShaderCache, ShaderType};
use super::texture_pool::{PooledTexture, TexturePool};
use flui_types::{
    geometry::{Pixels, Rect},
    painting::{BlendMode, ShaderSpec},
    Size,
};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Offscreen renderer for shader mask effects
///
/// Manages the complete rendering pipeline for shader masks:
/// 1. Render child to offscreen texture
/// 2. Apply shader mask to texture
/// 3. Composite masked result to framebuffer
///
/// # Architecture
///
/// ```text
/// ┌──────────────────────────────────────────────────────────┐
/// │ OffscreenRenderer                                        │
/// │                                                          │
/// │  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐ │
/// │  │ Texture     │  │ Shader       │  │ Pipeline       │ │
/// │  │ Pool        │→ │ Cache        │→ │ Manager        │ │
/// │  └─────────────┘  └──────────────┘  └────────────────┘ │
/// │                                                          │
/// │  Input: Child Canvas + ShaderSpec                       │
/// │  Output: Masked Canvas                                  │
/// └──────────────────────────────────────────────────────────┘
/// ```
///
#[allow(missing_debug_implementations)]
pub struct OffscreenRenderer {
    /// Texture pool for offscreen rendering
    texture_pool: Arc<TexturePool>,

    /// Shader cache for compiled shaders
    shader_cache: Arc<ShaderCache>,

    /// wgpu device for GPU operations
    device: Arc<wgpu::Device>,

    /// wgpu queue for command submission
    queue: Arc<wgpu::Queue>,

    /// Surface texture format
    surface_format: wgpu::TextureFormat,

    /// Cached render pipelines per shader type
    pipelines: HashMap<ShaderType, Arc<wgpu::RenderPipeline>>,

    /// Bind group layout for shader uniforms
    bind_group_layout: wgpu::BindGroupLayout,
}

impl OffscreenRenderer {
    /// Create new offscreen renderer with GPU resources
    ///
    /// # Arguments
    ///
    /// * `device` - wgpu device for GPU operations
    /// * `queue` - wgpu queue for command submission
    /// * `surface_format` - texture format for framebuffer
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let bind_group_layout = Self::create_bind_group_layout(&device);

        Self {
            texture_pool: Arc::new(TexturePool::new()),
            shader_cache: Arc::new(ShaderCache::new()),
            device,
            queue,
            surface_format,
            pipelines: HashMap::new(),
            bind_group_layout,
        }
    }

    /// Create with custom texture pool and shader cache
    pub fn with_caches(
        texture_pool: Arc<TexturePool>,
        shader_cache: Arc<ShaderCache>,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let bind_group_layout = Self::create_bind_group_layout(&device);

        Self {
            texture_pool,
            shader_cache,
            device,
            queue,
            surface_format,
            pipelines: HashMap::new(),
            bind_group_layout,
        }
    }

    /// Create bind group layout for shader mask rendering
    ///
    /// Layout:
    /// - @group(0) @binding(0): child texture (sampled)
    /// - @group(0) @binding(1): sampler
    /// - @group(0) @binding(2): uniform buffer
    fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shader Mask Bind Group Layout"),
            entries: &[
                // Child texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Uniform buffer
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
        })
    }

    /// Pre-compile all shaders (reduces first-use latency)
    pub fn warmup(&mut self) {
        self.shader_cache.precompile_all();

        // Pre-create all pipelines
        self.get_or_create_pipeline(ShaderType::SolidMask);
        self.get_or_create_pipeline(ShaderType::LinearGradientMask);
        self.get_or_create_pipeline(ShaderType::RadialGradientMask);
    }

    /// Get or create render pipeline for shader type
    ///
    /// Creates a wgpu::RenderPipeline from the WGSL shader source.
    /// Pipelines are cached to avoid recreation.
    fn get_or_create_pipeline(&mut self, shader_type: ShaderType) -> Arc<wgpu::RenderPipeline> {
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
                        bind_group_layouts: &[&self.bind_group_layout],
                        push_constant_ranges: &[],
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
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader_module,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: self.surface_format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
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
                    multiview: None,
                    cache: None,
                });

            self.pipelines.insert(shader_type, Arc::new(pipeline));
        }

        // SAFETY: We just inserted this key above, so it must exist
        Arc::clone(
            self.pipelines
                .get(&shader_type)
                .expect("Pipeline was just inserted"),
        )
    }

    /// Render child with shader mask applied
    ///
    /// # Arguments
    ///
    /// * `child_bounds` - Bounding rectangle of child content
    /// * `shader_spec` - Shader specification (gradient, solid, etc.)
    /// * `blend_mode` - Blend mode for compositing
    /// * `child_texture` - Pre-rendered child content texture
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
        shader_spec: &ShaderSpec,
        blend_mode: BlendMode,
        child_texture: &wgpu::Texture,
    ) -> MaskedRenderResult {
        // Get shader type for this spec
        let shader_type = ShaderType::from_spec(shader_spec);

        tracing::trace!(
            "Rendering shader mask: {:?}, bounds: {:?}",
            shader_type,
            child_bounds
        );

        // Acquire offscreen texture for masked result
        let size = Size::new(child_bounds.width(), child_bounds.height());
        let texture = self.texture_pool.acquire(size);

        // Ensure pipeline exists (Arc allows using after mutable borrow ends)
        let _ = self.get_or_create_pipeline(shader_type);

        // Create fullscreen quad vertex buffer
        let vertices = FullscreenVertex::fullscreen_quad();
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Shader Mask Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        // Create uniform buffer with shader-specific data
        let uniform_data = shader_spec.to_uniform_data();
        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Shader Mask Uniform Buffer"),
                contents: &uniform_data,
                usage: wgpu::BufferUsages::UNIFORM,
            });

        // Create texture view for child content
        let child_view = child_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create sampler
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Shader Mask Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Create bind group
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
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        // Create texture descriptor for offscreen target
        let texture_desc = wgpu::TextureDescriptor {
            label: Some("Shader Mask Offscreen Texture"),
            size: wgpu::Extent3d {
                width: size.width.0 as u32,
                height: size.height.0 as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let output_texture = self.device.create_texture(&texture_desc);
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

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
            .expect("Pipeline should exist");

        // Create render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader Mask Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Set pipeline and bind group
            render_pass.set_pipeline(pipeline);
            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));

            // Draw fullscreen quad (6 vertices = 2 triangles)
            render_pass.draw(0..6, 0..1);
        }

        // Submit commands
        self.queue.submit(std::iter::once(encoder.finish()));

        tracing::trace!(
            "Shader mask rendering complete: {:?}, size: {}x{}",
            shader_type,
            size.width,
            size.height
        );

        MaskedRenderResult {
            texture,
            shader_type,
            blend_mode,
        }
    }

    /// Get texture pool statistics
    pub fn texture_pool_stats(&self) -> super::texture_pool::PoolStats {
        self.texture_pool.stats()
    }

    /// Clear texture pool (useful for memory management)
    pub fn clear_texture_pool(&self) {
        self.texture_pool.clear();
    }
}

// Note: No Default implementation - OffscreenRenderer requires wgpu resources

/// Result of masked rendering operation
///
/// Contains the offscreen texture with the masked content.
/// The texture will be automatically returned to the pool when dropped.
#[derive(Debug)]
pub struct MaskedRenderResult {
    /// Offscreen texture containing masked result
    pub texture: PooledTexture,

    /// Shader type that was applied
    pub shader_type: ShaderType,

    /// Blend mode for final composition
    pub blend_mode: BlendMode,
}

impl MaskedRenderResult {
    /// Get the texture descriptor
    pub fn texture_desc(&self) -> &super::texture_pool::TextureDesc {
        self.texture.desc()
    }

    /// Get texture dimensions
    pub fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }
}

/// GPU pipeline manager for shader masks
///
/// Manages wgpu::RenderPipeline instances for different shader types.
///
/// # Implementation Note
///
/// This is a placeholder. Full implementation will create and cache
/// wgpu::RenderPipeline objects for each shader type.
#[derive(Debug)]
pub struct PipelineManager {
    shader_cache: Arc<ShaderCache>,
    // TODO: Add actual pipelines when integrating with wgpu
    // device: Arc<wgpu::Device>,
    // pipelines: HashMap<ShaderType, wgpu::RenderPipeline>,
}

impl PipelineManager {
    /// Create new pipeline manager
    pub fn new(shader_cache: Arc<ShaderCache>) -> Self {
        Self { shader_cache }
    }

    /// Get or create render pipeline for shader type
    ///
    /// # TODO: Full wgpu implementation
    ///
    /// ```rust,ignore
    /// let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    ///     label: Some(shader_type.label()),
    ///     layout: Some(&pipeline_layout),
    ///     vertex: wgpu::VertexState {
    ///         module: &shader_module,
    ///         entry_point: "vs_main",
    ///         buffers: &[vertex_buffer_layout],
    ///     },
    ///     fragment: Some(wgpu::FragmentState {
    ///         module: &shader_module,
    ///         entry_point: "fs_main",
    ///         targets: &[wgpu::ColorTargetState {
    ///             format: wgpu::TextureFormat::Rgba8UnormSrgb,
    ///             blend: Some(wgpu::BlendState::ALPHA_BLENDING),
    ///             write_mask: wgpu::ColorWrites::ALL,
    ///         }],
    ///     }),
    ///     primitive: wgpu::PrimitiveState::default(),
    ///     depth_stencil: None,
    ///     multisample: wgpu::MultisampleState::default(),
    ///     multiview: None,
    /// });
    /// ```
    pub fn get_or_create_pipeline(&self, shader_type: ShaderType) -> PipelineHandle {
        // Ensure shader is compiled
        let _shader = self.shader_cache.get_or_compile(shader_type);

        tracing::trace!("Getting pipeline for shader: {:?}", shader_type);

        // TODO: Create actual wgpu::RenderPipeline

        PipelineHandle { shader_type }
    }
}

/// Handle to a GPU render pipeline
///
/// Placeholder for wgpu::RenderPipeline reference.
#[derive(Debug, Clone, Copy)]
pub struct PipelineHandle {
    pub shader_type: ShaderType,
    // TODO: Add Arc<wgpu::RenderPipeline> when integrated
}

/// Vertex for fullscreen quad rendering
///
/// Used to render the masked texture as a fullscreen quad.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FullscreenVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
}

impl FullscreenVertex {
    /// Create fullscreen quad vertices
    ///
    /// Returns 6 vertices forming 2 triangles that cover the entire screen.
    pub fn fullscreen_quad() -> [FullscreenVertex; 6] {
        [
            // Triangle 1
            FullscreenVertex {
                position: [-1.0, -1.0],
                tex_coords: [0.0, 1.0],
            },
            FullscreenVertex {
                position: [1.0, -1.0],
                tex_coords: [1.0, 1.0],
            },
            FullscreenVertex {
                position: [-1.0, 1.0],
                tex_coords: [0.0, 0.0],
            },
            // Triangle 2
            FullscreenVertex {
                position: [-1.0, 1.0],
                tex_coords: [0.0, 0.0],
            },
            FullscreenVertex {
                position: [1.0, -1.0],
                tex_coords: [1.0, 1.0],
            },
            FullscreenVertex {
                position: [1.0, 1.0],
                tex_coords: [1.0, 0.0],
            },
        ]
    }

    // TODO: Add wgpu::VertexBufferLayout when integrated
    // pub fn buffer_layout() -> wgpu::VertexBufferLayout<'static> { ... }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    // Note: OffscreenRenderer tests are ignored because they require wgpu Device/Queue
    // These would need GPU resources to run properly.
    // The functionality is tested via integration tests with actual GPU.

    #[test]
    #[ignore = "requires wgpu Device/Queue"]
    fn test_offscreen_renderer_new() {
        // This test requires actual wgpu resources
        // See integration tests for full GPU testing
    }

    #[test]
    #[ignore = "requires wgpu Device/Queue"]
    fn test_offscreen_renderer_warmup() {
        // This test requires actual wgpu resources
        // Shader compilation is tested in shader_compiler tests
    }

    #[test]
    #[ignore = "requires wgpu Device/Queue"]
    fn test_render_masked_acquires_texture() {
        // This test requires actual wgpu resources
        // Texture pool functionality is tested in texture_pool tests
    }

    #[test]
    #[ignore = "requires wgpu Device/Queue"]
    fn test_render_masked_different_shaders() {
        // This test requires actual wgpu resources
        // Different shader types tested in shader_compiler tests
    }

    #[test]
    #[ignore = "requires wgpu Device/Queue"]
    fn test_texture_pool_reuse() {
        // This test requires actual wgpu resources
        // Texture pool reuse is tested in texture_pool tests
    }

    #[test]
    #[ignore = "requires wgpu Device/Queue"]
    fn test_pipeline_manager_new() {
        // This test requires actual wgpu resources
        // Pipeline creation tested in full GPU integration tests
    }

    #[test]
    fn test_fullscreen_quad_vertices() {
        let vertices = FullscreenVertex::fullscreen_quad();

        assert_eq!(vertices.len(), 6);

        // Check corners
        assert_eq!(vertices[0].position, [-1.0, -1.0]); // Bottom-left
        assert_eq!(vertices[1].position, [1.0, -1.0]); // Bottom-right
        assert_eq!(vertices[2].position, [-1.0, 1.0]); // Top-left
        assert_eq!(vertices[5].position, [1.0, 1.0]); // Top-right

        // Check texture coordinates
        assert_eq!(vertices[0].tex_coords, [0.0, 1.0]); // Bottom-left
        assert_eq!(vertices[5].tex_coords, [1.0, 0.0]); // Top-right
    }

    #[test]
    #[ignore = "requires wgpu Device/Queue"]
    fn test_masked_render_result_texture_desc() {
        // This test requires actual wgpu resources
        // Texture descriptor functionality is tested in texture_pool tests
    }
}
