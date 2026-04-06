//! Pooled GPU buffer allocation for vertex and instance data.
//!
//! Tracks vertex, index, and uniform buffers for reuse across frames,
//! reducing GPU memory allocation overhead.

#[cfg(feature = "wgpu-backend")]
use wgpu;

/// Statistics for buffer pool allocation and reuse.
#[derive(Default, Debug, Clone)]
pub struct PoolStats {
    /// Total number of new buffer allocations.
    pub allocations: u64,
    /// Total number of buffer reuses from the pool.
    pub reuses: u64,
}

#[cfg(feature = "wgpu-backend")]
struct PooledBuffer {
    buffer: wgpu::Buffer,
    size: u64,
    in_use: bool,
}

/// Reusable GPU buffer allocator.
///
/// Maintains separate pools for vertex, index, and uniform buffers.
/// At the start of each frame, call [`reset`](BufferPool::reset) to mark
/// all buffers as available. When a buffer is requested, the pool tries
/// to find an existing buffer of sufficient size before allocating a new one.
#[cfg(feature = "wgpu-backend")]
pub struct BufferPool {
    vertex_buffers: Vec<PooledBuffer>,
    index_buffers: Vec<PooledBuffer>,
    uniform_buffers: Vec<PooledBuffer>,
    stats: PoolStats,
}

#[cfg(feature = "wgpu-backend")]
impl BufferPool {
    /// Create an empty buffer pool.
    pub fn new() -> Self {
        Self {
            vertex_buffers: Vec::new(),
            index_buffers: Vec::new(),
            uniform_buffers: Vec::new(),
            stats: PoolStats::default(),
        }
    }

    /// Get or create a vertex buffer of at least the given size.
    pub fn get_vertex_buffer(&mut self, device: &wgpu::Device, size: u64) -> &wgpu::Buffer {
        let index = Self::get_or_create(
            &mut self.vertex_buffers,
            device,
            size,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            "vertex",
            &mut self.stats,
        );
        &self.vertex_buffers[index].buffer
    }

    /// Get or create an index buffer of at least the given size.
    pub fn get_index_buffer(&mut self, device: &wgpu::Device, size: u64) -> &wgpu::Buffer {
        let index = Self::get_or_create(
            &mut self.index_buffers,
            device,
            size,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            "index",
            &mut self.stats,
        );
        &self.index_buffers[index].buffer
    }

    /// Get or create a uniform buffer of at least the given size.
    pub fn get_uniform_buffer(&mut self, device: &wgpu::Device, size: u64) -> &wgpu::Buffer {
        let index = Self::get_or_create(
            &mut self.uniform_buffers,
            device,
            size,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            "uniform",
            &mut self.stats,
        );
        &self.uniform_buffers[index].buffer
    }

    /// Mark all buffers as not in use. Call at the start of each frame.
    pub fn reset(&mut self) {
        for buf in &mut self.vertex_buffers {
            buf.in_use = false;
        }
        for buf in &mut self.index_buffers {
            buf.in_use = false;
        }
        for buf in &mut self.uniform_buffers {
            buf.in_use = false;
        }
    }

    /// Return allocation/reuse statistics.
    pub fn stats(&self) -> &PoolStats {
        &self.stats
    }

    fn get_or_create(
        pool: &mut Vec<PooledBuffer>,
        device: &wgpu::Device,
        size: u64,
        usage: wgpu::BufferUsages,
        label: &str,
        stats: &mut PoolStats,
    ) -> usize {
        // Try to find an existing buffer that is not in use and large enough
        if let Some(idx) = pool
            .iter()
            .position(|b| !b.in_use && b.size >= size)
        {
            pool[idx].in_use = true;
            stats.reuses += 1;
            return idx;
        }

        // Allocate a new buffer
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        });
        pool.push(PooledBuffer {
            buffer,
            size,
            in_use: true,
        });
        stats.allocations += 1;
        pool.len() - 1
    }
}

#[cfg(feature = "wgpu-backend")]
impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}
