//! GPU-accelerated 2D painter using wgpu + glyphon + lyon
//!
//! This is the unified painter implementation that combines:
//! - Shape rendering via vertex batching
//! - Text rendering via glyphon
//! - Path tessellation via lyon
//! - Transform stack for coordinate transformations
//!
//! Follows SOLID and KISS principles with clean separation of concerns.

use std::sync::Arc;

use flui_painting::{Paint, PaintStyle};
use flui_types::{
    Offset, Point, Rect,
    geometry::{Pixels, RRect, px},
    painting::{Path, TextureId},
};
use wgpu::util::DeviceExt;

use super::{
    pipeline::{self, PipelineCache, PipelineKey},
    tessellator::Tessellator,
    text::TextRenderer,
    vertex::Vertex,
};

/// A recorded batch of tessellated geometry sharing the same pipeline key.
///
/// During a frame, each call to [`WgpuPainter::add_tessellated`] appends
/// vertices/indices to the global buffers.  When the pipeline key changes
/// a new batch is started so that the render pass can switch pipelines at
/// the correct index boundary.
/// Scissor rect type (x, y, width, height) in physical pixels.
type ScissorRect = Option<(u32, u32, u32, u32)>;

/// Tracks a sub-range of instances that share the same scissor state.
/// Used to split instanced draw calls when clipping changes.
#[derive(Debug, Clone)]
struct ScissorRegion {
    scissor: ScissorRect,
    start: u32,
    count: u32,
}

#[derive(Debug, Clone)]
struct TessellatedBatch {
    /// Pipeline variant to use for this batch
    pipeline_key: PipelineKey,
    /// Scissor rect active when this batch was recorded
    scissor: ScissorRect,
    /// First index (inclusive) into the shared index buffer
    index_start: u32,
    /// Number of indices in this batch
    index_count: u32,
}

/// GPU painter for hardware-accelerated 2D rendering
///
/// Batches all drawing operations per frame for efficient GPU rendering.
/// Supports shapes, text, transforms, and clipping.
///
/// # Example
/// ```ignore
/// let mut painter = WgpuPainter::new(device, queue, surface_format, (800, 600));
///
/// painter.rect(Rect::from_ltrb(10.0, 10.0, 100.0, 100.0), &Paint::fill(Color::RED));
/// painter.text("Hello", Point::new(10.0, 120.0), 16.0, &Paint::fill(Color::BLACK));
///
/// painter.render(&view, &mut encoder)?;
/// ```
#[allow(missing_debug_implementations)]
/// A pending offscreen texture waiting to be composited into the main render target.
///
/// Created by [`WgpuPainter::queue_offscreen_result`] and consumed during
/// [`WgpuPainter::render`] after all other drawing is complete.
struct PendingOffscreenTexture {
    texture: super::texture_pool::PooledTexture,
    bounds: Rect<Pixels>,
}

/// Saved render state for `save_layer`/`restore_layer` offscreen compositing.
///
/// When `save_layer` is called, the current draw state is captured into this
/// struct and a fresh segment begins. All subsequent drawing goes into the new
/// segment. On `restore_layer`, the offscreen content is composited back onto
/// the parent surface with the layer's opacity applied as a group.
struct SavedLayer {
    /// Previous draw order (restored on pop)
    saved_draw_order: Vec<DrawItem>,
    /// Previous segment (restored on pop)
    saved_segment: DrawSegment,
    /// Previous opacity stack (restored on pop)
    saved_opacity_stack: Vec<f32>,
    /// Previous accumulated opacity (restored on pop)
    saved_opacity: f32,
    /// Opacity to apply when compositing the offscreen layer
    layer_opacity: f32,
    /// Bounds of the layer in screen space [x, y, w, h], or None for full viewport
    bounds: Option<[f32; 4]>,
}

/// A segment of draw commands that share the same rendering phase ordering.
///
/// When an offscreen texture is queued, the current segment is finalized and
/// a new one starts. This ensures that content drawn before the offscreen
/// texture renders before it, and content drawn after renders after it,
/// preserving correct Z-order.
struct DrawSegment {
    /// Rectangle instance batch
    rect_batch: super::instancing::InstanceBatch<super::instancing::RectInstance>,
    /// Circle instance batch
    circle_batch: super::instancing::InstanceBatch<super::instancing::CircleInstance>,
    /// Arc instance batch
    arc_batch: super::instancing::InstanceBatch<super::instancing::ArcInstance>,
    /// Shadow instance batch
    shadow_batch: super::instancing::InstanceBatch<super::instancing::ShadowInstance>,
    /// Linear gradient instance batch
    linear_gradient_batch:
        super::instancing::InstanceBatch<super::instancing::LinearGradientInstance>,
    /// Radial gradient instance batch
    radial_gradient_batch:
        super::instancing::InstanceBatch<super::instancing::RadialGradientInstance>,
    /// Sweep gradient instance batch
    sweep_gradient_batch:
        super::instancing::InstanceBatch<super::instancing::SweepGradientInstance>,
    /// Accumulated gradient stops for this segment
    current_gradient_stops: Vec<super::effects::GradientStop>,
    /// Batched vertices for tessellation path
    vertices: Vec<Vertex>,
    /// Batched indices for tessellation path
    indices: Vec<u32>,
    /// Recorded tessellated batches for this segment
    tess_batches: Vec<TessellatedBatch>,
    /// Current pipeline key (for batching draws with same pipeline)
    current_pipeline_key: Option<PipelineKey>,
    /// Scissor regions for rect instanced batch
    rect_scissors: Vec<ScissorRegion>,
    /// Scissor regions for circle instanced batch
    circle_scissors: Vec<ScissorRegion>,
    /// Scissor regions for arc instanced batch
    arc_scissors: Vec<ScissorRegion>,
    /// Scissor regions for linear gradient batch
    linear_grad_scissors: Vec<ScissorRegion>,
    /// Scissor regions for radial gradient batch
    radial_grad_scissors: Vec<ScissorRegion>,
    /// Scissor regions for sweep gradient batch
    sweep_grad_scissors: Vec<ScissorRegion>,
    /// Cached image draws queued for this segment.
    cached_images: Vec<(
        super::texture_cache::TextureId,
        super::instancing::TextureInstance,
    )>,
}

impl DrawSegment {
    /// Create an empty draw segment with pre-allocated batch capacities.
    fn new() -> Self {
        Self {
            rect_batch: super::instancing::InstanceBatch::new(1024),
            circle_batch: super::instancing::InstanceBatch::new(1024),
            arc_batch: super::instancing::InstanceBatch::new(1024),
            shadow_batch: super::instancing::InstanceBatch::new(1024),
            linear_gradient_batch: super::instancing::InstanceBatch::new(512),
            radial_gradient_batch: super::instancing::InstanceBatch::new(512),
            sweep_gradient_batch: super::instancing::InstanceBatch::new(512),
            current_gradient_stops: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            tess_batches: Vec::new(),
            current_pipeline_key: None,
            rect_scissors: Vec::new(),
            circle_scissors: Vec::new(),
            arc_scissors: Vec::new(),
            linear_grad_scissors: Vec::new(),
            radial_grad_scissors: Vec::new(),
            sweep_grad_scissors: Vec::new(),
            cached_images: Vec::new(),
        }
    }

    /// Record an instance addition for a given scissor region tracker.
    /// Extends the last region if scissor matches, or creates a new one.
    fn push_scissor_region(regions: &mut Vec<ScissorRegion>, scissor: ScissorRect) {
        if let Some(last) = regions.last_mut()
            && last.scissor == scissor
        {
            last.count += 1;
            return;
        }
        regions.push(ScissorRegion {
            scissor,
            start: regions.last().map_or(0, |r| r.start + r.count),
            count: 1,
        });
    }

    /// Returns `true` if this segment has no drawing commands.
    fn is_empty(&self) -> bool {
        self.rect_batch.is_empty()
            && self.circle_batch.is_empty()
            && self.arc_batch.is_empty()
            && self.shadow_batch.is_empty()
            && self.linear_gradient_batch.is_empty()
            && self.radial_gradient_batch.is_empty()
            && self.sweep_gradient_batch.is_empty()
            && self.vertices.is_empty()
            && self.tess_batches.is_empty()
            && self.cached_images.is_empty()
    }
}

/// An item in the draw order list, either a segment of batched commands
/// or an offscreen texture to composite.
enum DrawItem {
    /// A segment of instanced/tessellated/gradient draw commands.
    Segment(DrawSegment),
    /// An offscreen texture to composite at its bounds.
    OffscreenTexture(PendingOffscreenTexture),
    /// An opacity layer: a group of draw items to render offscreen and composite
    /// with the given alpha. Created by `save_layer`/`restore_layer`.
    OpacityLayer(PendingOpacityLayer),
}

/// A pending opacity layer waiting to be rendered offscreen and composited.
///
/// Created by [`WgpuPainter::restore_layer`] when opacity < 1.0.
/// During [`WgpuPainter::render`], the contained segments are flushed to a
/// pooled offscreen texture, then that texture is composited onto the main
/// surface with the layer opacity applied as tint alpha.
struct PendingOpacityLayer {
    /// Draw items accumulated between save_layer and restore_layer
    items: Vec<DrawItem>,
    /// Final segment at the time of restore_layer (may have content)
    final_segment: DrawSegment,
    /// Group opacity to apply during compositing (0.0-1.0)
    opacity: f32,
    /// Compositing bounds in screen coordinates
    bounds: Rect<Pixels>,
}

/// GPU painter for wgpu-based rendering.
///
/// Manages instanced batching, tessellation, text rendering, and offscreen compositing.
pub struct WgpuPainter {
    // ===== GPU State =====
    /// wgpu device (Arc for sharing with text renderer)
    device: Arc<wgpu::Device>,

    /// wgpu queue (Arc for sharing with text renderer)
    queue: Arc<wgpu::Queue>,

    /// Surface texture format (needed for offscreen pipeline creation)
    surface_format: wgpu::TextureFormat,

    /// Viewport size (width, height)
    size: (u32, u32),

    // ===== Buffer Management =====
    /// Buffer pool for efficient buffer reuse (10-20% CPU reduction)
    buffer_pool: super::buffer_pool::BufferPool,

    // ===== Shape Rendering =====
    /// Pipeline cache for specialized rendering pipelines
    pipeline_cache: PipelineCache,

    // ===== Instanced Rendering =====
    /// Instanced rectangle pipeline (100x faster for UI)
    instanced_rect_pipeline: wgpu::RenderPipeline,

    /// Viewport uniform buffer (for instanced shader)
    viewport_buffer: wgpu::Buffer,

    /// Viewport bind group (for instanced pipelines)
    viewport_bind_group: wgpu::BindGroup,

    /// Shared unit quad vertex buffer (reused for all instances)
    unit_quad_buffer: wgpu::Buffer,

    /// Shared unit quad index buffer
    unit_quad_index_buffer: wgpu::Buffer,

    /// Instanced circle pipeline (100x faster for UI)
    instanced_circle_pipeline: wgpu::RenderPipeline,

    /// Instanced arc pipeline (100x faster for progress indicators)
    instanced_arc_pipeline: wgpu::RenderPipeline,

    /// Instanced texture pipeline (100x faster for images/icons)
    instanced_texture_pipeline: wgpu::RenderPipeline,

    /// Texture instance batch
    texture_batch: super::instancing::InstanceBatch<super::instancing::TextureInstance>,

    /// Texture bind group layout (for texture + sampler)
    texture_bind_group_layout: wgpu::BindGroupLayout,

    // ===== Advanced Effects =====
    /// Linear gradient pipeline (GPU-accelerated gradients)
    linear_gradient_pipeline: wgpu::RenderPipeline,

    /// Radial gradient pipeline (GPU-accelerated radial gradients)
    radial_gradient_pipeline: wgpu::RenderPipeline,
    sweep_gradient_pipeline: wgpu::RenderPipeline,

    /// Shadow pipeline (analytical shadows with single-pass rendering)
    shadow_pipeline: wgpu::RenderPipeline,

    /// Gradient stops storage buffer (shared for all gradients)
    gradient_stops_buffer: wgpu::Buffer,

    /// Gradient stops bind group layout
    gradient_bind_group_layout: wgpu::BindGroupLayout,

    /// Current gradient stops bind group (recreated when stops change)
    gradient_bind_group: Option<wgpu::BindGroup>,

    /// Default texture sampler (linear filtering with repeat)
    default_sampler: wgpu::Sampler,

    /// Texture cache for efficient texture loading and reuse
    texture_cache: super::texture_cache::TextureCache,

    /// External texture registry for video/camera/platform textures
    external_texture_registry: super::external_texture_registry::ExternalTextureRegistry,

    // ===== Tessellation =====
    /// Lyon-based path tessellator for complex shapes
    tessellator: Tessellator,

    /// Cache for tessellated path geometry (avoids re-tessellation of identical paths)
    path_cache: super::path_cache::PathCache,

    /// Cache for tessellated superellipse (iOS-squircle) paths.
    ///
    /// Mirrors [`PathCache`](super::path_cache::PathCache) ownership: per-
    /// Painter, single-threaded, with `max_entries` + frame-based eviction.
    /// Replaces the previously-unbounded `thread_local!` cache in
    /// `layer_render.rs`. Consulted by `Backend::superellipse_path`
    /// override; the trait default for non-Painter backends regenerates
    /// without caching.
    superellipse_cache: super::superellipse_cache::SuperellipsePathCache,

    // ===== Text Rendering =====
    /// Glyphon-based text renderer
    text_renderer: TextRenderer,

    // ===== Transform Stack =====
    /// Stack of saved transforms
    transform_stack: Vec<glam::Mat4>,

    /// Current active transform
    current_transform: glam::Mat4,

    // ===== Clipping =====
    /// Stack of scissor rectangles for axis-aligned clipping
    /// Each element is (x, y, width, height) in physical pixels
    scissor_stack: Vec<(u32, u32, u32, u32)>,

    /// Current active scissor rect (None = no clipping)
    current_scissor: Option<(u32, u32, u32, u32)>,

    // ===== SDF Clip Stack =====
    /// Stack of SDF rounded rectangle clip regions.
    /// Each element is `[x, y, width, height, radius_tl, radius_tr, radius_br, radius_bl]`.
    /// Used to restore clip state on `restore()`.
    rrect_clip_stack: Vec<[f32; 8]>,

    /// SDF rsuperellipse clip stack (for save/restore).
    ///
    /// Stack of `[f32; 12]` superellipse clip uniforms — 4 floats outer
    /// rect (x, y, w, h) + 8 floats per-corner radii (rx/ry per 4 corners
    /// in tl, tr, br, bl order). Mirrors the `rrect_clip_stack` shape with
    /// a wider tuple to carry the per-corner separate-axis radii.
    rsuperellipse_clip_stack: Vec<[f32; 12]>,

    /// Current SDF rounded rectangle clip.
    /// All zeros means no clip is active.
    /// When non-zero, each new instance gets this clip data so the fragment
    /// shader can perform per-pixel SDF clipping without a stencil buffer.
    current_rrect_clip: [f32; 8],

    /// Active SDF rsuperellipse clip uniform (zero when no clip is active).
    ///
    /// Tuple layout: indices 0-3 = outer rect `(x, y, w, h)`; indices 4-11
    /// = per-corner radii `(tl_x, tl_y, tr_x, tr_y, br_x, br_y, bl_x, bl_y)`.
    /// Exponent `n = 4` is hardcoded in the WGSL shader, not stored here.
    current_rsuperellipse_clip: [f32; 12],

