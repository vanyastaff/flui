//! GPU Buffer Pooling System
//!
//! Provides efficient buffer reuse to minimize per-frame allocations.
//! Instead of creating a new GPU buffer every frame, buffers are pooled
//! and reused across frames.
//!
//! # Performance Impact
//!
//! - **CPU overhead reduction:** 10-20% (eliminates allocation overhead)
//! - **Memory efficiency:** Reuses buffers instead of creating new ones
//! - **Driver overhead:** Reduces driver calls for buffer creation
//!
//! # Architecture
//!
//! ```text
//! Frame N:   Allocate buffer A → Use → Return to pool
//! Frame N+1: Reuse buffer A (if size matches) → Use → Return to pool
//! Frame N+2: Reuse buffer A (if size matches) → Use → Return to pool
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! let mut pool = BufferPool::new();
//!
//! // Get buffer (creates or reuses)
//! let buffer = pool.get_vertex_buffer(
//!     &device,
//!     "Instance Buffer",
//!     &instance_data,
//! );
//!
//! // Use buffer in render pass
//! render_pass.set_vertex_buffer(1, buffer.slice(..));
//!
//! // Return buffer to pool for next frame (automatic on pool.reset())
//! pool.reset();  // Call at end of frame
//! ```

use wgpu::{
    Buffer, BufferUsages, Device,
    util::{BufferInitDescriptor, DeviceExt},
};

/// Buffer pool entry
struct PooledBuffer {
    buffer: Buffer,
    size: usize,
    in_use: bool,
    #[allow(dead_code)]
    usage: BufferUsages,
}

/// GPU buffer pool for efficient buffer reuse
///
/// Maintains separate pools for different buffer types (vertex, index,
/// uniform). Buffers are matched by size - if requested size matches pooled
/// buffer size, the buffer is reused. Otherwise, a new buffer is created.
#[derive(Default)]
pub struct BufferPool {
    vertex_buffers: Vec<PooledBuffer>,
    index_buffers: Vec<PooledBuffer>,
    uniform_buffers: Vec<PooledBuffer>,

    // Statistics
    allocations: usize,
    reuses: usize,
}

