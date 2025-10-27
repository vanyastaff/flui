//! Vertex formats for GPU rendering

use bytemuck::{Pod, Zeroable};

/// Vertex format for solid-colored geometry
///
/// Used for rendering rectangles, circles, lines with solid colors.
/// Colors are premultiplied alpha (RGB * A, A).
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct SolidVertex {
    /// Position in screen-space coordinates (pixels from top-left)
    pub position: [f32; 2],

    /// RGBA color (premultiplied alpha)
    pub color: [f32; 4],
}

impl SolidVertex {
    /// Create a new solid vertex
    pub fn new(x: f32, y: f32, color: [f32; 4]) -> Self {
        Self {
            position: [x, y],
            color,
        }
    }

    /// Get vertex buffer layout for wgpu
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
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
            ],
        }
    }
}

/// Uniform buffer for viewport information
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ViewportUniforms {
    /// Viewport size (width, height) in pixels
    pub viewport_size: [f32; 2],

    /// Padding for alignment (uniforms must be 16-byte aligned)
    pub _padding: [f32; 2],
}

impl ViewportUniforms {
    /// Create new viewport uniforms
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            viewport_size: [width, height],
            _padding: [0.0, 0.0],
        }
    }
}
