//! GPU instancing for batch rendering
//!
//! Based on Bevy's instancing pattern, this module provides efficient rendering
//! of multiple primitives in a single draw call using GPU instancing.
//!
//! # Performance Benefits
//!
//! - **100 rectangles:** 1 draw call instead of 100 (100x reduction)
//! - **1000 UI elements:** ~10 draw calls instead of 1000 (100x reduction)
//! - **CPU overhead:** Minimal (single draw call submission)
//! - **GPU efficiency:** Parallel processing of instances
//!
//! # Architecture
//!
//! ```text
//! Vertex Buffer (shared quad):
//!   [0,0] [1,0] [1,1] [0,1]  ← Single quad vertices
//!
//! Instance Buffer (per-rectangle data):
//!   Instance 0: bounds=[10,10,100,50], color=[255,0,0,255], radii=[0,0,0,0]
//!   Instance 1: bounds=[20,70,150,100], color=[0,255,0,255], radii=[5,5,5,5]
//!   Instance 2: bounds=[200,10,80,80], color=[0,0,255,255], radii=[10,10,10,10]
//!   ...
//!
//! Draw call: draw_indexed(indices=6, instances=N)
//! GPU processes N rectangles in parallel!
//! ```

use bytemuck::{Pod, Zeroable};
use flui_types::{styling::Color, Point, Rect};

/// Instance data for a rectangle
///
/// This is uploaded to GPU as an instance buffer. Each rectangle gets one instance.
/// The GPU shader reads this data per-instance and transforms a shared quad.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct RectInstance {
    /// Bounding box [x, y, width, height]
    pub bounds: [f32; 4],

    /// Color [r, g, b, a] in 0-1 range
    pub color: [f32; 4],

    /// Corner radii [top_left, top_right, bottom_right, bottom_left]
    pub corner_radii: [f32; 4],

    /// Transform matrix (simplified 2D: [scale_x, scale_y, translate_x, translate_y])
    /// Full matrix would be 16 floats, but for UI we only need 2D affine
    pub transform: [f32; 4],
}

impl RectInstance {
    /// Create a simple rectangular instance
    pub fn rect(rect: Rect, color: Color) -> Self {
        Self {
            bounds: [rect.left(), rect.top(), rect.width(), rect.height()],
            color: color.to_f32_array(),
            corner_radii: [0.0; 4],
            transform: [1.0, 1.0, 0.0, 0.0], // Identity transform
        }
    }