    // ===== Opacity/Layer Stack =====
    /// Stack of opacity values for save_layer/restore_layer
    /// Each element is the accumulated alpha (0.0-1.0)
    opacity_stack: Vec<f32>,

    /// Current accumulated opacity (1.0 = fully opaque)
    current_opacity: f32,

    // ===== Layer Compositing =====
    /// Stack of saved render state for save_layer/restore_layer.
    /// Each entry captures the draw state at the time of save_layer so that
    /// the subtree can be rendered to an offscreen texture and composited
    /// with group opacity.
    layer_stack: Vec<SavedLayer>,

    /// Texture pool for offscreen layer compositing (opacity layers).
    /// Textures are acquired when `restore_layer` flushes offscreen content
    /// and returned to the pool when the `PooledTexture` RAII handle is dropped.
    layer_texture_pool: super::texture_pool::TexturePool,

    // ===== Segmented Draw Order =====
    /// Current draw segment accumulating batched commands
    current_segment: DrawSegment,

    /// Ordered list of completed draw items (segments and offscreen textures)
    draw_order: Vec<DrawItem>,
}

// GPU rendering routinely converts between numeric types for pixel coordinates,
// color channels, buffer indices, and instance counts.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl WgpuPainter {
    /// Create a new GPU painter
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `queue` - wgpu queue
    /// * `surface_format` - Surface texture format
    /// * `size` - Initial viewport size (width, height)
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        Self::with_shared_device(Arc::new(device), Arc::new(queue), surface_format, size)
    }

    /// Create a WgpuPainter with shared device and queue.
    ///
    /// Use this when the device/queue are already wrapped in Arc
    /// (e.g., shared with Renderer).
    pub fn with_shared_device(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::new: format={:?}, size=({}, {})",
            surface_format,
            size.0,
            size.1
        );

        // ===== Viewport Setup (shared by all pipelines) =====

        // Create viewport uniform buffer
        let viewport_data = [size.0 as f32, size.1 as f32, 0.0, 0.0]; // [width, height, padding, padding]
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Viewport Uniform Buffer"),
            contents: bytemuck::cast_slice(&viewport_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind group layout for viewport (will be owned by PipelineCache)
        let viewport_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Viewport Bind Group Layout"),
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

        // Create pipeline cache FIRST - it will own the layout
        let pipeline_cache = PipelineCache::new(
            &device,
            super::shaders::SHAPE,
            surface_format,
            viewport_bind_group_layout,
        );

        // Now create bind group using layout FROM pipeline_cache
        // This ensures the bind group uses the EXACT SAME layout object as shape
        // pipeline
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Viewport Bind Group"),
            layout: pipeline_cache.viewport_bind_group_layout(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        // ===== Instanced Rendering Setup =====

        // Create instanced rectangle shader
        let instanced_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Instanced Rect Shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::RECT_INSTANCED.into()),
        });

        // Create instanced rectangle pipeline
        let instanced_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Instanced Rect Pipeline Layout"),
                bind_group_layouts: &[Some(pipeline_cache.viewport_bind_group_layout())],
                immediate_size: 0,
            });

        let instanced_rect_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Instanced Rect Pipeline"),
                layout: Some(&instanced_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &instanced_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        // Vertex buffer (shared unit quad)
                        wgpu::VertexBufferLayout {
                            array_stride: 8, // 2 floats (vec2)
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                        },
                        // Instance buffer
                        super::instancing::RectInstance::desc(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &instanced_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
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
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
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

        // Create shared unit quad vertex buffer (0,0 to 1,1)
        #[rustfmt::skip]
        let unit_quad_vertices: &[f32] = &[
            0.0, 0.0,  // Top-left
            1.0, 0.0,  // Top-right
            1.0, 1.0,  // Bottom-right
            0.0, 1.0,  // Bottom-left
        ];

        let unit_quad_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Unit Quad Vertex Buffer"),
            contents: bytemuck::cast_slice(unit_quad_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create shared unit quad index buffer (2 triangles)
        let unit_quad_indices: &[u16] = &[
            0, 1, 2, // Triangle 1
            0, 2, 3, // Triangle 2
        ];

        let unit_quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Unit Quad Index Buffer"),
            contents: bytemuck::cast_slice(unit_quad_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // (Rectangle instance batch moved to DrawSegment)

        // ===== Circle Instanced Rendering Setup =====

        // Create instanced circle shader
        let circle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Instanced Circle Shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::CIRCLE_INSTANCED.into()),
        });

        // Create instanced circle pipeline (reuses viewport bind group layout)
        let instanced_circle_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Instanced Circle Pipeline"),
                layout: Some(&instanced_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &circle_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        // Vertex buffer (shared unit quad)
                        wgpu::VertexBufferLayout {
                            array_stride: 8, // 2 floats (vec2)
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                        },
                        // Instance buffer
                        super::instancing::CircleInstance::desc(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &circle_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
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
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
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

        // (Circle instance batch moved to DrawSegment)

        // ===== Arc Instanced Rendering Setup =====

        // Create instanced arc shader
        let arc_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Instanced Arc Shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::ARC_INSTANCED.into()),
        });

        // Create instanced arc pipeline (reuses viewport bind group layout)
        let instanced_arc_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Instanced Arc Pipeline"),
                layout: Some(&instanced_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &arc_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        // Vertex buffer (shared unit quad)
                        wgpu::VertexBufferLayout {
                            array_stride: 8, // 2 floats (vec2)
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                        },
                        // Instance buffer
                        super::instancing::ArcInstance::desc(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &arc_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
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
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
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

        // (Arc instance batch moved to DrawSegment)

        // ===== Texture Instanced Rendering Setup =====

        // Create texture bind group layout
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Texture Bind Group Layout"),
                entries: &[
                    // Sampler
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    // Texture view
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

        // Create default sampler (linear filtering, repeat)
        let default_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Default Texture Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 100.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });

        // Create instanced texture shader
        let texture_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Instanced Texture Shader"),
            source: wgpu::ShaderSource::Wgsl(super::shaders::TEXTURE_INSTANCED.into()),
        });

        // Create texture pipeline layout
        let texture_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Instanced Texture Pipeline Layout"),
                bind_group_layouts: &[
                    Some(pipeline_cache.viewport_bind_group_layout()),
                    Some(&texture_bind_group_layout),
                ],
                immediate_size: 0,
            });

        // Create instanced texture pipeline
        let instanced_texture_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Instanced Texture Pipeline"),
                layout: Some(&texture_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &texture_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        // Vertex buffer (shared unit quad)
                        wgpu::VertexBufferLayout {
                            array_stride: 8, // 2 floats (vec2)
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                        },
                        // Instance buffer
                        super::instancing::TextureInstance::desc(),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &texture_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
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
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
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

        // Create texture instance batch
        let texture_batch = super::instancing::InstanceBatch::new(1024); // 1024 textures per batch

        // Create tessellator for complex shapes
        let tessellator = Tessellator::new();

        // Create text renderer
        let text_renderer = TextRenderer::new(&device, &queue, surface_format);

        // Initialize transform stack with identity
        let current_transform = glam::Mat4::IDENTITY;
        let transform_stack = Vec::new();

        // Create buffer pool for efficient buffer reuse
        let buffer_pool = super::buffer_pool::BufferPool::new();

        // ===== Advanced Effects Setup =====

        // Create gradient stops buffer and bind group layout
        let gradient_stops_buffer = super::effects_pipeline::create_gradient_stops_buffer(&device);
        let gradient_bind_group_layout =
            super::effects_pipeline::create_gradient_bind_group_layout(&device);

        // Create shared pipeline layout for all gradient pipelines
        let gradient_pipeline_layout = super::effects_pipeline::create_gradient_pipeline_layout(
            &device,
            pipeline_cache.viewport_bind_group_layout(),
            &gradient_bind_group_layout,
        );

        // Create gradient pipelines (all share the same PipelineLayout for bind group compatibility)
        let linear_gradient_pipeline = super::effects_pipeline::create_linear_gradient_pipeline(
            &device,
            surface_format,
            &gradient_pipeline_layout,
        );
        let radial_gradient_pipeline = super::effects_pipeline::create_radial_gradient_pipeline(
            &device,
            surface_format,
            &gradient_pipeline_layout,
        );
        let sweep_gradient_pipeline = super::effects_pipeline::create_sweep_gradient_pipeline(
            &device,
            surface_format,
            &gradient_pipeline_layout,
        );

        // (Sweep gradient batch moved to DrawSegment)

        // Create shadow pipeline
        let shadow_pipeline = super::effects_pipeline::create_shadow_pipeline(
            &device,
            surface_format,
            pipeline_cache.viewport_bind_group_layout(),
        );

        // (Shadow batch moved to DrawSegment)

        // No bind group yet (created on first gradient use)
        let gradient_bind_group = None;

        // Create texture cache (uses Arc for safe sharing)
        let texture_cache = super::texture_cache::TextureCache::new(device.clone(), queue.clone());

        // Create external texture registry for video/camera/platform textures
        let external_texture_registry =
            super::external_texture_registry::ExternalTextureRegistry::new(device.clone());

        // Create texture pool for opacity layer offscreen compositing
        let layer_texture_pool =
            super::texture_pool::TexturePool::with_capacity(Arc::clone(&device), 4);

        Self {
            device,
            queue,
            surface_format,
            size,
            buffer_pool,
            pipeline_cache,
            instanced_rect_pipeline,
            viewport_buffer,
            viewport_bind_group,
            unit_quad_buffer,
            unit_quad_index_buffer,
            instanced_circle_pipeline,
            instanced_arc_pipeline,
            instanced_texture_pipeline,
            texture_batch,
            texture_bind_group_layout,
            linear_gradient_pipeline,
            radial_gradient_pipeline,
            sweep_gradient_pipeline,
            shadow_pipeline,
            gradient_stops_buffer,
            gradient_bind_group_layout,
            gradient_bind_group,
            default_sampler,
            texture_cache,
            external_texture_registry,
            tessellator,
            path_cache: super::path_cache::PathCache::new(512),
            superellipse_cache: super::superellipse_cache::SuperellipsePathCache::new(256),
            text_renderer,
            transform_stack,
            current_transform,
            scissor_stack: Vec::new(),
            current_scissor: None,
            rrect_clip_stack: Vec::new(),
            current_rrect_clip: [0.0; 8],
            rsuperellipse_clip_stack: Vec::new(),
            current_rsuperellipse_clip: [0.0; 12],
            opacity_stack: Vec::new(),
            current_opacity: 1.0,
            layer_stack: Vec::new(),
            layer_texture_pool,
            current_segment: DrawSegment::new(),
            draw_order: Vec::new(),
        }
    }

    // ===== Accessors =====

    /// Returns a reference to the wgpu device.
    #[must_use]
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// Returns a reference to the wgpu queue.
    #[must_use]
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    /// Returns the surface texture format.
    #[must_use]
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    // ===== Frame Lifecycle =====

    /// Reset all per-frame clip/transform/opacity/layer state to pristine values.
    ///
    /// Must be called at the **start** of every frame, before any damage scissor
    /// or other per-frame setup, so that state from frame N is never visible in
    /// frame N+1.
    ///
    /// Without this call the damage-scissor that was intersected into
    /// `current_scissor` during a partial-damage frame leaks into the next
    /// frame, causing full-repaint frames to silently clip to the previous
    /// damage rect.
    pub fn reset_frame_state(&mut self) {
        self.current_scissor = None;
        self.scissor_stack.clear();
        self.current_rrect_clip = [0.0; 8];
        self.rrect_clip_stack.clear();
        self.current_rsuperellipse_clip = [0.0; 12];
        self.rsuperellipse_clip_stack.clear();
        self.current_opacity = 1.0;
        self.opacity_stack.clear();
        self.layer_stack.clear();
        // Identity is the construction-time value (see `new()`: `let current_transform =
        // glam::Mat4::IDENTITY`). Reset to the same initial value.
        self.current_transform = glam::Mat4::IDENTITY;
        self.transform_stack.clear();

        tracing::trace!("WgpuPainter::reset_frame_state: per-frame state cleared");
    }

    /// Returns the current scissor rect for testing purposes.
    ///
    /// Gated to match its sole consumer (`reset_frame_state_clears_damage_scissor`)
    /// so it is never dead code in either build configuration.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn current_scissor_for_test(&self) -> Option<(u32, u32, u32, u32)> {
        self.current_scissor
    }

    // ===== Offscreen Compositing =====

    /// Queue an offscreen-rendered texture for compositing into the main render target.
    ///
    /// This finalizes the current draw segment and inserts the offscreen texture
    /// into the draw order. Content drawn before this call will render before
    /// the offscreen texture, and content drawn after will render after it,
    /// preserving correct Z-ordering.
    pub fn queue_offscreen_result(
        &mut self,
        texture: super::texture_pool::PooledTexture,
        bounds: Rect<Pixels>,
    ) {
        // Finalize the current segment and start a new one
        let segment = std::mem::replace(&mut self.current_segment, DrawSegment::new());
        if !segment.is_empty() {
            self.draw_order.push(DrawItem::Segment(segment));
        }
        self.draw_order
            .push(DrawItem::OffscreenTexture(PendingOffscreenTexture {
                texture,
                bounds,
            }));
    }

    /// Re-integrate offscreen draw content back into the parent draw order.
    ///
    /// This is the fallback path used when full offscreen render-to-texture
    /// compositing is not yet available. It simply appends the offscreen
    /// segments and draw items back into the current draw order.
    ///
    /// When `_opacity` < 1.0, this produces incorrect results for overlapping
    /// children (each child gets independent alpha instead of the group being
    /// composited as a unit), but it preserves the existing behavior until
    /// the full offscreen path is implemented.
    fn reintegrate_offscreen_content(
        &mut self,
        offscreen_segment: DrawSegment,
        offscreen_order: Vec<DrawItem>,
        _opacity: f32,
    ) {
        // Merge the offscreen draw items into the parent draw order.
        // The offscreen_order items were recorded between save_layer and restore_layer.
        for item in offscreen_order {
            self.draw_order.push(item);
        }
        // Append the final segment if it has content
        if !offscreen_segment.is_empty() {
            self.draw_order.push(DrawItem::Segment(offscreen_segment));
        }
    }

    /// Render all batched geometry to a texture view
    ///
    /// This should be called once per frame after all drawing operations.
    /// Draw items are rendered in the order they were recorded, with offscreen
    /// textures interleaved at the correct Z-position.
    ///
    /// # Arguments
    /// * `view` - Texture view to render to
    /// * `encoder` - Command encoder
    #[tracing::instrument(level = "trace", skip_all)]
    #[must_use = "errors must be propagated or handled"]
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> crate::error::EngineResult<()> {
        // Advance path cache frame counters and evict stale entries
        self.path_cache.advance_frame();
        self.superellipse_cache.advance_frame();

        // Log rendering stats
        let text_count = self.text_renderer.text_count();
        let rect_count = self.current_segment.rect_batch.len();
        let circle_count = self.current_segment.circle_batch.len();
        let buffer_stats = self.buffer_pool.stats();

        tracing::trace!(
            vertices = self.current_segment.vertices.len(),
            indices = self.current_segment.indices.len(),
            text_count,
            rects = rect_count,
            circles = circle_count,
            segments = self.draw_order.len(),
            cache_hit_rate = format!("{:.0}%", buffer_stats.reuse_rate * 100.0),
            "Drawing commands"
        );

        // ===== Finalize current segment =====
        let final_segment = std::mem::replace(&mut self.current_segment, DrawSegment::new());
        if !final_segment.is_empty() {
            self.draw_order.push(DrawItem::Segment(final_segment));
        }

        // ===== Process draw items in order =====
        let items: Vec<DrawItem> = self.draw_order.drain(..).collect();
        for item in items {
            match item {
                DrawItem::Segment(mut seg) => {
                    self.flush_segment(&mut seg, encoder, view);
                }
                DrawItem::OffscreenTexture(p) => {
                    let instance = super::instancing::TextureInstance::new(
                        p.bounds,
                        flui_types::styling::Color::WHITE,
                    );
                    let _ = self.texture_batch.add(instance);
                    self.flush_texture_batch(encoder, view, p.texture.view());
                    // p.texture dropped here, returns to pool
                }
                DrawItem::OpacityLayer(layer) => {
                    self.flush_opacity_layer(layer, encoder, view);
                }
            }
        }

        // ===== Render Text (global - always on top) =====
        self.text_renderer
            .render(&self.device, &self.queue, view, encoder, self.size)?;

        // ===== Reset buffer pool for next frame =====
        self.buffer_pool.reset();

        // NOTE: texture-cache maintenance is intentionally NOT done here.
        // `render` runs multiple times per frame — each backdrop-filter flush
        // (backend.rs / renderer.rs) plus the final flush — on the SAME cache.
        // Resetting use-counters here would mis-classify textures used in an
        // earlier pass as unused and evict / atlas-reset them mid-frame. The
        // Renderer calls `end_frame_maintenance` exactly once per frame instead.

        Ok(())
    }

    /// Run end-of-frame texture-cache maintenance: evict over-budget textures,
    /// reclaim a full atlas that holds stale entries, then reset use-counters.
    ///
    /// Call EXACTLY ONCE per frame, after the final [`Self::render`] flush.
    /// `render` must not do this itself — it runs once per pass (backdrop-filter
    /// flushes invoke it mid-frame), so per-call maintenance would reset
    /// use-counters between passes and drop textures still in use this frame.
    pub fn end_frame_maintenance(&mut self) {
        let maint = self.texture_cache.end_frame_maintenance();
        if maint.evicted > 0 || maint.atlas_reset {
            tracing::debug!(
                evicted = maint.evicted,
                atlas_reset = maint.atlas_reset,
                memory_bytes = self.texture_cache.memory_bytes(),
                "Texture cache maintenance"
            );
        }
    }

    /// Flush a single draw segment by temporarily swapping it into current_segment
    /// and calling the existing flush methods.
    ///
    /// This avoids refactoring all flush methods to accept segment parameters.
    fn flush_segment(
        &mut self,
        seg: &mut DrawSegment,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        // Swap segment data into current_segment temporarily
        std::mem::swap(&mut self.current_segment, seg);

        // Flush order optimized to minimize GPU pipeline switches:
        // 1. Instanced primitives (rect, circle, arc, shadow) - similar pipelines
        // 2. Gradient primitives (linear, radial, sweep) - similar pipelines
        // 3. Tessellated geometry - different pipeline type
        // 4. Segment-cached images - grouped by texture while preserving draw order
        self.flush_all_instanced_batches(encoder, view);
        self.flush_gradient_batches(encoder, view);
        self.flush_tessellated_geometry(encoder, view);
        self.flush_segment_cached_images(encoder, view);

        // Swap back (now empty after flush)
        std::mem::swap(&mut self.current_segment, seg);
    }

    /// Flush an opacity layer by rendering its content to an offscreen texture
    /// and compositing the result onto the main surface with group opacity.
    ///
    /// This implements correct group opacity: all children within the layer
    /// are first rendered to an offscreen texture at full opacity, then the
    /// entire texture is composited with the layer's alpha. This avoids the
    /// incorrect per-primitive alpha blending that occurs when overlapping
    /// children each have independent alpha applied.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn flush_opacity_layer(
        &mut self,
        mut layer: PendingOpacityLayer,
        encoder: &mut wgpu::CommandEncoder,
        main_view: &wgpu::TextureView,
    ) {
        // Use the full viewport size for the offscreen texture.
        // Segments contain viewport-space coordinates, so using the full viewport
        // avoids coordinate translation. The transparent clear ensures only the
        // actually-drawn region contributes to the composite.
        let (vp_w, vp_h) = self.size;
        if vp_w == 0 || vp_h == 0 {
            return;
        }

        // Acquire a pooled offscreen texture
        let offscreen = self
            .layer_texture_pool
            .acquire(vp_w, vp_h, self.surface_format);
        let offscreen_view = offscreen.view();

        // Clear the offscreen texture to fully transparent
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Opacity Layer Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: offscreen_view,
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
            // Pass dropped immediately — just clearing
        }

        // Flush all inner draw items to the offscreen texture
        for item in layer.items.drain(..) {
            match item {
                DrawItem::Segment(mut seg) => {
                    self.flush_segment(&mut seg, encoder, offscreen_view);
                }
                DrawItem::OffscreenTexture(p) => {
                    let instance = super::instancing::TextureInstance::new(
                        p.bounds,
                        flui_types::styling::Color::WHITE,
                    );
                    let _ = self.texture_batch.add(instance);
                    self.flush_texture_batch(encoder, offscreen_view, p.texture.view());
                }
                DrawItem::OpacityLayer(nested) => {
                    // Recursively handle nested opacity layers
                    self.flush_opacity_layer(nested, encoder, offscreen_view);
                }
            }
        }

        // Flush the final segment (content drawn after the last draw order item)
        if !layer.final_segment.is_empty() {
            self.flush_segment(&mut layer.final_segment, encoder, offscreen_view);
        }

        // Composite the offscreen texture onto the main surface with group opacity.
        // The tint color is white with the layer opacity as alpha, so the texture
        // shader multiplies every texel by that alpha value.
        let alpha_u8 = (layer.opacity.clamp(0.0, 1.0) * 255.0) as u8;
        let tint = flui_types::styling::Color::rgba(255, 255, 255, alpha_u8);

        // Use layer bounds as the destination rect for compositing.
        // The UV coordinates map the bounds region from the full-viewport texture.
        let uv_left = layer.bounds.left().0 / vp_w as f32;
        let uv_top = layer.bounds.top().0 / vp_h as f32;
        let uv_right = layer.bounds.right().0 / vp_w as f32;
        let uv_bottom = layer.bounds.bottom().0 / vp_h as f32;

        let instance = super::instancing::TextureInstance::with_uv(
            layer.bounds,
            [uv_left, uv_top, uv_right, uv_bottom],
            tint,
        );
        let _ = self.texture_batch.add(instance);
        self.flush_texture_batch(encoder, main_view, offscreen_view);

        tracing::trace!(
            opacity = layer.opacity,
            bounds = ?layer.bounds,
            "Composited opacity layer"
        );

        // offscreen texture returned to pool when `offscreen` is dropped here
    }

    /// Flush tessellated geometry from the current segment.
    ///
    /// Uploads vertices/indices and renders all recorded tessellated batches
    /// in a single render pass, switching pipelines as needed.
    fn flush_tessellated_geometry(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        if self.current_segment.vertices.is_empty() || self.current_segment.tess_batches.is_empty()
        {
            return;
        }

        // Upload vertices and indices to GPU (using buffer pool for zero-copy reuse)
        let (vertex_buffer, index_buffer) = self.buffer_pool.get_vertex_and_index_buffers(
            &self.device,
            &self.queue,
            "Shape Vertex Buffer",
            bytemuck::cast_slice(&self.current_segment.vertices),
            "Shape Index Buffer",
            bytemuck::cast_slice(&self.current_segment.indices),
        );

        // Render shapes in single pass, switching pipelines per batch
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shape Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        let mut active_key: Option<PipelineKey> = None;
        for batch in &self.current_segment.tess_batches {
            if active_key != Some(batch.pipeline_key) {
                let pipeline = self
                    .pipeline_cache
                    .get_or_create(&self.device, batch.pipeline_key);
                render_pass.set_pipeline(pipeline);
                active_key = Some(batch.pipeline_key);
            }

            // Set per-batch scissor rect
            if let Some((x, y, w, h)) = batch.scissor {
                render_pass.set_scissor_rect(x, y, w, h);
            } else {
                render_pass.set_scissor_rect(0, 0, self.size.0, self.size.1);
            }

            let start = batch.index_start;
            let end = start + batch.index_count;
            render_pass.draw_indexed(start..end, 0, 0..1);
        }

        // Drop render pass
        drop(render_pass);

        // Clear tessellated data
        self.current_segment.vertices.clear();
        self.current_segment.indices.clear();
        self.current_segment.tess_batches.clear();
        self.current_segment.current_pipeline_key = None;
    }

    /// Returns the current viewport size as `(width, height)`.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Resize the viewport
    ///
    /// Call this when the window is resized.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.size = (width, height);

        // Update viewport uniform buffer for instanced rendering
        let viewport_data = [width as f32, height as f32, 0.0, 0.0];
        self.queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::cast_slice(&viewport_data),
        );
    }

    /// Returns the current save stack depth.
    ///
    /// This is useful for tracking how many `save()` calls have been made
    /// so that the corresponding number of `restore()` calls can be issued.
    pub fn save_count(&self) -> usize {
        self.transform_stack.len()
    }

    // ===== External Texture Registry Access =====

    /// Get a reference to the external texture registry
    ///
    /// Use this to register external textures (video frames, camera preview,
    /// etc.) that can be rendered via `Canvas::draw_texture()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::painting::TextureId;
    ///
    /// let texture_id = TextureId::new(42);
    /// painter.external_texture_registry()
    ///     .register(texture_id, gpu_texture, 1920, 1080, true, true);
    /// ```
    pub fn external_texture_registry(
        &self,
    ) -> &super::external_texture_registry::ExternalTextureRegistry {
        &self.external_texture_registry
    }

    /// Get a mutable reference to the external texture registry
    ///
    /// Use this to register, update, or unregister external textures.
    pub fn external_texture_registry_mut(
        &mut self,
    ) -> &mut super::external_texture_registry::ExternalTextureRegistry {
        &mut self.external_texture_registry
    }

    // ===== Helper Methods =====

    /// Apply current transform to a point
    fn apply_transform(&self, point: Point<Pixels>) -> Point<Pixels> {
        let p = self.current_transform * glam::vec4(point.x.0, point.y.0, 0.0, 1.0);
        Point::new(px(p.x), px(p.y))
    }

    /// Check if the current transform is axis-aligned (no rotation/skew).
    /// When false, rects must be tessellated rather than instanced.
    fn is_axis_aligned_transform(&self) -> bool {
        let m = self.current_transform;
        // Off-diagonal elements of the 2D part must be ~zero
        m.x_axis.y.abs() < 1e-6 && m.y_axis.x.abs() < 1e-6
    }

    /// Add tessellated shape from vertices/indices with pipeline key tracking.
    ///
    /// When the requested `key` matches the current batch's pipeline key the
    /// indices are simply appended.  When the key differs a new
    /// [`TessellatedBatch`] is started so the render pass can switch pipelines
    /// at the correct boundary.
    fn add_tessellated_with_key(
        &mut self,
        vertices: Vec<Vertex>,
        indices: &[u32],
        key: PipelineKey,
    ) {
        if indices.is_empty() {
            return;
        }

        let base_index = self.current_segment.vertices.len() as u32;
        let index_start = self.current_segment.indices.len() as u32;

        // Add vertices (already transformed by tessellator if needed)
        self.current_segment.vertices.extend(vertices);

        // Add indices with offset
        self.current_segment
            .indices
            .extend(indices.iter().map(|&i| i + base_index));

        let index_count = indices.len() as u32;

        // Try to extend the current batch if pipeline key AND scissor match
        if let Some(last) = self.current_segment.tess_batches.last_mut()
            && last.pipeline_key == key
            && last.scissor == self.current_scissor
        {
            last.index_count += index_count;
            return;
        }

        // Pipeline key changed (or first batch) — start a new batch
        self.current_segment.current_pipeline_key = Some(key);
        self.current_segment.tess_batches.push(TessellatedBatch {
            pipeline_key: key,
            scissor: self.current_scissor,
            index_start,
            index_count,
        });
    }

    /// Add tessellated shape using alpha-blend pipeline (convenience wrapper).
    ///
    /// Used by callers that don't have a [`Paint`] reference (e.g.
    /// `draw_vertices`, `draw_shadow`).
    fn add_tessellated(&mut self, vertices: Vec<Vertex>, indices: &[u32]) {
        self.add_tessellated_with_key(vertices, indices, PipelineKey::alpha_blend());
    }

    /// Convert a `Shader` into GPU `GradientStop`s (max 8).
    fn shader_to_gradient_stops(
        shader: &flui_types::painting::Shader,
    ) -> Vec<super::effects::GradientStop> {
        let (colors, stops) = match shader {
            flui_types::painting::Shader::LinearGradient { colors, stops, .. }
            | flui_types::painting::Shader::RadialGradient { colors, stops, .. }
            | flui_types::painting::Shader::SweepGradient { colors, stops, .. } => {
                (colors.as_slice(), stops.as_deref())
            }
            flui_types::painting::Shader::Solid { color } => {
                return vec![
                    super::effects::GradientStop::new(*color, 0.0),
                    super::effects::GradientStop::new(*color, 1.0),
                ];
            }
            _ => return vec![],
        };

        let count = colors.len().min(8);
        (0..count)
            .map(|i| {
                let position = if let Some(s) = stops {
                    s.get(i)
                        .copied()
                        .unwrap_or(i as f32 / (count - 1).max(1) as f32)
                } else {
                    i as f32 / (count - 1).max(1) as f32
                };
                super::effects::GradientStop::new(colors[i], position)
            })
            .collect()
    }

    /// Dispatch a filled rect/rrect/circle with a shader to the correct gradient pipeline.
    /// Returns `true` if the shader was handled, `false` if it should fall through to solid color.
    fn dispatch_shader_rect(
        &mut self,
        bounds: Rect<Pixels>,
        paint: &Paint,
        corner_radii: [f32; 4],
    ) -> bool {
        let Some(shader) = &paint.shader else {
            return false;
        };

        let stops = Self::shader_to_gradient_stops(shader);
        if stops.is_empty() {
            return false;
        }

        // Apply current transform to bounds
        let top_left = self.apply_transform(Point::new(bounds.left(), bounds.top()));
        let bottom_right = self.apply_transform(Point::new(bounds.right(), bounds.bottom()));
        let transformed = Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

        match shader {
            flui_types::painting::Shader::LinearGradient { from, to, .. } => {
                // Convert Offset<Pixels> to local coordinates relative to bounds
                let start =
                    glam::Vec2::new(from.dx.0 - bounds.left().0, from.dy.0 - bounds.top().0);
                let end = glam::Vec2::new(to.dx.0 - bounds.left().0, to.dy.0 - bounds.top().0);
                self.gradient_rect(transformed, start, end, &stops, corner_radii[0]);
            }
            flui_types::painting::Shader::RadialGradient { center, radius, .. } => {
                let c =
                    glam::Vec2::new(center.dx.0 - bounds.left().0, center.dy.0 - bounds.top().0);
                self.radial_gradient_rect(transformed, c, *radius, &stops, corner_radii[0]);
            }
            flui_types::painting::Shader::SweepGradient {
                center,
                start_angle,
                end_angle,
                ..
            } => {
                let c =
                    glam::Vec2::new(center.dx.0 - bounds.left().0, center.dy.0 - bounds.top().0);
                self.sweep_gradient_rect(
                    transformed,
                    c,
                    *start_angle,
                    *end_angle,
                    &stops,
                    corner_radii[0],
                );
            }
            flui_types::painting::Shader::Solid { color } => {
                // Just use the solid color directly — fall through
                let _ = color;
                return false;
            }
            _ => return false,
        }

        true
    }

    /// Flush all instanced batches using SINGLE render pass (Phase 9
    /// optimization)
    ///
    /// This method combines all instance data AND renders them in a SINGLE
    /// render pass by switching pipelines dynamically, reducing CPU
    /// overhead by an additional 2-3x.
    ///
    /// # Performance Impact
    ///
    /// **Before (Phase 8):** 1 buffer upload + 3 render passes + 3 draw calls
    /// **After (Phase 9):** 1 buffer upload + 1 render pass + 3 draw calls
    ///
    /// **Benefit:** Massive reduction in render pass overhead (3x fewer
    /// begin_render_pass calls)
    fn flush_all_instanced_batches(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        use super::multi_draw::{MultiDrawBatcher, PipelineId};

        // Check if we have any batches to flush
        let has_rects = !self.current_segment.rect_batch.is_empty();
        let has_circles = !self.current_segment.circle_batch.is_empty();
        let has_arcs = !self.current_segment.arc_batch.is_empty();
        let has_shadows = !self.current_segment.shadow_batch.is_empty();

        if !has_rects && !has_circles && !has_arcs && !has_shadows {
            return;
        }

        // Calculate instance data sizes
        let rect_size = self.current_segment.rect_batch.len()
            * std::mem::size_of::<super::instancing::RectInstance>();
        let circle_size = self.current_segment.circle_batch.len()
            * std::mem::size_of::<super::instancing::CircleInstance>();
        let arc_size = self.current_segment.arc_batch.len()
            * std::mem::size_of::<super::instancing::ArcInstance>();
        let shadow_size = self.current_segment.shadow_batch.len()
            * std::mem::size_of::<super::instancing::ShadowInstance>();

        // Build combined instance buffer
        // IMPORTANT: Shadows FIRST for correct z-ordering (background → foreground)
        let mut combined_buffer =
            Vec::with_capacity(shadow_size + rect_size + circle_size + arc_size);

        // Collect draw commands via MultiDrawBatcher
        let mut batcher = MultiDrawBatcher::new();

        // Append shadows first (render behind shapes)
        let shadow_offset = combined_buffer.len() as u64;
        if has_shadows {
            combined_buffer.extend_from_slice(self.current_segment.shadow_batch.as_bytes());
            batcher.add_quad_draw(
                PipelineId::Rectangle, // Shadow pipeline (rendered first for z-order)
                self.current_segment.shadow_batch.len() as u32,
                shadow_offset,
                shadow_size as u64,
            );
        }

        // Then append shapes (render on top of shadows)
        let rect_offset = combined_buffer.len() as u64;
        if has_rects {
            combined_buffer.extend_from_slice(self.current_segment.rect_batch.as_bytes());
            batcher.add_quad_draw(
                PipelineId::Rectangle,
                self.current_segment.rect_batch.len() as u32,
                rect_offset,
                rect_size as u64,
            );
        }

        let circle_offset = combined_buffer.len() as u64;
        if has_circles {
            combined_buffer.extend_from_slice(self.current_segment.circle_batch.as_bytes());
            batcher.add_quad_draw(
                PipelineId::Circle,
                self.current_segment.circle_batch.len() as u32,
                circle_offset,
                circle_size as u64,
            );
        }

        let arc_offset = combined_buffer.len() as u64;
        if has_arcs {
            combined_buffer.extend_from_slice(self.current_segment.arc_batch.as_bytes());
            batcher.add_quad_draw(
                PipelineId::Arc,
                self.current_segment.arc_batch.len() as u32,
                arc_offset,
                arc_size as u64,
            );
        }

        #[cfg(debug_assertions)]
        {
            let stats = batcher.stats();
            tracing::trace!(
                "WgpuPainter::flush_all_instanced_batches: draws={}, instances={}, buffer={}B",
                stats.active_draws,
                stats.active_instances,
                combined_buffer.len()
            );
        }

        // Upload combined buffer (using buffer pool with zero-copy)
        let instance_buffer = self.buffer_pool.get_vertex_buffer(
            &self.device,
            &self.queue,
            "Combined Instance Buffer",
            &combined_buffer,
        );

        // ===== SINGLE RENDER PASS FOR ALL PRIMITIVES =====
        // This is the key optimization: one render pass instead of three!
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Combined Instanced Primitives Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        // Set shared resources (geometry, bind groups)
        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        // Helper: set scissor rect for a region (full viewport when None)
        let full_w = self.size.0;
        let full_h = self.size.1;

        // Execute per-scissor-region draws for each shape type.
        // This replaces the old single-draw-per-type approach with granular
        // scissor clipping per sub-range of instances.

        // --- Shadows (rendered first for correct z-ordering) ---
        if has_shadows {
            render_pass.set_pipeline(&self.shadow_pipeline);
            let buf_start = shadow_offset;
            let buf_end = buf_start + shadow_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));
            // Shadows don't have per-shape scissor regions yet; draw all at once
            render_pass.set_scissor_rect(0, 0, full_w, full_h);
            render_pass.draw_indexed(0..6, 0, 0..self.current_segment.shadow_batch.len() as u32);
        }

        // --- Rectangles (per-scissor-region) ---
        if has_rects {
            render_pass.set_pipeline(&self.instanced_rect_pipeline);
            let buf_start = rect_offset;
            let buf_end = buf_start + rect_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &self.current_segment.rect_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // --- Circles (per-scissor-region) ---
        if has_circles {
            render_pass.set_pipeline(&self.instanced_circle_pipeline);
            let buf_start = circle_offset;
            let buf_end = buf_start + circle_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &self.current_segment.circle_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // --- Arcs (per-scissor-region) ---
        if has_arcs {
            render_pass.set_pipeline(&self.instanced_arc_pipeline);
            let buf_start = arc_offset;
            let buf_end = buf_start + arc_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &self.current_segment.arc_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // Drop render pass (explicit for clarity)
        drop(render_pass);

        // Clear batches for next frame
        self.current_segment.rect_batch.clear();
        self.current_segment.circle_batch.clear();
        self.current_segment.arc_batch.clear();
        self.current_segment.shadow_batch.clear();
        self.current_segment.rect_scissors.clear();
        self.current_segment.circle_scissors.clear();
        self.current_segment.arc_scissors.clear();
    }

    /// Flush gradient batches (linear and radial)
    ///
    /// Uploads gradient stops buffer and renders all gradient rectangles.
    /// Called automatically from render().
    fn flush_gradient_batches(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        // Check if we have any gradients to render
        let has_linear = !self.current_segment.linear_gradient_batch.is_empty();
        let has_radial = !self.current_segment.radial_gradient_batch.is_empty();
        let has_sweep = !self.current_segment.sweep_gradient_batch.is_empty();

        if !has_linear && !has_radial && !has_sweep {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::flush_gradient_batches: linear={}, radial={}, sweep={}, stops={}",
            self.current_segment.linear_gradient_batch.len(),
            self.current_segment.radial_gradient_batch.len(),
            self.current_segment.sweep_gradient_batch.len(),
            self.current_segment.current_gradient_stops.len()
        );

        // ===== Upload Gradient Stops to GPU =====
        if !self.current_segment.current_gradient_stops.is_empty() {
            self.queue.write_buffer(
                &self.gradient_stops_buffer,
                0,
                bytemuck::cast_slice(&self.current_segment.current_gradient_stops),
            );

            // Create/update bind group
            self.gradient_bind_group =
                Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Gradient Stops Bind Group"),
                    layout: &self.gradient_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.gradient_stops_buffer.as_entire_binding(),
                    }],
                }));
        }

        // Calculate buffer sizes
        let linear_size = self.current_segment.linear_gradient_batch.len()
            * std::mem::size_of::<super::instancing::LinearGradientInstance>();
        let radial_size = self.current_segment.radial_gradient_batch.len()
            * std::mem::size_of::<super::instancing::RadialGradientInstance>();
        let sweep_size = self.current_segment.sweep_gradient_batch.len()
            * std::mem::size_of::<super::instancing::SweepGradientInstance>();

        // Build combined instance buffer
        let mut combined_buffer = Vec::with_capacity(linear_size + radial_size + sweep_size);

        let linear_offset = 0;
        if has_linear {
            combined_buffer
                .extend_from_slice(self.current_segment.linear_gradient_batch.as_bytes());
        }

        let radial_offset = combined_buffer.len();
        if has_radial {
            combined_buffer
                .extend_from_slice(self.current_segment.radial_gradient_batch.as_bytes());
        }

        let sweep_offset = combined_buffer.len();
        if has_sweep {
            combined_buffer.extend_from_slice(self.current_segment.sweep_gradient_batch.as_bytes());
        }

        // Upload combined buffer (zero-copy via queue.write_buffer)
        let instance_buffer = self.buffer_pool.get_vertex_buffer(
            &self.device,
            &self.queue,
            "Gradient Instance Buffer",
            &combined_buffer,
        );

        // ===== SINGLE RENDER PASS FOR ALL GRADIENTS =====
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Gradient Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        // Set shared resources
        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        if let Some(ref gradient_bind_group) = self.gradient_bind_group {
            render_pass.set_bind_group(1, gradient_bind_group, &[]);
        }
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let full_w = self.size.0;
        let full_h = self.size.1;

        // ===== Draw Linear Gradients (per-scissor-region) =====
        if has_linear {
            render_pass.set_pipeline(&self.linear_gradient_pipeline);
            // Re-set bind groups after pipeline switch (WebGPU invalidates bind groups
            // when the new pipeline's PipelineLayout is a different object)
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = self.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let linear_start = linear_offset as u64;
            let linear_end = linear_start + linear_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(linear_start..linear_end));

            for region in &self.current_segment.linear_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // ===== Draw Radial Gradients (per-scissor-region) =====
        if has_radial {
            render_pass.set_pipeline(&self.radial_gradient_pipeline);
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = self.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let radial_start = radial_offset as u64;
            let radial_end = radial_start + radial_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(radial_start..radial_end));

            for region in &self.current_segment.radial_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // ===== Draw Sweep Gradients (per-scissor-region) =====
        if has_sweep {
            render_pass.set_pipeline(&self.sweep_gradient_pipeline);
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = self.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let sweep_start = sweep_offset as u64;
            let sweep_end = sweep_start + sweep_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(sweep_start..sweep_end));

            for region in &self.current_segment.sweep_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // Drop render pass
        drop(render_pass);

        // Clear batches for next frame
        self.current_segment.linear_gradient_batch.clear();
        self.current_segment.radial_gradient_batch.clear();
        self.current_segment.sweep_gradient_batch.clear();
        self.current_segment.current_gradient_stops.clear();
        self.current_segment.linear_grad_scissors.clear();
        self.current_segment.radial_grad_scissors.clear();
        self.current_segment.sweep_grad_scissors.clear();
    }

    /// Flush texture instance batch with given texture
    ///
    /// Renders all batched textures in a single draw call using GPU instancing.
    /// This is 50-100x faster than individual draw calls for image-heavy UIs.
    ///
    /// # Arguments
    /// * `encoder` - Command encoder
    /// * `view` - Render target view
    /// * `texture_view` - Texture to use for all instances in this batch
    pub fn flush_texture_batch(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
    ) {
        if self.texture_batch.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::flush_texture_batch: {} instances",
            self.texture_batch.len()
        );

        // Create texture bind group for this batch
        let texture_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Texture Instance Bind Group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.default_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
            ],
        });

        // Upload instance buffer (using buffer pool for efficient zero-copy reuse)
        let instance_buffer = self.buffer_pool.get_vertex_buffer(
            &self.device,
            &self.queue,
            "Texture Instance Buffer",
            self.texture_batch.as_bytes(),
        );

        // Create render pass
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Instanced Texture Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load, // Don't clear - render on top
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        // Set pipeline and buffers
        render_pass.set_pipeline(&self.instanced_texture_pipeline);
        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_bind_group(1, &texture_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_vertex_buffer(1, instance_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        // Draw all instances in ONE draw call! 🚀
        render_pass.draw_indexed(0..6, 0, 0..self.texture_batch.len() as u32);

        drop(render_pass);

        // Clear batch for next frame
        self.texture_batch.clear();
    }

    fn flush_segment_cached_images(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let mut pending_images: Vec<(
            super::texture_cache::TextureId,
            super::instancing::TextureInstance,
        )> = self.current_segment.cached_images.drain(..).collect();

        if pending_images.is_empty() {
            return;
        }

        let mut active_texture_id: Option<super::texture_cache::TextureId> = None;
        let mut active_texture_view: Option<wgpu::TextureView> = None;

        for (texture_id, instance) in pending_images.drain(..) {
            if active_texture_id.as_ref() != Some(&texture_id) {
                if let Some(texture_view) = active_texture_view.as_ref() {
                    self.flush_texture_batch(encoder, view, texture_view);
                }
                active_texture_id = Some(texture_id.clone());
                active_texture_view = self
                    .texture_cache
                    .get(&texture_id)
                    .map(|cached| cached.view.clone());
            }

            if let Some(texture_view) = active_texture_view.as_ref()
                && self.texture_batch.add(instance)
            {
                self.flush_texture_batch(encoder, view, texture_view);
            }
        }

        if let Some(texture_view) = active_texture_view.as_ref() {
            self.flush_texture_batch(encoder, view, texture_view);
        }
    }
}

