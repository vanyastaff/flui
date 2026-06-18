//! Offscreen rendering infrastructure for shader masks
//!
//! Manages GPU pipelines, render passes, and offscreen texture rendering
//! for ShaderMaskLayer effects.

use std::{collections::HashMap, sync::Arc};

use bytemuck::{Pod, Zeroable};
use flui_types::{
    Size,
    geometry::{Pixels, Rect},
    painting::{BlendMode, Shader},
};
use wgpu::util::DeviceExt;

use super::{
    shader_compiler::{ShaderCache, ShaderType},
    texture_pool::{PooledTexture, TexturePool},
};

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
/// │  Input: Child Canvas + Shader                            │
/// │  Output: Masked Canvas                                  │
/// └──────────────────────────────────────────────────────────┘
/// ```
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

    /// Bind group layout for blur shaders (uniform + texture + sampler)
    blur_bind_group_layout: wgpu::BindGroupLayout,

    /// Cached blur pipelines (downsample, upsample)
    blur_pipelines: Option<BlurPipelines>,

    /// Cached morphological filter pipelines (dilate, erode)
    morph_pipelines: Option<MorphPipelines>,

    /// Fullscreen blit pipeline — lazily created when the intermediate-active
    /// present path is first used (COPY_SRC-less adapters, or forced in tests).
    blit_pipeline: Option<BlitPipeline>,
}

// ---------------------------------------------------------------------------
// Blit pipeline — intermediate → swapchain surface (no blend, Replace/Copy)
// ---------------------------------------------------------------------------

/// Cached GPU resources for the fullscreen intermediate→surface blit.
///
/// The pipeline uses no blend equation (`blend: None`) so every texel of the
/// surface is overwritten.  Nearest-neighbour sampling keeps the blit
/// pixel-identical to a direct render.
///
/// All fields are `Arc`-wrapped so `blit_to_surface` can clone them out before
/// dropping the `&mut self` borrow (wgpu handle types are not `Clone`).
///
/// `sampler` and `vertex_buffer` are frame-invariant (Nearest parameters and a
/// static fullscreen-quad layout do not change per frame) so they are cached
/// here and reused every blit instead of being re-allocated each frame.
struct BlitPipeline {
    pipeline: Arc<wgpu::RenderPipeline>,
    bind_group_layout: Arc<wgpu::BindGroupLayout>,
    /// Nearest-neighbour sampler — cached because its parameters never change.
    sampler: Arc<wgpu::Sampler>,
    /// Fullscreen-quad vertex buffer — cached because its contents never change.
    vertex_buffer: Arc<wgpu::Buffer>,
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
        let blur_bind_group_layout = Self::create_blur_bind_group_layout(&device);

        Self {
            texture_pool: Arc::new(TexturePool::new(Arc::clone(&device))),
            shader_cache: Arc::new(ShaderCache::new()),
            device,
            queue,
            surface_format,
            pipelines: HashMap::new(),
            bind_group_layout,
            blur_bind_group_layout,
            blur_pipelines: None,
            morph_pipelines: None,
            blit_pipeline: None,
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
        let blur_bind_group_layout = Self::create_blur_bind_group_layout(&device);

        Self {
            texture_pool,
            shader_cache,
            device,
            queue,
            surface_format,
            pipelines: HashMap::new(),
            bind_group_layout,
            blur_bind_group_layout,
            blur_pipelines: None,
            morph_pipelines: None,
            blit_pipeline: None,
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
        blend_mode: BlendMode,
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

        // Create sampler
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Shader Mask Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
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
            .expect("Pipeline should exist");

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
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));

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

