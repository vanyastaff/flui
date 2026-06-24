//! Offscreen rendering infrastructure for shader masks
//!
//! Manages GPU pipelines, render passes, and offscreen texture rendering
//! for ShaderMaskLayer effects.
//!
//! # Sub-modules
//!
//! - `mask`  — `get_or_create_pipeline` + `render_masked`
//! - `blur`  — `get_or_create_blur_pipelines` + `render_blur`
//! - `blit`  — `get_or_create_blit_pipeline` + `blit_to_surface`

use std::{collections::HashMap, sync::Arc};

use bytemuck::{Pod, Zeroable};
use flui_types::painting::BlendMode;
use wgpu::util::DeviceExt;

use super::{
    shader_compiler::{ShaderCache, ShaderType},
    texture_pool::{PooledTexture, TexturePool},
};

mod blit;
mod blur;
mod mask;

/// Maximum number of Dual Kawase blur iterations.
///
/// Matches the `.clamp(1, 5)` in `render_blur`; used to pre-allocate the
/// reusable uniform buffer pool so `render_blur` never calls `create_buffer_init`
/// inside its hot loop.
const MAX_BLUR_ITERATIONS: usize = 5;

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

    /// Fullscreen blit pipeline — lazily created when the intermediate-active
    /// present path is first used (COPY_SRC-less adapters, or forced in tests).
    blit_pipeline: Option<BlitPipeline>,

    /// Shared linear sampler reused by `render_masked` and `render_blur`.
    ///
    /// Parameters: `ClampToEdge` × `Linear` — invariant across all calls.
    /// Created once in the constructor; eliminates one `create_sampler` call
    /// per `render_masked` invocation and one per `render_blur` invocation.
    linear_sampler: wgpu::Sampler,

    /// Fullscreen-quad vertex buffer shared by `render_masked` and `render_blur`.
    ///
    /// Contains 6 vertices (2 triangles) covering clip-space `[-1, 1]²`.
    /// Created once in the constructor; eliminates one `create_buffer_init` per
    /// `render_masked` invocation and one per `render_blur` invocation.
    fullscreen_quad_vb: wgpu::Buffer,

    /// Pre-allocated `BlurParams` uniform buffers — one slot per possible
    /// Dual Kawase iteration (`MAX_BLUR_ITERATIONS = 5`).
    ///
    /// Both the downsample pass (iterations 0..N) and the upsample pass
    /// (iterations N-1..0) index into this pool, so the pool needs
    /// `MAX_BLUR_ITERATIONS` slots.  Each slot is updated with
    /// `queue.write_buffer` before the pass that uses it, replacing the
    /// previous `create_buffer_init` call that allocated a fresh GPU buffer
    /// every iteration.
    ///
    /// Soundness: each buffer is written before it is used in the same
    /// submission, and `queue.submit` is called once at the end of
    /// `render_blur` — so a write at iteration `i` is always visible to the
    /// draw call that references slot `i`.
    blur_uniform_buffers: Vec<wgpu::Buffer>,
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
        let linear_sampler = Self::create_linear_sampler(&device);
        let fullscreen_quad_vb = Self::create_fullscreen_quad_vb(&device);
        let blur_uniform_buffers = Self::create_blur_uniform_buffers(&device);

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
            blit_pipeline: None,
            linear_sampler,
            fullscreen_quad_vb,
            blur_uniform_buffers,
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
        let linear_sampler = Self::create_linear_sampler(&device);
        let fullscreen_quad_vb = Self::create_fullscreen_quad_vb(&device);
        let blur_uniform_buffers = Self::create_blur_uniform_buffers(&device);

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
            blit_pipeline: None,
            linear_sampler,
            fullscreen_quad_vb,
            blur_uniform_buffers,
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

    /// Create the shared linear sampler used by `render_masked` and `render_blur`.
    ///
    /// Parameters are `ClampToEdge × Linear` — invariant across all calls.
    fn create_linear_sampler(device: &wgpu::Device) -> wgpu::Sampler {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Offscreen Linear Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        })
    }

    /// Create the shared fullscreen-quad vertex buffer.
    ///
    /// 6 vertices (2 triangles) covering clip-space `[-1, 1]²` — content
    /// never changes, so it is allocated once and reused across all passes.
    fn create_fullscreen_quad_vb(device: &wgpu::Device) -> wgpu::Buffer {
        let vertices = FullscreenVertex::fullscreen_quad();
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Offscreen Fullscreen Quad VB"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        })
    }

    /// Pre-allocate `MAX_BLUR_ITERATIONS` reusable `BlurParams` uniform buffers.
    ///
    /// Each buffer is sized for one `BlurParams` struct and flagged
    /// `UNIFORM | COPY_DST` so `queue.write_buffer` can update it in-place
    /// before each pass.  This eliminates the per-iteration `create_buffer_init`
    /// call inside `render_blur`.
    fn create_blur_uniform_buffers(device: &wgpu::Device) -> Vec<wgpu::Buffer> {
        let buf_size = std::mem::size_of::<BlurParams>() as u64;
        (0..MAX_BLUR_ITERATIONS)
            .map(|i| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("Blur Uniform Buffer {i}")),
                    size: buf_size,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect()
    }

    /// Pre-compile all shaders (reduces first-use latency)
    pub fn warmup(&mut self) {
        self.shader_cache.precompile_all();

        // Pre-create all pipelines
        self.get_or_create_pipeline(ShaderType::SolidMask);
        self.get_or_create_pipeline(ShaderType::LinearGradientMask);
        self.get_or_create_pipeline(ShaderType::RadialGradientMask);
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
    // Device/Queue. The functionality is tested via integration tests with actual GPU.

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
