//! Linear / radial / sweep gradient descriptors and their shared color stop.

use bytemuck::{Pod, Zeroable};
use flui_painting::styling::Color;
use glam::Vec2;

// =============================================================================
// Gradient Types
// =============================================================================

/// A single color stop in a gradient
///
/// Gradients are defined by a series of stops, each with a color and position.
/// Colors are linearly interpolated between stops.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct GradientStop {
    /// RGBA color, straight (not premultiplied), each channel in `0.0..=1.0`.
    color: [f32; 4],
    /// Position along the gradient, clamped to `0.0..=1.0` at construction.
    position: f32,
    /// GPU alignment padding, always zero.
    ///
    /// Private on purpose: this value is uploaded verbatim as the shader's
    /// `_pad0.._pad2`, so a caller-supplied value would be GPU-visible data
    /// the caller has no reason to control. Construction goes through
    /// [`Self::new`] / [`Self::from_rgba`], which zero it.
    padding: [f32; 3],
}

impl GradientStop {
    /// Create a new gradient stop. `position` is clamped to `0.0..=1.0`.
    #[must_use]
    pub(crate) fn new(color: Color, position: f32) -> Self {
        Self::from_rgba(color.to_rgba_f32().into(), position)
    }

    /// Create a stop from straight RGBA channels, each in `0.0..=1.0`.
    ///
    /// The `Color`-taking constructor covers the common path; this exists for
    /// callers already holding a channel array (a colour filter's output, a
    /// vertex colour) that would otherwise round-trip through `Color`.
    #[must_use]
    pub(crate) fn from_rgba(rgba: [f32; 4], position: f32) -> Self {
        Self {
            color: rgba,
            position: position.clamp(0.0, 1.0),
            padding: [0.0; 3],
        }
    }
}

/// Linear gradient instance data for GPU instancing
///
/// Each instance represents one gradient-filled rectangle.
/// Multiple instances can be batched into a single draw call.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct LinearGradientInstance {
    /// Rectangle bounds [x, y, width, height]
    pub bounds: [f32; 4],
    /// Local parameter coefficients `[a, b, c, 0]`: `t = a*x + b*y + c`.
    pub linear_parameter: [f32; 4],
    /// Corner radii [top-left, top-right, bottom-right, bottom-left]
    pub corner_radii: [f32; 4],
    /// Number of validated gradient stops
    pub stop_count: u32,
    /// Offset into the shared gradient stops buffer
    pub stop_offset: u32,
    /// Clip-local rounded-rect clip: `[x, y, w, h, tl, tr, br, bl]`.
    ///
    /// All-zero means "no clip". Attached by `apply_active_clip`; the shader
    /// maps a fragment's device position into this space with
    /// `clip_device_to_local` before evaluating it. The scissor alone can only
    /// express the clip's axis-aligned bounding box, which leaves square
    /// corners on a `ClipRRect`.
    ///
    /// Placed BEFORE the trailing padding field on purpose: `desc()` builds its
    /// attribute offsets with `vertex_attr_array!`, which lays attributes out
    /// contiguously and knows nothing about padding fields. A clip slot after
    /// the padding would be read 8 bytes early.
    pub clip_rrect: [f32; 8],
    /// `[kind, _, _, _]`: 0 = none, 1 = rrect, 2 = rounded superellipse.
    pub clip_kind: [u32; 4],
    /// Device-to-clip-local linear part: `[a, b, c, d]`, columns first.
    ///
    /// The clip's bounds and radii are in the space the caller set them in;
    /// this maps a device-space fragment position back there. Identity is
    /// `[1, 0, 0, 1]`.
    pub clip_device_to_local: [f32; 4],
    /// Device-to-clip-local translation, padded to a `vec4` attribute:
    /// `[tx, ty, 0, 0]`.
    pub clip_local_origin: [f32; 4],
    /// Local-to-device affine columns.
    pub transform: [f32; 4],
    /// Local-to-device translation, padded for the vertex attribute.
    pub transform_translate: [f32; 4],
    /// Padding for GPU alignment
    pub padding: [u32; 2],
}

impl LinearGradientInstance {
    /// Create a new linear gradient instance
    pub(crate) fn new(
        bounds: [f32; 4],
        linear_parameter: [f32; 4],
        corner_radii: [f32; 4],
        stop_count: u32,
    ) -> Self {
        Self {
            bounds,
            linear_parameter,
            corner_radii,
            stop_count,
            stop_offset: 0,
            padding: [0; 2],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
            transform: [1.0, 0.0, 0.0, 1.0],
            transform_translate: [0.0; 4],
        }
    }

