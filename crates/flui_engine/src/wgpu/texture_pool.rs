//! Texture pooling for offscreen rendering
//!
//! Manages GPU texture allocation and reuse to minimize allocation overhead
//! during shader mask rendering.

use flui_types::{geometry::Pixels, Size};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Texture descriptor for identifying pooled textures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureDesc {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Texture format (simplified for now)
    pub format: TextureFormat,
}

impl TextureDesc {
    /// Create texture descriptor from size
    pub fn from_size(size: Size<Pixels>) -> Self {
        Self {
            width: size.width.0.ceil() as u32,
            height: size.height.0.ceil() as u32,
            format: TextureFormat::Rgba8,
        }
    }

    /// Get total size in bytes
    pub fn size_bytes(&self) -> usize {
        let bytes_per_pixel = self.format.bytes_per_pixel();
        (self.width * self.height) as usize * bytes_per_pixel
    }
}

/// Simplified texture format enum
///
/// In full implementation, this would map to wgpu::TextureFormat
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    /// RGBA 8-bit per channel (32-bit total)
    Rgba8,
    /// RGBA 16-bit float per channel (64-bit total)
    Rgba16Float,
}

impl TextureFormat {
    /// Get bytes per pixel for this format
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            TextureFormat::Rgba8 => 4,       // 4 bytes (8 bits × 4 channels)
            TextureFormat::Rgba16Float => 8, // 8 bytes (16 bits × 4 channels)
        }
    }
}

/// Pooled texture handle
///
/// When dropped, returns the texture to the pool for reuse.
///
/// # Implementation Note
///
/// In full wgpu integration, this would hold:
/// - wgpu::Texture
/// - wgpu::TextureView
/// - Descriptor information
#[derive(Debug)]
pub struct PooledTexture {
    desc: TextureDesc,
    pool: Arc<Mutex<TexturePoolInner>>,
    // TODO: Add actual wgpu::Texture when integrating with renderer
    // texture: wgpu::Texture,
    // view: wgpu::TextureView,
}

impl PooledTexture {
    /// Get texture descriptor
    pub fn desc(&self) -> &TextureDesc {
        &self.desc
    }

    /// Get width
    pub fn width(&self) -> u32 {
        self.desc.width
    }

    /// Get height
    pub fn height(&self) -> u32 {
        self.desc.height
    }

    // TODO: Add wgpu texture accessors when integrated
    // pub fn texture(&self) -> &wgpu::Texture { &self.texture }
    // pub fn view(&self) -> &wgpu::TextureView { &self.view }
}

impl Drop for PooledTexture {
    fn drop(&mut self) {
        // Return texture to pool (parking_lot::Mutex never panics on lock)
        let mut pool = self.pool.lock();
        pool.return_texture(self.desc);
        tracing::trace!("Returned texture to pool: {:?}", self.desc);
    }
}

/// Internal texture pool implementation
#[derive(Debug)]
struct TexturePoolInner {
    /// Available textures by descriptor
    available: HashMap<TextureDesc, Vec<TextureDesc>>,
    /// Total allocated textures count
    total_allocated: usize,
    /// Maximum pool size (number of textures)
    max_pool_size: usize,
    /// Total memory used (bytes)
    total_memory_bytes: usize,
}

impl TexturePoolInner {
    fn new(max_pool_size: usize) -> Self {
        Self {
            available: HashMap::new(),
            total_allocated: 0,
            max_pool_size,
            total_memory_bytes: 0,
        }
    }

    fn acquire_texture(&mut self, desc: TextureDesc) -> Option<TextureDesc> {
        if let Some(textures) = self.available.get_mut(&desc) {
            if let Some(texture_desc) = textures.pop() {
                tracing::trace!("Texture pool hit: {:?}", desc);
                return Some(texture_desc);
            }
        }
        None
    }

    fn create_texture(&mut self, desc: TextureDesc) -> TextureDesc {
        self.total_allocated += 1;
        self.total_memory_bytes += desc.size_bytes();
        tracing::trace!(
            "Created new texture: {:?} (total: {}, memory: {} MB)",
            desc,
            self.total_allocated,
            self.total_memory_bytes / (1024 * 1024)
        );
        desc
    }

    fn return_texture(&mut self, desc: TextureDesc) {
        let entry = self.available.entry(desc).or_default();

        // Check if pool is full
        if entry.len() < self.max_pool_size {
            entry.push(desc);
            tracing::trace!("Texture returned to pool: {:?}", desc);
        } else {
            // Pool full, discard texture
            self.total_allocated -= 1;
            self.total_memory_bytes -= desc.size_bytes();
            tracing::trace!("Texture pool full, discarding: {:?}", desc);
        }
    }

    fn clear(&mut self) {
        let total_textures: usize = self.available.values().map(|v| v.len()).sum();
        self.available.clear();
        self.total_allocated -= total_textures;
        self.total_memory_bytes = 0;
        tracing::info!(
            "Texture pool cleared ({} textures released)",
            total_textures
        );
    }
}

