//! GPU device abstraction wrapping wgpu Device + Queue.
//!
//! [`GpuDevice`] is the shared GPU state for the application. It owns the
//! `wgpu::Device`, `wgpu::Queue`, compiled pipelines, and resource pools.
//! Thread-safety is provided by `Arc` internals and `parking_lot::Mutex`
//! around mutable resources.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::context::capabilities::GpuCapabilities;
use crate::error::{RenderError, RenderResult};
use crate::pipelines::registry::PipelineRegistry;
use crate::resources::buffer_pool::BufferPool;
use crate::resources::texture_cache::TextureCache;

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
    default_format: wgpu::TextureFormat,
    unit_quad_vbo: wgpu::Buffer,
    unit_quad_ibo: wgpu::Buffer,
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
        let (unit_quad_vbo, unit_quad_ibo) = Self::create_unit_quad_buffers(&device);

        Ok(Self {
            device,
            queue,
            adapter_info,
            capabilities,
            pipelines,
            buffer_pool: parking_lot::Mutex::new(BufferPool::new()),
            texture_cache: parking_lot::Mutex::new(TextureCache::new()),
            default_format,
            unit_quad_vbo,
            unit_quad_ibo,
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
        let (unit_quad_vbo, unit_quad_ibo) = Self::create_unit_quad_buffers(&device);

        Ok(Self {
            device,
            queue,
            adapter_info,
            capabilities,
            pipelines,
            buffer_pool: parking_lot::Mutex::new(BufferPool::new()),
            texture_cache: parking_lot::Mutex::new(TextureCache::new()),
            default_format,
            unit_quad_vbo,
            unit_quad_ibo,
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

    /// The default texture format for this device.
    #[must_use]
    pub fn default_format(&self) -> wgpu::TextureFormat {
        self.default_format
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
