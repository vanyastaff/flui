//! GPU Buffer Pooling System
//!
//! Provides efficient buffer reuse to minimize per-frame allocations.
//! Instead of creating a new GPU buffer every frame, buffers are pooled
//! and reused across frames.
//!
//! # Reuse model
//!
//! Buffers are bucketed by **capacity rounded up to a power of two** (floor
//! [`MIN_BUCKET_BYTES`]). A request reuses any free buffer in its bucket, so
//! geometry whose byte size fluctuates frame to frame (a growing/shrinking
//! instance count) still reuses a buffer instead of forcing a fresh allocation
//! on every size change — the old exact-size match defeated reuse for anything
//! dynamic. The data is written at offset 0 and draws use explicit vertex/index
//! counts (or sub-range slices), so a buffer larger than its payload renders
//! identically; the unused tail is never read.
//!
//! # Memory budget
//!
//! [`BufferPool::evict_over_budget`] drops least-recently-used **free** buffers
//! once total capacity exceeds a byte budget, so a transient spike (one huge
//! frame) does not pin VRAM forever. Call it once per frame at the end-of-frame
//! seam (`WgpuPainter::end_frame_maintenance`), after the final submit: every
//! buffer is then free, and dropping a `wgpu::Buffer` only schedules the GPU
//! free once outstanding submissions finish (wgpu ref-counts the resource), so
//! eviction never frees memory the in-flight frame still reads.
//!
//! # Reset vs eviction
//!
//! [`BufferPool::reset`] (per `WgpuPainter::render`) only flips `in_use` flags so
//! the next pass may reuse a buffer. Cross-pass reuse within a frame is sound
//! because the engine submits per pass (each `render`'s `write_buffer`s attach
//! to that pass's own submit, and submits are serialized) — so a reused buffer's
//! prior read completes before its next write executes. Eviction is the separate
//! per-frame budget pass.

use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device};

/// Floor for bucket capacity. Requests below this round up to it, so tiny
/// uniform-sized payloads don't each spawn their own bucket.
const MIN_BUCKET_BYTES: usize = 256;

/// Default capacity budget for [`BufferPool::evict_over_budget`].
///
/// Generous enough that steady-state UI frames never evict; eviction reclaims
/// only the bloat left by a transient large frame. Tunable.
pub const DEFAULT_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Round a byte size up to its pooling bucket: `next_power_of_two`, floored at
/// [`MIN_BUCKET_BYTES`]. For payloads at or above the floor this bounds waste at
/// < 2×; smaller payloads round up to the 256-byte floor. Collapsing fluctuating
/// sizes onto a shared bucket lets them reuse one buffer.
fn bucket_capacity(size: usize) -> usize {
    size.max(MIN_BUCKET_BYTES).next_power_of_two()
}

/// Buffer pool entry. `capacity` is the buffer's actual byte length (the bucket
/// size), which is what reuse matches on and what eviction accounts.
struct PooledBuffer {
    buffer: Buffer,
    capacity: usize,
    in_use: bool,
    /// Recency clock value at the last acquire — drives LRU eviction.
    last_used_frame: u64,
}

/// GPU buffer pool for efficient buffer reuse.
///
/// Maintains separate pools for vertex, index, and uniform buffers. Buffers are
/// matched by power-of-two capacity bucket; over-budget free buffers are evicted
/// LRU-first by [`evict_over_budget`](BufferPool::evict_over_budget).
#[derive(Default)]
pub struct BufferPool {
    vertex_buffers: Vec<PooledBuffer>,
    index_buffers: Vec<PooledBuffer>,
    uniform_buffers: Vec<PooledBuffer>,

    // Statistics
    allocations: usize,
    reuses: usize,

    /// Monotonic recency clock, advanced once per frame by `evict_over_budget`.
    /// Each acquire stamps its entry with the current value; eviction drops the
    /// smallest (oldest) first.
    current_frame: u64,
}

