//! Texture cache for reusing GPU textures across frames.
//!
//! Caches loaded textures by a `u64` ID, tracking usage count
//! for eviction or diagnostic purposes.

#[cfg(feature = "wgpu-backend")]
use wgpu;

use std::collections::HashMap;

/// A cached GPU texture with associated view and metadata.
#[cfg(feature = "wgpu-backend")]
pub struct CachedTexture {
    /// The GPU texture.
    pub texture: wgpu::Texture,
    /// Pre-created texture view for binding.
    pub view: wgpu::TextureView,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Number of times this texture has been accessed via [`TextureCache::get`].
    pub use_count: u64,
}

/// Cache of GPU textures indexed by a `u64` identifier.
#[cfg(feature = "wgpu-backend")]
pub struct TextureCache {
    entries: HashMap<u64, CachedTexture>,
}

#[cfg(feature = "wgpu-backend")]
impl TextureCache {
    /// Create an empty texture cache.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Look up a cached texture by ID. Increments the use count on access.
    pub fn get(&mut self, id: u64) -> Option<&CachedTexture> {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.use_count += 1;
            // Re-borrow as immutable
            self.entries.get(&id)
        } else {
            None
        }
    }

    /// Insert a texture into the cache.
    pub fn insert(
        &mut self,
        id: u64,
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        self.entries.insert(
            id,
            CachedTexture {
                texture,
                view,
                width,
                height,
                use_count: 0,
            },
        );
    }

    /// Remove a texture from the cache by ID.
    pub fn remove(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    /// Check if the cache contains a texture with the given ID.
    pub fn contains(&self, id: u64) -> bool {
        self.entries.contains_key(&id)
    }

    /// Return the number of cached textures.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove all cached textures.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(feature = "wgpu-backend")]
impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}