    pub(crate) fn with_transform(mut self, matrix: glam::DMat4, origin: [f64; 2]) -> Self {
        // Rebase in f64 before packing. Adding a tiny local extent to a large
        // f32 origin would destroy the quad before an opposing CTM translation.
        let mapped = matrix * glam::dvec4(origin[0], origin[1], 0.0, 1.0);
        self.bounds[0] = 0.0;
        self.bounds[1] = 0.0;
        self.transform = [
            matrix.x_axis.x as f32,
            matrix.x_axis.y as f32,
            matrix.y_axis.x as f32,
            matrix.y_axis.y as f32,
        ];
        self.transform_translate = [mapped.x as f32, mapped.y as f32, 0.0, 0.0];
        self
    }

    pub(crate) fn device_corners(&self) -> [[f32; 2]; 4] {
        let [x, y, w, h] = self.bounds;
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| {
            [
                self.transform[0] * p[0] + self.transform[2] * p[1] + self.transform_translate[0],
                self.transform[1] * p[0] + self.transform[3] * p[1] + self.transform_translate[1],
            ]
        })
    }

    /// Set the offset into the shared gradient stops buffer
    pub(crate) fn with_stop_offset(mut self, offset: u32) -> Self {
        self.stop_offset = offset;
        self
    }
}

/// Radial gradient instance data for GPU instancing
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct RadialGradientInstance {
    /// Rectangle bounds [x, y, width, height]
    pub bounds: [f32; 4],
    /// Second circle center in normalized bounds-local coordinates
    pub center: [f32; 2],
    /// Normalized second circle radius
    pub radius: f32,
    /// Tile mode: Clamp=0, Repeat=1, Mirror=2, Decal=3.
    pub tile_mode: f32,
    /// Corner radii [top-left, top-right, bottom-right, bottom-left]
    pub corner_radii: [f32; 4],
    /// Number of validated gradient stops
    pub stop_count: u32,
    /// Offset into the shared gradient stops buffer
    pub stop_offset: u32,
    /// Clip-local rounded-rect clip: `[x, y, w, h, tl, tr, br, bl]`.
    ///
    /// All-zero means "no clip". Attached by `apply_active_clip`; the shader
    /// maps a fragment's device position into this space with
    /// `clip_device_to_local` before evaluating it. The scissor alone can only
    /// express the clip's axis-aligned bounding box, which leaves square
    /// corners on a `ClipRRect`.
    ///
    /// Placed BEFORE the trailing padding field on purpose: `desc()` builds its
    /// attribute offsets with `vertex_attr_array!`, which lays attributes out
    /// contiguously and knows nothing about padding fields. A clip slot after
    /// the padding would be read 8 bytes early.
    pub clip_rrect: [f32; 8],
    /// `[kind, _, _, _]`: 0 = none, 1 = rrect, 2 = rounded superellipse.
    pub clip_kind: [u32; 4],
    /// Device-to-clip-local linear part: `[a, b, c, d]`, columns first.
    ///
    /// The clip's bounds and radii are in the space the caller set them in;
    /// this maps a device-space fragment position back there. Identity is
    /// `[1, 0, 0, 1]`.
    pub clip_device_to_local: [f32; 4],
    /// Device-to-clip-local translation, padded to a `vec4` attribute:
    /// `[tx, ty, 0, 0]`.
    pub clip_local_origin: [f32; 4],
    /// Local-to-device affine columns.
    pub transform: [f32; 4],
    /// Local-to-device translation, padded for the vertex attribute.
    pub transform_translate: [f32; 4],
    /// Normalized focal center, initial radius and inverse coordinate scale.
    pub focal: [f32; 4],
    /// Padding for GPU alignment
    pub padding2: [u32; 2],
}

impl RadialGradientInstance {
    /// Create a new radial gradient instance
    pub(crate) fn new(
        bounds: [f32; 4],
        center: Vec2,
        radius: f32,
        corner_radii: [f32; 4],
        stop_count: u32,
    ) -> Self {
        Self {
            bounds,
            center: [center.x, center.y],
            radius,
            tile_mode: 0.0,
            corner_radii,
            stop_count,
            stop_offset: 0,
            padding2: [0; 2],
            focal: [center.x, center.y, 0.0, 1.0],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
            transform: [1.0, 0.0, 0.0, 1.0],
            transform_translate: [0.0; 4],
        }
    }

    pub(crate) fn with_circles(mut self, focal: [f32; 4], tile: f32) -> Self {
        self.focal = focal;
        self.tile_mode = tile;
        self
    }

    pub(crate) fn with_transform(mut self, matrix: glam::DMat4, origin: [f64; 2]) -> Self {
        // Rebase in f64 before packing. Adding a tiny local extent to a large
        // f32 origin would destroy the quad before an opposing CTM translation.
        let mapped = matrix * glam::dvec4(origin[0], origin[1], 0.0, 1.0);
        self.bounds[0] = 0.0;
        self.bounds[1] = 0.0;
        self.transform = [
            matrix.x_axis.x as f32,
            matrix.x_axis.y as f32,
            matrix.y_axis.x as f32,
            matrix.y_axis.y as f32,
        ];
        self.transform_translate = [mapped.x as f32, mapped.y as f32, 0.0, 0.0];
        self
    }

