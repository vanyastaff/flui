//! GPU device abstraction wrapping wgpu Device + Queue.
//!
//! [`GpuDevice`] is the shared GPU state for the application. It owns the
//! `wgpu::Device`, `wgpu::Queue`, compiled pipelines, and resource pools.
//! Thread-safety is provided by `Arc` internals and `parking_lot::Mutex`
//! around mutable resources.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::context::capabilities::GpuCapabilities;
use crate::error::{RenderError, RenderResult};
use crate::pipelines::registry::PipelineRegistry;
use crate::resources::buffer_pool::BufferPool;
use crate::resources::texture_cache::TextureCache;
use crate::text::system::TextSystem;

/// Shared GPU state, one per application.
///
/// Created via [`GpuDevice::new_headless`] (for testing/CI) or
/// [`GpuDevice::new_with_surface`] (for windowed rendering).
pub struct GpuDevice {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    #[allow(dead_code)]
    adapter_info: wgpu::AdapterInfo,
    capabilities: GpuCapabilities,
    pipelines: PipelineRegistry,
    buffer_pool: parking_lot::Mutex<BufferPool>,
    texture_cache: parking_lot::Mutex<TextureCache>,
    text_system: parking_lot::Mutex<TextSystem>,
    default_format: wgpu::TextureFormat,
    unit_quad_vbo: wgpu::Buffer,
    unit_quad_ibo: wgpu::Buffer,
    texture_id_counter: AtomicU64,
    image_sampler: wgpu::Sampler,
    image_bind_group_layout: Arc<wgpu::BindGroupLayout>,
}

