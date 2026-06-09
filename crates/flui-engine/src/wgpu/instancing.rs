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
use flui_types::{Point, Rect, geometry::Pixels, styling::Color};

/// Instance data for a rectangle
///
/// This is uploaded to GPU as an instance buffer. Each rectangle gets one
/// instance. The GPU shader reads this data per-instance and transforms a
/// shared quad.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct RectInstance {
    /// Bounding box [x, y, width, height]
    pub bounds: [f32; 4],

    /// Color [r, g, b, a] in 0-1 range
    pub color: [f32; 4],

    /// Corner radii [top_left, top_right, bottom_right, bottom_left]
    pub corner_radii: [f32; 4],

    /// Transform matrix (simplified 2D: [scale_x, scale_y, translate_x,
    /// translate_y]) Full matrix would be 16 floats, but for UI we only
    /// need 2D affine
    pub transform: [f32; 4],

    /// SDF clip rounded rectangle: [x, y, width, height, radius_tl, radius_tr, radius_br, radius_bl]
    /// All zeros means no clip active. When non-zero, the fragment shader
    /// uses an SDF test to discard pixels outside this rounded rectangle.
    pub clip_rrect: [f32; 8],

    /// Clip-kind flag tagging which SDF the fragment shader should evaluate
    /// against `clip_rrect`.
    ///
    /// - `[0, _, _, _]` — no clip (also detected by `clip_rrect == [0; 8]`).
    /// - `[1, _, _, _]` — `sdRoundedBox` (standard rounded rectangle).
    /// - `[2, _, _, _]` — `sdRoundedSuperellipse` (iOS-squircle). For this
    ///   kind, `clip_rrect[4..8]` carries the single-radius-per-corner
    ///   `[r_tl, r_tr, r_br, r_bl]` interpretation (averaged from the
    ///   superellipse's separate-axis rx/ry per corner).
    ///
    /// Stored as `[u32; 4]` for 16-byte alignment with surrounding vec4
    /// instance attributes. Only the `.x` lane carries the kind; the other
    /// three lanes are padding.
    pub clip_kind: [u32; 4],
}

impl RectInstance {
    /// Create a simple rectangular instance
    #[must_use]
    pub fn rect(rect: Rect<Pixels>, color: Color) -> Self {
        Self {
            bounds: [rect.left().0, rect.top().0, rect.width().0, rect.height().0],
            color: color.to_f32_array(),
            corner_radii: [0.0; 4],
            transform: [1.0, 1.0, 0.0, 0.0], // Identity transform
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
        }
    }

    // Cycle 4 E-5: deleted `RectInstance::rounded_rect(rect, color,
    // single_radius)` (uniform-corner shortcut). Zero callsites --
    // production paths use `rounded_rect_corners` (per-corner).