// ===== Public Drawing API =====
//
// These methods used to be the `impl Painter for WgpuPainter` trait impl;
// the `Painter` trait was deleted in Mythos U5 (1 production impl, 6 default
// `tracing::warn!("not implemented")` impls, no second backend planned).
// The methods stay as inherent on `WgpuPainter` for direct use by `Backend`
// (the CommandRenderer impl) and external callers like `examples/painting_demo`.

// GPU rendering routinely converts between f32/u8/u32/i32 for pixel
// coordinates, color channels, and buffer indices. These truncations are
// intentional.
//
// `missing_docs` is allowed on this impl block: the methods were originally
// trait methods carrying their docs on the trait declaration; redocumenting
// every one here is deferred to a follow-up doc-sweep (recorded in
// crates/flui-engine/ARCHITECTURE.md `## Outstanding refactors`).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    missing_docs
)]
impl WgpuPainter {
    pub fn rect(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::rect: rect={:?}, paint={:?}", rect, paint);

        if paint.style == PaintStyle::Fill {
            // Check for shader (gradient) — dispatch to gradient pipeline
            if paint.has_shader() && self.dispatch_shader_rect(rect, paint, [0.0; 4]) {
                return;
            }

            // Apply current opacity to color
            let color = if self.current_opacity < 1.0 {
                let alpha = (f32::from(paint.color.a) * self.current_opacity) as u8;
                flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
            } else {
                paint.color
            };

            if self.is_axis_aligned_transform() {
                // Fast path: GPU instancing for axis-aligned rects
                let top_left = self.apply_transform(Point::new(rect.left(), rect.top()));
                let bottom_right = self.apply_transform(Point::new(rect.right(), rect.bottom()));
                let transformed_rect =
                    Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);
                let instance = self.apply_active_clip(super::instancing::RectInstance::rect(
                    transformed_rect,
                    color,
                ));
                let _ = self.current_segment.rect_batch.add(instance);
                DrawSegment::push_scissor_region(
                    &mut self.current_segment.rect_scissors,
                    self.current_scissor,
                );
            } else {
                // Slow path: tessellate rotated/skewed rects as a transformed quad
                let tl = self.apply_transform(Point::new(rect.left(), rect.top()));
                let tr = self.apply_transform(Point::new(rect.right(), rect.top()));
                let br = self.apply_transform(Point::new(rect.right(), rect.bottom()));
                let bl = self.apply_transform(Point::new(rect.left(), rect.bottom()));
                let rgba = color.to_rgba_f32_array();
                let vertices = vec![
                    Vertex {
                        position: [tl.x.0, tl.y.0],
                        color: rgba,
                        tex_coord: [0.0, 0.0],
                    },
                    Vertex {
                        position: [tr.x.0, tr.y.0],
                        color: rgba,
                        tex_coord: [1.0, 0.0],
                    },
                    Vertex {
                        position: [br.x.0, br.y.0],
                        color: rgba,
                        tex_coord: [1.0, 1.0],
                    },
                    Vertex {
                        position: [bl.x.0, bl.y.0],
                        color: rgba,
                        tex_coord: [0.0, 1.0],
                    },
                ];
                let indices = vec![0, 1, 2, 0, 2, 3];
                self.add_tessellated_with_key(
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        } else {
            // Stroked rect - use tessellator (less common, fallback path)
            // Paint already contains stroke information (stroke_width, stroke_cap,
            // stroke_join)
            if let Ok((vertices, indices)) = self.tessellator.tessellate_rect_stroke(rect, paint) {
                self.add_tessellated_with_key(
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        }
    }

    pub fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        if paint.style == PaintStyle::Fill {
            // Check for shader (gradient) — dispatch to gradient pipeline
            if paint.has_shader() {
                let corner_radii = [
                    rrect.top_left.x.0.max(rrect.top_left.y.0),
                    rrect.top_right.x.0.max(rrect.top_right.y.0),
                    rrect.bottom_right.x.0.max(rrect.bottom_right.y.0),
                    rrect.bottom_left.x.0.max(rrect.bottom_left.y.0),
                ];
                if self.dispatch_shader_rect(rrect.bounding_rect(), paint, corner_radii) {
                    return;
                }
            }

            // Apply current transform to rect bounds
            let top_left = self.apply_transform(Point::new(rrect.rect.left(), rrect.rect.top()));
            let bottom_right =
                self.apply_transform(Point::new(rrect.rect.right(), rrect.rect.bottom()));
            let transformed_rect =
                Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

            // Apply current opacity to color
            let color = if self.current_opacity < 1.0 {
                let alpha = (f32::from(paint.color.a) * self.current_opacity) as u8;
                flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
            } else {
                paint.color
            };

            // Use GPU instancing for filled rounded rects (100x faster!)
            let instance =
                self.apply_active_clip(super::instancing::RectInstance::rounded_rect_corners(
                    transformed_rect,
                    color,
                    rrect.top_left.x.0.max(rrect.top_left.y.0),
                    rrect.top_right.x.0.max(rrect.top_right.y.0),
                    rrect.bottom_right.x.0.max(rrect.bottom_right.y.0),
                    rrect.bottom_left.x.0.max(rrect.bottom_left.y.0),
                ));
            let _ = self.current_segment.rect_batch.add(instance);
            DrawSegment::push_scissor_region(
                &mut self.current_segment.rect_scissors,
                self.current_scissor,
            );
        } else {
            // Stroked rounded rect - use tessellator (fallback)
            if let Ok((vertices, indices)) = self.tessellator.tessellate_rrect(rrect, paint) {
                self.add_tessellated_with_key(
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        }
    }

    pub fn circle(&mut self, center: Point<Pixels>, radius: f32, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::circle: center={:?}, radius={}, paint={:?}",
            center,
            radius,
            paint
        );

        if paint.style == PaintStyle::Fill {
            // Check for shader (gradient) — dispatch to gradient pipeline
            if paint.has_shader() {
                let bounds = Rect::from_xywh(
                    center.x - px(radius),
                    center.y - px(radius),
                    px(radius * 2.0),
                    px(radius * 2.0),
                );
                // Use large corner radius to make it circular
                if self.dispatch_shader_rect(bounds, paint, [radius; 4]) {
                    return;
                }
            }

            // Apply current transform to center point
            let transformed_center = self.apply_transform(center);

            // Apply current opacity to color
            let color = if self.current_opacity < 1.0 {
                let alpha = (f32::from(paint.color.a) * self.current_opacity) as u8;
                flui_types::Color::rgba(paint.color.r, paint.color.g, paint.color.b, alpha)
            } else {
                paint.color
            };

            // Use GPU instancing for filled circles (100x faster!)
            let instance =
                super::instancing::CircleInstance::new(transformed_center, radius, color);
            let _ = self.current_segment.circle_batch.add(instance);
            DrawSegment::push_scissor_region(
                &mut self.current_segment.circle_scissors,
                self.current_scissor,
            );
            // Note: Auto-flush happens in render() - no need to flush here
        } else {
            // Stroked circle - use tessellator (less common, fallback path)
            if let Ok((vertices, indices)) =
                self.tessellator.tessellate_circle(center, radius, paint)
            {
                self.add_tessellated_with_key(
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
        }
    }

    pub fn oval(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::oval: rect={:?}, paint={:?}", rect, paint);

        // Tessellate the oval/ellipse
        let center = rect.center();
        let radii = Point::new(rect.width() / 2.0, rect.height() / 2.0);

        if let Ok((vertices, indices)) = self.tessellator.tessellate_ellipse(center, radii, paint) {
            self.add_tessellated_with_key(
                vertices,
                &indices,
                pipeline::pipeline_key_from_paint(paint),
            );
        }
    }

    pub fn draw_arc(
        &mut self,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_arc: rect={:?}, start={}, sweep={}, use_center={}, paint={:?}",
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint
        );

        let center = rect.center();
        let radius = (rect.width() + rect.height()) / px(4.0); // Average radius for elliptical arcs

        if paint.style == PaintStyle::Fill && use_center {
            // Use GPU instancing for filled arcs with center (pie slices)
            let instance = super::instancing::ArcInstance::new(
                center,
                radius,
                start_angle,
                sweep_angle,
                paint.color,
            );
            let _ = self.current_segment.arc_batch.add(instance);
            DrawSegment::push_scissor_region(
                &mut self.current_segment.arc_scissors,
                self.current_scissor,
            );
        } else {
            // For stroked arcs or arcs without center, use tessellation
            // TODO: Implement proper arc tessellation in Tessellator
            // For now, approximate with instanced arc (less accurate for strokes)
            if paint.style == PaintStyle::Fill {
                let instance = super::instancing::ArcInstance::new(
                    center,
                    radius,
                    start_angle,
                    sweep_angle,
                    paint.color,
                );
                let _ = self.current_segment.arc_batch.add(instance);
                DrawSegment::push_scissor_region(
                    &mut self.current_segment.arc_scissors,
                    self.current_scissor,
                );
            } else {
                // Use tessellation for stroked arcs
                match self.tessellator.tessellate_arc(
                    rect,
                    start_angle,
                    sweep_angle,
                    use_center,
                    paint,
                ) {
                    Ok((vertices, indices)) => {
                        self.add_tessellated_with_key(
                            vertices,
                            &indices,
                            pipeline::pipeline_key_from_paint(paint),
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to tessellate stroked arc: {}", e);
                    }
                }
            }
        }
    }

    pub fn draw_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_drrect: outer={:?}, inner={:?}, paint={:?}",
            outer,
            inner,
            paint
        );

        // Tessellate the DRRect (ring with inner cutout)
        match self.tessellator.tessellate_drrect(&outer, &inner, paint) {
            Ok((vertices, indices)) => {
                self.add_tessellated_with_key(
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
            Err(e) => {
                tracing::error!("Failed to tessellate DRRect: {}", e);
            }
        }
    }

    pub fn line(&mut self, p1: Point<Pixels>, p2: Point<Pixels>, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::line: p1={:?}, p2={:?}, paint={:?}",
            p1,
            p2,
            paint
        );

        // Use tessellator for line stroke
        // Paint already contains stroke information
        match self.tessellator.tessellate_line(p1, p2, paint) {
            Ok((vertices, indices)) => {
                #[cfg(debug_assertions)]
                tracing::trace!(
                    "WgpuPainter::line: Adding {} vertices, {} indices to batch",
                    vertices.len(),
                    indices.len()
                );
                self.add_tessellated_with_key(
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
            Err(e) => {
                tracing::error!("WgpuPainter::line: Tessellation failed - {}", e);
            }
        }
    }

    pub fn text(&mut self, text: &str, position: Point<Pixels>, font_size: f32, paint: &Paint) {
        tracing::trace!(
            text,
            ?position,
            font_size,
            color = ?paint.color,
            "WgpuPainter::text"
        );
        let transformed_position = self.apply_transform(position);
        self.text_renderer
            .add_text(text, transformed_position, font_size, paint.color);
    }

    /// Renders a sequence of styled runs as rich text.
    ///
    /// `runs` is the flattened output of `collect_styled_spans`: each entry is
    /// `(text_fragment, merged_style)` with `text_scale_factor` already baked
    /// into `style.font_size`.  `base_font_size` is the buffer-level default
    /// for runs with no explicit size; `base_color` is the fallback for runs
    /// with no color.
    pub fn rich_text(
        &mut self,
        runs: &[(String, Option<flui_types::typography::TextStyle>)],
        position: Point<Pixels>,
        base_font_size: f32,
        base_color: flui_types::styling::Color,
        wrap_width: Option<f32>,
    ) {
        tracing::trace!(
            run_count = runs.len(),
            ?position,
            base_font_size,
            ?base_color,
            ?wrap_width,
            "WgpuPainter::rich_text"
        );
        let transformed_position = self.apply_transform(position);
        self.text_renderer.add_rich_text(
            runs,
            transformed_position,
            base_font_size,
            base_color,
            wrap_width,
        );
    }

    pub fn texture(&mut self, texture_id: TextureId, dst_rect: Rect<Pixels>) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::texture: id={:?}, dst_rect={:?}",
            texture_id,
            dst_rect
        );

        // Look up texture in external texture registry
        if self.external_texture_registry.get(texture_id).is_none() {
            #[cfg(debug_assertions)]
            tracing::warn!(
                "WgpuPainter::texture: texture {:?} not found in registry",
                texture_id
            );
            return;
        }

        // Apply transform to rect
        let top_left = self.apply_transform(Point::new(dst_rect.left(), dst_rect.top()));
        let bottom_right = self.apply_transform(Point::new(dst_rect.right(), dst_rect.bottom()));

        let transformed_rect =
            Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

        // Create texture instance (full UV mapping, no rotation, white tint)
        let instance = super::instancing::TextureInstance::new(
            transformed_rect,
            flui_types::Color::WHITE, // White tint (no color modification)
        );

        // Add to texture batch
        let _ = self.texture_batch.add(instance);

        // NOTE: Actual rendering will happen in flush_texture_batch()
        // The texture bind group is created per-batch with the actual texture
    }

    pub fn draw_path(&mut self, path: &flui_types::painting::path::Path, paint: &Paint) {
        // Compute cache key from path geometry + paint tessellation parameters
        let path_hash = super::path_cache::PathCache::compute_path_hash(
            path,
            paint.style,
            paint.stroke_width,
            paint.stroke_cap,
            paint.stroke_join,
        );

        // Check cache for previously tessellated geometry
        if let Some((positions, cached_indices)) = self.path_cache.get(path_hash) {
            // Reconstruct full Vertex data with current paint color
            let rgba = paint.color.to_rgba_f32_array();
            let vertices: Vec<Vertex> = positions
                .iter()
                .map(|&pos| Vertex::new(pos, rgba, [0.0, 0.0]))
                .collect();
            let indices: Vec<u32> = cached_indices.to_vec();
            self.add_tessellated_with_key(
                vertices,
                &indices,
                pipeline::pipeline_key_from_paint(paint),
            );
            return;
        }

        // Cache miss — tessellate and store
        let result = if paint.style == PaintStyle::Fill {
            self.tessellator.tessellate_flui_path_fill(path, paint)
        } else {
            self.tessellator.tessellate_flui_path_stroke(path, paint)
        };

        match result {
            Ok((vertices, indices)) => {
                // Extract position data for cache (color-independent)
                let positions: Vec<[f32; 2]> = vertices.iter().map(|v| v.position).collect();
                self.path_cache
                    .insert(path_hash, positions, indices.clone());

                self.add_tessellated_with_key(
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
            Err(e) => {
                tracing::warn!("Failed to tessellate path: {}", e);
            }
        }
    }

    pub fn draw_image(&mut self, image: &flui_types::painting::Image, dst_rect: Rect<Pixels>) {
        let top_left = self.apply_transform(Point::new(dst_rect.left(), dst_rect.top()));
        let bottom_right = self.apply_transform(Point::new(dst_rect.right(), dst_rect.bottom()));
        let transformed_rect =
            Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

        // Use Arc pointer identity for O(1) cache lookup instead of hashing all pixels
        let texture_id = super::texture_cache::TextureId::from_ptr(image.data_ptr());
        let data = image.data();

        // Load or get cached texture (small images are auto-packed into the atlas)
        match self.texture_cache.load_from_rgba(
            texture_id.clone(),
            image.width(),
            image.height(),
            data,
        ) {
            Ok(cached_texture) => {
                // Preserve atlas UVs when the image is packed into the shared atlas.
                let instance = if let Some(uv_rect) = cached_texture.uv_rect {
                    super::instancing::TextureInstance::with_uv(
                        transformed_rect,
                        uv_rect,
                        flui_types::styling::Color::WHITE,
                    )
                } else {
                    super::instancing::TextureInstance::new(
                        transformed_rect,
                        flui_types::styling::Color::WHITE,
                    )
                };

                // Keep cached image draws in segment order for correct layer compositing.
                self.current_segment
                    .cached_images
                    .push((texture_id, instance));
            }
            Err(e) => {
                tracing::error!("Failed to load image texture: {}", e);
            }
        }
    }

    pub fn draw_image_repeat(
        &mut self,
        image: &flui_types::painting::Image,
        dst: Rect<Pixels>,
        repeat: flui_painting::display_list::ImageRepeat,
    ) {
        use flui_painting::display_list::ImageRepeat;

        let img_w = image.width() as f32;
        let img_h = image.height() as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return;
        }

        match repeat {
            ImageRepeat::NoRepeat => {
                // Single draw, no tiling
                self.draw_image(image, dst);
            }
            ImageRepeat::Repeat => {
                // Tile in both directions
                let mut y = dst.top().0;
                while y < dst.bottom().0 {
                    let mut x = dst.left().0;
                    while x < dst.right().0 {
                        let tile_w = img_w.min(dst.right().0 - x);
                        let tile_h = img_h.min(dst.bottom().0 - y);
                        let tile_dst = Rect::from_xywh(px(x), px(y), px(tile_w), px(tile_h));
                        self.draw_image(image, tile_dst);
                        x += img_w;
                    }
                    y += img_h;
                }
            }
            ImageRepeat::RepeatX => {
                // Tile only horizontally
                let tile_h = img_h.min(dst.height().0);
                let mut x = dst.left().0;
                while x < dst.right().0 {
                    let tile_w = img_w.min(dst.right().0 - x);
                    let tile_dst = Rect::from_xywh(px(x), dst.top(), px(tile_w), px(tile_h));
                    self.draw_image(image, tile_dst);
                    x += img_w;
                }
            }
            ImageRepeat::RepeatY => {
                // Tile only vertically
                let tile_w = img_w.min(dst.width().0);
                let mut y = dst.top().0;
                while y < dst.bottom().0 {
                    let tile_h = img_h.min(dst.bottom().0 - y);
                    let tile_dst = Rect::from_xywh(dst.left(), px(y), px(tile_w), px(tile_h));
                    self.draw_image(image, tile_dst);
                    y += img_h;
                }
            }
        }
    }

    #[allow(
        clippy::type_complexity,
        reason = "nine-slice src/dst tuple layout is local detail; refactoring into a named type adds no clarity"
    )]
    pub fn draw_image_nine_slice(
        &mut self,
        image: &flui_types::painting::Image,
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
    ) {
        let img_w = image.width() as f32;
        let img_h = image.height() as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return;
        }

        // Slice boundaries in image space
        let sl = center_slice.left().0;
        let st = center_slice.top().0;
        let sr = center_slice.right().0;
        let sb = center_slice.bottom().0;

        // Destination boundaries
        let dl = dst.left().0;
        let dt = dst.top().0;
        let dr = dst.right().0;
        let db = dst.bottom().0;

        // Inner destination boundaries (corners keep their pixel size)
        let d_inner_left = dl + sl;
        let d_inner_top = dt + st;
        let d_inner_right = dr - (img_w - sr);
        let d_inner_bottom = db - (img_h - sb);

        // Clamp: if dst is too small, inner edges collapse
        let d_inner_left = d_inner_left.min(dr);
        let d_inner_top = d_inner_top.min(db);
        let d_inner_right = d_inner_right.max(d_inner_left);
        let d_inner_bottom = d_inner_bottom.max(d_inner_top);

        // Helper: draw a sub-image region to a destination rect
        // Since draw_image draws the full image into dst_rect, we use it per-slice.
        // For a proper 9-slice we'd need draw_image_src_dst (src rect -> dst rect).
        // As a pragmatic v1, we draw the full image scaled into each 9 region
        // using the existing draw_image, which stretches the whole image.
        //
        // For correct 9-slice, we create sub-images from the pixel data.
        let data = image.data();
        let stride = (img_w as u32) * 4;

        // Extract a sub-region of the image as a new Image
        let extract = |sx: f32, sy: f32, sw: f32, sh: f32| -> Option<flui_types::painting::Image> {
            let sx = sx.max(0.0) as u32;
            let sy = sy.max(0.0) as u32;
            let sw = sw.max(0.0) as u32;
            let sh = sh.max(0.0) as u32;
            if sw == 0 || sh == 0 {
                return None;
            }
            let mut sub = Vec::with_capacity((sw * sh * 4) as usize);
            for row in sy..(sy + sh) {
                let start = (row * stride + sx * 4) as usize;
                let end = start + (sw * 4) as usize;
                if end <= data.len() {
                    sub.extend_from_slice(&data[start..end]);
                }
            }
            if sub.len() == (sw * sh * 4) as usize {
                Some(flui_types::painting::Image::from_rgba8(sw, sh, sub))
            } else {
                None
            }
        };

        // 9 slices: (src_x, src_y, src_w, src_h) -> dst rect
        let slices: [(f32, f32, f32, f32, f32, f32, f32, f32); 9] = [
            // Top-left corner
            (
                0.0,
                0.0,
                sl,
                st,
                dl,
                dt,
                d_inner_left - dl,
                d_inner_top - dt,
            ),
            // Top center
            (
                sl,
                0.0,
                sr - sl,
                st,
                d_inner_left,
                dt,
                d_inner_right - d_inner_left,
                d_inner_top - dt,
            ),
            // Top-right corner
            (
                sr,
                0.0,
                img_w - sr,
                st,
                d_inner_right,
                dt,
                dr - d_inner_right,
                d_inner_top - dt,
            ),
            // Middle-left
            (
                0.0,
                st,
                sl,
                sb - st,
                dl,
                d_inner_top,
                d_inner_left - dl,
                d_inner_bottom - d_inner_top,
            ),
            // Center
            (
                sl,
                st,
                sr - sl,
                sb - st,
                d_inner_left,
                d_inner_top,
                d_inner_right - d_inner_left,
                d_inner_bottom - d_inner_top,
            ),
            // Middle-right
            (
                sr,
                st,
                img_w - sr,
                sb - st,
                d_inner_right,
                d_inner_top,
                dr - d_inner_right,
                d_inner_bottom - d_inner_top,
            ),
            // Bottom-left corner
            (
                0.0,
                sb,
                sl,
                img_h - sb,
                dl,
                d_inner_bottom,
                d_inner_left - dl,
                db - d_inner_bottom,
            ),
            // Bottom center
            (
                sl,
                sb,
                sr - sl,
                img_h - sb,
                d_inner_left,
                d_inner_bottom,
                d_inner_right - d_inner_left,
                db - d_inner_bottom,
            ),
            // Bottom-right corner
            (
                sr,
                sb,
                img_w - sr,
                img_h - sb,
                d_inner_right,
                d_inner_bottom,
                dr - d_inner_right,
                db - d_inner_bottom,
            ),
        ];

        for (sx, sy, sw, sh, dx, dy, dw, dh) in slices {
            if dw <= 0.0 || dh <= 0.0 || sw <= 0.0 || sh <= 0.0 {
                continue;
            }
            if let Some(sub_image) = extract(sx, sy, sw, sh) {
                let tile_dst = Rect::from_xywh(px(dx), px(dy), px(dw), px(dh));
                self.draw_image(&sub_image, tile_dst);
            }
        }
    }

    #[allow(
        clippy::many_single_char_names,
        reason = "w/h/r/g/b/a are idiomatic in CPU-side color-matrix pixel loops"
    )]
    pub fn draw_image_filtered(
        &mut self,
        image: &flui_types::painting::Image,
        dst: Rect<Pixels>,
        filter: flui_painting::display_list::ColorFilter,
    ) {
        use flui_painting::display_list::ColorFilter;

        match filter {
            ColorFilter::Mode {
                color,
                blend_mode: _,
            } => {
                // Pragmatic v1: draw image then overlay a tinted rect
                // First draw the image normally
                self.draw_image(image, dst);

                // Then overlay with the tint color using a semi-transparent rect
                let tint_paint = Paint {
                    color: color.with_alpha(color.a / 2),
                    style: flui_painting::PaintStyle::Fill,
                    ..Default::default()
                };
                self.rect(dst, &tint_paint);

                tracing::debug!(
                    "draw_image_filtered: Mode filter applied as color overlay (color={:?})",
                    color
                );
            }
            ColorFilter::Matrix(matrix) => {
                // Apply color matrix to image pixel data on CPU
                let data = image.data();
                let w = image.width();
                let h = image.height();
                let mut new_data = Vec::with_capacity(data.len());

                for pixel in data.chunks_exact(4) {
                    let r = f32::from(pixel[0]) / 255.0;
                    let g = f32::from(pixel[1]) / 255.0;
                    let b = f32::from(pixel[2]) / 255.0;
                    let a = f32::from(pixel[3]) / 255.0;

                    let nr =
                        (matrix[0] * r + matrix[1] * g + matrix[2] * b + matrix[3] * a + matrix[4])
                            .clamp(0.0, 1.0);
                    let ng =
                        (matrix[5] * r + matrix[6] * g + matrix[7] * b + matrix[8] * a + matrix[9])
                            .clamp(0.0, 1.0);
                    let nb = (matrix[10] * r
                        + matrix[11] * g
                        + matrix[12] * b
                        + matrix[13] * a
                        + matrix[14])
                        .clamp(0.0, 1.0);
                    let na = (matrix[15] * r
                        + matrix[16] * g
                        + matrix[17] * b
                        + matrix[18] * a
                        + matrix[19])
                        .clamp(0.0, 1.0);

                    new_data.push((nr * 255.0) as u8);
                    new_data.push((ng * 255.0) as u8);
                    new_data.push((nb * 255.0) as u8);
                    new_data.push((na * 255.0) as u8);
                }

                let filtered = flui_types::painting::Image::from_rgba8(w, h, new_data);
                self.draw_image(&filtered, dst);

                tracing::debug!("draw_image_filtered: Matrix filter applied via CPU");
            }
            ColorFilter::LinearToSrgbGamma => {
                // Apply linear-to-sRGB gamma correction on CPU
                let data = image.data();
                let w = image.width();
                let h = image.height();
                let mut new_data = Vec::with_capacity(data.len());

                for pixel in data.chunks_exact(4) {
                    for &ch in &pixel[..3] {
                        let linear = f32::from(ch) / 255.0;
                        let srgb = if linear <= 0.003_130_8 {
                            linear * 12.92
                        } else {
                            1.055 * linear.powf(1.0 / 2.4) - 0.055
                        };
                        new_data.push((srgb.clamp(0.0, 1.0) * 255.0) as u8);
                    }
                    new_data.push(pixel[3]); // Alpha unchanged
                }

                let filtered = flui_types::painting::Image::from_rgba8(w, h, new_data);
                self.draw_image(&filtered, dst);

                tracing::debug!("draw_image_filtered: LinearToSrgbGamma applied via CPU");
            }
            ColorFilter::SrgbToLinearGamma => {
                // Apply sRGB-to-linear gamma correction on CPU
                let data = image.data();
                let w = image.width();
                let h = image.height();
                let mut new_data = Vec::with_capacity(data.len());

                for pixel in data.chunks_exact(4) {
                    for &ch in &pixel[..3] {
                        let srgb = f32::from(ch) / 255.0;
                        let linear = if srgb <= 0.04045 {
                            srgb / 12.92
                        } else {
                            ((srgb + 0.055) / 1.055).powf(2.4)
                        };
                        new_data.push((linear.clamp(0.0, 1.0) * 255.0) as u8);
                    }
                    new_data.push(pixel[3]); // Alpha unchanged
                }

                let filtered = flui_types::painting::Image::from_rgba8(w, h, new_data);
                self.draw_image(&filtered, dst);

                tracing::debug!("draw_image_filtered: SrgbToLinearGamma applied via CPU");
            }
        }
    }

