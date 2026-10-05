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
    buffer_pool::BufferPool,
    external_texture_registry::{ExternalAllocationLease, ExternalTextureRegistry},
    texture_cache::TextureCache,
    texture_pool::TexturePool,
    uniform_pool::UniformPool,
};

/// Single owner of the four GPU resource managers used by [`crate::painter::WgpuPainter`].
///
/// Provides `pub(crate)` accessors for each sub-pool so call sites reach them
/// without coupling to the other pools.
pub(crate) struct GpuResources {
    domain: Arc<crate::device_domain::DeviceDomain>,
    prepared: Vec<Arc<crate::device_domain::PreparedPermit>>,
    // Identifies the pending ownership bundle, not a frame or content revision.
    // Exhaustion disables cache enrollment reuse rather than wrapping identity.
    prepared_epoch: Option<u64>,
    pending_external_leases: Vec<ExternalAllocationLease>,
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
    pub(crate) fn new(domain: Arc<crate::device_domain::DeviceDomain>) -> Self {
        let device = Arc::clone(domain.device());
        let queue = Arc::clone(domain.queue());
        let buffer_pool = BufferPool::new();
        // Built before `texture_cache` consumes `queue` (and `layer_texture_pool`
        // consumes `device`): the uniform pool keeps its own `Arc` clones.
        let uniform_pool = UniformPool::new(device.clone(), queue.clone());
        let texture_cache = TextureCache::new(device.clone(), queue);
        let external_texture_registry = ExternalTextureRegistry::new(&domain);
        let layer_texture_pool = TexturePool::with_capacity(device, 4);

        Self {
            domain,
            prepared: Vec::new(),
            prepared_epoch: Some(0),
            pending_external_leases: Vec::new(),
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

    /// Admit new prepared resources before allocation; submission owns their retirement.
    pub(crate) fn reserve_prepared(
        &mut self,
        cost: crate::device_domain::PreparedCost,
    ) -> crate::error::EngineResult<()> {
        let permit = self.domain.reserve(cost)?;
        self.retain_external_binding_charge(&permit)
    }

    /// Admit a foreground filter before any offscreen allocation or GPU pass.
    /// Charges conservative H/V destination storage and per-pass preparation;
    /// reusable idle pool residency remains a separate accounting scope.
    pub(crate) fn admit_foreground_filter(
        &mut self,
        dimensions: (u32, u32),
        format: wgpu::TextureFormat,
        passes: &[crate::command_ir::ImageFilterPass],
        budget: &crate::recording_budget::RecordingBudget,
    ) -> crate::error::EngineResult<()> {
        use crate::command_ir::ImageFilterPass;
        let limit = self.domain.device().limits().max_texture_dimension_2d;
        if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0.max(dimensions.1) > limit {
            return Err(crate::error::EngineError::PreparedResourceLimit {
                resource: "foreground filter dimension",
                requested: dimensions.0.max(dimensions.1) as usize,
                limit: limit as usize,
            });
        }
        let overflow = || crate::error::EngineError::PreparedResourceOverflow;
        let pixels = (dimensions.0 as usize)
            .checked_mul(dimensions.1 as usize)
            .ok_or_else(overflow)?;
        let mut work = 0usize;
        let mut subpasses = 0usize;
        for pass in passes {
            let (rx, ry) = pass.support_radius()?;
            let (taps, count) = match pass {
                ImageFilterPass::Blur { .. } | ImageFilterPass::Morph { .. } => {
                    let taps = (rx as usize)
                        .checked_add(ry as usize)
                        .and_then(|r| r.checked_mul(2))
                        .and_then(|r| r.checked_add(2))
                        .ok_or_else(overflow)?;
                    (taps, 2)
                }
                ImageFilterPass::ColorMatrix(_) => (1, 1),
                ImageFilterPass::Identity => (0, 0),
            };
            work = work
                .checked_add(pixels.checked_mul(taps).ok_or_else(overflow)?)
                .ok_or_else(overflow)?;
            subpasses = subpasses.checked_add(count).ok_or_else(overflow)?;
        }
        budget.admit_effect_work(work)?;
        let texel_bytes = format.block_copy_size(None).ok_or_else(overflow)? as usize;
        let uniform_bytes = subpasses.checked_mul(80).ok_or_else(overflow)?;
        let destinations = if subpasses == 0 { 0 } else { 2 };
        let bytes = pixels
            .checked_mul(texel_bytes)
            .and_then(|v| v.checked_mul(destinations))
            .and_then(|v| v.checked_add(uniform_bytes))
            .ok_or_else(overflow)?;
        let objects = subpasses.checked_mul(6).ok_or_else(overflow)?;
        self.reserve_prepared(crate::device_domain::PreparedCost {
            gpu_bytes: bytes,
            cpu_bytes: subpasses.checked_mul(64).ok_or_else(overflow)?,
            objects,
        })
    }

    /// Admit attachment storage for an input or nested target in a foreground
    /// filter domain. The caller separately admits pass work and metadata.
    pub(crate) fn admit_foreground_target(
        &mut self,
        dimensions: (u32, u32),
        format: wgpu::TextureFormat,
        count: usize,
    ) -> crate::error::EngineResult<()> {
        let limit = self.domain.device().limits().max_texture_dimension_2d;
        if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0.max(dimensions.1) > limit {
            return Err(crate::error::EngineError::PreparedResourceLimit {
                resource: "foreground attachment dimension",
                requested: dimensions.0.max(dimensions.1) as usize,
                limit: limit as usize,
            });
        }
        let overflow = || crate::error::EngineError::PreparedResourceOverflow;
        let texel_bytes = format.block_copy_size(None).ok_or_else(overflow)? as usize;
        let bytes = (dimensions.0 as usize)
            .checked_mul(dimensions.1 as usize)
            .and_then(|pixels| pixels.checked_mul(texel_bytes))
            .and_then(|bytes| bytes.checked_mul(count))
            .ok_or_else(overflow)?;
        self.reserve_prepared(crate::device_domain::PreparedCost {
            gpu_bytes: bytes,
            cpu_bytes: 0,
            objects: count.checked_mul(2).ok_or_else(overflow)?,
        })
    }

    pub(crate) fn prepared_epoch(&self) -> Option<u64> {
        self.prepared_epoch
    }

    pub(crate) fn take_prepared_permits(
        &mut self,
    ) -> Vec<Arc<crate::device_domain::PreparedPermit>> {
        let permits = std::mem::take(&mut self.prepared);
        self.prepared_epoch = self.prepared_epoch.and_then(|epoch| epoch.checked_add(1));
        permits
    }

    pub(crate) fn retain_external_lease(
        &mut self,
        lease: ExternalAllocationLease,
    ) -> crate::error::EngineResult<()> {
        if !lease.is_for_domain(&self.domain) {
            return Err(crate::error::ExternalTextureError::ForeignOwner.into());
        }
        // Adjacent uses of one allocation need only one completion reference.
        if self
            .pending_external_leases
            .last()
            .is_some_and(|previous| previous.same_allocation(&lease))
        {
            return Ok(());
        }
        let capacity = self.pending_external_leases.capacity();
        let needed = self
            .pending_external_leases
            .len()
            .checked_add(1)
            .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
        let target = if needed > capacity {
            capacity
                .checked_mul(2)
                .ok_or(crate::error::EngineError::PreparedResourceOverflow)?
                .max(needed)
                .max(4)
        } else {
            capacity
        };
        let bytes = target
            .saturating_sub(capacity)
            .checked_mul(std::mem::size_of::<ExternalAllocationLease>())
            .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
        self.reserve_prepared(crate::device_domain::PreparedCost {
            gpu_bytes: 0,
            cpu_bytes: bytes,
            objects: 1,
        })?;
        if target > capacity {
            self.pending_external_leases
                .try_reserve_exact(target - self.pending_external_leases.len())
                .map_err(
                    |source| crate::error::EngineError::PreparedResourceAllocation {
                        resource: "external lease references",
                        source,
                    },
                )?;
        }
        self.pending_external_leases.push(lease);
        Ok(())
    }

    pub(crate) fn reserve_external_binding(
        &mut self,
        cost: crate::device_domain::PreparedCost,
    ) -> crate::error::EngineResult<Arc<crate::device_domain::PreparedPermit>> {
        let permit = self.domain.reserve(cost)?;
        self.retain_external_binding_charge(&permit)?;
        Ok(permit)
    }

    pub(crate) fn retain_external_binding_charge(
        &mut self,
        permit: &Arc<crate::device_domain::PreparedPermit>,
    ) -> crate::error::EngineResult<()> {
        if self
            .prepared
            .last()
            .is_some_and(|previous| Arc::ptr_eq(previous, permit))
        {
            return Ok(());
        }
        let capacity = self.prepared.capacity();
        let needed = self
            .prepared
            .len()
            .checked_add(1)
            .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
        if needed > capacity {
            // Growth also needs a slot for its own infallible retirement charge.
            let target = capacity
                .checked_mul(2)
                .and_then(|grown| {
                    needed
                        .checked_add(1)
                        .map(|minimum| grown.max(minimum).max(4))
                })
                .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
            let bytes = target
                .saturating_sub(capacity)
                .checked_mul(std::mem::size_of::<Arc<crate::device_domain::PreparedPermit>>())
                .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
            let metadata = self.domain.reserve(crate::device_domain::PreparedCost {
                gpu_bytes: 0,
                cpu_bytes: bytes,
                objects: 0,
            })?;
            self.prepared
                .try_reserve_exact(target - self.prepared.len())
                .map_err(
                    |source| crate::error::EngineError::PreparedResourceAllocation {
                        resource: "prepared external binding references",
                        source,
                    },
                )?;
            self.prepared.push(metadata);
        }
        self.prepared.push(Arc::clone(permit));
        Ok(())
    }

    pub(crate) fn take_external_leases(&mut self) -> Vec<ExternalAllocationLease> {
        std::mem::take(&mut self.pending_external_leases)
    }

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
