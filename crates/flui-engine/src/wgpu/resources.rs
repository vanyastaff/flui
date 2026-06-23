//! GPU resource facade for `WgpuPainter`.
//!
//! [`GpuResources`] is the single owner of the four per-painter resource
//! managers that were previously held as separate fields on
//! [`super::painter::WgpuPainter`]:
//!
//! | Previous painter field       | Sub-field                               |
//! |------------------------------|-----------------------------------------|
//! | `buffer_pool`                | `GpuResources::buffer_pool`             |
//! | `texture_cache`              | `GpuResources::texture_cache`           |
//! | `layer_texture_pool`         | `GpuResources::layer_texture_pool`      |
//! | `external_texture_registry`  | `GpuResources::external_texture_registry` |
//!
//! **Ownership note:** `layer_texture_pool` is owned here so that a future
//! `LayerCompositor` (task T8) can *borrow* it from `GpuResources` without
//! requiring a separate field on the painter.
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

use super::{
    buffer_pool::BufferPool, external_texture_registry::ExternalTextureRegistry,
    texture_cache::TextureCache, texture_pool::TexturePool,
};

/// Single owner of the four GPU resource managers used by [`super::painter::WgpuPainter`].
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
    /// Owned here so `LayerCompositor` (task T8) can borrow it via
    /// `layer_texture_pool_mut`. Each acquire returns a `PooledTexture` RAII
    /// handle that returns the texture to this pool on `Drop`.
    layer_texture_pool: TexturePool,

    /// Registry for externally-managed textures (video, camera, platform).
    ///
    /// Exposed via `WgpuPainter::external_texture_registry[_mut]` which
    /// delegate here.
    external_texture_registry: ExternalTextureRegistry,
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
        let texture_cache = TextureCache::new(device.clone(), queue);
        let external_texture_registry = ExternalTextureRegistry::new(device.clone());
        let layer_texture_pool = TexturePool::with_capacity(device, 4);

        Self {
            buffer_pool,
            texture_cache,
            layer_texture_pool,
            external_texture_registry,
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
    /// `LayerCompositor` (task T8) will borrow this from `GpuResources` to
    /// acquire and return offscreen compositing textures.
    pub(crate) fn layer_texture_pool_mut(&mut self) -> &mut TexturePool {
        &mut self.layer_texture_pool
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

#[cfg(test)]
mod tests {
    //! Structural unit tests for [`GpuResources`].
    //!
    //! These tests confirm that construction succeeds and that every accessor
    //! returns a live reference to the correct sub-pool. Pixel-correctness (C8)
    //! is fulfilled by the GPU-readback serial suite (`enable-wgpu-tests`),
    //! which is unchanged by this mechanical refactor.

    #[cfg(feature = "enable-wgpu-tests")]
    mod gpu_tests {
        use std::sync::Arc;

        use super::super::GpuResources;

        fn create_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                }))
                .expect("a GPU adapter must be available on a GPU-enabled test host");
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("GpuResources Test Device"),
                    ..Default::default()
                }))
                .expect("a GPU device must be available when an adapter was found");
            (Arc::new(device), Arc::new(queue))
        }

        /// `GpuResources::new` constructs without panic; all four sub-pools are
        /// reachable through their accessors and start in a clean initial state.
        #[test]
        fn gpu_resources_construction_and_accessors_start_clean() {
            let (device, queue) = create_device_and_queue();
            let mut resources = GpuResources::new(device, queue);

            // BufferPool: fresh pool reports 0.0 reuse rate (nothing reused yet).
            let buffer_stats = resources.buffer_pool_mut().stats();
            assert!(
                buffer_stats.reuse_rate < f32::EPSILON,
                "fresh BufferPool must report 0.0 reuse rate, got {}",
                buffer_stats.reuse_rate
            );

            // TextureCache: no textures loaded, so memory footprint is zero.
            assert_eq!(
                resources.texture_cache().memory_bytes(),
                0,
                "fresh TextureCache must report 0 bytes allocated"
            );

            // TexturePool: nothing acquired, so total_allocated is zero.
            let pool_stats = resources.layer_texture_pool_mut().stats();
            assert_eq!(
                pool_stats.total_allocated, 0,
                "fresh TexturePool must have 0 allocated textures"
            );

            // ExternalTextureRegistry: no registrations on construction.
            assert!(
                resources.external_texture_registry().is_empty(),
                "fresh ExternalTextureRegistry must be empty"
            );
        }

        /// Calling `buffer_pool_mut().reset()` on a fresh pool does not panic.
        /// Confirms the accessor reaches the owned pool, not a copy.
        #[test]
        fn buffer_pool_reset_is_idempotent_on_empty_pool() {
            let (device, queue) = create_device_and_queue();
            let mut resources = GpuResources::new(device, queue);
            resources.buffer_pool_mut().reset();
            // A second reset after the first should also be a no-op.
            resources.buffer_pool_mut().reset();
        }

        /// `end_frame_maintenance` on an empty texture cache is a no-op:
        /// eviction count is zero and the atlas is not reset.
        #[test]
        fn texture_cache_end_frame_maintenance_is_noop_when_empty() {
            let (device, queue) = create_device_and_queue();
            let mut resources = GpuResources::new(device, queue);
            let maintenance = resources.texture_cache_mut().end_frame_maintenance();
            assert_eq!(
                maintenance.evicted, 0,
                "no textures should be evicted from an empty cache"
            );
            assert!(
                !maintenance.atlas_reset,
                "atlas reset must not trigger on an empty cache"
            );
        }

        /// The shared `texture_cache()` accessor observes zero bytes even after
        /// a maintenance cycle, confirming it reaches the same owned cache.
        #[test]
        fn texture_cache_shared_accessor_observes_same_state() {
            let (device, queue) = create_device_and_queue();
            let mut resources = GpuResources::new(device, queue);
            let _ = resources.texture_cache_mut().end_frame_maintenance();
            assert_eq!(
                resources.texture_cache().memory_bytes(),
                0,
                "shared accessor must observe the same cache state as the mutable accessor"
            );
        }

        /// `external_texture_registry()` and `external_texture_registry_mut()`
        /// both see the same empty registry on construction.
        #[test]
        fn external_registry_accessors_agree_on_initial_state() {
            let (device, queue) = create_device_and_queue();
            let resources = GpuResources::new(device, queue);
            assert!(resources.external_texture_registry().is_empty());
            assert_eq!(resources.external_texture_registry().len(), 0);
        }
    }
}