    pub fn draw_shadow(
        &mut self,
        path: &flui_types::painting::path::Path,
        color: flui_types::styling::Color,
        elevation: f32,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_shadow: elevation={}, color={:?}",
            elevation,
            color
        );

        // Calculate blur radius from elevation (Material Design style)
        // elevation controls both offset and blur amount
        let blur_radius = elevation.max(0.0);
        let offset_y = elevation / 2.0; // Shadow offset downwards

        if blur_radius < 0.1 {
            // No shadow for very small elevations
            return;
        }

        // Multi-pass blur approximation
        // Draw the shadow path multiple times with decreasing alpha to simulate blur
        let num_layers = (blur_radius / 2.0).ceil().min(8.0) as usize; // Max 8 layers for performance

        if num_layers == 0 {
            return;
        }

        let alpha_per_layer = f32::from(color.a) / num_layers as f32;

        for i in 0..num_layers {
            let offset_scale = (i as f32 + 1.0) / num_layers as f32;
            let current_blur = blur_radius * offset_scale;

            // Create shadow paint with decreasing alpha
            let shadow_alpha = (alpha_per_layer * (1.0 - offset_scale * 0.5)) as u8;
            let shadow_color =
                flui_types::styling::Color::rgba(color.r, color.g, color.b, shadow_alpha);

            let shadow_paint = Paint::fill(shadow_color);

            // Save transform, apply shadow offset
            self.save();
            self.translate(flui_types::Offset::new(
                px(current_blur * 0.5),
                px(offset_y + current_blur * 0.5),
            ));

            // Draw the shadow layer
            match self
                .tessellator
                .tessellate_flui_path_fill(path, &shadow_paint)
            {
                Ok((vertices, indices)) => {
                    self.add_tessellated(vertices, &indices);
                }
                Err(e) => {
                    tracing::error!("Failed to tessellate shadow path: {}", e);
                }
            }

            // Restore transform
            self.restore();
        }
    }

    /// Draw indexed triangle geometry with per-vertex color + uv.
    ///
    /// # `tex_coords` parameter
    ///
    /// Cycle 4 E-12: pre-cycle the parameter carried a `// TODO: Full
    /// texture coordinate support` comment that was misleading -- the
    /// per-vertex uv extraction IS implemented (the `tex_coords` slice
    /// is consumed at the per-vertex loop below, copied into
    /// `Vertex::tex_coord`, and baked into the GPU vertex buffer).
    /// What is NOT yet wired is the **texture-binding pipeline path**:
    /// `pipeline_key_from_paint(paint)` returns a solid-color pipeline
    /// today, so the uv values reach the vertex shader but the fragment
    /// shader has no texture to sample. A textured pipeline-key variant
    /// is a follow-up audit item; until then `tex_coords` callers pre-
    /// populate the vertex stream for forward-compat (the data path is
    /// correct, only the pipeline binding is missing).
    pub fn draw_vertices(
        &mut self,
        vertices: &[Point<Pixels>],
        colors: Option<&[flui_types::styling::Color]>,
        tex_coords: Option<&[Point<Pixels>]>,
        indices: &[u16],
        paint: &Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_vertices: vertices={}, indices={}",
            vertices.len(),
            indices.len()
        );

        // Validate input
        if vertices.is_empty() || indices.is_empty() {
            return;
        }

        if let Some(colors_arr) = colors
            && colors_arr.len() != vertices.len()
        {
            #[cfg(debug_assertions)]
            tracing::error!(
                "DrawVertices: color count ({}) doesn't match vertex count ({})",
                colors_arr.len(),
                vertices.len()
            );
            return;
        }

        // Convert to our Vertex format
        let default_color = paint.color;
        let our_vertices: Vec<super::vertex::Vertex> = vertices
            .iter()
            .enumerate()
            .map(|(i, pos)| {
                let color = colors
                    .and_then(|c| c.get(i))
                    .copied()
                    .unwrap_or(default_color);

                let uv = tex_coords
                    .and_then(|tc| tc.get(i))
                    .map_or([0.0, 0.0], |p| [p.x.0, p.y.0]);

                super::vertex::Vertex {
                    position: [pos.x.0, pos.y.0],
                    color: color.to_f32_array(),
                    tex_coord: uv,
                }
            })
            .collect();

        // Convert indices to u32
        let our_indices: Vec<u32> = indices.iter().map(|&i| u32::from(i)).collect();

        // Add to tessellated geometry (bypassing tessellator since we already have
        // triangles)
        self.add_tessellated_with_key(
            our_vertices,
            &our_indices,
            pipeline::pipeline_key_from_paint(paint),
        );
    }

    pub fn draw_atlas(
        &mut self,
        image: &flui_types::painting::Image,
        sprites: &[Rect<Pixels>],
        transforms: &[flui_types::Matrix4],
        colors: Option<&[flui_types::styling::Color]>,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_atlas: image={}x{}, sprites={}",
            image.width(),
            image.height(),
            sprites.len()
        );

        // Validate input
        if sprites.len() != transforms.len() {
            #[cfg(debug_assertions)]
            tracing::error!(
                "DrawAtlas: sprite count ({}) doesn't match transform count ({})",
                sprites.len(),
                transforms.len()
            );
            return;
        }

        if let Some(colors_arr) = colors
            && colors_arr.len() != sprites.len()
        {
            #[cfg(debug_assertions)]
            tracing::error!(
                "DrawAtlas: color count ({}) doesn't match sprite count ({})",
                colors_arr.len(),
                sprites.len()
            );
            return;
        }

        // Use Arc pointer identity for O(1) cache lookup instead of hashing all pixels
        let texture_id = super::texture_cache::TextureId::from_ptr(image.data_ptr());

        match self.texture_cache.load_from_rgba(
            texture_id,
            image.width(),
            image.height(),
            image.data(),
        ) {
            Ok(_cached_texture) => {
                let image_width = image.width() as f32;
                let image_height = image.height() as f32;

                // Create texture instances for each sprite
                for (i, (sprite_rect, transform)) in
                    sprites.iter().zip(transforms.iter()).enumerate()
                {
                    // Get color tint for this sprite (default to white)
                    let tint = colors
                        .and_then(|c| c.get(i))
                        .copied()
                        .unwrap_or(flui_types::styling::Color::WHITE);

                    // Calculate UV coordinates from sprite rect
                    let src_uv = [
                        (sprite_rect.left() / image_width).0,
                        (sprite_rect.top() / image_height).0,
                        (sprite_rect.right() / image_width).0,
                        (sprite_rect.bottom() / image_height).0,
                    ];

                    // Extract position from transform matrix
                    // Matrix4 is column-major: m[12] = x translation, m[13] = y translation
                    let dst_x = transform.m[12];
                    let dst_y = transform.m[13];
                    let dst_width = sprite_rect.width();
                    let dst_height = sprite_rect.height();

                    let dst_rect = Rect::from_xywh(px(dst_x), px(dst_y), dst_width, dst_height);

                    // Create texture instance
                    let instance =
                        super::instancing::TextureInstance::with_uv(dst_rect, src_uv, tint);
                    let _ = self.texture_batch.add(instance);
                }
            }
            Err(e) => {
                tracing::error!("Failed to load atlas texture: {}", e);
            }
        }
    }

    pub fn draw_texture(
        &mut self,
        texture_id: flui_types::painting::TextureId,
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
        _filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_texture: id={}, dst={:?}, src={:?}, opacity={}",
            texture_id.get(),
            dst,
            src,
            opacity
        );

        // Look up the external texture in the registry
        if let Some(entry) = self.external_texture_registry.get(texture_id) {
            // Calculate UV coordinates from source rect
            let src_uv = if let Some(src_rect) = src {
                // Normalize to texture dimensions
                let tex_width = entry.width as f32;
                let tex_height = entry.height as f32;
                [
                    (src_rect.left() / tex_width).0,
                    (src_rect.top() / tex_height).0,
                    (src_rect.right() / tex_width).0,
                    (src_rect.bottom() / tex_height).0,
                ]
            } else {
                // Full texture
                [0.0, 0.0, 1.0, 1.0]
            };

            // Apply opacity via tint color alpha
            let tint = flui_types::styling::Color::rgba(255, 255, 255, (opacity * 255.0) as u8);

            // Create texture instance
            let instance = super::instancing::TextureInstance::with_uv(dst, src_uv, tint);
            let _ = self.texture_batch.add(instance);

            // Note: The actual texture rendering happens in flush_all_instanced_batches()
            // which needs to use entry.bind_group for the texture binding.
            // For now, the texture batch uses a placeholder - full integration requires
            // modifying the texture rendering pass to support per-texture bind groups.

            #[cfg(debug_assertions)]
            tracing::trace!(
                "External texture {} found: {}x{}, frame={}",
                texture_id.get(),
                entry.width,
                entry.height,
                entry.frame_count
            );
        } else {
            // Texture not registered - render placeholder for debugging
            #[cfg(debug_assertions)]
            tracing::warn!(
                "External texture {} not registered - rendering placeholder",
                texture_id.get()
            );

            // Create a placeholder color based on texture ID (for debugging)
            let id_hash = texture_id.get();
            let r = (id_hash & 0xFF) as u8;
            let g = ((id_hash >> 8) & 0xFF) as u8;
            let b = ((id_hash >> 16) & 0xFF) as u8;
            let a = (opacity * 255.0) as u8;
            let placeholder_color =
                flui_types::styling::Color::rgba(r.max(64), g.max(64), b.max(64), a);

            // Default UV coordinates
            let src_uv = if let Some(src_rect) = src {
                [
                    src_rect.left().0,
                    src_rect.top().0,
                    src_rect.right().0,
                    src_rect.bottom().0,
                ]
            } else {
                [0.0, 0.0, 1.0, 1.0]
            };

            let instance =
                super::instancing::TextureInstance::with_uv(dst, src_uv, placeholder_color);
            let _ = self.texture_batch.add(instance);
        }
    }

    // ===== Transform Stack =====

    pub fn save(&mut self) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::save: stack depth={}",
            self.transform_stack.len()
        );

        // Save both transform and scissor state
        self.transform_stack.push(self.current_transform);

        // Save current scissor (if any) by pushing to stack
        if let Some(scissor) = self.current_scissor {
            self.scissor_stack.push(scissor);
        }

        // Save current SDF rrect clip
        self.rrect_clip_stack.push(self.current_rrect_clip);

        // Save current SDF rsuperellipse clip
        self.rsuperellipse_clip_stack
            .push(self.current_rsuperellipse_clip);
    }

    pub fn restore(&mut self) {
        if let Some(transform) = self.transform_stack.pop() {
            self.current_transform = transform;

            // Restore scissor state
            // Pop from scissor stack if there was a saved scissor
            if self.scissor_stack.is_empty() {
                // No scissor was saved, clear current
                self.current_scissor = None;
            } else {
                self.current_scissor = self.scissor_stack.pop();
            }

            // Restore SDF rrect clip state
            if let Some(clip) = self.rrect_clip_stack.pop() {
                self.current_rrect_clip = clip;
            } else {
                self.current_rrect_clip = [0.0; 8];
            }

            // Restore SDF rsuperellipse clip state
            if let Some(clip) = self.rsuperellipse_clip_stack.pop() {
                self.current_rsuperellipse_clip = clip;
            } else {
                self.current_rsuperellipse_clip = [0.0; 12];
            }

            #[cfg(debug_assertions)]
            tracing::trace!(
                "WgpuPainter::restore: stack depth={}",
                self.transform_stack.len()
            );
        } else {
            #[cfg(debug_assertions)]
            tracing::warn!("WgpuPainter::restore: stack underflow");
        }
    }

    pub fn translate(&mut self, offset: Offset<Pixels>) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::translate: offset={:?}", offset);

        let translation = glam::Mat4::from_translation(glam::vec3(offset.dx.0, offset.dy.0, 0.0));
        self.current_transform *= translation;
    }

    pub fn rotate(&mut self, angle: f32) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::rotate: angle={}", angle);

        let rotation = glam::Mat4::from_rotation_z(angle);
        self.current_transform *= rotation;
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::scale: sx={}, sy={}", sx, sy);

        let scaling = glam::Mat4::from_scale(glam::vec3(sx, sy, 1.0));
        self.current_transform *= scaling;
    }

    // ===== Clipping =====

    pub fn clip_rect(&mut self, rect: Rect<Pixels>) {
        // Apply current transform to get screen-space coordinates
        let transform = self.current_transform;

        // Compute axis-aligned bounding box in screen space
        let (x, y, width, height) = if transform == glam::Mat4::IDENTITY {
            // Fast path: no transform, use rect directly
            let x = rect.left().0.max(0.0) as u32;
            let y = rect.top().0.max(0.0) as u32;
            let right = rect.right().0.min(self.size.0 as f32) as u32;
            let bottom = rect.bottom().0.min(self.size.1 as f32) as u32;
            (x, y, right.saturating_sub(x), bottom.saturating_sub(y))
        } else {
            // Transform all 4 corners to screen space and compute AABB
            // This is a conservative approximation for rotated/skewed clips
            let corners = [
                transform.transform_point3(glam::Vec3::new(rect.left().0, rect.top().0, 0.0)),
                transform.transform_point3(glam::Vec3::new(rect.right().0, rect.top().0, 0.0)),
                transform.transform_point3(glam::Vec3::new(rect.right().0, rect.bottom().0, 0.0)),
                transform.transform_point3(glam::Vec3::new(rect.left().0, rect.bottom().0, 0.0)),
            ];
            let min_x = corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min);
            let min_y = corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
            let max_x = corners
                .iter()
                .map(|c| c.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let max_y = corners
                .iter()
                .map(|c| c.y)
                .fold(f32::NEG_INFINITY, f32::max);

            // Clamp to surface bounds
            let x = min_x.max(0.0) as u32;
            let y = min_y.max(0.0) as u32;
            let w = (max_x.min(self.size.0 as f32) - min_x.max(0.0))
                .ceil()
                .max(0.0) as u32;
            let h = (max_y.min(self.size.1 as f32) - min_y.max(0.0))
                .ceil()
                .max(0.0) as u32;
            (x, y, w, h)
        };

        // Intersect with current scissor if any
        let scissor = if let Some((cur_x, cur_y, cur_w, cur_h)) = self.current_scissor {
            // Compute intersection
            let intersect_x = x.max(cur_x);
            let intersect_y = y.max(cur_y);
            let intersect_right = (x + width).min(cur_x + cur_w);
            let intersect_bottom = (y + height).min(cur_y + cur_h);

            let intersect_width = intersect_right.saturating_sub(intersect_x);
            let intersect_height = intersect_bottom.saturating_sub(intersect_y);

            (intersect_x, intersect_y, intersect_width, intersect_height)
        } else {
            (x, y, width, height)
        };

        self.current_scissor = Some(scissor);

        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::clip_rect: rect={:?} → scissor=({}, {}, {}, {})",
            rect,
            scissor.0,
            scissor.1,
            scissor.2,
            scissor.3
        );
    }

    #[allow(
        clippy::similar_names,
        reason = "r_tl/r_tr/r_br/r_bl mirror the rrect-corner field names; renaming would obscure intent"
    )]
    pub fn clip_rrect(&mut self, rrect: RRect) {
        // SDF-based rounded rectangle clipping: pass clip bounds and radii
        // to each instance so the fragment shader can do per-pixel SDF clipping.
        // This avoids stencil buffers and tessellation entirely.

        // Apply current transform to get screen-space clip coordinates
        let transform = self.current_transform;
        let rect = rrect.rect;

        let (x, y, w, h) = if transform == glam::Mat4::IDENTITY {
            (rect.left().0, rect.top().0, rect.width().0, rect.height().0)
        } else {
            // Transform corners and compute AABB
            let tl = transform * glam::Vec4::new(rect.left().0, rect.top().0, 0.0, 1.0);
            let br = transform * glam::Vec4::new(rect.right().0, rect.bottom().0, 0.0, 1.0);
            let min_x = tl.x.min(br.x);
            let min_y = tl.y.min(br.y);
            let max_x = tl.x.max(br.x);
            let max_y = tl.y.max(br.y);
            (min_x, min_y, max_x - min_x, max_y - min_y)
        };

        // Use the maximum of each corner's x/y radius (same approach as draw_rrect)
        let r_tl = rrect.top_left.x.0.max(rrect.top_left.y.0);
        let r_tr = rrect.top_right.x.0.max(rrect.top_right.y.0);
        let r_br = rrect.bottom_right.x.0.max(rrect.bottom_right.y.0);
        let r_bl = rrect.bottom_left.x.0.max(rrect.bottom_left.y.0);

        self.current_rrect_clip = [x, y, w, h, r_tl, r_tr, r_br, r_bl];
        // Clear any previously-set rsuperellipse clip so `apply_active_clip`
        // doesn't keep applying the squircle SDF after the caller has
        // switched to a plain rrect clip. The two clip kinds are mutually
        // exclusive at the per-instance `clip_kind` level — setting one
        // must invalidate the other.
        self.current_rsuperellipse_clip = [0.0; 12];

        // Also apply bounding-box scissor clip for early rejection by the rasterizer
        self.clip_rect(rrect.rect);

        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::clip_rrect: SDF clip set [{:.1}, {:.1}, {:.1}, {:.1}] radii=[{:.1}, {:.1}, {:.1}, {:.1}]",
            x,
            y,
            w,
            h,
            r_tl,
            r_tr,
            r_br,
            r_bl
        );
    }

    /// Look up or generate a tessellated superellipse path via the
    /// Painter-owned bounded cache.
    ///
    /// Consulted by `Backend::superellipse_path` (the `CommandRenderer`
    /// trait override) so `ClipSuperellipseLayer::render`'s layer-tree
    /// clip path benefits from frame-bounded caching. On a miss the path
    /// is generated via `generate_superellipse_path` (the iOS-squircle
    /// math) and inserted; eviction follows PathCache semantics
    /// (`max_entries` + `last_used_frame`).
    pub(crate) fn superellipse_path(
        &mut self,
        rse: &flui_types::geometry::RSuperellipse,
    ) -> flui_types::painting::Path {
        let key = super::superellipse_cache::SuperellipseKey::from_superellipse(rse);
        if let Some(path) = self.superellipse_cache.get(&key) {
            return path;
        }
        let path = super::layer_render::generate_superellipse_path(rse);
        self.superellipse_cache.insert(key, path.clone());
        path
    }

    /// Apply the currently-active SDF clip (rrect or rsuperellipse) to a
    /// `RectInstance`.
    ///
    /// Branch order: if `current_rsuperellipse_clip` is non-trivial, the
    /// superellipse clip wins (kind = 2). Otherwise the rrect clip slot
    /// is used (kind = 1 when non-zero, kind = 0 when both are zero).
    /// Centralizes the per-instance clip-kind selection so the two
    /// `rect`/`rrect` batch-build sites don't drift apart.
    fn apply_active_clip(
        &self,
        instance: super::instancing::RectInstance,
    ) -> super::instancing::RectInstance {
        // Exact equality against the all-zero "no clip active" sentinel is
        // intentional: the field is set bit-exact to `[0.0; 12]` whenever
        // the clip is cleared, never via arithmetic that would introduce
        // ULP noise.
        #[expect(
            clippy::float_cmp,
            reason = "exact comparison against the bit-exact `[0.0; 12]` 'no clip' sentinel"
        )]
        let superellipse_active = self.current_rsuperellipse_clip != [0.0; 12];
        if superellipse_active {
            instance.with_clip_rsuperellipse(self.current_rsuperellipse_clip)
        } else {
            instance.with_clip_rrect(self.current_rrect_clip)
        }
    }

    /// Set an SDF rounded-superellipse clip (iOS-squircle).
    ///
    /// Parallel to [`Self::clip_rrect`]: populates `current_rsuperellipse_clip`
    /// with the bounding rect + per-corner radii, applies a bounding-rect
    /// scissor for early rasterizer rejection, and relies on
    /// `rect_instanced.wgsl`'s per-pixel SDF evaluation to clip pixels
    /// outside the iOS-squircle curve (wired in U9 / U10).
    #[allow(
        clippy::similar_names,
        reason = "tl_r/tr_r/br_r/bl_r mirror the rsuperellipse-corner field names; renaming would obscure intent"
    )]
    pub fn clip_rsuperellipse(&mut self, rse: flui_types::geometry::RSuperellipse) {
        // Apply current transform to outer rect (identical AABB logic to
        // `clip_rrect`).
        let transform = self.current_transform;
        let rect = rse.outer_rect();

        let (x, y, w, h) = if transform == glam::Mat4::IDENTITY {
            (rect.left().0, rect.top().0, rect.width().0, rect.height().0)
        } else {
            let tl = transform * glam::Vec4::new(rect.left().0, rect.top().0, 0.0, 1.0);
            let br = transform * glam::Vec4::new(rect.right().0, rect.bottom().0, 0.0, 1.0);
            let min_x = tl.x.min(br.x);
            let min_y = tl.y.min(br.y);
            let max_x = tl.x.max(br.x);
            let max_y = tl.y.max(br.y);
            (min_x, min_y, max_x - min_x, max_y - min_y)
        };

        // Per-corner separate-axis radii (rx, ry per corner).
        let tl_r = rse.tl_radius();
        let tr_r = rse.tr_radius();
        let br_r = rse.br_radius();
        let bl_r = rse.bl_radius();

        self.current_rsuperellipse_clip = [
            x, y, w, h, tl_r.x.0, tl_r.y.0, tr_r.x.0, tr_r.y.0, br_r.x.0, br_r.y.0, bl_r.x.0,
            bl_r.y.0,
        ];
        // Clear any previously-set rrect clip so `apply_active_clip`
        // doesn't fall back to it. Mirror of the corresponding clear in
        // `clip_rrect`; the two clip kinds are mutually exclusive at the
        // per-instance `clip_kind` level.
        self.current_rrect_clip = [0.0; 8];

        // Bounding-box scissor for early rasterizer rejection (same pattern
        // as `clip_rrect`).
        self.clip_rect(rect);

        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::clip_rsuperellipse: SDF clip set [{:.1}, {:.1}, {:.1}, {:.1}] radii=[(tl {:.1},{:.1}) (tr {:.1},{:.1}) (br {:.1},{:.1}) (bl {:.1},{:.1})]",
            x,
            y,
            w,
            h,
            tl_r.x.0,
            tl_r.y.0,
            tr_r.x.0,
            tr_r.y.0,
            br_r.x.0,
            br_r.y.0,
            bl_r.x.0,
            bl_r.y.0,
        );
    }

    pub fn clip_path(&mut self, _path: &Path) {
        // Path clipping requires stencil buffer or path tessellation
        // This is a complex feature that needs:
        // 1. Stencil buffer configuration in render pass
        // 2. Tessellate path and render to stencil buffer
        // 3. Enable stencil test for subsequent draws
        // 4. Stack management for nested clips
        // 5. Handle even-odd vs non-zero fill rules
        //
        // Additionally, Path::bounds() requires &mut Path for caching,
        // but we only have &Path in this context.
        //
        // For now, this is a no-op. Applications should use ClipRect or ClipRRect
        // for hardware-accelerated clipping. Path clipping will be implemented
        // in a future version with proper stencil buffer support.

        // Cycle 4 E-1: pre-cycle this path emitted a debug-only
        // `tracing::trace!` and returned silently. Production scrapes
        // never saw the missing clip — content rendered without the
        // intended clip. Upgrade to release-build `tracing::warn!` so
        // any consumer that hits the path gets a visible signal.
        tracing::warn!(
            "WgpuPainter::clip_path: path clipping not implemented; \
             content will render without the intended clip. \
             Use ClipRect or ClipRRect for hardware-accelerated clipping. \
             Path clipping requires stencil-buffer support (cycle 4 E-1)"
        );
    }

    // ===== Viewport Information =====

    pub fn viewport_bounds(&self) -> Rect<Pixels> {
        Rect::from_ltrb(
            px(0.0),
            px(0.0),
            px(self.size.0 as f32),
            px(self.size.1 as f32),
        )
    }

    // ===== Layer Operations (Opacity) =====

    pub fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint) {
        let paint_alpha = f32::from(paint.color.a) / 255.0;
        let layer_opacity = self.current_opacity * paint_alpha;

        // Convert bounds to [x, y, w, h] if provided
        let bounds_array = bounds.map(|r| [r.left().0, r.top().0, r.width().0, r.height().0]);

        // Save current draw state — all subsequent draws go into a fresh segment/draw_order
        let saved = SavedLayer {
            saved_draw_order: std::mem::take(&mut self.draw_order),
            saved_segment: std::mem::replace(&mut self.current_segment, DrawSegment::new()),
            saved_opacity_stack: std::mem::take(&mut self.opacity_stack),
            saved_opacity: self.current_opacity,
            layer_opacity,
            bounds: bounds_array,
        };
        self.layer_stack.push(saved);

        // Reset opacity for the offscreen subtree — children draw at full opacity
        // within the layer; the group opacity is applied during compositing
        self.current_opacity = 1.0;

        tracing::trace!(
            "WgpuPainter::save_layer: layer_opacity={:.3}, bounds={:?}",
            layer_opacity,
            bounds_array
        );
    }

    pub fn restore_layer(&mut self) {
        if let Some(saved) = self.layer_stack.pop() {
            // Capture the offscreen content drawn since save_layer
            let offscreen_segment =
                std::mem::replace(&mut self.current_segment, saved.saved_segment);
            let offscreen_order = std::mem::replace(&mut self.draw_order, saved.saved_draw_order);

            // Restore parent opacity state
            self.opacity_stack = saved.saved_opacity_stack;
            self.current_opacity = saved.saved_opacity;

            // Determine compositing bounds — use provided bounds or fall back to viewport
            let composite_bounds = if let Some(b) = saved.bounds {
                Rect::from_ltrb(px(b[0]), px(b[1]), px(b[0] + b[2]), px(b[1] + b[3]))
            } else {
                self.viewport_bounds()
            };

            let has_offscreen_content =
                !offscreen_segment.is_empty() || !offscreen_order.is_empty();

            if has_offscreen_content && (1.0 - saved.layer_opacity).abs() > f32::EPSILON {
                // Offscreen render-to-texture compositing:
                // Package the offscreen content as an OpacityLayer draw item.
                // During render(), this will be flushed to a pooled offscreen texture
                // and composited onto the main surface with the layer opacity as tint alpha.

                // Finalize the current parent segment so the opacity layer is
                // inserted at the correct Z-position in the draw order
                let parent_segment =
                    std::mem::replace(&mut self.current_segment, DrawSegment::new());
                if !parent_segment.is_empty() {
                    self.draw_order.push(DrawItem::Segment(parent_segment));
                }

                self.draw_order
                    .push(DrawItem::OpacityLayer(PendingOpacityLayer {
                        items: offscreen_order,
                        final_segment: offscreen_segment,
                        opacity: saved.layer_opacity,
                        bounds: composite_bounds,
                    }));

                tracing::trace!(
                    "WgpuPainter::restore_layer: queued OpacityLayer \
                     (opacity={:.3}, bounds={:?})",
                    saved.layer_opacity,
                    composite_bounds
                );
            } else if has_offscreen_content {
                // Opacity is ~1.0, no compositing needed — merge content back.
                // Finalize the parent's pre-save content into the draw order
                // BEFORE re-integrating the offscreen items so that the parent
                // content renders beneath the layer subtree (correct Z-order).
                // Without this flush the parent segment sits in `current_segment`
                // and is emitted last by `render()`, placing it on top of the
                // layer — an inversion.  Mirror the mem::replace pattern used by
                // the opacity < 1.0 branch above.
                let parent_segment =
                    std::mem::replace(&mut self.current_segment, DrawSegment::new());
                if !parent_segment.is_empty() {
                    self.draw_order.push(DrawItem::Segment(parent_segment));
                }
                self.reintegrate_offscreen_content(offscreen_segment, offscreen_order, 1.0);
            }

            tracing::trace!(
                "WgpuPainter::restore_layer: restored opacity={:.3}, had_content={}",
                self.current_opacity,
                has_offscreen_content
            );
        } else {
            tracing::warn!("WgpuPainter::restore_layer: layer_stack underflow");

            // Fall back to legacy opacity_stack behavior for callers that didn't
            // go through the new save_layer path
            if let Some(prev_opacity) = self.opacity_stack.pop() {
                self.current_opacity = prev_opacity;
            }
        }
    }
}

