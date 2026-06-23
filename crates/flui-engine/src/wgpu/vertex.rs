//! Vertex formats for GPU rendering
//!
//! This module defines vertex structures for all primitive types.
//! All vertices use bytemuck for zero-copy GPU uploads.
//!
//! # Cycle 4 wave 5 E-10
//!
//! The 4 parallel vertex types `RectVertex`, `RectInstance`,
//! `PathVertex`, `ImageInstance` (and their `new` / `desc`
//! constructors + their 6 feature-gated tests) were deleted as
//! workspace zombies:
//!
//! - `RectInstance` collided with the live
//!   [`super::instancing::RectInstance`] — the only `RectInstance`
//!   actually consumed by `painter`'s instancing batch shaders.
//!   The vertex.rs copy had an incompatible field layout (no
//!   `border_radius`, different `desc()`) and zero consumers.
//! - `RectVertex` / `PathVertex` / `ImageInstance` had zero
//!   constructors called and zero `desc()` callers anywhere in the
//!   workspace. They were forward-looking stubs from the original
//!   vertex-format design phase that never wired through to
//!   pipeline construction.
//! - Tests that exercised the dead types lived behind
//!   `#[cfg(all(test, feature = "enable-wgpu-tests"))]`; their
//!   deletion follows from the type deletion (no point keeping
//!   tests for code that doesn't exist).
//!
//! `Vertex` (general-purpose textured vertex) stays — it's the live
//! vertex format consumed by `painter`, `pipeline.rs`,
//! `tessellator.rs`.

use bytemuck::{Pod, Zeroable};

/// Generic vertex for rendering
///
/// Used for general-purpose rendering with position, color, and texture
/// coordinates.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
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