    /// Create a rounded rectangular instance
    pub fn rounded_rect(rect: Rect, color: Color, radius: f32) -> Self {
        Self {
            bounds: [rect.left(), rect.top(), rect.width(), rect.height()],
            color: color.to_f32_array(),
            corner_radii: [radius; 4],
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create an instance with per-corner radii
    pub fn rounded_rect_corners(
        rect: Rect,
        color: Color,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    ) -> Self {
        Self {
            bounds: [rect.left(), rect.top(), rect.width(), rect.height()],
            color: color.to_f32_array(),
            corner_radii: [top_left, top_right, bottom_right, bottom_left],
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create an instance with transform
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

    /// Get wgpu vertex buffer layout for instance data
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds (location 2)
            2 => Float32x4,
            // Color (location 3)
            3 => Float32x4,
            // Corner radii (location 4)
            4 => Float32x4,
            // Transform (location 5)
            5 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for a circle
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CircleInstance {
    /// Center point [x, y] and radius [radius, _padding]
    pub center_radius: [f32; 4],

    /// Color [r, g, b, a] in 0-1 range
    pub color: [f32; 4],

    /// Transform (for ellipses: scale_x, scale_y)
    pub transform: [f32; 4],
}

impl CircleInstance {
    /// Create a circle instance
    pub fn new(center: Point, radius: f32, color: Color) -> Self {
        Self {
            center_radius: [center.x, center.y, radius, 0.0],
            color: color.to_f32_array(),
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create an ellipse instance (stretched circle)
    pub fn ellipse(center: Point, radius_x: f32, radius_y: f32, color: Color) -> Self {
        Self {
            center_radius: [center.x, center.y, radius_x.max(radius_y), 0.0],
            color: color.to_f32_array(),
            transform: [
                radius_x / radius_x.max(radius_y),
                radius_y / radius_x.max(radius_y),
                0.0,
                0.0,
            ],
        }
    }

    /// Get wgpu vertex buffer layout for instance data
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Center + radius (location 2)
            2 => Float32x4,
            // Color (location 3)
            3 => Float32x4,
            // Transform (location 4)
            4 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CircleInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for an arc (partial circle)
///
/// Used for progress indicators, pie charts, and other arc-based UI elements.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ArcInstance {
    /// Center point [x, y], radius, and padding [radius, _padding]
    pub center_radius: [f32; 4],

    /// Angles in radians [start_angle, sweep_angle, _padding, _padding]
    /// start_angle: where the arc begins (0 = right, π/2 = bottom, π = left, 3π/2 = top)
    /// sweep_angle: how much to sweep (positive = clockwise, negative = counter-clockwise)
    pub angles: [f32; 4],

    /// Color [r, g, b, a] in 0-1 range
    pub color: [f32; 4],

    /// Transform (for elliptical arcs: scale_x, scale_y, translate_x, translate_y)
    pub transform: [f32; 4],
}

impl ArcInstance {
    /// Create an arc instance
    ///
    /// # Arguments
    /// * `center` - Center point of the arc
    /// * `radius` - Radius of the arc
    /// * `start_angle` - Starting angle in radians (0 = right)
    /// * `sweep_angle` - Sweep angle in radians (positive = clockwise)
    /// * `color` - Arc color
    pub fn new(
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        color: Color,
    ) -> Self {
        Self {
            center_radius: [center.x, center.y, radius, 0.0],
            angles: [start_angle, sweep_angle, 0.0, 0.0],
            color: color.to_f32_array(),
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    /// Create an elliptical arc instance
    pub fn ellipse(
        center: Point,
        radius_x: f32,
        radius_y: f32,
        start_angle: f32,
        sweep_angle: f32,
        color: Color,
    ) -> Self {
        let max_radius = radius_x.max(radius_y);
        Self {
            center_radius: [center.x, center.y, max_radius, 0.0],
            angles: [start_angle, sweep_angle, 0.0, 0.0],
            color: color.to_f32_array(),
            transform: [radius_x / max_radius, radius_y / max_radius, 0.0, 0.0],
        }
    }

    /// Get wgpu vertex buffer layout for instance data
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Center + radius (location 2)
            2 => Float32x4,
            // Angles (location 3)
            3 => Float32x4,
            // Color (location 4)
            4 => Float32x4,
            // Transform (location 5)
            5 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ArcInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Instance data for a textured quad (images, sprites, icons)
///
/// Used for rendering images, icons, and sprites with GPU instancing.
/// Supports texture atlases via UV coordinates.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct TextureInstance {
    /// Destination rectangle [x, y, width, height] in screen space
    pub dst_rect: [f32; 4],

    /// Source UV coordinates [u_min, v_min, u_max, v_max] in 0-1 range
    /// For whole texture: [0.0, 0.0, 1.0, 1.0]
    /// For atlas region: [u_start, v_start, u_end, v_end]
    pub src_uv: [f32; 4],

    /// Color tint [r, g, b, a] in 0-1 range
    /// Use [1.0, 1.0, 1.0, 1.0] for no tint
    pub tint: [f32; 4],

    /// Transform (rotation and additional translation)
    /// [cos(angle), sin(angle), translate_x, translate_y]
    /// For no rotation: [1.0, 0.0, 0.0, 0.0]
    pub transform: [f32; 4],
}

impl TextureInstance {
    /// Create a simple textured quad instance
    ///
    /// # Arguments
    /// * `dst_rect` - Destination rectangle in screen coordinates
    /// * `tint` - Color tint (use Color::WHITE for no tint)
    pub fn new(dst_rect: flui_types::Rect, tint: Color) -> Self {
        Self {
            dst_rect: [
                dst_rect.left(),
                dst_rect.top(),
                dst_rect.width(),
                dst_rect.height(),
            ],
            src_uv: [0.0, 0.0, 1.0, 1.0], // Full texture
            tint: tint.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 0.0], // No rotation
        }
    }

    /// Create a textured quad with custom UV coordinates (for texture atlas)
    ///
    /// # Arguments
    /// * `dst_rect` - Destination rectangle in screen coordinates
    /// * `src_uv` - Source UV rectangle [u_min, v_min, u_max, v_max]
    /// * `tint` - Color tint
    pub fn with_uv(dst_rect: flui_types::Rect, src_uv: [f32; 4], tint: Color) -> Self {
        Self {
            dst_rect: [
                dst_rect.left(),
                dst_rect.top(),
                dst_rect.width(),
                dst_rect.height(),
            ],
            src_uv,
            tint: tint.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 0.0],
        }
    }

    /// Create a textured quad with rotation
    ///
    /// # Arguments
    /// * `dst_rect` - Destination rectangle in screen coordinates
    /// * `angle` - Rotation angle in radians
    /// * `tint` - Color tint
    pub fn with_rotation(dst_rect: flui_types::Rect, angle: f32, tint: Color) -> Self {
        Self {
            dst_rect: [
                dst_rect.left(),
                dst_rect.top(),
                dst_rect.width(),
                dst_rect.height(),
            ],
            src_uv: [0.0, 0.0, 1.0, 1.0],
            tint: tint.to_f32_array(),
            transform: [angle.cos(), angle.sin(), 0.0, 0.0],
        }
    }

    /// Get wgpu vertex buffer layout for instance data
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Destination rect (location 2)
            2 => Float32x4,
            // Source UV (location 3)
            3 => Float32x4,
            // Tint color (location 4)
            4 => Float32x4,
            // Transform (location 5)
            5 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TextureInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Gradient Instances (from effects.rs for API consistency)
// =============================================================================

/// A single color stop in a gradient
pub use super::effects::GradientStop;

/// Linear gradient instance data for GPU instancing
///
/// See `crate::painter::effects::LinearGradientInstance` for full documentation.
pub use super::effects::LinearGradientInstance;

impl LinearGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds (location 2)
            2 => Float32x4,
            // Gradient start (location 3)
            3 => Float32x2,
            // Gradient end (location 4)
            4 => Float32x2,
            // Corner radii (location 5)
            5 => Float32x4,
            // Stop count (location 6) - using Uint32 for integer
            6 => Uint32,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LinearGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

/// Radial gradient instance data for GPU instancing
pub use super::effects::RadialGradientInstance;

impl RadialGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds (location 2)
            2 => Float32x4,
            // Center (location 3)
            3 => Float32x2,
            // Radius + padding (location 4)
            4 => Float32x2,
            // Corner radii (location 5)
            5 => Float32x4,
            // Stop count (location 6)
            6 => Uint32,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RadialGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Shadow Instances
// =============================================================================

/// Shadow parameters for Material Design elevation levels
pub use super::effects::ShadowParams;

/// Shadow instance data for GPU instancing
pub use super::effects::ShadowInstance;

impl ShadowInstance {
    /// Get wgpu vertex buffer layout for instance data
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Shadow bounds (location 2)
            2 => Float32x4,
            // Rect pos (location 3)
            3 => Float32x2,
            // Rect size (location 4)
            4 => Float32x2,
            // Corner radius + padding (location 5)
            5 => Float32x4,
            // Shadow offset (location 6)
            6 => Float32x2,
            // Blur sigma + padding (location 7)
            7 => Float32x2,
            // Shadow color (location 8)
            8 => Float32x4,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ShadowInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Generic Instance Batch
// =============================================================================

/// Batch of instances ready for rendering
///
/// Groups instances by type for efficient rendering.
pub struct InstanceBatch<T> {
    /// Instance data
    pub instances: Vec<T>,

    /// Maximum instances before auto-flush
    pub max_instances: usize,
}

impl<T> InstanceBatch<T> {
    /// Create a new instance batch
    pub fn new(max_instances: usize) -> Self {
        Self {
            instances: Vec::with_capacity(max_instances),
            max_instances,
        }
    }

    /// Add an instance to the batch
    ///
    /// Returns true if batch is full and should be flushed.
    pub fn add(&mut self, instance: T) -> bool {
        self.instances.push(instance);
        self.instances.len() >= self.max_instances
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Get number of instances
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Clear the batch
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    /// Get instance data as byte slice
    pub fn as_bytes(&self) -> &[u8]
    where
        T: Pod,
    {
        bytemuck::cast_slice(&self.instances)
    }
}

impl<T> Default for InstanceBatch<T> {
    fn default() -> Self {
        Self::new(1024) // Default: 1024 instances per batch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_instance_size() {
        // Verify struct is tightly packed for GPU
        assert_eq!(
            std::mem::size_of::<RectInstance>(),
            16 * 4 // 16 floats = 64 bytes
        );
    }

    #[test]
    fn test_circle_instance_size() {
        assert_eq!(
            std::mem::size_of::<CircleInstance>(),
            12 * 4 // 12 floats = 48 bytes
        );
    }

    #[test]
    fn test_arc_instance_size() {
        // Verify struct is tightly packed for GPU
        assert_eq!(
            std::mem::size_of::<ArcInstance>(),
            16 * 4 // 16 floats = 64 bytes
        );
    }

    #[test]
    fn test_texture_instance_size() {
        // Verify struct is tightly packed for GPU
        assert_eq!(
            std::mem::size_of::<TextureInstance>(),
            16 * 4 // 16 floats = 64 bytes
        );
    }

    #[test]
    fn test_instance_batch() {
        let mut batch = InstanceBatch::<RectInstance>::new(2);

        // Add first instance
        let should_flush = batch.add(RectInstance::rect(
            Rect::from_ltrb(0.0, 0.0, 100.0, 50.0),
            Color::RED,
        ));
        assert!(!should_flush);
        assert_eq!(batch.len(), 1);

        // Add second instance (reaches max)
        let should_flush = batch.add(RectInstance::rect(
            Rect::from_ltrb(10.0, 10.0, 110.0, 60.0),
            Color::BLUE,
        ));
        assert!(should_flush);
        assert_eq!(batch.len(), 2);

        // Clear
        batch.clear();
        assert!(batch.is_empty());
    }

    #[test]
    fn test_color_conversion() {
        let instance = RectInstance::rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0), Color::RED);

        // RED should be [1.0, 0.0, 0.0, 1.0] in normalized form
        assert_eq!(instance.color[0], 1.0); // R
        assert_eq!(instance.color[1], 0.0); // G
        assert_eq!(instance.color[2], 0.0); // B
        assert_eq!(instance.color[3], 1.0); // A
    }
}
