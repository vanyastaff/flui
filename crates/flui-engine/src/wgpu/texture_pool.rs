//! Texture pooling for offscreen rendering
//!
//! Manages GPU texture allocation and reuse to minimize allocation overhead
//! during shader mask rendering. Textures are created via `wgpu::Device` and
//! returned to the pool on drop for reuse.

use std::sync::Arc;

use flui_types::{Size, geometry::Pixels};
use parking_lot::Mutex;

/// Texture descriptor key for matching pooled textures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureDesc {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// wgpu texture format
    pub format: wgpu::TextureFormat,
}

impl TextureDesc {
    /// Create texture descriptor from size and format
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn from_size(size: Size<Pixels>, format: wgpu::TextureFormat) -> Self {
        Self {
            width: size.width.0.ceil().max(1.0) as u32,
            height: size.height.0.ceil().max(1.0) as u32,
            format,
        }
    }

    /// Get total size in bytes (approximate)
    pub fn size_bytes(&self) -> usize {
        let bpp = self.format.block_copy_size(None).unwrap_or(4) as usize;
        (self.width as usize) * (self.height as usize) * bpp
    }
}

/// GPU texture with its view, managed by the pool
///
/// Holds ownership of a `wgpu::Texture` and a default `wgpu::TextureView`.
/// These are moved in and out of the pool — never cloned.
pub struct GpuTexture {
    /// The actual GPU texture
    pub texture: wgpu::Texture,
    /// Default texture view (created at allocation time)
    pub view: wgpu::TextureView,
    /// Descriptor used to create this texture (for matching)
    pub desc: TextureDesc,
}

// wgpu::Texture does not implement Debug
impl std::fmt::Debug for GpuTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuTexture")
            .field("desc", &self.desc)
            .finish_non_exhaustive()
    }
}

/// Handle to a pooled texture. Returns the texture to the pool on drop.
///
/// Access the underlying GPU texture and view via [`texture()`](Self::texture)
/// and [`view()`](Self::view).
pub struct PooledTexture {
    /// Inner GPU texture — `Option` so we can `take()` in Drop
    gpu_texture: Option<GpuTexture>,
    /// Reference back to the pool for return-on-drop
    pool: Arc<Mutex<TexturePoolInner>>,
}

// Manual Debug because GpuTexture uses manual Debug. The `pool` Arc field is
// intentionally omitted — printing the inner `TexturePoolInner` would deadlock
// if Debug is called while the pool lock is held.
impl std::fmt::Debug for PooledTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledTexture")
            .field("desc", &self.desc())
            .field("has_texture", &self.gpu_texture.is_some())
            .finish_non_exhaustive()
    }
}

impl PooledTexture {
    /// Get texture descriptor
    pub fn desc(&self) -> &TextureDesc {
        &self
            .gpu_texture
            .as_ref()
            .expect("PooledTexture: gpu_texture taken before access")
            .desc
    }

    /// Get width in pixels
    pub fn width(&self) -> u32 {
        self.desc().width
    }

    /// Get height in pixels
    pub fn height(&self) -> u32 {
        self.desc().height
    }

    /// Get the underlying wgpu texture
    pub fn texture(&self) -> &wgpu::Texture {
        &self
            .gpu_texture
            .as_ref()
            .expect("PooledTexture: gpu_texture taken before access")
            .texture
    }

    /// Get the default texture view
    pub fn view(&self) -> &wgpu::TextureView {
        &self
            .gpu_texture
            .as_ref()
            .expect("PooledTexture: gpu_texture taken before access")
            .view
    }
}

impl Drop for PooledTexture {
    fn drop(&mut self) {
        if let Some(gpu_tex) = self.gpu_texture.take() {
            let mut pool = self.pool.lock();
            tracing::trace!("Returning texture to pool: {:?}", gpu_tex.desc);
            pool.return_texture(gpu_tex);
        }
    }
}

/// Internal texture pool state
struct TexturePoolInner {
    /// Available (idle) textures keyed by descriptor
    available: Vec<GpuTexture>,
    /// Total number of textures ever allocated (including those currently out)
    total_allocated: usize,
    /// Maximum number of idle textures to keep in the pool
    max_pool_size: usize,
    /// Total memory used by all allocated textures (bytes)
    total_memory_bytes: usize,
}

// Manual Debug because GpuTexture uses manual Debug
impl std::fmt::Debug for TexturePoolInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TexturePoolInner")
            .field("available_count", &self.available.len())
            .field("total_allocated", &self.total_allocated)
            .field("max_pool_size", &self.max_pool_size)
            .field("total_memory_bytes", &self.total_memory_bytes)
            .finish()
    }
}

impl TexturePoolInner {
    fn new(max_pool_size: usize) -> Self {
        Self {
            available: Vec::new(),
            total_allocated: 0,
            max_pool_size,
            total_memory_bytes: 0,
        }
    }