// =============================================================================
// Advanced Effects API (Gradients, Shadows, Blur)
// =============================================================================

#[allow(clippy::cast_possible_truncation)]
impl WgpuPainter {
    /// Draw a rectangle with a linear gradient
    ///
    /// # Arguments
    /// * `bounds` - Rectangle bounds
    /// * `gradient_start` - Gradient start point (local coordinates)
    /// * `gradient_end` - Gradient end point (local coordinates)
    /// * `stops` - Gradient color stops (max 8)
    /// * `corner_radius` - Corner radius (uniform, 0.0 = sharp corners)
    ///
    /// # Example
    /// ```ignore
    /// // Vertical gradient from red to blue
    /// painter.gradient_rect(
    ///     Rect::from_ltrb(10.0, 10.0, 210.0, 110.0),
    ///     glam::Vec2::new(0.0, 0.0),   // Top
    ///     glam::Vec2::new(0.0, 100.0), // Bottom
    ///     &[
    ///         GradientStop::start(Color::RED),
    ///         GradientStop::end(Color::BLUE),
    ///     ],
    ///     12.0, // Rounded corners
    /// );
    /// ```
    pub fn gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        gradient_start: glam::Vec2,
        gradient_end: glam::Vec2,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::instancing::LinearGradientInstance;