impl BufferPool {
    /// Create a new buffer pool
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a vertex buffer
    ///
    /// Reuses a free buffer from the request's capacity bucket if available,
    /// else allocates a fresh bucket-sized buffer.
    ///
    /// # Arguments
    /// * `device` - WGPU device
    /// * `queue` - WGPU queue (for zero-copy buffer updates)
    /// * `label` - Debug label for the buffer
    /// * `contents` - Buffer data
    ///
    /// # Returns
    /// Reference to buffer (valid until next reset())
    pub fn get_vertex_buffer(
        &mut self,
        device: &Device,
        queue: &wgpu::Queue,
        label: &str,
        contents: &[u8],
    ) -> &Buffer {
        Self::get_buffer_internal(
            device,
            queue,
            label,
            contents,
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
            &mut self.vertex_buffers,
            &mut self.allocations,
            &mut self.reuses,
            self.current_frame,
        )
    }

    // The `get_index_buffer` and `get_uniform_buffer` standalone entry
    // points were deleted -- zero workspace callers. The
    // joint `get_vertex_and_index_buffers` (below) is the live index-
    // buffer path (split-borrow to dodge the borrow checker), and
    // uniform buffers don't go through this pool at all (the filter
    // passes use the dedicated `UniformPool`; pipeline construction
    // creates its uniforms once via `device.create_buffer`).
    // `index_buffers` / `uniform_buffers` Vec fields stay because
    // `BufferPool::reset` / `evict_over_budget` drain all three pools,
    // and the joint accessor writes into `index_buffers` directly.

