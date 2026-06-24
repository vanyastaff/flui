//! Reusable per-frame uniform-buffer pool for the filter/composite passes.
//!
//! Every GPU filter pass (gamma, blur, morphology, color_matrix, mode,
//! advanced_blend) needs a small `#[repr(C)]` uniform block bound at group 0.
//! Each pass used to `device.create_buffer_init` a fresh `wgpu::Buffer` on every
//! invocation — one driver allocation + tracking object per pass, per frame, for
//! a 16–80 byte payload.  This pool hands out **reusable** buffers instead,
//! mirroring flui's existing persistent-uniform pattern (`resources.rs` viewport
//! uniform; `offscreen/blur.rs` pre-allocated blur slots).
//!
//! ## The write-after-read hazard this design avoids
//!
//! `queue.write_buffer` writes are applied at submit time, *before* the command
//! buffer executes.  If two passes in the **same submit** referenced one reused
//! buffer that was `write_buffer`-rewritten between them, both would read the
//! last write — silently wrong output.  The engine submits per pass-group
//! (clear / each backdrop-filter flush / final render are separate submits), but
//! a single submit (the final render) can hold many filter passes, and the
//! backdrop-flush submits run earlier in the *same frame*.
//!
//! So the pool hands out a **distinct buffer per `alloc` call within a frame**
//! (the cursor only rewinds on [`UniformPool::reset_frame`], called once
//! per frame after the final submit).  No buffer is reused within a frame →
//! no within-frame `write_buffer` collapse.  Across frames the same buffers are
//! rewritten — the standard, wgpu-synchronised "update a persistent uniform each
//! frame" pattern (the next frame's write is queued after the previous frame's
//! submit, so the GPU read completes first).
//!
//! [`UniformPool::reset_frame`] therefore belongs at the **once-per-frame** seam
//! (`WgpuPainter::end_frame_maintenance`), **not** the per-`render` buffer-pool
//! reset — a per-`render` rewind would alias a backdrop-flush buffer with a
//! final-render buffer in the same frame.

use std::collections::HashMap;
use std::sync::Arc;

/// One size class: reusable buffers all exactly `size` bytes, handed out in
/// order and rewound by [`UniformPool::reset_frame`].
#[derive(Default)]
struct Bucket {
    buffers: Vec<wgpu::Buffer>,
    /// Index of the next buffer to hand out this frame.
    next: usize,
}

/// Pool of reusable `UNIFORM | COPY_DST` buffers, bucketed by exact byte size.
///
/// Bucketing by exact size keeps every buffer's length equal to its uniform
/// block's size, so a whole-buffer binding (`BufferBinding { offset: 0, size:
/// None }`) still satisfies the generated layout's `min_binding_size` — no
/// dynamic offsets, no 256-byte alignment math.
pub(crate) struct UniformPool {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    buckets: HashMap<u64, Bucket>,
    /// Cumulative count of buffers ever created — plateaus once a repeating
    /// workload's buffers are all reused.  Used by tests to prove reuse.
    total_created: u64,
}

impl UniformPool {
    /// Create an empty pool.  Buffers are allocated lazily on first [`alloc`].
    ///
    /// [`alloc`]: Self::alloc
    pub(crate) fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self {
            device,
            queue,
            buckets: HashMap::new(),
            total_created: 0,
        }
    }

    /// Write `data` into a reusable uniform buffer and return it for binding.
    ///
    /// Returns a buffer distinct from every other live `alloc` this frame, so
    /// callers may bind several uniforms in one submit without aliasing.  The
    /// returned borrow ends once the bind group is built, freeing the pool for
    /// the next `alloc`.
    pub(crate) fn alloc(&mut self, data: &[u8]) -> &wgpu::Buffer {
        let size = data.len() as u64;
        // Split the borrow into disjoint fields: `device`/`total_created` are held
        // while `buckets` is borrowed mutably by `entry`. Buckets are keyed by
        // exact size, so different filter types that share a size (blur+mode at
        // 32 B, color_matrix+advanced_blend at 80 B) share a bucket — still safe,
        // because the cursor advances per `alloc` regardless of caller, so every
        // live uniform this frame gets a distinct buffer.
        let device = &self.device;
        let total_created = &mut self.total_created;
        let bucket = self.buckets.entry(size).or_default();

        if bucket.next == bucket.buffers.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Pooled Uniform Buffer"),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            bucket.buffers.push(buffer);
            *total_created += 1;
        }

        let index = bucket.next;
        bucket.next += 1;
        self.queue.write_buffer(&bucket.buffers[index], 0, data);
        &bucket.buffers[index]
    }

    /// Rewind every size class so the next frame reuses the same buffers.
    ///
    /// Call EXACTLY ONCE per frame, after the final submit — never between
    /// passes or per `WgpuPainter::render` (see the module docs for the
    /// within-frame aliasing hazard that would cause).
    pub(crate) fn reset_frame(&mut self) {
        for bucket in self.buckets.values_mut() {
            bucket.next = 0;
        }
    }

    /// Total uniform buffers ever allocated across all size classes.
    ///
    /// Plateaus once a steady-state workload's buffers are all reused — the
    /// signal the per-frame reuse is working.  Test-only: the reuse contract is
    /// observable but not part of the production API.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn total_created(&self) -> u64 {
        self.total_created
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use std::sync::Arc;

    use super::UniformPool;

    fn device_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available on a GPU-enabled test host");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("UniformPool Test Device"),
            ..Default::default()
        }))
        .expect("GPU device creation succeeded when adapter was found");
        (Arc::new(device), Arc::new(queue))
    }

    /// The pool reuses buffers across frames and hands out distinct buffers
    /// within a frame.
    ///
    /// Red→green: without [`UniformPool::reset_frame`] (or with broken reuse) the
    /// second frame's allocations would create three more buffers — `total_created`
    /// would be 6, not 3.  The within-frame counts also prove distinctness: two
    /// 16-byte allocations in one frame create two buffers, never one reused.
    #[test]
    fn reuses_across_frames_and_distinct_within_frame() {
        let (device, queue) = device_queue();
        let mut pool = UniformPool::new(device, queue);

        // Frame 1: two 16-byte + one 32-byte uniform.
        pool.alloc(&[0u8; 16]);
        pool.alloc(&[0u8; 32]);
        pool.alloc(&[0u8; 16]);
        assert_eq!(
            pool.total_created(),
            3,
            "frame 1 must create one buffer per alloc (2× size-16 distinct + 1× size-32)"
        );

        // Frame 2: identical workload must reuse every buffer — zero new allocations.
        pool.reset_frame();
        pool.alloc(&[0u8; 16]);
        pool.alloc(&[0u8; 32]);
        pool.alloc(&[0u8; 16]);
        assert_eq!(
            pool.total_created(),
            3,
            "frame 2's identical workload must reuse all buffers (no new allocations)"
        );

        // Exceeding a size class's high-water mark grows it by exactly one.
        pool.alloc(&[0u8; 16]); // 3rd size-16 this frame → one new buffer
        assert_eq!(
            pool.total_created(),
            4,
            "a third concurrent size-16 uniform must allocate exactly one more buffer"
        );
    }
}