/// Thread-safe texture pool for offscreen rendering
///
/// Manages allocation and reuse of GPU textures to minimize overhead.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::layer::TexturePool;
/// use flui_types::Size;
///
/// let pool = TexturePool::new();
/// let texture = pool.acquire(Size::new(1920.0, 1080.0));
///
/// // Use texture for rendering...
///
/// // Texture automatically returned to pool when dropped
/// ```
#[derive(Debug)]
pub struct TexturePool {
    inner: Arc<Mutex<TexturePoolInner>>,
}

impl TexturePool {
    /// Create new texture pool with default settings
    ///
    /// Default max pool size: 10 textures per descriptor
    pub fn new() -> Self {
        Self::with_capacity(10)
    }

    /// Create texture pool with specific max pool size
    pub fn with_capacity(max_pool_size: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TexturePoolInner::new(max_pool_size))),
        }
    }

    /// Acquire a texture from the pool or create a new one
    ///
    /// Returns a pooled texture that will be automatically returned
    /// to the pool when dropped.
    #[must_use]
    pub fn acquire(&self, size: Size<Pixels>) -> PooledTexture {
        let desc = TextureDesc::from_size(size);
        let mut pool = self.inner.lock();

        // Try to get from pool first
        if pool.acquire_texture(desc).is_none() {
            // Not in pool, create new texture
            pool.create_texture(desc);
        }

        PooledTexture {
            desc,
            pool: Arc::clone(&self.inner),
        }
    }

    /// Get pool statistics
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let pool = self.inner.lock();
        PoolStats {
            total_allocated: pool.total_allocated,
            total_memory_bytes: pool.total_memory_bytes,
            available_count: pool.available.values().map(|v| v.len()).sum(),
        }
    }

    /// Clear all textures from the pool
    pub fn clear(&self) {
        let mut pool = self.inner.lock();
        pool.clear();
    }
}

impl Default for TexturePool {
    fn default() -> Self {
        Self::new()
    }
}

/// Texture pool statistics
#[derive(Debug, Clone, Copy)]
pub struct PoolStats {
    /// Total number of textures allocated
    pub total_allocated: usize,
    /// Total memory used by textures (bytes)
    pub total_memory_bytes: usize,
    /// Number of textures available in pool
    pub available_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_texture_desc_from_size() {
        let size = Size::new(1920.0, 1080.0);
        let desc = TextureDesc::from_size(size);

        assert_eq!(desc.width, 1920);
        assert_eq!(desc.height, 1080);
        assert_eq!(desc.format, TextureFormat::Rgba8);
    }

    #[test]
    fn test_texture_desc_size_bytes() {
        let desc = TextureDesc {
            width: 100,
            height: 100,
            format: TextureFormat::Rgba8,
        };

        // 100 * 100 * 4 bytes per pixel = 40,000 bytes
        assert_eq!(desc.size_bytes(), 40_000);
    }

    #[test]
    fn test_texture_format_bytes_per_pixel() {
        assert_eq!(TextureFormat::Rgba8.bytes_per_pixel(), 4);
        assert_eq!(TextureFormat::Rgba16Float.bytes_per_pixel(), 8);
    }

    #[test]
    fn test_texture_pool_acquire_creates_new() {
        let pool = TexturePool::new();
        let size = Size::new(100.0, 100.0);

        let texture = pool.acquire(size);
        assert_eq!(texture.width(), 100);
        assert_eq!(texture.height(), 100);

        let stats = pool.stats();
        assert_eq!(stats.total_allocated, 1);
    }

    #[test]
    fn test_texture_pool_reuse() {
        let pool = TexturePool::new();
        let size = Size::new(100.0, 100.0);

        // Acquire and drop first texture
        {
            let _texture1 = pool.acquire(size);
            assert_eq!(pool.stats().total_allocated, 1);
        }

        // Should be returned to pool now
        assert_eq!(pool.stats().available_count, 1);

        // Acquire again - should reuse
        {
            let _texture2 = pool.acquire(size);
            assert_eq!(pool.stats().total_allocated, 1); // Still 1, reused
            assert_eq!(pool.stats().available_count, 0); // Taken from pool
        }
    }

    #[test]
    fn test_texture_pool_different_sizes() {
        let pool = TexturePool::new();

        let size1 = Size::new(100.0, 100.0);
        let size2 = Size::new(200.0, 200.0);

        let _texture1 = pool.acquire(size1);
        let _texture2 = pool.acquire(size2);

        // Should have 2 different textures
        let stats = pool.stats();
        assert_eq!(stats.total_allocated, 2);
    }

    #[test]
    fn test_texture_pool_clear() {
        let pool = TexturePool::new();
        let size = Size::new(100.0, 100.0);

        {
            let _texture = pool.acquire(size);
        }

        assert_eq!(pool.stats().available_count, 1);

        pool.clear();
        assert_eq!(pool.stats().available_count, 0);
        assert_eq!(pool.stats().total_allocated, 0);
    }

    #[test]
    fn test_pooled_texture_drop_returns_to_pool() {
        let pool = TexturePool::new();
        let size = Size::new(100.0, 100.0);

        {
            let _texture = pool.acquire(size);
            assert_eq!(pool.stats().available_count, 0);
        }

        // After drop, should be back in pool
        assert_eq!(pool.stats().available_count, 1);
    }
}
