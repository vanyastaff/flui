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
pub(crate) const DEFAULT_BUDGET_BYTES: usize = 64 * 1024 * 1024;

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
pub(crate) struct BufferPool {
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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn get_vertex_buffer(
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
    #[expect(clippy::too_many_arguments)]
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

    /// Get or create a vertex buffer AND an index buffer simultaneously.
    ///
    /// Both pools and both statistics counters are disjoint fields of `self`,
    /// so the borrow checker splits them without help: the first call's
    /// `&mut self.vertex_buffers` / `&mut self.allocations` / `&mut self.reuses`
    /// are three disjoint field borrows, and the second call's
    /// `&mut self.index_buffers` / `&mut self.allocations` / `&mut self.reuses`
    /// are three more. Nothing here needs raw pointers.
    pub(crate) fn get_vertex_and_index_buffers(
        &mut self,
        device: &Device,
        queue: &wgpu::Queue,
        vertex_label: &str,
        vertex_contents: &[u8],
        index_label: &str,
        index_contents: &[u8],
    ) -> (&Buffer, &Buffer) {
        let current_frame = self.current_frame;

        let vertex_buf = Self::get_buffer_internal(
            device,
            queue,
            vertex_label,
            vertex_contents,
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
            &mut self.vertex_buffers,
            &mut self.allocations,
            &mut self.reuses,
            current_frame,
        );

        let index_buf = Self::get_buffer_internal(
            device,
            queue,
            index_label,
            index_contents,
            BufferUsages::INDEX | BufferUsages::COPY_DST,
            &mut self.index_buffers,
            &mut self.allocations,
            &mut self.reuses,
            current_frame,
        );

        (vertex_buf, index_buf)
    }

    /// Reset pool for next pass/frame.
    ///
    /// Marks all buffers available for reuse. Called per `WgpuPainter::render`
    /// (which runs multiple times per frame); only flips `in_use` flags and frees
    /// nothing — see [`evict_over_budget`](Self::evict_over_budget) for reclaim.
    pub(crate) fn reset(&mut self) {
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
    pub(crate) fn evict_over_budget(&mut self, budget_bytes: usize) {
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
    pub(crate) fn total_capacity_bytes(&self) -> usize {
        let sum = |pool: &[PooledBuffer]| pool.iter().map(|entry| entry.capacity).sum::<usize>();
        sum(&self.vertex_buffers) + sum(&self.index_buffers) + sum(&self.uniform_buffers)
    }

    /// Get reuse rate (0.0 to 1.0)
    ///
    /// 1.0 = 100% reuse (perfect)
    /// 0.0 = 0% reuse (all allocations)
    pub(crate) fn reuse_rate(&self) -> f32 {
        let total = self.allocations + self.reuses;
        if total == 0 {
            0.0
        } else {
            self.reuses as f32 / total as f32
        }
    }

    /// Get statistics for the painter's per-frame log line.
    pub(crate) fn stats(&self) -> BufferPoolStats {
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
pub(crate) struct BufferPoolStats {
    /// Reuse rate (0.0 to 1.0)
    pub reuse_rate: f32,
}