    /// Internal: Get or create a buffer from a specific pool.
    ///
    /// `current_frame` is the recency-clock value stamped onto the acquired
    /// entry (taken by value, so the split-borrow accessor can hand it to both
    /// calls without an extra borrow).
    #[allow(clippy::too_many_arguments)]
    fn get_buffer_internal<'a>(
        device: &Device,
        queue: &wgpu::Queue,
        label: &str,
        contents: &[u8],
        usage: BufferUsages,
        pool: &'a mut Vec<PooledBuffer>,
        allocations: &mut usize,
        reuses: &mut usize,
        current_frame: u64,
    ) -> &'a Buffer {
        // wgpu's `write_buffer` requires the payload length be a multiple of
        // COPY_BUFFER_ALIGNMENT (4). All pooled payloads are `bytemuck`-cast
        // `#[repr(C)]` vertices/instances or `Uint32` indices, so this always
        // holds; assert it loudly here rather than surfacing as an opaque wgpu
        // validation error at submit.
        debug_assert!(
            contents.len().is_multiple_of(4),
            "pooled buffer payload length {} must be 4-byte aligned (wgpu COPY_BUFFER_ALIGNMENT)",
            contents.len()
        );

        let capacity = bucket_capacity(contents.len());

        // Reuse a free buffer from the same capacity bucket.
        let reuse_index = pool
            .iter()
            .position(|entry| !entry.in_use && entry.capacity == capacity);

        if let Some(index) = reuse_index {
            let entry = &mut pool[index];
            entry.in_use = true;
            entry.last_used_frame = current_frame;
            *reuses += 1;

            // Zero-copy update of the existing GPU allocation. The payload is
            // written at offset 0; the bucket-sized tail (if `contents` is
            // smaller than `capacity`) is left untouched and never read, because
            // draws use explicit vertex/index counts or sub-range slices.
            queue.write_buffer(&entry.buffer, 0, contents);

            return &pool[index].buffer;
        }

        // No free buffer in this bucket — allocate one at the bucket capacity.
        *allocations += 1;

        #[cfg(debug_assertions)]
        {
            let total = *allocations + *reuses;
            #[allow(clippy::cast_precision_loss)]
            let reuse_rate = if total == 0 {
                0.0
            } else {
                *reuses as f32 / total as f32
            };
            tracing::trace!(
                "BufferPool: new buffer (payload={}, capacity={}, pool_size={}, reuse_rate={:.1}%)",
                contents.len(),
                capacity,
                pool.len() + 1,
                reuse_rate * 100.0
            );
        }

        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: capacity as u64,
            usage,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, contents);

        let index = pool.len();
        pool.push(PooledBuffer {
            buffer,
            capacity,
            in_use: true,
            last_used_frame: current_frame,
        });

        // Safe: We just pushed, so pool[index] exists
        &pool[index].buffer
    }

    /// Get or create a vertex buffer AND an index buffer simultaneously
    ///
    /// This method solves the borrow checker issue where calling
    /// `get_vertex_buffer` and `get_index_buffer` separately would
    /// require two `&mut self` borrows. By combining them into one call,
    /// both buffer references can be held simultaneously.
    ///
    /// # Safety Note
    ///
    /// Uses raw pointers internally to split borrows on disjoint fields
    /// (`vertex_buffers` vs `index_buffers`). This is sound because:
    /// - The two Vec fields are disjoint memory regions
    /// - The statistics counters are simple increment-only values
    /// - No reallocation of `vertex_buffers` occurs during the index buffer call
    #[expect(
        unsafe_code,
        reason = "disjoint-field &mut borrow of vertex+index buffers via raw pointers; see SAFETY note"
    )]
    pub fn get_vertex_and_index_buffers(
        &mut self,
        device: &Device,
        queue: &wgpu::Queue,
        vertex_label: &str,
        vertex_contents: &[u8],
        index_label: &str,
        index_contents: &[u8],
    ) -> (&Buffer, &Buffer) {
        // We must call get_buffer_internal twice with disjoint borrows.
        // `vertex_buffers` and `index_buffers` are separate Vec fields, so
        // lending &mut to each simultaneously is safe at the value level.
        // The `allocations`/`reuses` counters are shared between the two calls
        // via raw pointers; each `unsafe { &mut *ptr }` expression lives only
        // for the duration of one call argument list, so no two `&mut` aliases
        // to the same counter are simultaneously live. `current_frame` is copied
        // by value into each call, so it needs no borrow.
        let allocations = &raw mut self.allocations;
        let reuses = &raw mut self.reuses;
        let current_frame = self.current_frame;

        let vertex_buf = Self::get_buffer_internal(
            device,
            queue,
            vertex_label,
            vertex_contents,
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
            &mut self.vertex_buffers,
            // SAFETY: `allocations` is a valid, aligned, initialised `usize` owned by
            // `self`. This `&mut` is the only live reference to it at this point —
            // `reuses` is a separate field and the borrow ends before the next call.
            unsafe { &mut *allocations },
            // SAFETY: `reuses` is a valid, aligned, initialised `usize` owned by
            // `self`, distinct from `allocations`. This `&mut` ends at the call site.
            unsafe { &mut *reuses },
            current_frame,
        );

        // Convert to raw pointer to release the mutable borrow on vertex_buffers.
        let vertex_ptr = std::ptr::from_ref::<Buffer>(vertex_buf);

        let index_buf = Self::get_buffer_internal(
            device,
            queue,
            index_label,
            index_contents,
            BufferUsages::INDEX | BufferUsages::COPY_DST,
            &mut self.index_buffers,
            // SAFETY: the previous call's `&mut *allocations` borrow has ended
            // (it was a temporary for the function call above). This is the only
            // live `&mut` to `allocations` at this point.
            unsafe { &mut *allocations },
            // SAFETY: same reasoning as above for `reuses`.
            unsafe { &mut *reuses },
            current_frame,
        );

        // SAFETY: valid because the index-buffer call mutates only `index_buffers`;
        // it never pushes to / reallocates `vertex_buffers`, so `vertex_ptr` stays
        // in-bounds of a live allocation and its borrow tag is not invalidated by
        // the disjoint `index_buffers`/counter borrows.
        let vertex_ref = unsafe { &*vertex_ptr };

        (vertex_ref, index_buf)
    }

    /// Reset pool for next pass/frame.
    ///
    /// Marks all buffers available for reuse. Called per `WgpuPainter::render`
    /// (which runs multiple times per frame); only flips `in_use` flags and frees
    /// nothing — see [`evict_over_budget`](Self::evict_over_budget) for reclaim.
    pub fn reset(&mut self) {
        for entry in &mut self.vertex_buffers {
            entry.in_use = false;
        }
        for entry in &mut self.index_buffers {
            entry.in_use = false;
        }
        for entry in &mut self.uniform_buffers {
            entry.in_use = false;
        }
    }

    /// Drop least-recently-used free buffers until total capacity ≤ `budget_bytes`,
    /// then advance the recency clock for the next frame.
    ///
    /// Call EXACTLY ONCE per frame, at the end-of-frame seam after the final
    /// submit (`WgpuPainter::end_frame_maintenance`). At that point every buffer
    /// is free, and dropping a `wgpu::Buffer` only schedules the GPU free once
    /// outstanding submissions finish, so this never reclaims memory the in-flight
    /// frame still reads. Only `!in_use` buffers are ever dropped.
    pub fn evict_over_budget(&mut self, budget_bytes: usize) {
        while self.total_capacity_bytes() > budget_bytes {
            // Find the oldest free entry across all three pools.
            let mut oldest: Option<(BufferKind, usize, u64)> = None;
            for (kind, pool) in [
                (BufferKind::Vertex, &self.vertex_buffers),
                (BufferKind::Index, &self.index_buffers),
                (BufferKind::Uniform, &self.uniform_buffers),
            ] {
                for (index, entry) in pool.iter().enumerate() {
                    if entry.in_use {
                        continue;
                    }
                    if oldest.is_none_or(|(_, _, frame)| entry.last_used_frame < frame) {
                        oldest = Some((kind, index, entry.last_used_frame));
                    }
                }
            }

            match oldest {
                // Dropping the entry drops its `wgpu::Buffer` (deferred GPU free).
                Some((BufferKind::Vertex, index, _)) => {
                    self.vertex_buffers.swap_remove(index);
                }
                Some((BufferKind::Index, index, _)) => {
                    self.index_buffers.swap_remove(index);
                }
                Some((BufferKind::Uniform, index, _)) => {
                    self.uniform_buffers.swap_remove(index);
                }
                // No free buffer left to drop — everything is in use this frame.
                None => break,
            }
        }

        self.current_frame = self.current_frame.wrapping_add(1);
    }

    /// Total byte capacity held across all three pools (live + free).
    pub fn total_capacity_bytes(&self) -> usize {
        let sum = |pool: &[PooledBuffer]| pool.iter().map(|entry| entry.capacity).sum::<usize>();
        sum(&self.vertex_buffers) + sum(&self.index_buffers) + sum(&self.uniform_buffers)
    }

    /// Get reuse rate (0.0 to 1.0)
    ///
    /// 1.0 = 100% reuse (perfect)
    /// 0.0 = 0% reuse (all allocations)
    pub fn reuse_rate(&self) -> f32 {
        let total = self.allocations + self.reuses;
        if total == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let rate = self.reuses as f32 / total as f32;
            rate
        }
    }

    /// Get statistics for the painter's per-frame log line.
    pub fn stats(&self) -> BufferPoolStats {
        BufferPoolStats {
            reuse_rate: self.reuse_rate(),
        }
    }
}