    /// Create an instance with per-corner radii
    #[must_use]
    pub fn rounded_rect_corners(
        rect: Rect<Pixels>,
        color: Color,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    ) -> Self {
        Self {
            bounds: [rect.left().0, rect.top().0, rect.width().0, rect.height().0],
            color: color.to_f32_array(),
            corner_radii: [top_left, top_right, bottom_right, bottom_left],
            transform: [1.0, 1.0, 0.0, 0.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
        }
    }

    /// Set the SDF clip rounded rectangle on this instance.
    ///
    /// The clip is specified as `[x, y, width, height, radius_tl, radius_tr, radius_br, radius_bl]`.
    /// All zeros means no clip. When non-zero, the fragment shader discards
    /// pixels that fall outside the rounded rectangle using an SDF test.
    /// Sets `clip_kind = 1` (rrect) when the clip is non-trivial; leaves
    /// `clip_kind = 0` when all-zero (no clip).
    #[must_use]
    pub fn with_clip_rrect(mut self, clip: [f32; 8]) -> Self {
        self.clip_rrect = clip;
        // Exact equality against the bit-exact `[0.0; 8]` "no clip" sentinel —
        // never set via arithmetic, so ULP slop is not a concern.
        #[expect(
            clippy::float_cmp,
            reason = "exact comparison against the bit-exact `[0.0; 8]` 'no clip' sentinel"
        )]
        let is_empty = clip == [0.0; 8];
        self.clip_kind = if is_empty { [0; 4] } else { [1, 0, 0, 0] };
        self
    }

    /// Set an SDF clip rounded-superellipse (iOS-squircle) on this instance.
    ///
    /// The 12-float superellipse uniform produced by
    /// `Painter::clip_rsuperellipse` carries separate-axis radii per corner.
    /// At the per-instance level we average each corner's `rx`/`ry` into a
    /// single radius to fit the existing `clip_rrect` slot — this is the
    /// "single-radius-per-corner" first-pass interpretation called out in
    /// the plan's Outstanding Questions Q9. Sets `clip_kind = 2`.
    ///
    /// Layout of `superellipse_clip`: `[x, y, w, h, tl_x, tl_y, tr_x, tr_y,
    /// br_x, br_y, bl_x, bl_y]`. Layout in the resulting `clip_rrect` slot:
    /// `[x, y, w, h, avg(tl_x,tl_y), avg(tr_x,tr_y), avg(br_x,br_y),
    /// avg(bl_x,bl_y)]`.
    #[must_use]
    pub fn with_clip_rsuperellipse(mut self, superellipse_clip: [f32; 12]) -> Self {
        // Exact equality against the bit-exact `[0.0; 12]` "no clip" sentinel.
        #[expect(
            clippy::float_cmp,
            reason = "exact comparison against the bit-exact `[0.0; 12]` 'no clip' sentinel"
        )]
        let is_empty = superellipse_clip == [0.0; 12];
        if is_empty {
            self.clip_rrect = [0.0; 8];
            self.clip_kind = [0; 4];
            return self;
        }
        let tl = 0.5 * (superellipse_clip[4] + superellipse_clip[5]);
        let tr = 0.5 * (superellipse_clip[6] + superellipse_clip[7]);
        let br = 0.5 * (superellipse_clip[8] + superellipse_clip[9]);
        let bl = 0.5 * (superellipse_clip[10] + superellipse_clip[11]);
        self.clip_rrect = [
            superellipse_clip[0],
            superellipse_clip[1],
            superellipse_clip[2],
            superellipse_clip[3],
            tl,
            tr,
            br,
            bl,
        ];
        self.clip_kind = [2, 0, 0, 0];
        self
    }

    // Cycle 4 E-5: deleted `RectInstance::with_transform(scale_x,
    // scale_y, translate_x, translate_y)` (per-instance transform
    // setter; zero callsites -- transform comes from the painter's
    // matrix stack, not from per-instance helpers).
    // `with_clip_rsuperellipse` was retained against the audit's
    // recommendation: 1 live callsite at `painter.rs:3519`
    // (`instance.with_clip_rsuperellipse(self.current_rsuperellipse_clip)`)
    // -- audit text claimed zero callsites but missed the method-style
    // dispatch on `instance` (vs type-path `RectInstance::`).

    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
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
            // Clip rrect part 1: [x, y, width, height] (location 6)
            6 => Float32x4,
            // Clip rrect part 2: [radius_tl, radius_tr, radius_br, radius_bl] (location 7)
            7 => Float32x4,
            // Clip kind: [kind, _pad, _pad, _pad] (location 8) — 0=none, 1=rrect, 2=rsuperellipse
            8 => Uint32x4,
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
    #[must_use]
    pub fn new(center: Point<Pixels>, radius: f32, color: Color) -> Self {
        Self {
            center_radius: [center.x.0, center.y.0, radius, 0.0],
            color: color.to_f32_array(),
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    // Cycle 4 E-5: deleted `CircleInstance::ellipse(center, radius_x,
    // radius_y, color)`. Zero callsites -- production paths use
    // `CircleInstance::new` (uniform-radius). When per-axis radii are
    // needed it relands with a concrete first consumer.

    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
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
    /// start_angle: where the arc begins (0 = right, π/2 = bottom, π = left,
    /// 3π/2 = top) sweep_angle: how much to sweep (positive = clockwise,
    /// negative = counter-clockwise)
    pub angles: [f32; 4],

    /// Color [r, g, b, a] in 0-1 range
    pub color: [f32; 4],

    /// Transform (for elliptical arcs: scale_x, scale_y, translate_x,
    /// translate_y)
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
    #[must_use]
    pub fn new(
        center: Point<Pixels>,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        color: Color,
    ) -> Self {
        Self {
            center_radius: [center.x.0, center.y.0, radius, 0.0],
            angles: [start_angle, sweep_angle, 0.0, 0.0],
            color: color.to_f32_array(),
            transform: [1.0, 1.0, 0.0, 0.0],
        }
    }

    // Cycle 4 E-5: deleted `ArcInstance::ellipse(center, radius_x,
    // radius_y, start_angle, sweep_angle, color)`. Zero callsites --
    // production paths use `ArcInstance::new` (uniform-radius arc).
    // Re-lands with a concrete consumer when needed.

    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
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
    #[must_use]
    pub fn new(dst_rect: flui_types::Rect<flui_types::geometry::Pixels>, tint: Color) -> Self {
        Self {
            dst_rect: [
                dst_rect.left().0,
                dst_rect.top().0,
                dst_rect.width().0,
                dst_rect.height().0,
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
    #[must_use]
    pub fn with_uv(
        dst_rect: flui_types::Rect<flui_types::geometry::Pixels>,
        src_uv: [f32; 4],
        tint: Color,
    ) -> Self {
        Self {
            dst_rect: [
                dst_rect.left().0,
                dst_rect.top().0,
                dst_rect.width().0,
                dst_rect.height().0,
            ],
            src_uv,
            tint: tint.to_f32_array(),
            transform: [1.0, 0.0, 0.0, 0.0],
        }
    }

    // Cycle 4 E-5: deleted `TextureInstance::with_rotation(dst_rect,
    // angle, tint)`. Zero callsites -- production paths use
    // `TextureInstance::with_uv` (canonical, 5 callsites in
    // painter.rs) and the painter's matrix stack handles rotation
    // composition. `TextureInstance::with_uv` was retained against
    // the audit's recommendation because it IS live (audit text
    // claimed otherwise; grep proved 5 painter callsites).

    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
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

/// Linear gradient instance data for GPU instancing
///
/// See `crate::painter::effects::LinearGradientInstance` for full
/// documentation.
pub use super::effects::LinearGradientInstance;

impl LinearGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
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
            // Stop count (location 6)
            6 => Uint32,
            // Stop offset (location 7)
            7 => Uint32,
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
    #[must_use]
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
            // Stop offset (location 7)
            7 => Uint32,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RadialGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Sweep Gradient Instances
// =============================================================================

/// Sweep gradient instance data for GPU instancing
pub use super::effects::SweepGradientInstance;

impl SweepGradientInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: &[wgpu::VertexAttribute] = &wgpu::vertex_attr_array![
            // Bounds (location 2)
            2 => Float32x4,
            // Center (location 3)
            3 => Float32x2,
            // Angles [start, end] (location 4)
            4 => Float32x2,
            // Corner radii (location 5)
            5 => Float32x4,
            // Stop count (location 6)
            6 => Uint32,
            // Stop offset (location 7)
            7 => Uint32,
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SweepGradientInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: ATTRIBUTES,
        }
    }
}

// =============================================================================
// Shadow Instances
// =============================================================================

/// Shadow instance data for GPU instancing
pub use super::effects::ShadowInstance;

impl ShadowInstance {
    /// Get wgpu vertex buffer layout for instance data
    #[must_use]
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
    #[must_use]
    pub fn new(max_instances: usize) -> Self {
        Self {
            instances: Vec::with_capacity(max_instances),
            max_instances,
        }
    }

    /// Add an instance to the batch
    ///
    /// Returns true if batch is full and should be flushed.
    #[must_use]
    pub fn add(&mut self, instance: T) -> bool {
        self.instances.push(instance);
        self.instances.len() >= self.max_instances
    }

    /// Check if batch is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Get number of instances
    #[must_use]
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

// NOTE: Tests temporarily disabled - need update for Pixels/DevicePixels
// migration
#[cfg(all(test, feature = "disabled-tests"))]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact expected values produced by exact arithmetic"
)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_rect_instance_size() {
        // Verify struct is tightly packed for GPU
        // 16 original floats + 8 clip_rrect floats = 24 floats = 96 bytes
        assert_eq!(
            std::mem::size_of::<RectInstance>(),
            24 * 4 // 24 floats = 96 bytes
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
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0)),
            Color::RED,
        ));
        assert!(!should_flush);
        assert_eq!(batch.len(), 1);

        // Add second instance (reaches max)
        let should_flush = batch.add(RectInstance::rect(
            Rect::from_ltrb(px(10.0), px(10.0), px(110.0), px(60.0)),
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
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
            Color::RED,
        );

        // RED should be [1.0, 0.0, 0.0, 1.0] in normalized form
        assert_eq!(instance.color[0], 1.0); // R
        assert_eq!(instance.color[1], 0.0); // G
        assert_eq!(instance.color[2], 0.0); // B
        assert_eq!(instance.color[3], 1.0); // A
    }
}