impl GpuDevice {
    /// Create a headless GPU device (no window surface).
    ///
    /// Selects the best backend for the current platform and creates a device
    /// with default limits. Useful for testing, CI, and off-screen rendering.
    pub fn new_headless() -> RenderResult<Self> {
        let backends = Self::select_backends();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|_| RenderError::NoAdapter)?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("flui_gpu_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
            trace: Default::default(),
        }))
        .map_err(|e| RenderError::DeviceCreation(Box::new(e)))?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let adapter_info = adapter.get_info();
        let capabilities = GpuCapabilities::from_adapter_info(&adapter_info, &adapter);
        let default_format = wgpu::TextureFormat::Bgra8Unorm;
        let pipelines = PipelineRegistry::new(&device, default_format);
        let text_system = TextSystem::new(
            &device,
            &queue,
            default_format,
            Some(Self::text_depth_stencil_state()),
        );
        let (unit_quad_vbo, unit_quad_ibo) = Self::create_unit_quad_buffers(&device);
        let (image_sampler, image_bind_group_layout) = Self::create_image_resources(&device);

        Ok(Self {
            device,
            queue,
            adapter_info,
            capabilities,
            pipelines,
            buffer_pool: parking_lot::Mutex::new(BufferPool::new()),
            texture_cache: parking_lot::Mutex::new(TextureCache::new()),
            text_system: parking_lot::Mutex::new(text_system),
            default_format,
            unit_quad_vbo,
            unit_quad_ibo,
            texture_id_counter: AtomicU64::new(0),
            image_sampler,
            image_bind_group_layout,
        })
    }

    /// Create a GPU device compatible with the given window surface.
    ///
    /// The surface is used to select the preferred texture format and a
    /// compatible adapter, but is **not** stored in `GpuDevice` (it belongs
    /// in [`RenderSurface`](super::render_surface)).
    pub fn new_with_surface(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'_>,
    ) -> RenderResult<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| RenderError::NoAdapter)?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("flui_gpu_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
            trace: Default::default(),
        }))
        .map_err(|e| RenderError::DeviceCreation(Box::new(e)))?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let adapter_info = adapter.get_info();
        let capabilities = GpuCapabilities::from_adapter_info(&adapter_info, &adapter);
        let default_format = surface
            .get_capabilities(&adapter)
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let pipelines = PipelineRegistry::new(&device, default_format);
        let text_system = TextSystem::new(
            &device,
            &queue,
            default_format,
            Some(Self::text_depth_stencil_state()),
        );
        let (unit_quad_vbo, unit_quad_ibo) = Self::create_unit_quad_buffers(&device);
        let (image_sampler, image_bind_group_layout) = Self::create_image_resources(&device);

        Ok(Self {
            device,
            queue,
            adapter_info,
            capabilities,
            pipelines,
            buffer_pool: parking_lot::Mutex::new(BufferPool::new()),
            texture_cache: parking_lot::Mutex::new(TextureCache::new()),
            text_system: parking_lot::Mutex::new(text_system),
            default_format,
            unit_quad_vbo,
            unit_quad_ibo,
            texture_id_counter: AtomicU64::new(0),
            image_sampler,
            image_bind_group_layout,
        })
    }

    /// The wgpu device handle.
    #[must_use]
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// The wgpu queue handle.
    #[must_use]
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    /// Detected GPU capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &GpuCapabilities {
        &self.capabilities
    }

    /// The compiled pipeline registry.
    #[must_use]
    pub fn pipelines(&self) -> &PipelineRegistry {
        &self.pipelines
    }

    /// The pooled buffer allocator (locked on access).
    #[must_use]
    pub fn buffer_pool(&self) -> &parking_lot::Mutex<BufferPool> {
        &self.buffer_pool
    }

    /// The texture cache (locked on access).
    #[must_use]
    pub fn texture_cache(&self) -> &parking_lot::Mutex<TextureCache> {
        &self.texture_cache
    }

    /// The text rendering system (locked on access).
    #[must_use]
    pub fn text_system(&self) -> &parking_lot::Mutex<TextSystem> {
        &self.text_system
    }

    /// The default texture format for this device.
    #[must_use]
    pub fn default_format(&self) -> wgpu::TextureFormat {
        self.default_format
    }

    /// The shared image sampler for all texture rendering.
    #[must_use]
    pub fn image_sampler(&self) -> &wgpu::Sampler {
        &self.image_sampler
    }

    /// The shared bind group layout for image texture + sampler at group(1).
    #[must_use]
    pub fn image_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.image_bind_group_layout
    }

    /// Load image from raw bytes (PNG, JPEG, etc.) and upload to GPU.
    ///
    /// Returns a unique texture ID that can be used with the image batcher.
    /// The texture is cached in the [`TextureCache`] and can be reused across frames.
    #[cfg(feature = "images")]
    pub fn load_image(&self, data: &[u8]) -> RenderResult<u64> {
        let img = image::load_from_memory(data)
            .map_err(|e| RenderError::resource(format!("image decode: {e}")))?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("loaded_image"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let id = self.texture_id_counter.fetch_add(1, Ordering::Relaxed) + 1;
        self.texture_cache
            .lock()
            .insert(id, texture, view, width, height);
        Ok(id)
    }

    /// The shared bind group layout used by all pipelines for `FrameUniforms`.
    #[must_use]
    pub fn bind_group_layout(&self) -> &Arc<wgpu::BindGroupLayout> {
        self.pipelines.bind_group_layout()
    }

    /// Shared unit quad vertex buffer (4 vertices as `[f32; 2]`).
    #[must_use]
    pub fn unit_quad_vbo(&self) -> &wgpu::Buffer {
        &self.unit_quad_vbo
    }

    /// Shared unit quad index buffer (6 indices as `u16`).
    #[must_use]
    pub fn unit_quad_ibo(&self) -> &wgpu::Buffer {
        &self.unit_quad_ibo
    }

    /// Create an offscreen render texture and its view.
    ///
    /// The texture is suitable for use as a `RENDER_ATTACHMENT` and `COPY_SRC`,
    /// enabling headless rendering followed by pixel readback via
    /// [`read_texture_to_rgba`](super::headless_render::read_texture_to_rgba).
    #[must_use]
    pub fn create_render_texture(
        &self,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless_render_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.default_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Create the shared image sampler and bind group layout.
    fn create_image_resources(
        device: &wgpu::Device,
    ) -> (wgpu::Sampler, Arc<wgpu::BindGroupLayout>) {
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let image_bind_group_layout = Arc::new(device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("image_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
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
            },
        ));

        (image_sampler, image_bind_group_layout)
    }

    /// Depth/stencil state used by the text renderer pipeline.
    ///
    /// The text renderer must be aware of the depth/stencil attachment on
    /// the render pass even though it neither reads nor writes depth/stencil
    /// values. Without this, wgpu rejects the render pass due to pipeline
    /// incompatibility.
    fn text_depth_stencil_state() -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24PlusStencil8,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Keep,
                },
                back: wgpu::StencilFaceState {
                    compare: wgpu::CompareFunction::Always,
                    fail_op: wgpu::StencilOperation::Keep,
                    depth_fail_op: wgpu::StencilOperation::Keep,
                    pass_op: wgpu::StencilOperation::Keep,
                },
                read_mask: 0xFF,
                write_mask: 0x00,
            },
            bias: wgpu::DepthBiasState::default(),
        }
    }

    /// Create the shared unit quad vertex and index buffers.
    ///
    /// The quad covers `[0,0]..=[1,1]` and is used by all instanced draw calls
    /// (rects, circles, arcs, images, shadows).
    fn create_unit_quad_buffers(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::Buffer) {
        let vertices: &[f32] = &[
            0.0, 0.0, // top-left
            1.0, 0.0, // top-right
            1.0, 1.0, // bottom-right
            0.0, 1.0, // bottom-left
        ];
        let indices: &[u16] = &[0, 1, 2, 0, 2, 3];

        let vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("unit_quad_vbo"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("unit_quad_ibo"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        (vbo, ibo)
    }

    /// Select the best wgpu backend for the current platform.
    fn select_backends() -> wgpu::Backends {
        #[cfg(target_os = "macos")]
        {
            wgpu::Backends::METAL
        }
        #[cfg(target_os = "windows")]
        {
            wgpu::Backends::DX12
        }
        #[cfg(target_os = "linux")]
        {
            wgpu::Backends::VULKAN
        }
        #[cfg(target_arch = "wasm32")]
        {
            wgpu::Backends::BROWSER_WEBGPU
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_arch = "wasm32"
        )))]
        {
            wgpu::Backends::all()
        }
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    #[test]
    fn new_headless_creates_device() {
        let gpu = GpuDevice::new_headless().expect("headless GPU should work");
        assert!(!gpu.capabilities().adapter_name.is_empty());
    }
}
