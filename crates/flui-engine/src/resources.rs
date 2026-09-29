//! GPU resource facade for `WgpuPainter`.
//!
//! [`GpuResources`] is the single owner of the four per-painter resource
//! managers that were previously held as separate fields on
//! [`crate::painter::WgpuPainter`]:
//!
//! | Previous painter field       | Sub-field                               |
//! |------------------------------|-----------------------------------------|
//! | `buffer_pool`                | `GpuResources::buffer_pool`             |
//! | `texture_cache`              | `GpuResources::texture_cache`           |
//! | `layer_texture_pool`         | `GpuResources::layer_texture_pool`      |
//! | `external_texture_registry`  | `GpuResources::external_texture_registry` |
//!
//! **Ownership note:** `layer_texture_pool` is owned here so that the
//! offscreen-effect passes can *borrow* it from `GpuResources` without
//! requiring a separate field on the painter. Those borrowers are the passes
//! that render into an intermediate texture — `advanced_blend`, `blur`,
//! `color_matrix`, `gamma`, and `mode` — each reaching it through
//! [`GpuResources::layer_texture_pool_mut`].
//!
//! **RAII is preserved verbatim.** `PooledTexture` returns to its pool on
//! `Drop`, `BufferPool` resets `in_use` counters on `BufferPool::reset()`, and
//! `TextureCache::end_frame_maintenance` eviction ordering is unchanged —
//! callers invoke it through [`GpuResources::texture_cache_mut`].
//!
//! ## Borrow-split safety
//!
//! All four sub-fields are distinct struct fields. No method on this struct
//! takes `&mut` references to two sub-fields simultaneously; callers reach each
//! pool via its independent accessor. The one call site that accesses both
//! `texture_cache` and `buffer_pool` in sequence (`flush_segment_cached_images`
//! in `painter`) borrows them sequentially — `texture_cache.get()` returns a
//! cloned view before `flush_texture_batch` (which uses `buffer_pool`) is
//! called — so no `&mut` aliasing issue arises at the call site.

use std::sync::Arc;

use crate::{
    buffer_pool::BufferPool, external_texture_registry::ExternalTextureRegistry,
    texture_cache::TextureCache, texture_pool::TexturePool, uniform_pool::UniformPool,
};

/// Single owner of the four GPU resource managers used by [`crate::painter::WgpuPainter`].
///
/// Provides `pub(crate)` accessors for each sub-pool so call sites reach them
/// without coupling to the other pools.
pub(crate) struct GpuResources {
    /// Per-frame vertex/index buffer pool.
    ///
    /// Resets `in_use` markers on `BufferPool::reset()` at frame end. Slice
    /// borrows inside a frame are scope-bound; the pool itself lives here for
    /// the painter's lifetime.
    buffer_pool: BufferPool,

    /// LRU texture cache with atlas packing for small images.
    ///
    /// `end_frame_maintenance` must be called **exactly once per frame** after
    /// the final `WgpuPainter::render` invocation. The painter forwards this
    /// call via `WgpuPainter::end_frame_maintenance`.
    texture_cache: TextureCache,

    /// Pool of offscreen textures used for opacity-layer compositing.
    ///
    /// Owned here so `LayerCompositor` can borrow it via
    /// `layer_texture_pool_mut`. Each acquire returns a `PooledTexture` RAII
    /// handle that returns the texture to this pool on `Drop`.
    layer_texture_pool: TexturePool,

    /// Registry for externally-managed textures (video, camera, platform).
    ///
    /// Exposed via `WgpuPainter::external_texture_registry[_mut]` which
    /// delegate here.
    external_texture_registry: ExternalTextureRegistry,

    /// Reusable uniform-buffer pool for the filter/composite passes.
    ///
    /// `reset_frame` must run **exactly once per frame** (after the final
    /// submit) via `WgpuPainter::end_frame_maintenance` — NOT at the per-`render`
    /// `buffer_pool` reset, which would alias a backdrop-flush uniform with a
    /// final-render uniform in the same frame. See [`UniformPool`].
    uniform_pool: UniformPool,
}

impl GpuResources {
    /// Construct all four resource managers.
    ///
    /// `layer_texture_pool` is built **last** because it consumes `device` by
    /// value (`TexturePool::with_capacity(device, …)`); the other three clone
    /// the `Arc` first. This mirrors the construction previously inline in
    /// `WgpuPainter::with_shared_device`.
    pub(crate) fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let buffer_pool = BufferPool::new();
        // Built before `texture_cache` consumes `queue` (and `layer_texture_pool`
        // consumes `device`): the uniform pool keeps its own `Arc` clones.
        let uniform_pool = UniformPool::new(device.clone(), queue.clone());
        let texture_cache = TextureCache::new(device.clone(), queue);
        let external_texture_registry = ExternalTextureRegistry::new(device.clone());
        let layer_texture_pool = TexturePool::with_capacity(device, 4);

        Self {
            buffer_pool,
            texture_cache,
            layer_texture_pool,
            external_texture_registry,
            uniform_pool,
        }
    }

    // -------------------------------------------------------------------------
    // BufferPool accessors
    // -------------------------------------------------------------------------

    /// Exclusive reference to the per-frame vertex/index buffer pool.
    pub(crate) fn buffer_pool_mut(&mut self) -> &mut BufferPool {
        &mut self.buffer_pool
    }

    // -------------------------------------------------------------------------
    // TextureCache accessors
    // -------------------------------------------------------------------------

    /// Shared reference to the LRU texture cache.
    pub(crate) fn texture_cache(&self) -> &TextureCache {
        &self.texture_cache
    }

    /// Exclusive reference to the LRU texture cache.
    pub(crate) fn texture_cache_mut(&mut self) -> &mut TextureCache {
        &mut self.texture_cache
    }

    // -------------------------------------------------------------------------
    // TexturePool accessor
    // -------------------------------------------------------------------------

    /// Exclusive reference to the offscreen layer texture pool.
    ///
    /// `LayerCompositor` borrows this from `GpuResources` to
    /// acquire and return offscreen compositing textures.
    pub(crate) fn layer_texture_pool_mut(&mut self) -> &mut TexturePool {
        &mut self.layer_texture_pool
    }

    // -------------------------------------------------------------------------
    // UniformPool accessor
    // -------------------------------------------------------------------------

    /// Exclusive reference to the reusable filter/composite uniform-buffer pool.
    pub(crate) fn uniform_pool_mut(&mut self) -> &mut UniformPool {
        &mut self.uniform_pool
    }

    // -------------------------------------------------------------------------
    // ExternalTextureRegistry accessors
    // -------------------------------------------------------------------------

    /// Shared reference to the external texture registry.
    pub(crate) fn external_texture_registry(&self) -> &ExternalTextureRegistry {
        &self.external_texture_registry
    }

    /// Exclusive reference to the external texture registry.
    pub(crate) fn external_texture_registry_mut(&mut self) -> &mut ExternalTextureRegistry {
        &mut self.external_texture_registry
    }
}
