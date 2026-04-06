//! Consolidated vertex and instance types for GPU rendering
//!
//! All vertex structures use `#[repr(C)]` and derive `Pod`/`Zeroable` for
//! zero-copy GPU uploads via bytemuck. Fields use raw `[f32; N]` arrays
//! rather than higher-level types to match GPU buffer layouts exactly.

use bytemuck::{Pod, Zeroable};

// ============================================================================
// Vertex types
// ============================================================================

/// General-purpose vertex with position, color, and texture coordinates.
///
/// Used for textured quads and general rendering.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    /// Position in device pixels `[x, y]`
    pub position: [f32; 2],

    /// RGBA color in 0.0..1.0 range
    pub color: [f32; 4],

    /// Texture UV coordinates `[u, v]`
    pub tex_coord: [f32; 2],
}

impl Vertex {
    /// Create a new vertex.
    #[must_use]
    pub fn new(position: [f32; 2], color: [f32; 4], tex_coord: [f32; 2]) -> Self {
        Self {
            position,
            color,
            tex_coord,
        }
    }

    /// Vertex buffer layout descriptor for this vertex type.
    #[cfg(feature = "wgpu-backend")]
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            0 => Float32x2, // position
            1 => Float32x4, // color
            2 => Float32x2, // tex_coord
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRIBUTES,
        }
    }
}

/// Vertex for tessellated vector paths (lyon output).
///
/// Minimal vertex carrying only position and color; no texture coordinates.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct PathVertex {
    /// Position in device pixels `[x, y]`
    pub position: [f32; 2],

    /// RGBA color in 0.0..1.0 range
    pub color: [f32; 4],
}

impl PathVertex {
    /// Create a new path vertex from raw coordinates and color.
    #[must_use]
    pub fn new(position: [f32; 2], color: [f32; 4]) -> Self {
        Self { position, color }
    }

    /// Vertex buffer layout descriptor for this vertex type.
    #[cfg(feature = "wgpu-backend")]
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            0 => Float32x2, // position
            1 => Float32x4, // color
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<PathVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: ATTRIBUTES,
        }
    }
}

// ============================================================================
// Instance types
// ============================================================================

/// Instance data for rectangle rendering via GPU instancing.
///
/// The GPU shader transforms a shared unit quad using these per-instance values.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct RectInstance {
    /// Bounding box `[x, y, width, height]`
    pub bounds: [f32; 4],

    /// RGBA color in 0.0..1.0 range
    pub color: [f32; 4],

    /// Per-corner radii `[top_left, top_right, bottom_right, bottom_left]`
    pub corner_radii: [f32; 4],

    /// Simplified 2D transform `[scale_x, scale_y, translate_x, translate_y]`
    pub transform: [f32; 4],
}