        // Append gradient stops to global buffer (max 8 per gradient)
        let stop_count = stops.len().min(8);
        let current_len = self.current_segment.current_gradient_stops.len();
        if current_len + stop_count > super::effects_pipeline::MAX_GRADIENT_STOPS {
            // Logged once per process: a >MAX_GRADIENT_STOPS frame would
            // otherwise spam this for every overflowing instance, every frame.
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = super::effects_pipeline::MAX_GRADIENT_STOPS,
                    "gradient_rect: gradient stop buffer full; dropping linear gradient \
                     instance (logged once per process)"
                );
            }
            return;
        }
        let stop_offset = current_len as u32;
        self.current_segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = LinearGradientInstance::new(
            [
                bounds.left().0,
                bounds.top().0,
                bounds.width().0,
                bounds.height().0,
            ],
            gradient_start,
            gradient_end,
            [corner_radius; 4],
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);

        if self.current_segment.linear_gradient_batch.add(instance) {
            // Batch full, flush will happen in render()
        }
        DrawSegment::push_scissor_region(
            &mut self.current_segment.linear_grad_scissors,
            self.current_scissor,
        );
    }

    /// Draw a rectangle with a radial gradient
    ///
    /// # Arguments
    /// * `bounds` - Rectangle bounds
    /// * `center` - Gradient center point (local coordinates)
    /// * `radius` - Gradient radius
    /// * `stops` - Gradient color stops (max 8)
    /// * `corner_radius` - Corner radius (uniform, 0.0 = sharp corners)
    ///
    /// # Example
    /// ```ignore
    /// // Radial gradient from white center to transparent edge
    /// painter.radial_gradient_rect(
    ///     Rect::from_ltrb(10.0, 10.0, 110.0, 110.0),
    ///     glam::Vec2::new(50.0, 50.0), // Center
    ///     50.0,                         // Radius
    ///     &[
    ///         GradientStop::start(Color::WHITE),
    ///         GradientStop::end(Color::TRANSPARENT),
    ///     ],
    ///     0.0, // Sharp corners
    /// );
    /// ```
    pub fn radial_gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        radius: f32,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::instancing::RadialGradientInstance;

        // Append gradient stops to global buffer (max 8 per gradient)
        let stop_count = stops.len().min(8);
        let current_len = self.current_segment.current_gradient_stops.len();
        if current_len + stop_count > super::effects_pipeline::MAX_GRADIENT_STOPS {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = super::effects_pipeline::MAX_GRADIENT_STOPS,
                    "radial_gradient_rect: gradient stop buffer full; dropping radial \
                     gradient instance (logged once per process)"
                );
            }
            return;
        }
        let stop_offset = current_len as u32;
        self.current_segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = RadialGradientInstance::new(
            [
                bounds.left().0,
                bounds.top().0,
                bounds.width().0,
                bounds.height().0,
            ],
            center,
            radius,
            [corner_radius; 4],
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);

        if self.current_segment.radial_gradient_batch.add(instance) {
            // Batch full, flush will happen in render()
        }
        DrawSegment::push_scissor_region(
            &mut self.current_segment.radial_grad_scissors,
            self.current_scissor,
        );
    }

    /// Draw a rectangle with a sweep (angular/conic) gradient
    ///
    /// # Arguments
    /// * `bounds` - Rectangle bounds
    /// * `center` - Gradient center point (local coordinates)
    /// * `start_angle` - Start angle in radians
    /// * `end_angle` - End angle in radians
    /// * `stops` - Gradient color stops (max 8)
    /// * `corner_radius` - Corner radius (uniform, 0.0 = sharp corners)
    pub fn sweep_gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        start_angle: f32,
        end_angle: f32,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        use super::instancing::SweepGradientInstance;

        // Append gradient stops to global buffer (max 8 per gradient)
        let stop_count = stops.len().min(8);
        let current_len = self.current_segment.current_gradient_stops.len();
        if current_len + stop_count > super::effects_pipeline::MAX_GRADIENT_STOPS {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    current_stops = current_len,
                    requested = stop_count,
                    limit = super::effects_pipeline::MAX_GRADIENT_STOPS,
                    "sweep_gradient_rect: gradient stop buffer full; dropping sweep \
                     gradient instance (logged once per process)"
                );
            }
            return;
        }
        let stop_offset = current_len as u32;
        self.current_segment
            .current_gradient_stops
            .extend_from_slice(&stops[..stop_count]);

        let instance = SweepGradientInstance::new(
            [
                bounds.left().0,
                bounds.top().0,
                bounds.width().0,
                bounds.height().0,
            ],
            center,
            start_angle,
            end_angle,
            [corner_radius; 4],
            stop_count as u32,
        )
        .with_stop_offset(stop_offset);

        if self.current_segment.sweep_gradient_batch.add(instance) {
            // Batch full, flush will happen in render()
        }
        DrawSegment::push_scissor_region(
            &mut self.current_segment.sweep_grad_scissors,
            self.current_scissor,
        );
    }

    /// Draw a shadow for a rectangle
    ///
    /// Renders an analytical shadow using Evan Wallace's technique.
    /// Single-pass O(1) rendering with quality indistinguishable from real
    /// Gaussian.
    ///
    /// # Arguments
    /// * `rect_pos` - Rectangle position [x, y]
    /// * `rect_size` - Rectangle size [width, height]
    /// * `corner_radius` - Corner radius (uniform)
    /// * `params` - Shadow parameters (offset, blur, color)
    ///
    /// # Example
    /// ```ignore
    /// use flui_engine::painter::effects::ShadowParams;
    /// use flui_types::styling::Color;
    /// use glam::Vec2;
    ///
    /// // Material Design elevation 2 shadow (offset.y=2, sigma=4, ~0.16 alpha)
    /// painter.shadow_rect(
    ///     [10.0, 10.0],
    ///     [200.0, 100.0],
    ///     12.0,
    ///     &ShadowParams::new(Vec2::new(0.0, 2.0), 4.0, Color::rgba(0, 0, 0, 41)),
    /// );
    /// ```
    pub fn shadow_rect(
        &mut self,
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &super::effects::ShadowParams,
    ) {
        use super::instancing::ShadowInstance;

        let instance = ShadowInstance::new(rect_pos, rect_size, corner_radius, params);

        if self.current_segment.shadow_batch.add(instance) {
            // Batch full, flush will happen in render()
        }
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use std::sync::Arc;

    use flui_types::{Point, Rect, Size, geometry::px};

    use super::WgpuPainter;

    /// Headless GPU device + queue for painter tests.
    fn test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter for painter tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Painter Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device for painter tests");
        (Arc::new(device), Arc::new(queue))
    }

    /// Regression for the damage-scissor leak: the painter is reused across
    /// frames, so `reset_frame_state` MUST clear a per-frame scissor or it
    /// would clip subsequent frames to a stale damage rect.
    #[test]
    fn reset_frame_state_clears_damage_scissor() {
        let (device, queue) = test_device_and_queue();
        let mut painter = WgpuPainter::with_shared_device(
            device,
            queue,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (100, 100),
        );

        // Simulate the per-frame damage clip the Renderer applies (unpaired).
        painter.clip_rect(Rect::from_origin_size(
            Point::ZERO,
            Size::new(px(50.0), px(50.0)),
        ));
        assert!(
            painter.current_scissor_for_test().is_some(),
            "clip_rect must set the current scissor"
        );

        painter.reset_frame_state();
        assert!(
            painter.current_scissor_for_test().is_none(),
            "reset_frame_state must clear the scissor so it cannot leak into the next frame"
        );
    }
}