    pub(crate) fn device_corners(&self) -> [[f32; 2]; 4] {
        let [x, y, w, h] = self.bounds;
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| {
            [
                self.transform[0] * p[0] + self.transform[2] * p[1] + self.transform_translate[0],
                self.transform[1] * p[0] + self.transform[3] * p[1] + self.transform_translate[1],
            ]
        })
    }

    /// Set the offset into the shared gradient stops buffer
    pub(crate) fn with_stop_offset(mut self, offset: u32) -> Self {
        self.stop_offset = offset;
        self
    }
}

/// Sweep (angular/conic) gradient instance data for GPU instancing
///
/// Layout matches the sweep.wgsl InstanceInput struct.
/// Each instance represents one sweep-gradient-filled rectangle.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct SweepGradientInstance {
    /// Rectangle bounds [x, y, width, height]
    pub bounds: [f32; 4],
    /// Gradient center point (local coordinates)
    pub center: [f32; 2],
    /// Start and end angles in radians [start_angle, end_angle]
    pub angles: [f32; 2],
    /// Corner radii [top-left, top-right, bottom-right, bottom-left]
    pub corner_radii: [f32; 4],
    /// Number of validated gradient stops
    pub stop_count: u32,
    /// Offset into the shared gradient stops buffer
    pub stop_offset: u32,
    /// Clip-local rounded-rect clip: `[x, y, w, h, tl, tr, br, bl]`.
    ///
    /// All-zero means "no clip". Attached by `apply_active_clip`; the shader
    /// maps a fragment's device position into this space with
    /// `clip_device_to_local` before evaluating it. The scissor alone can only
    /// express the clip's axis-aligned bounding box, which leaves square
    /// corners on a `ClipRRect`.
    ///
    /// Placed BEFORE the trailing padding field on purpose: `desc()` builds its
    /// attribute offsets with `vertex_attr_array!`, which lays attributes out
    /// contiguously and knows nothing about padding fields. A clip slot after
    /// the padding would be read 8 bytes early.
    pub clip_rrect: [f32; 8],
    /// `[kind, _, _, _]`: 0 = none, 1 = rrect, 2 = rounded superellipse.
    pub clip_kind: [u32; 4],
    /// Device-to-clip-local linear part: `[a, b, c, d]`, columns first.
    ///
    /// The clip's bounds and radii are in the space the caller set them in;
    /// this maps a device-space fragment position back there. Identity is
    /// `[1, 0, 0, 1]`.
    pub clip_device_to_local: [f32; 4],
    /// Device-to-clip-local translation, padded to a `vec4` attribute:
    /// `[tx, ty, 0, 0]`.
    pub clip_local_origin: [f32; 4],
    /// Local-to-device affine columns.
    pub transform: [f32; 4],
    /// Local-to-device translation, padded for the vertex attribute.
    pub transform_translate: [f32; 4],
    /// Padding for GPU alignment
    pub padding: [u32; 2],
}

impl SweepGradientInstance {
    /// Create a new sweep gradient instance
    pub(crate) fn new(
        bounds: [f32; 4],
        center: Vec2,
        start_angle: f32,
        end_angle: f32,
        corner_radii: [f32; 4],
        stop_count: u32,
    ) -> Self {
        Self {
            bounds,
            center: [center.x, center.y],
            angles: [start_angle, end_angle],
            corner_radii,
            stop_count,
            stop_offset: 0,
            padding: [0; 2],
            clip_rrect: [0.0; 8],
            clip_kind: [0; 4],
            clip_device_to_local: [1.0, 0.0, 0.0, 1.0],
            clip_local_origin: [0.0; 4],
            transform: [1.0, 0.0, 0.0, 1.0],
            transform_translate: [0.0; 4],
        }
    }

    pub(crate) fn with_transform(mut self, matrix: glam::DMat4, origin: [f64; 2]) -> Self {
        // Rebase in f64 before packing. Adding a tiny local extent to a large
        // f32 origin would destroy the quad before an opposing CTM translation.
        let mapped = matrix * glam::dvec4(origin[0], origin[1], 0.0, 1.0);
        self.bounds[0] = 0.0;
        self.bounds[1] = 0.0;
        self.transform = [
            matrix.x_axis.x as f32,
            matrix.x_axis.y as f32,
            matrix.y_axis.x as f32,
            matrix.y_axis.y as f32,
        ];
        self.transform_translate = [mapped.x as f32, mapped.y as f32, 0.0, 0.0];
        self
    }

    pub(crate) fn device_corners(&self) -> [[f32; 2]; 4] {
        let [x, y, w, h] = self.bounds;
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| {
            [
                self.transform[0] * p[0] + self.transform[2] * p[1] + self.transform_translate[0],
                self.transform[1] * p[0] + self.transform[3] * p[1] + self.transform_translate[1],
            ]
        })
    }

    /// Set the offset into the shared gradient stops buffer
    pub(crate) fn with_stop_offset(mut self, offset: u32) -> Self {
        self.stop_offset = offset;
        self
    }
}