    /// Try to find and remove a matching texture from the available pool
    fn take_matching(&mut self, desc: &TextureDesc) -> Option<GpuTexture> {
        if let Some(idx) = self.available.iter().position(|t| t.desc == *desc) {
            tracing::trace!("Texture pool hit: {:?}", desc);
            Some(self.available.swap_remove(idx))
        } else {
            None
        }
    }

    /// Return a texture to the pool for future reuse
    fn return_texture(&mut self, gpu_tex: GpuTexture) {
        if self.available.len() < self.max_pool_size {
            tracing::trace!("Texture returned to pool: {:?}", gpu_tex.desc);
            self.available.push(gpu_tex);
        } else {
            // Pool full — discard the texture (GPU resource dropped)
            self.total_allocated = self.total_allocated.saturating_sub(1);
            self.total_memory_bytes = self
                .total_memory_bytes
                .saturating_sub(gpu_tex.desc.size_bytes());
            tracing::trace!("Texture pool full, discarding: {:?}", gpu_tex.desc);
            // gpu_tex is dropped here, releasing the GPU resource
        }
    }

    /// Clear all idle textures from the pool
    fn clear(&mut self) {
        let count = self.available.len();
        let freed_bytes: usize = self.available.iter().map(|t| t.desc.size_bytes()).sum();
        self.available.clear();
        self.total_allocated = self.total_allocated.saturating_sub(count);
        self.total_memory_bytes = self.total_memory_bytes.saturating_sub(freed_bytes);
        tracing::info!("Texture pool cleared ({count} textures released)");
    }
}

/// Thread-safe texture pool for offscreen rendering
///
/// Manages allocation and reuse of GPU textures to minimize overhead.
/// Textures are created via `wgpu::Device::create_texture()` with
/// `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC` usage flags.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::wgpu::TexturePool;
///
/// let pool = TexturePool::new(device.clone());
/// let texture = pool.acquire(800, 600, wgpu::TextureFormat::Rgba8UnormSrgb);
///
/// // Use texture.texture() and texture.view() for rendering...
///
/// // Texture automatically returned to pool when dropped
/// ```
#[allow(missing_debug_implementations)]
pub struct TexturePool {
    inner: Arc<Mutex<TexturePoolInner>>,
    device: Arc<wgpu::Device>,
}

impl TexturePool {
    /// Create new texture pool with default settings
    ///
    /// Default max pool size: 16 idle textures
    pub fn new(device: Arc<wgpu::Device>) -> Self {
        Self::with_capacity(device, 16)
    }

    /// Create texture pool with specific max pool size for idle textures
    pub fn with_capacity(device: Arc<wgpu::Device>, max_pool_size: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TexturePoolInner::new(max_pool_size))),
            device,
        }
    }

    /// Acquire a texture from the pool (or create a new one)
    ///
    /// The returned [`PooledTexture`] automatically returns the GPU texture
    /// to the pool when dropped.
    #[must_use]
    pub fn acquire(&self, width: u32, height: u32, format: wgpu::TextureFormat) -> PooledTexture {
        let desc = TextureDesc {
            width: width.max(1),
            height: height.max(1),
            format,
        };

        let mut pool = self.inner.lock();

        // Try to reuse an existing texture
        let gpu_texture = if let Some(existing) = pool.take_matching(&desc) {
            existing
        } else {
            // Create a new GPU texture
            let gpu_tex = self.create_gpu_texture(&desc);
            pool.total_allocated += 1;
            pool.total_memory_bytes += desc.size_bytes();
            tracing::trace!(
                "Created new texture: {:?} (total: {}, memory: {} KB)",
                desc,
                pool.total_allocated,
                pool.total_memory_bytes / 1024
            );
            gpu_tex
        };

        PooledTexture {
            gpu_texture: Some(gpu_texture),
            pool: Arc::clone(&self.inner),
        }
    }

    /// Acquire a texture sized from a `Size<Pixels>` value
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn acquire_from_size(
        &self,
        size: Size<Pixels>,
        format: wgpu::TextureFormat,
    ) -> PooledTexture {
        let w = size.width.0.ceil().max(1.0) as u32;
        let h = size.height.0.ceil().max(1.0) as u32;
        self.acquire(w, h, format)
    }

    /// Get pool statistics
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let pool = self.inner.lock();
        PoolStats {
            total_allocated: pool.total_allocated,
            total_memory_bytes: pool.total_memory_bytes,
            available_count: pool.available.len(),
        }
    }

    /// Clear all idle textures from the pool
    pub fn clear(&self) {
        let mut pool = self.inner.lock();
        pool.clear();
    }

    /// Create a GPU texture matching the given descriptor
    fn create_gpu_texture(&self, desc: &TextureDesc) -> GpuTexture {
        let wgpu_desc = wgpu::TextureDescriptor {
            label: Some("TexturePool Offscreen"),
            size: wgpu::Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: desc.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };

        let texture = self.device.create_texture(&wgpu_desc);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        GpuTexture {
            texture,
            view,
            desc: *desc,
        }
    }
}

