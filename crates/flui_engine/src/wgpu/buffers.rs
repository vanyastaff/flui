//! GPU buffer management
//!
//! This module provides efficient buffer allocation and management for GPU rendering.
//! Supports dynamic vertex/instance buffers with automatic resizing.

use std::sync::Arc;
use wgpu::{Buffer, BufferUsages, Device};

/// Dynamic buffer that automatically grows as needed
///
/// Used for vertex and instance data that changes every frame.
/// Automatically reallocates to larger sizes when needed.
pub struct DynamicBuffer {
    /// GPU buffer
    buffer: Buffer,

    /// Current capacity in bytes
    capacity: u64,

    /// Buffer usage flags
    usage: BufferUsages,

    /// GPU device for reallocation
    device: Arc<Device>,

    /// Debug label
    label: String,
}

impl DynamicBuffer {
    /// Create a new dynamic buffer
    ///
    /// # Arguments
    ///
    /// * `device` - GPU device
    /// * `label` - Debug label
    /// * `initial_capacity` - Initial capacity in bytes
    /// * `usage` - Buffer usage flags
    pub fn new(
        device: Arc<Device>,
        label: &str,
        initial_capacity: u64,
        usage: BufferUsages,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: initial_capacity,
            usage,
            mapped_at_creation: false,
        });

        Self {
            buffer,
            capacity: initial_capacity,
            usage,
            device,
            label: label.to_string(),
        }
    }

    /// Write data to buffer, reallocating if needed
    ///
    /// # Arguments
    ///
    /// * `queue` - GPU queue for data upload
    /// * `data` - Data to write (as bytes)
    ///
    /// # Returns
    ///
    /// True if buffer was reallocated
    pub fn write(&mut self, queue: &wgpu::Queue, data: &[u8]) -> bool {
        let required_size = data.len() as u64;

        // Check if we need to reallocate
        if required_size > self.capacity {
            self.reallocate(required_size);
            queue.write_buffer(&self.buffer, 0, data);
            true
        } else {
            queue.write_buffer(&self.buffer, 0, data);
            false
        }
    }

    /// Get the underlying buffer
    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Get current capacity in bytes
    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Reallocate buffer to new capacity
    ///
    /// Uses growth factor of 1.5x to avoid frequent reallocations
    fn reallocate(&mut self, min_capacity: u64) {
        // Grow by 1.5x or to min_capacity, whichever is larger
        let new_capacity = (self.capacity * 3 / 2).max(min_capacity);

        tracing::debug!(
            label = %self.label,
            old_capacity = self.capacity,
            new_capacity,
            "Reallocating dynamic buffer"
        );

        self.buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&self.label),
            size: new_capacity,
            usage: self.usage,
            mapped_at_creation: false,
        });

        self.capacity = new_capacity;
    }
}

/// Buffer manager for all GPU buffers
///
/// Manages vertex, instance, and uniform buffers with automatic growth.
pub struct BufferManager {
    /// Vertex buffer for rectangles
    rect_vertex_buffer: DynamicBuffer,

    /// Instance buffer for rectangles
    rect_instance_buffer: DynamicBuffer,

    /// Vertex buffer for paths
    path_vertex_buffer: DynamicBuffer,

    /// Index buffer for paths
    path_index_buffer: DynamicBuffer,

    /// Instance buffer for images
    image_instance_buffer: DynamicBuffer,

    /// Uniform buffer for viewport transform
    uniform_buffer: Buffer,

    /// GPU device
    device: Arc<Device>,
}

