//! Vertex formats for GPU rendering
//!
//! This module defines vertex structures for all primitive types.
//! All vertices use bytemuck for zero-copy GPU uploads.

use bytemuck::{Pod, Zeroable};
use flui_types::{geometry::{Point, DevicePixels}, styling::Color};

/// Generic vertex for rendering
///
/// Used for general-purpose rendering with position, color, and texture coordinates.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    /// Position in device pixels
    pub position: [f32; 2],

    /// RGBA color (0.0 - 1.0)
    pub color: [f32; 4],

    /// Texture coordinates (UV)
    pub tex_coord: [f32; 2],
}

impl Vertex {
    /// Create a new vertex
    #[must_use]
    pub fn new(position: [f32; 2], color: [f32; 4], tex_coord: [f32; 2]) -> Self {
        Self {
            position,
            color,
            tex_coord,
        }
    }

    /// Get the vertex buffer layout descriptor
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // Position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // Texture coordinates
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 2]>() + std::mem::size_of::<[f32; 4]>())
                        as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

/// Vertex for rectangle rendering
///
/// Used for instanced rectangle drawing. Each instance is a quad.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct RectVertex {
    /// Position in device pixels
    pub position: [f32; 2],

    /// RGBA color (0.0 - 1.0)
    pub color: [f32; 4],
}

impl RectVertex {
    /// Create a new rectangle vertex
    #[must_use]
    pub fn new(position: Point<DevicePixels>, color: Color) -> Self {
        Self {
            position: [position.x.0 as f32, position.y.0 as f32],
            color: color.to_linear_f32(),
        }
    }

    /// Get the vertex buffer layout descriptor
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // Position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// Instance data for rectangle rendering
///
/// Each instance represents one rectangle to draw.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct RectInstance {
    /// Top-left corner position
    pub position: [f32; 2],

    /// Size (width, height)
    pub size: [f32; 2],

    /// Border radius (for rounded corners)
    pub border_radius: f32,

    /// RGBA color
    pub color: [f32; 4],

    /// Padding for alignment
    pub _padding: [f32; 3],
}

impl RectInstance {
    /// Create a new rectangle instance
    #[must_use]
    pub fn new(
        position: Point<DevicePixels>,
        width: DevicePixels,
        height: DevicePixels,
        border_radius: f32,
        color: Color,
    ) -> Self {
        Self {
            position: [position.x.0 as f32, position.y.0 as f32],
            size: [width.0 as f32, height.0 as f32],
            border_radius,
            color: color.to_linear_f32(),
            _padding: [0.0; 3],
        }
    }

    /// Get the instance buffer layout descriptor
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // Position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Size
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Border radius
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 2]>() * 2) as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
                // Color
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 2]>() * 2 + std::mem::size_of::<f32>())
                        as wgpu::BufferAddress,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// Vertex for path rendering (lyon tessellation)
///
/// Used for filled vector paths. Lyon generates these vertices.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct PathVertex {
    /// Position in device pixels
    pub position: [f32; 2],

    /// RGBA color
    pub color: [f32; 4],
}

impl PathVertex {
    /// Create a new path vertex
    #[must_use]
    pub fn new(position: Point<DevicePixels>, color: Color) -> Self {
        Self {
            position: [position.x.0 as f32, position.y.0 as f32],
            color: color.to_linear_f32(),
        }
    }

    /// Get the vertex buffer layout descriptor
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<PathVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // Position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// Instance data for textured image rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ImageInstance {
    /// Destination position (top-left)
    pub dst_position: [f32; 2],

    /// Destination size (width, height)
    pub dst_size: [f32; 2],

    /// Source UV top-left (0.0 - 1.0)
    pub src_uv_min: [f32; 2],

    /// Source UV bottom-right (0.0 - 1.0)
    pub src_uv_max: [f32; 2],
}

impl ImageInstance {
    /// Create a new image instance
    #[must_use]
    pub fn new(
        dst_position: Point<DevicePixels>,
        dst_width: DevicePixels,
        dst_height: DevicePixels,
        src_uv_min: [f32; 2],
        src_uv_max: [f32; 2],
    ) -> Self {
        Self {
            dst_position: [dst_position.x.0 as f32, dst_position.y.0 as f32],
            dst_size: [dst_width.0 as f32, dst_height.0 as f32],
            src_uv_min,
            src_uv_max,
        }
    }

    /// Get the instance buffer layout descriptor
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // Dst position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Dst size
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Src UV min
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 2]>() * 2) as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Src UV max
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 2]>() * 3) as wgpu::BufferAddress,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(value: f32) -> DevicePixels {
        DevicePixels(value as i32)
    }

    #[test]
    fn test_rect_vertex_creation() {
        let vertex = RectVertex::new(Point::new(px(10.0), px(20.0)), Color::RED);
        assert_eq!(vertex.position, [10.0, 20.0]);
        assert_eq!(vertex.color[0], 1.0); // Red channel
    }

    #[test]
    fn test_rect_instance_creation() {
        let instance = RectInstance::new(
            Point::new(px(0.0), px(0.0)),
            px(100.0),
            px(50.0),
            5.0,
            Color::BLUE,
        );
        assert_eq!(instance.position, [0.0, 0.0]);
        assert_eq!(instance.size, [100.0, 50.0]);
        assert_eq!(instance.border_radius, 5.0);
        assert_eq!(instance.color[2], 1.0); // Blue channel
    }

    #[test]
    fn test_path_vertex_creation() {
        let vertex = PathVertex::new(Point::new(px(30.0), px(40.0)), Color::GREEN);
        assert_eq!(vertex.position, [30.0, 40.0]);
        assert_eq!(vertex.color[1], 1.0); // Green channel
    }

    #[test]
    fn test_image_instance_creation() {
        let instance = ImageInstance::new(
            Point::new(px(0.0), px(0.0)),
            px(200.0),
            px(100.0),
            [0.0, 0.0],
            [1.0, 1.0],
        );
        assert_eq!(instance.dst_position, [0.0, 0.0]);
        assert_eq!(instance.dst_size, [200.0, 100.0]);
        assert_eq!(instance.src_uv_min, [0.0, 0.0]);
        assert_eq!(instance.src_uv_max, [1.0, 1.0]);
    }

    #[test]
    fn test_vertex_sizes() {
        // Ensure proper alignment for GPU
        assert_eq!(std::mem::size_of::<RectVertex>(), 24); // 2 floats + 4 floats
        assert_eq!(std::mem::size_of::<PathVertex>(), 24); // Same layout
        assert_eq!(std::mem::size_of::<RectInstance>() % 16, 0); // 16-byte aligned
        assert_eq!(std::mem::size_of::<ImageInstance>() % 16, 0); // 16-byte aligned
    }

    #[test]
    fn test_vertex_pod() {
        // Ensure vertices are Plain Old Data (required for bytemuck)
        let _ = bytemuck::cast::<RectVertex, [u8; 24]>(RectVertex {
            position: [0.0, 0.0],
            color: [0.0, 0.0, 0.0, 0.0],
        });
    }
}
