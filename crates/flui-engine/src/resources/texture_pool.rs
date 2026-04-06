//! Texture pool for recycling GPU texture allocations.
//!
//! Manages a pool of offscreen render target textures for compositing
//! operations, reducing the cost of repeated texture allocation.

#[cfg(feature = "wgpu-backend")]
use wgpu;

/// A pooled offscreen render target texture.
#[cfg(feature = "wgpu-backend")]
pub struct PooledTexture {
    /// The GPU texture.
    pub texture: wgpu::Texture,
    /// Pre-created texture view.
    pub view: wgpu::TextureView,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Pool of offscreen render target textures for compositing operations.
///
/// Textures are acquired for rendering and released back when no longer
/// needed. The pool attempts to reuse textures with matching dimensions
/// to avoid repeated GPU allocations.
#[cfg(feature = "wgpu-backend")]
pub struct TexturePool {
    available: Vec<PooledTexture>,
    in_use_count: u32,
}

#[cfg(feature = "wgpu-backend")]
impl TexturePool {
    /// Create an empty texture pool.
    pub fn new() -> Self {
        Self {
            available: Vec::new(),
            in_use_count: 0,
        }
    }

    /// Acquire a render target texture of the given dimensions and format.
    ///
    /// If a matching texture exists in the pool, it is reused. Otherwise
    /// a new texture is created.
    pub fn acquire(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> PooledTexture {
        // Try to find a matching texture
        if let Some(idx) = self
            .available
            .iter()
            .position(|t| t.width == width && t.height == height)
        {
            self.in_use_count += 1;
            return self.available.swap_remove(idx);
        }

        // Create a new texture
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pool_render_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.in_use_count += 1;

        PooledTexture {
            texture,
            view,
            width,
            height,
        }
    }

    /// Return a texture to the pool for future reuse.
    pub fn release(&mut self, texture: PooledTexture) {
        self.in_use_count = self.in_use_count.saturating_sub(1);
        self.available.push(texture);
    }

    /// Drop all pooled textures.
    pub fn clear(&mut self) {
        self.available.clear();
    }

    /// Return the number of textures currently available in the pool.
    pub fn available_count(&self) -> usize {
        self.available.len()
    }
}

#[cfg(feature = "wgpu-backend")]
impl Default for TexturePool {
    fn default() -> Self {
        Self::new()
    }
}
