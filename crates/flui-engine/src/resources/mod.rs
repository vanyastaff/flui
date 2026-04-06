//! GPU resource management
//!
//! Buffer pools, texture caches, and atlas management for efficient
//! GPU memory usage across frames.

pub mod buffer_pool;
pub mod texture_atlas;
pub mod texture_cache;
pub mod texture_pool;

// Re-export primary types
pub use texture_atlas::{AtlasRect, TextureAtlas};

#[cfg(feature = "wgpu-backend")]
pub use buffer_pool::{BufferPool, PoolStats};

#[cfg(feature = "wgpu-backend")]
pub use texture_cache::{CachedTexture, TextureCache};

#[cfg(feature = "wgpu-backend")]
pub use texture_pool::{PooledTexture, TexturePool};