    /// Create bind group layout for blur shaders
    ///
    /// Layout matches the Dual Kawase blur shaders:
    /// - @group(0) @binding(0): uniform buffer (BlurParams)
    /// - @group(0) @binding(1): input texture (sampled)
    /// - @group(0) @binding(2): sampler
    fn create_blur_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Blur Bind Group Layout"),
            entries: &[
                // BlurParams uniform
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
                // Input texture
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
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

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
                .expect("Downsample shader module should be compiled");

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
                .expect("Upsample shader module should be compiled");

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
            .expect("Blur pipelines were just created")
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

        // Create shared resources
        let vertices = FullscreenVertex::fullscreen_quad();
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Blur Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Blur Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

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
        for i in 0..iterations {
            let src_idx = i as usize;
            let dst_idx = (i + 1) as usize;

            let src_w = mip_chain[src_idx].width() as f32;
            let src_h = mip_chain[src_idx].height() as f32;

            let params = BlurParams {
                texture_size: [src_w, src_h],
                offset,
                _padding: 0.0,
            };

            let uniform_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Blur Downsample Params"),
                        contents: bytemuck::bytes_of(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

            let src_view = mip_chain[src_idx].view();
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur Downsample Bind Group"),
                layout: &self.blur_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            let dst_view = mip_chain[dst_idx].view();
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
                render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        // === Upsample passes ===
        for i in (0..iterations).rev() {
            let src_idx = (i + 1) as usize;
            let dst_idx = i as usize;

            let src_w = mip_chain[src_idx].width() as f32;
            let src_h = mip_chain[src_idx].height() as f32;

            let params = BlurParams {
                texture_size: [src_w, src_h],
                offset,
                _padding: 0.0,
            };

            let uniform_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Blur Upsample Params"),
                        contents: bytemuck::bytes_of(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

            let src_view = mip_chain[src_idx].view();
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur Upsample Bind Group"),
                layout: &self.blur_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            let dst_view = mip_chain[dst_idx].view();
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
                render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        // Submit all blur passes
        self.queue.submit(std::iter::once(encoder.finish()));

        tracing::debug!("Blur rendering complete: {} iterations", iterations);

        // Return mip[0] which now contains the blurred result at original resolution
        // Drop the rest of the mip chain (returned to pool automatically)
        let result = mip_chain.remove(0);
        drop(mip_chain);
        result
    }

    /// Get or create the morphological filter pipelines (dilate + erode)
    ///
    /// Lazily creates both pipelines on first call, then caches them.
    /// Reuses `blur_bind_group_layout` since bindings are identical
    /// (uniform + texture + sampler).
    fn get_or_create_morph_pipelines(&mut self) -> &MorphPipelines {
        if self.morph_pipelines.is_none() {
            tracing::debug!("Creating morphological filter pipelines");

            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("Morph Pipeline Layout"),
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

            // Dilate pipeline
            let dilate_shader = self
                .shader_cache
                .get_or_compile_module(ShaderType::MorphDilate, &self.device);
            let dilate_module = dilate_shader
                .module
                .as_ref()
                .expect("Dilate shader module should be compiled");

            let dilate_pipeline =
                self.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("Morphological Dilate Pipeline"),
                        layout: Some(&pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: dilate_module,
                            entry_point: Some("vs_main"),
                            buffers: std::slice::from_ref(&vertex_buffer_layout),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: dilate_module,
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

            // Erode pipeline
            let erode_shader = self
                .shader_cache
                .get_or_compile_module(ShaderType::MorphErode, &self.device);
            let erode_module = erode_shader
                .module
                .as_ref()
                .expect("Erode shader module should be compiled");

            let erode_pipeline =
                self.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("Morphological Erode Pipeline"),
                        layout: Some(&pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: erode_module,
                            entry_point: Some("vs_main"),
                            buffers: &[vertex_buffer_layout],
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: erode_module,
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

            self.morph_pipelines = Some(MorphPipelines {
                dilate: Arc::new(dilate_pipeline),
                erode: Arc::new(erode_pipeline),
            });
        }

        self.morph_pipelines
            .as_ref()
            .expect("Morph pipelines were just created")
    }

    /// Apply morphological filter (dilate or erode) to an input texture
    ///
    /// Uses a two-pass separable approach (horizontal then vertical) for O(N)
    /// per pixel instead of O(N²).
    ///
    /// # Arguments
    ///
    /// * `input` - Source texture to filter
    /// * `radius` - Kernel radius in pixels
    /// * `is_dilate` - `true` for dilate (max filter), `false` for erode (min filter)
    ///
    /// # Returns
    ///
    /// A new `PooledTexture` containing the filtered result at the original resolution.
    pub fn render_morphological(
        &mut self,
        input: &PooledTexture,
        radius: f32,
        is_dilate: bool,
    ) -> PooledTexture {
        let filter_name = if is_dilate { "dilate" } else { "erode" };

        tracing::debug!(
            "Rendering morphological {}: radius={}, input={}x{}",
            filter_name,
            radius,
            input.width(),
            input.height()
        );

        // Ensure morph pipelines exist and pick the right one
        let pipelines = self.get_or_create_morph_pipelines();
        let pipeline = if is_dilate {
            Arc::clone(&pipelines.dilate)
        } else {
            Arc::clone(&pipelines.erode)
        };

        // Acquire intermediate texture for horizontal pass result
        let temp = self
            .texture_pool
            .acquire(input.width(), input.height(), self.surface_format);

        // Acquire output texture for vertical pass result
        let output = self
            .texture_pool
            .acquire(input.width(), input.height(), self.surface_format);

        // Create shared resources
        let vertices = FullscreenVertex::fullscreen_quad();
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Morph Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Morph Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let tex_w = input.width() as f32;
        let tex_h = input.height() as f32;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Morph Command Encoder"),
            });

        // === Pass 1: Horizontal (input → temp) ===
        {
            let params = MorphParams {
                texture_size: [tex_w, tex_h],
                radius,
                direction: 0.0, // horizontal
            };

            let uniform_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Morph Horizontal Params"),
                        contents: bytemuck::bytes_of(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

            let src_view = input.view();
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Morph Horizontal Bind Group"),
                layout: &self.blur_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            let dst_view = temp.view();
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Morph Horizontal Pass"),
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

            render_pass.set_pipeline(&pipeline);
            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.draw(0..6, 0..1);
        }

        // === Pass 2: Vertical (temp → output) ===
        {
            let params = MorphParams {
                texture_size: [tex_w, tex_h],
                radius,
                direction: 1.0, // vertical
            };

            let uniform_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Morph Vertical Params"),
                        contents: bytemuck::bytes_of(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

            let src_view = temp.view();
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Morph Vertical Bind Group"),
                layout: &self.blur_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            let dst_view = output.view();
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Morph Vertical Pass"),
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

            render_pass.set_pipeline(&pipeline);
            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.draw(0..6, 0..1);
        }

        // Submit both passes
        self.queue.submit(std::iter::once(encoder.finish()));

        tracing::debug!(
            "Morphological {} rendering complete: radius={}",
            filter_name,
            radius
        );

        // temp texture is dropped here, returned to pool automatically
        output
    }

    /// Access the wgpu device
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// Access the wgpu queue
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    /// Get the surface texture format
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    /// Access the texture pool
    pub fn texture_pool(&self) -> &Arc<TexturePool> {
        &self.texture_pool
    }

    /// Get texture pool statistics
    pub fn texture_pool_stats(&self) -> super::texture_pool::PoolStats {
        self.texture_pool.stats()
    }

    /// Clear texture pool (useful for memory management)
    pub fn clear_texture_pool(&self) {
        self.texture_pool.clear();
    }

    // -----------------------------------------------------------------------
    // Intermediate-blit pipeline (COPY_SRC-less present path)
    // -----------------------------------------------------------------------

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
                        include_str!("shaders/effects/blit.wgsl").into(),
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
            .expect("blit_pipeline is Some after get_or_create_blit_pipeline")
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

    /// Consume the result and extract the pooled texture for compositing.
    pub fn into_texture(self) -> PooledTexture {
        self.texture
    }
}

/// Uniform parameters for Dual Kawase blur shaders
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BlurParams {
    /// Size of the source texture in pixels
    pub texture_size: [f32; 2],
    /// Sample offset multiplier (controls blur spread)
    pub offset: f32,
    /// Padding for 16-byte alignment
    pub _padding: f32,
}

/// Cached Dual Kawase blur pipelines (downsample + upsample)
#[allow(missing_debug_implementations)]
struct BlurPipelines {
    downsample: Arc<wgpu::RenderPipeline>,
    upsample: Arc<wgpu::RenderPipeline>,
}

/// Uniform parameters for morphological filter shaders (dilate/erode)
///
/// Same alignment as [`BlurParams`] (16 bytes, 4 floats) so it can reuse
/// the `blur_bind_group_layout`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MorphParams {
    /// Size of the source texture in pixels
    pub texture_size: [f32; 2],
    /// Kernel radius in pixels
    pub radius: f32,
    /// Pass direction: 0.0 = horizontal, 1.0 = vertical
    pub direction: f32,
}

/// Cached morphological filter pipelines (dilate + erode)
#[allow(missing_debug_implementations)]
struct MorphPipelines {
    dilate: Arc<wgpu::RenderPipeline>,
    erode: Arc<wgpu::RenderPipeline>,
}

// Pre-cycle this module exposed `PipelineManager` + `PipelineHandle` as a
// forward-looking wrapper around `ShaderCache` and `wgpu::RenderPipeline`.
// Both types were deleted in cycle 4 E-3:
//   - `PipelineManager` carried only a `shader_cache: Arc<ShaderCache>` field;
//     the `device` / `pipelines` fields were commented-out TODOs.
//   - `PipelineHandle` carried only `shader_type: ShaderType`, semantically
//     equivalent to a `(ShaderType,)` newtype.
// The real pipeline ownership lives in `wgpu/pipelines.rs::PipelineCache`,
// which `Backend` actually uses. Forward-looking shapes that have been TODO
// for >18 months and have zero workspace consumers are codified design drift,
// not "cost-cheap options" — see audit
// `docs/research/2026-05-22-flui-rendering-engine-audit.md` E-3 and the
// cycle-1 PR #93 `typestate.rs` deletion precedent.

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

    // Vertex buffer layout descriptor for this type, for use when
    // `FullscreenVertex` is bound as a vertex buffer in a render pipeline.
    // Uncomment and implement when `OffscreenRenderer` grows a wired-up
    // vertex-based fullscreen pass (current path uses hard-coded clip-space
    // triangles via a storage buffer).
    // pub fn buffer_layout() -> wgpu::VertexBufferLayout<'static> { ... }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact expected values produced by exact arithmetic"
)]
mod tests {
    use super::*;

    // Note: OffscreenRenderer tests are ignored because they require wgpu
    // Device/Queue These would need GPU resources to run properly.
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