impl BufferManager {
    /// Create a new buffer manager
    ///
    /// # Arguments
    ///
    /// * `device` - GPU device
    pub fn new(device: Arc<Device>) -> Self {
        // Create dynamic buffers with initial capacities
        let rect_vertex_buffer = DynamicBuffer::new(
            device.clone(),
            "Rect Vertex Buffer",
            4096, // 4KB initial
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
        );

        let rect_instance_buffer = DynamicBuffer::new(
            device.clone(),
            "Rect Instance Buffer",
            8192, // 8KB initial
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
        );

        let path_vertex_buffer = DynamicBuffer::new(
            device.clone(),
            "Path Vertex Buffer",
            16384, // 16KB initial
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
        );

        let path_index_buffer = DynamicBuffer::new(
            device.clone(),
            "Path Index Buffer",
            8192, // 8KB initial
            BufferUsages::INDEX | BufferUsages::COPY_DST,
        );

        let image_instance_buffer = DynamicBuffer::new(
            device.clone(),
            "Image Instance Buffer",
            4096, // 4KB initial
            BufferUsages::VERTEX | BufferUsages::COPY_DST,
        );

        // Create uniform buffer for viewport transform (constant size)
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Viewport Uniform Buffer"),
            size: 64, // 4x4 matrix (16 floats)
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            rect_vertex_buffer,
            rect_instance_buffer,
            path_vertex_buffer,
            path_index_buffer,
            image_instance_buffer,
            uniform_buffer,
            device,
        }
    }

    /// Get rectangle vertex buffer
    #[must_use]
    pub fn rect_vertex_buffer(&self) -> &Buffer {
        self.rect_vertex_buffer.buffer()
    }

    /// Get rectangle instance buffer
    #[must_use]
    pub fn rect_instance_buffer(&self) -> &Buffer {
        self.rect_instance_buffer.buffer()
    }

    /// Get path vertex buffer
    #[must_use]
    pub fn path_vertex_buffer(&self) -> &Buffer {
        self.path_vertex_buffer.buffer()
    }

    /// Get path index buffer
    #[must_use]
    pub fn path_index_buffer(&self) -> &Buffer {
        self.path_index_buffer.buffer()
    }

    /// Get image instance buffer
    #[must_use]
    pub fn image_instance_buffer(&self) -> &Buffer {
        self.image_instance_buffer.buffer()
    }

    /// Get uniform buffer
    #[must_use]
    pub fn uniform_buffer(&self) -> &Buffer {
        &self.uniform_buffer
    }

    /// Write rectangle vertices
    pub fn write_rect_vertices(&mut self, queue: &wgpu::Queue, data: &[u8]) {
        self.rect_vertex_buffer.write(queue, data);
    }

    /// Write rectangle instances
    pub fn write_rect_instances(&mut self, queue: &wgpu::Queue, data: &[u8]) {
        self.rect_instance_buffer.write(queue, data);
    }

    /// Write path vertices
    pub fn write_path_vertices(&mut self, queue: &wgpu::Queue, data: &[u8]) {
        self.path_vertex_buffer.write(queue, data);
    }

    /// Write path indices
    pub fn write_path_indices(&mut self, queue: &wgpu::Queue, data: &[u8]) {
        self.path_index_buffer.write(queue, data);
    }

    /// Write image instances
    pub fn write_image_instances(&mut self, queue: &wgpu::Queue, data: &[u8]) {
        self.image_instance_buffer.write(queue, data);
    }

    /// Write uniform data (viewport transform)
    pub fn write_uniform(&self, queue: &wgpu::Queue, data: &[u8]) {
        queue.write_buffer(&self.uniform_buffer, 0, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_buffer_capacity() {
        // This is a compile-time check
        // Actual GPU tests require integration tests with a device
        let _ = std::marker::PhantomData::<DynamicBuffer>;
    }

    #[test]
    fn test_buffer_manager_exists() {
        // Compile-time check for BufferManager API
        let _ = std::marker::PhantomData::<BufferManager>;
    }

    #[test]
    fn test_growth_factor() {
        // Test growth calculation
        let old_capacity = 1000u64;
        let new_capacity = (old_capacity * 3 / 2).max(1500);
        assert_eq!(new_capacity, 1500); // Should grow to at least min_capacity
    }

    #[test]
    fn test_growth_factor_large() {
        // Test growth when 1.5x is larger than min
        let old_capacity = 2000u64;
        let new_capacity = (old_capacity * 3 / 2).max(1000);
        assert_eq!(new_capacity, 3000); // Should grow by 1.5x
    }
}