/// Which sub-pool an entry lives in — used by eviction to remove from the right Vec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufferKind {
    Vertex,
    Index,
    Uniform,
}

/// Buffer pool statistics surfaced to the painter's per-frame log.
#[derive(Debug, Clone, Copy)]
pub struct BufferPoolStats {
    /// Reuse rate (0.0 to 1.0)
    pub reuse_rate: f32,
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact expected values produced by exact arithmetic"
)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn device_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available on a GPU-enabled test host");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("BufferPool Test Device"),
            ..Default::default()
        }))
        .expect("GPU device creation succeeded when adapter was found");
        (Arc::new(device), Arc::new(queue))
    }

    #[test]
    fn test_buffer_pool_size() {
        // BufferPool should stay small (header only; buffers live behind Vecs).
        assert!(std::mem::size_of::<BufferPool>() <= 128);
    }

    #[test]
    fn test_buffer_pool_stats_empty_pool_zero_reuse_rate() {
        let pool = BufferPool::new();
        let stats = pool.stats();
        assert_eq!(stats.reuse_rate, 0.0);
    }

    /// Grow-to-fit: two payloads of DIFFERENT byte sizes that share a capacity
    /// bucket reuse one buffer across frames.
    ///
    /// Red→green vs the old exact-size match: 300 and 400 bytes both bucket to
    /// 512, so frame 2 reuses frame 1's buffer → reuse_rate 0.5. Exact-size
    /// matching would miss (300 ≠ 400) → two allocations, reuse_rate 0.0.
    #[test]
    fn grow_to_fit_reuses_across_fluctuating_size() {
        let (device, queue) = device_queue();
        let mut pool = BufferPool::new();

        pool.get_vertex_buffer(&device, &queue, "f1", &[0u8; 300]); // miss → bucket 512
        pool.reset();
        pool.get_vertex_buffer(&device, &queue, "f2", &[0u8; 400]); // hit → same bucket 512

        assert_eq!(
            pool.reuse_rate(),
            0.5,
            "a 400-byte payload must reuse the 300-byte payload's 512-byte bucket"
        );
    }

    /// A payload exceeding its bucket's prior high-water mark crosses into the
    /// next power-of-two bucket and allocates fresh (no false reuse).
    #[test]
    fn distinct_buckets_do_not_alias() {
        let (device, queue) = device_queue();
        let mut pool = BufferPool::new();

        pool.get_vertex_buffer(&device, &queue, "small", &[0u8; 300]); // bucket 512
        pool.reset();
        pool.get_vertex_buffer(&device, &queue, "large", &[0u8; 600]); // bucket 1024 → miss

        assert_eq!(
            pool.reuse_rate(),
            0.0,
            "a 600-byte payload (bucket 1024) must not reuse a 512-byte buffer"
        );
    }

    /// Eviction drops least-recently-used free buffers until under budget.
    ///
    /// Three distinct buckets allocated across three frames (so each has a
    /// distinct recency stamp and none reuses another): 512 + 1024 + 2048 =
    /// 3584 bytes. Red→green: without `evict_over_budget` the pool keeps all
    /// three; with a 2048-byte budget the two oldest (512, then 1024) are
    /// dropped LRU-first, leaving the newest 2048.
    #[test]
    fn evict_over_budget_drops_lru_free_buffers() {
        let (device, queue) = device_queue();
        let mut pool = BufferPool::new();

        pool.get_vertex_buffer(&device, &queue, "oldest", &[0u8; 300]); // bucket 512, frame 0
        pool.reset();
        pool.evict_over_budget(usize::MAX); // no eviction; advances the clock
        pool.get_vertex_buffer(&device, &queue, "middle", &[0u8; 600]); // bucket 1024, frame 1
        pool.reset();
        pool.evict_over_budget(usize::MAX);
        pool.get_vertex_buffer(&device, &queue, "newest", &[0u8; 1200]); // bucket 2048, frame 2
        pool.reset();

        assert_eq!(
            pool.total_capacity_bytes(),
            512 + 1024 + 2048,
            "three distinct buckets before eviction"
        );

        // Budget fits only the newest bucket → the two oldest are evicted.
        pool.evict_over_budget(2048);
        assert_eq!(
            pool.total_capacity_bytes(),
            2048,
            "eviction must drop the 512 and 1024 buckets LRU-first, keeping the newest 2048"
        );
    }
}