impl RectInstance {
    /// Create a simple (non-rounded) rectangle instance.
    #[must_use]
    pub fn rect(bounds: [f32; 4], color: [f32; 4]) -> Self {
        Self {
            bounds,
            color,
            corner_radii: [0.0; 4],
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create a rectangle with uniform corner radius.
    #[must_use]
    pub fn rounded_rect(bounds: [f32; 4], color: [f32; 4], radius: f32) -> Self {
        Self {
            bounds,
            color,
            corner_radii: [radius; 4],
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Apply a 2D transform (builder pattern).
    #[must_use]
    pub fn with_transform(
        mut self,
        scale_x: f32,
        scale_y: f32,
        translate_x: f32,
        translate_y: f32,
    ) -> Self {
        self.transform = [scale_x, scale_y, translate_x, translate_y];
        self
    }

    /// Instance buffer layout descriptor.
    #[cfg(feature = "wgpu-backend")]
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            2 => Float32x4, // bounds
            3 => Float32x4, // color
            4 => Float32x4, // corner_radii
            5 => Float32x4, // transform
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for circle / ellipse rendering via GPU instancing.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CircleInstance {
    /// Center point `[x, y]`
    pub center: [f32; 2],

    /// Radii `[rx, ry]` (equal for circles, different for ovals)
    pub radius: [f32; 2],

    /// RGBA color in 0.0..1.0 range
    pub color: [f32; 4],

    /// Simplified 2D transform `[scale_x, scale_y, translate_x, translate_y]`
    pub transform: [f32; 4],
}

impl CircleInstance {
    /// Create a circle instance with equal radii.
    #[must_use]
    pub fn circle(center: [f32; 2], radius: f32, color: [f32; 4]) -> Self {
        Self {
            center,
            radius: [radius, radius],
            color,
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create an oval (ellipse) instance with independent radii.
    #[must_use]
    pub fn oval(center: [f32; 2], rx: f32, ry: f32, color: [f32; 4]) -> Self {
        Self {
            center,
            radius: [rx, ry],
            color,
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Instance buffer layout descriptor.
    #[cfg(feature = "wgpu-backend")]
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            2 => Float32x2, // center
            3 => Float32x2, // radius
            4 => Float32x4, // color
            5 => Float32x4, // transform
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CircleInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for arc (partial circle) rendering.
///
/// Used for progress indicators, pie charts, and similar arc-based UI elements.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ArcInstance {
    /// Center point `[x, y]`
    pub center: [f32; 2],

    /// Arc radius
    pub radius: f32,

    /// Start angle in radians (0 = right, pi/2 = down)
    pub start_angle: f32,

    /// Sweep angle in radians (positive = clockwise)
    pub sweep_angle: f32,

    /// RGBA color in 0.0..1.0 range
    pub color: [f32; 4],

    /// Padding for 16-byte alignment
    pub _padding: [f32; 3],
}

impl ArcInstance {
    /// Create a new arc instance.
    #[must_use]
    pub fn new(
        center: [f32; 2],
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        color: [f32; 4],
    ) -> Self {
        Self {
            center,
            radius,
            start_angle,
            sweep_angle,
            color,
            _padding: [0.0; 3],
        }
    }

    /// Instance buffer layout descriptor.
    #[cfg(feature = "wgpu-backend")]
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            2 => Float32x2, // center
            3 => Float32,   // radius
            4 => Float32,   // start_angle
            5 => Float32,   // sweep_angle
            6 => Float32x4, // color
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ArcInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for line segment rendering.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct LineInstance {
    /// Start point `[x, y]`
    pub start: [f32; 2],

    /// End point `[x, y]`
    pub end: [f32; 2],

    /// RGBA color in 0.0..1.0 range
    pub color: [f32; 4],

    /// Line width in device pixels
    pub width: f32,

    /// Padding for 16-byte alignment
    pub _padding: [f32; 3],
}

impl LineInstance {
    /// Create a new line instance.
    #[must_use]
    pub fn new(start: [f32; 2], end: [f32; 2], color: [f32; 4], width: f32) -> Self {
        Self {
            start,
            end,
            color,
            width,
            _padding: [0.0; 3],
        }
    }
}

/// Instance data for textured image quad rendering.
///
/// Supports texture atlases via UV coordinates and color tinting.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ImageQuadInstance {
    /// Destination bounds `[x, y, width, height]` in screen space
    pub dst_bounds: [f32; 4],

    /// Source UV coordinates `[u_min, v_min, u_max, v_max]`
    pub src_uv: [f32; 4],

    /// Color tint (use `[1.0, 1.0, 1.0, 1.0]` for no tint)
    pub color: [f32; 4],

    /// Transform `[cos(angle), sin(angle), translate_x, translate_y]`
    pub transform: [f32; 4],
}

impl ImageQuadInstance {
    /// Create a new image quad instance.
    ///
    /// Uses full UV range `[0, 0, 1, 1]`, identity transform, and the given tint color.
    #[must_use]
    pub fn new(dst_bounds: [f32; 4], color: [f32; 4]) -> Self {
        Self {
            dst_bounds,
            src_uv: [0.0, 0.0, 1.0, 1.0],
            color,
            transform: [1.0, 0.0, 0.0, 0.0],
        }
    }

    /// Instance buffer layout descriptor.
    #[cfg(feature = "wgpu-backend")]
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            2 => Float32x4, // dst_bounds
            3 => Float32x4, // src_uv
            4 => Float32x4, // color
            5 => Float32x4, // transform
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageQuadInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// ============================================================================
// Uniforms
// ============================================================================

/// Per-frame uniform data uploaded to the GPU.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct FrameUniforms {
    /// Viewport size in physical pixels `[width, height]`
    pub viewport_size: [f32; 2],

    /// Display scale factor (e.g. 2.0 for Retina)
    pub scale_factor: f32,

    /// Padding for 16-byte alignment
    pub _padding: f32,
}

impl FrameUniforms {
    /// Create new frame uniforms.
    #[must_use]
    pub fn new(width: f32, height: f32, scale_factor: f32) -> Self {
        Self {
            viewport_size: [width, height],
            scale_factor,
            _padding: 0.0,
        }
    }
}

// ============================================================================
// Shared geometry constants
// ============================================================================

/// Unit quad vertex positions for instanced rendering.
///
/// A `[0,0]..[1,1]` quad that the vertex shader scales per-instance.
pub const UNIT_QUAD_VERTICES: &[[f32; 2]] = &[
    [0.0, 0.0], // top-left
    [1.0, 0.0], // top-right
    [1.0, 1.0], // bottom-right
    [0.0, 1.0], // bottom-left
];

/// Index buffer for [`UNIT_QUAD_VERTICES`] (two triangles).
pub const UNIT_QUAD_INDICES: &[u16] = &[
    0, 1, 2, // first triangle
    0, 2, 3, // second triangle
];

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_sizes_are_gpu_aligned() {
        // Vertex: 2 + 4 + 2 = 8 floats = 32 bytes
        assert_eq!(std::mem::size_of::<Vertex>(), 32);
        // PathVertex: 2 + 4 = 6 floats = 24 bytes
        assert_eq!(std::mem::size_of::<PathVertex>(), 24);
        // RectInstance: 4*4 = 16 floats = 64 bytes (16-byte aligned)
        assert_eq!(std::mem::size_of::<RectInstance>(), 64);
        assert_eq!(std::mem::size_of::<RectInstance>() % 16, 0);
        // CircleInstance: 2+2+4+4 = 12 floats = 48 bytes (16-byte aligned)
        assert_eq!(std::mem::size_of::<CircleInstance>(), 48);
        assert_eq!(std::mem::size_of::<CircleInstance>() % 16, 0);
        // ArcInstance: 2+1+1+1+4+3 = 12 floats = 48 bytes (16-byte aligned)
        assert_eq!(std::mem::size_of::<ArcInstance>(), 48);
        assert_eq!(std::mem::size_of::<ArcInstance>() % 16, 0);
        // LineInstance: 2+2+4+1+3 = 12 floats = 48 bytes (16-byte aligned)
        assert_eq!(std::mem::size_of::<LineInstance>(), 48);
        assert_eq!(std::mem::size_of::<LineInstance>() % 16, 0);
        // ImageQuadInstance: 4*4 = 16 floats = 64 bytes (16-byte aligned)
        assert_eq!(std::mem::size_of::<ImageQuadInstance>(), 64);
        assert_eq!(std::mem::size_of::<ImageQuadInstance>() % 16, 0);
        // FrameUniforms: 2+1+1 = 4 floats = 16 bytes
        assert_eq!(std::mem::size_of::<FrameUniforms>(), 16);
    }

    #[test]
    fn bytemuck_roundtrip() {
        let rect = RectInstance::rect([10.0, 20.0, 100.0, 50.0], [1.0, 0.0, 0.0, 1.0]);
        let bytes: &[u8] = bytemuck::bytes_of(&rect);
        let back: &RectInstance = bytemuck::from_bytes(bytes);
        assert_eq!(back.bounds, rect.bounds);
        assert_eq!(back.color, rect.color);
    }

    #[test]
    fn unit_quad_geometry() {
        assert_eq!(UNIT_QUAD_VERTICES.len(), 4);
        assert_eq!(UNIT_QUAD_INDICES.len(), 6);
    }

    #[test]
    fn frame_uniforms_construction() {
        let u = FrameUniforms::new(1920.0, 1080.0, 2.0);
        assert_eq!(u.viewport_size, [1920.0, 1080.0]);
        assert_eq!(u.scale_factor, 2.0);
        assert_eq!(u._padding, 0.0);
    }

    #[test]
    fn circle_constructors() {
        let c = CircleInstance::circle([100.0, 200.0], 50.0, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(c.center, [100.0, 200.0]);
        assert_eq!(c.radius, [50.0, 50.0]);

        let o = CircleInstance::oval([100.0, 200.0], 50.0, 30.0, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(o.radius, [50.0, 30.0]);
    }

    #[test]
    fn rect_with_transform() {
        let r = RectInstance::rect([0.0; 4], [1.0; 4]).with_transform(2.0, 2.0, 10.0, 20.0);
        assert_eq!(r.transform, [2.0, 2.0, 10.0, 20.0]);
    }
}