/// Texture pool statistics
#[derive(Debug, Clone, Copy)]
pub struct PoolStats {
    /// Total number of textures allocated (in-use + idle)
    pub total_allocated: usize,
    /// Total memory used by textures (bytes, approximate)
    pub total_memory_bytes: usize,
    /// Number of textures currently idle in the pool
    pub available_count: usize,
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    /// Helper: create a wgpu device for testing (headless)
    fn create_test_device() -> Arc<wgpu::Device> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("Failed to find a suitable GPU adapter for testing");

        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Test Device"),
                ..Default::default()
            }))
            .expect("Failed to create GPU device for testing");

        Arc::new(device)
    }

    #[test]
    fn test_texture_desc_size_bytes() {
        let desc = TextureDesc {
            width: 100,
            height: 100,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
        };
        // 100 * 100 * 4 bytes per pixel = 40,000 bytes
        assert_eq!(desc.size_bytes(), 40_000);
    }

    #[test]
    fn test_texture_desc_from_size() {
        let size = Size::new(px(1920.0), px(1080.0));
        let desc = TextureDesc::from_size(size, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(desc.width, 1920);
        assert_eq!(desc.height, 1080);
        assert_eq!(desc.format, wgpu::TextureFormat::Rgba8UnormSrgb);
    }

    #[test]
    fn test_texture_pool_acquire_creates_new() {
        let device = create_test_device();
        let pool = TexturePool::new(device);

        let texture = pool.acquire(100, 100, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(texture.width(), 100);
        assert_eq!(texture.height(), 100);

        let stats = pool.stats();
        assert_eq!(stats.total_allocated, 1);
    }

    #[test]
    fn test_texture_pool_reuse() {
        let device = create_test_device();
        let pool = TexturePool::new(device);
        let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;

        // Acquire and drop
        {
            let _tex = pool.acquire(100, 100, fmt);
            assert_eq!(pool.stats().total_allocated, 1);
        }

        // Should be returned to pool
        assert_eq!(pool.stats().available_count, 1);

        // Acquire again — should reuse
        {
            let _tex = pool.acquire(100, 100, fmt);
            assert_eq!(pool.stats().total_allocated, 1); // Still 1, reused
            assert_eq!(pool.stats().available_count, 0); // Taken from pool
        }
    }

    #[test]
    fn test_texture_pool_different_sizes() {
        let device = create_test_device();
        let pool = TexturePool::new(device);
        let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;

        let _tex1 = pool.acquire(100, 100, fmt);
        let _tex2 = pool.acquire(200, 200, fmt);

        assert_eq!(pool.stats().total_allocated, 2);
    }

    #[test]
    fn test_texture_pool_clear() {
        let device = create_test_device();
        let pool = TexturePool::new(device);
        let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;

        {
            let _tex = pool.acquire(100, 100, fmt);
        }
        assert_eq!(pool.stats().available_count, 1);

        pool.clear();
        assert_eq!(pool.stats().available_count, 0);
        assert_eq!(pool.stats().total_allocated, 0);
    }

    #[test]
    fn test_pooled_texture_drop_returns_to_pool() {
        let device = create_test_device();
        let pool = TexturePool::new(device);
        let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;

        {
            let _tex = pool.acquire(100, 100, fmt);
            assert_eq!(pool.stats().available_count, 0);
        }
        // After drop
        assert_eq!(pool.stats().available_count, 1);
    }

    #[test]
    fn test_pooled_texture_has_real_gpu_texture() {
        let device = create_test_device();
        let pool = TexturePool::new(device);

        let tex = pool.acquire(256, 256, wgpu::TextureFormat::Rgba8UnormSrgb);
        // Access the real wgpu::Texture and TextureView
        let _ = tex.texture();
        let _ = tex.view();
        assert_eq!(tex.width(), 256);
        assert_eq!(tex.height(), 256);
    }

    #[test]
    fn test_pool_max_size_eviction() {
        let device = create_test_device();
        let pool = TexturePool::with_capacity(device, 2);
        let fmt = wgpu::TextureFormat::Rgba8UnormSrgb;

        // Create and drop 3 textures — pool max is 2
        {
            let _t1 = pool.acquire(10, 10, fmt);
            let _t2 = pool.acquire(10, 10, fmt);
            let _t3 = pool.acquire(10, 10, fmt);
        }
        // Only 2 should be in the pool (third evicted)
        assert_eq!(pool.stats().available_count, 2);
    }

    #[test]
    fn test_different_formats_not_reused() {
        let device = create_test_device();
        let pool = TexturePool::new(device);

        // Drop an Rgba8UnormSrgb texture
        {
            let _tex = pool.acquire(64, 64, wgpu::TextureFormat::Rgba8UnormSrgb);
        }
        assert_eq!(pool.stats().available_count, 1);

        // Acquire Rgba16Float — should NOT reuse, should create new
        let _tex = pool.acquire(64, 64, wgpu::TextureFormat::Rgba16Float);
        assert_eq!(pool.stats().total_allocated, 2);
        // The Rgba8 one is still idle in pool
        assert_eq!(pool.stats().available_count, 1);
    }
}