impl BufferPool {
    /// Create a new buffer pool
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a vertex buffer
    ///
    /// If a vertex buffer of the same size is available in the pool, it will be
    /// reused. Otherwise, a new buffer will be created.
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
        )
    }

    // Cycle 4 wave 5 E-10: `get_index_buffer` and `get_uniform_buffer`
    // standalone entry points deleted -- zero workspace callers. The
    // joint `get_vertex_and_index_buffers` (below) is the live index-
    // buffer path (split-borrow to dodge the borrow checker), and
    // uniform buffers don't go through this pool at all (pipeline
    // construction creates them once via `device.create_buffer`).
    // `index_buffers` / `uniform_buffers` Vec fields stay because
    // `BufferPool::reset` (the per-frame "mark all buffers free"
    // pass on line 277) drains all three pools, and the joint
    // accessor writes into `index_buffers` directly. PR #117 review
    // fix: prior comment said `BufferPool::shrink` was the
    // load-bearer; that method was also deleted in this wave, so
    // `reset` is the actual reason these fields survive.

    /// Internal: Get or create a buffer from specific pool
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
    ) -> &'a Buffer {
        let size = contents.len();

        // Try to find available buffer with matching size
        let reuse_index = pool
            .iter()
            .position(|entry| !entry.in_use && entry.size == size);

        if let Some(index) = reuse_index {
            // Reuse buffer with zero-copy update
            let entry = &mut pool[index];
            entry.in_use = true;
            *reuses += 1;

            #[cfg(debug_assertions)]
            {
                let total = *allocations + *reuses;
                let reuse_rate = if total == 0 {
                    0.0
                } else {
                    *reuses as f32 / total as f32
                };
                tracing::trace!(
                    "BufferPool: Reusing buffer with zero-copy (size={}, reuse_rate={:.1}%)",
                    size,
                    reuse_rate * 100.0
                );
            }

            // Zero-copy buffer update via queue.write_buffer
            // This is MUCH faster than recreating the buffer!
            // Benefits:
            // - No GPU buffer allocation overhead
            // - No driver synchronization overhead
            // - Direct DMA transfer to existing GPU memory
            queue.write_buffer(&entry.buffer, 0, contents);

            return &pool[index].buffer;
        }

        // No matching buffer found - create new one
        *allocations += 1;

        #[cfg(debug_assertions)]
        {
            let total = *allocations + *reuses;
            let reuse_rate = if total == 0 {
                0.0
            } else {
                *reuses as f32 / total as f32
            };
            let pool_len = pool.len();
            tracing::trace!(
                "BufferPool: Creating new buffer (size={}, pool_size={}, reuse_rate={:.1}%)",
                size,
                pool_len + 1,
                reuse_rate * 100.0
            );
        }

        let buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some(label),
            contents,
            usage,
        });

        let index = pool.len();
        pool.push(PooledBuffer {
            buffer,
            size,
            in_use: true,
            usage,
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
    #[allow(unsafe_code)]
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
        // Since vertex_buffers and index_buffers are separate fields,
        // we split the borrow manually.
        let allocations = &raw mut self.allocations;
        let reuses = &raw mut self.reuses;

        let vertex_buf = Self::get_buffer_internal(
            device,
            queue,
            vertex_label,
            vertex_contents,
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
            &mut self.vertex_buffers,
            // SAFETY: allocations/reuses are only used for statistics counting.
            // The two calls never alias the same Vec, and the counters are
            // simple increments with no observable side effects between calls.
            unsafe { &mut *allocations },
            unsafe { &mut *reuses },
        );

        // Convert to raw pointer to release the mutable borrow on vertex_buffers
        let vertex_ptr = std::ptr::from_ref::<Buffer>(vertex_buf);

        let index_buf = Self::get_buffer_internal(
            device,
            queue,
            index_label,
            index_contents,
            BufferUsages::INDEX | BufferUsages::COPY_DST,
            &mut self.index_buffers,
            unsafe { &mut *allocations },
            unsafe { &mut *reuses },
        );

        // SAFETY: vertex_ptr points into self.vertex_buffers which is not
        // modified by the index buffer call (separate Vec). The buffer
        // reference is valid for the lifetime of &mut self.
        let vertex_ref = unsafe { &*vertex_ptr };

        (vertex_ref, index_buf)
    }

    /// Reset pool for next frame
    ///
    /// Call this at the end of each frame to mark all buffers as available
    /// for reuse in the next frame.
    pub fn reset(&mut self) {
        // Mark all buffers as available
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

    /// Get reuse rate (0.0 to 1.0)
    ///
    /// 1.0 = 100% reuse (perfect)
    /// 0.0 = 0% reuse (all allocations)
    pub fn reuse_rate(&self) -> f32 {
        let total = self.allocations + self.reuses;
        if total == 0 {
            0.0
        } else {
            self.reuses as f32 / total as f32
        }
    }

    /// Get statistics for the painter's per-frame log line.
    ///
    /// Cycle 4 wave 5 E-10: trimmed from a 6-field struct
    /// (`vertex_buffers`/`index_buffers`/`uniform_buffers`/
    /// `allocations`/`reuses`/`reuse_rate`) to a single field. The
    /// other 5 were set on construction but read by zero callers
    /// in the workspace; the painter's per-frame log line only
    /// surfaces the cache-hit ratio.
    pub fn stats(&self) -> BufferPoolStats {
        BufferPoolStats {
            reuse_rate: self.reuse_rate(),
        }
    }

    // Cycle 4 wave 5 E-10: `BufferPool::shrink` deleted. Workspace
    // grep showed zero callers; the only `shrink()` callsites in
    // flui-engine live on `TextureCache::shrink` (a different
    // method). Buffer pools grow with batch size and there's no
    // tear-down path that calls `.shrink()` -- the eventual
    // budget-watching tool (separate cleanup wave) can reintroduce
    // it if needed. The per-pool `retain(|e| e.in_use)` body is
    // trivial to rewrite when that lands.
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
    use super::*;

    #[test]
    fn test_buffer_pool_size() {
        // BufferPool should be small
        assert!(std::mem::size_of::<BufferPool>() <= 128);
    }

    // Cycle 4 wave 5 PR #117 review fix: prior `test_buffer_pool_stats`
    // asserted on 5 BufferPoolStats fields (`vertex_buffers`,
    // `index_buffers`, `uniform_buffers`, `allocations`, `reuses`)
    // that the E-10 trim deleted. The fresh shape exposes only
    // `reuse_rate`; an empty pool's reuse rate is 0.0 (no allocs +
    // no reuses → `0 / 0` short-circuits to 0.0 per `reuse_rate()`'s
    // `total == 0` branch).
    #[test]
    fn test_buffer_pool_stats_empty_pool_zero_reuse_rate() {
        let pool = BufferPool::new();
        let stats = pool.stats();
        assert_eq!(stats.reuse_rate, 0.0);
    }
}
