//! GPU vertex structures and shader uniforms
//!
//! This module contains low-level GPU data structures that are directly uploaded
//! to the GPU. All types use `#[repr(C)]` for consistent memory layout and implement
//! `Pod + Zeroable` from bytemuck for safe memory casting.

use bytemuck::{Pod, Zeroable};
use flui_types::{styling::Color, Point};

/// GPU-optimized vertex structure
///
/// Memory layout (32 bytes total):
/// - position: 8 bytes (2x f32)
/// - color: 16 bytes (4x f32)
/// - uv: 8 bytes (2x f32)
///
/// # Example
/// ```ignore
/// use flui_engine::painter::{Vertex, Color, Point};
///
/// let vertex = Vertex::new(
///     Point::new(100.0, 200.0),
///     Color::RED,
///     [0.5, 0.5]
/// );
/// ```
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    /// 2D position (x, y) in screen space
    pub position: [f32; 2],

    /// RGBA color (r, g, b, a) normalized to [0.0, 1.0]
    pub color: [f32; 4],

    /// UV texture coordinates (u, v) for texture sampling
    pub uv: [f32; 2],
}

impl Vertex {
    /// Create a new vertex with position, color, and UV coordinates
    #[inline]
    #[must_use]
    pub fn new(position: Point, color: Color, uv: [f32; 2]) -> Self {
        let (r, g, b, a) = color.to_rgba_f32();
        Self {
            position: [position.x, position.y],
            color: [r, g, b, a],
            uv,
        }
    }

    /// Create a vertex with only position and color (UV defaults to [0.0, 0.0])
    #[inline]
    #[must_use]
    pub fn with_color(position: Point, color: Color) -> Self {
        Self::new(position, color, [0.0, 0.0])
    }

    /// Get wgpu vertex buffer layout descriptor
    ///
    /// This describes the memory layout for the GPU pipeline.
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // Position (location 0)
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Color (location 1)
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // UV (location 2)
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertex_size() {
        // Vertex should be exactly 32 bytes for optimal GPU transfer
        assert_eq!(std::mem::size_of::<Vertex>(), 32);
    }

    #[test]
    fn test_vertex_creation() {
        let vertex = Vertex::new(
            Point::new(100.0, 200.0),
            Color::rgba(255, 128, 64, 255),
            [0.5, 0.5],
        );

        assert_eq!(vertex.position, [100.0, 200.0]);
        assert_eq!(vertex.uv, [0.5, 0.5]);
    }
}
