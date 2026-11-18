//! Moving recorded IR by whole device pixels (ADR-0304 §9).
//!
//! A chunk's IR depends on its transform's linear part and on the fraction
//! of its translation; the whole pixels of the translation, its
//! `PixelOrigin`, only move it. A kept recording replays at another origin
//! by adding the difference, a `PixelShift`, to every device position it
//! holds. Each `Shift` impl destructures its instance whole, so a field
//! added to one has to say here whether it is a device position. Positions in
//! a shape's local space, colours, texel coordinates and the SDF clip (the
//! entry's, which does not move with the chunk) stay as recorded.
//!
//! Two layouts place an instance: a baked one, whose bounds are device
//! pixels under an identity linear part and a zero translation, and an
//! affine one, whose translation carries the device placement. An instance
//! with an identity linear part and a zero translation is moved as a baked
//! one either way: both draw it at its bounds.
//!
//! The shift is a whole number below 2^24, so it is exact in `f32`, and
//! adding it to a whole or dyadic position of that range is exact. Any other
//! position rounds once, within one `f32` ulp of what recording at the new
//! origin computes; a glyph keeps the subpixel bin of the fraction it was
//! recorded at, which that rounding cannot reach below 2^22 pixels.

use crate::effects::{
    LinearGradientInstance, RadialGradientInstance, ShadowInstance, SweepGradientInstance,
};
use crate::instancing::{
    ArcInstance, CircleInstance, GlyphInstance, RectInstance, TextureInstance,
};
use crate::vertex::Vertex;

/// The whole device pixels of a chunk's translation, `floor(t)` on each axis.
/// [`crate::recording_cache::RasterKey::of`] admits translations below 2^23,
/// so each axis fits an `f32` exactly and two of them differ by less than
/// 2^24.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PixelOrigin {
    x: i64,
    y: i64,
}

impl PixelOrigin {
    /// The origin of a translation already bounded below 2^23 pixels.
    pub(crate) fn of_bounded(tx: f64, ty: f64) -> Self {
        debug_assert!(tx.abs() < 8_388_608.0 && ty.abs() < 8_388_608.0);
        Self {
            x: tx.floor() as i64,
            y: ty.floor() as i64,
        }
    }

    /// The whole device pixels of the origin, `(x, y)`.
    pub(crate) fn pixels(self) -> (i64, i64) {
        (self.x, self.y)
    }

    /// The move from `self` to `to`; `None` when they are the same, so a
    /// replay in place copies its IR bit for bit (adding `0.0` would turn
    /// `-0.0` into `0.0`).
    pub(crate) fn shift_to(self, to: Self) -> Option<PixelShift> {
        // Each axis is below 2^23 in magnitude, so the difference is below
        // 2^24 and exact as `f32`.
        let (dx, dy) = (to.x - self.x, to.y - self.y);
        (dx != 0 || dy != 0).then_some(PixelShift {
            dx: dx as f32,
            dy: dy as f32,
        })
    }
}

/// A move by whole device pixels, right and down; never zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PixelShift {
    dx: f32,
    dy: f32,
}

impl PixelShift {
    fn apply(self, position: &mut [f32]) {
        position[0] += self.dx;
        position[1] += self.dy;
    }
}

/// IR that holds device positions and moves with its chunk.
pub(crate) trait Shift: Copy {
    /// `self` drawn `by` further right and down.
    #[must_use]
    fn shifted(self, by: PixelShift) -> Self;
}

const IDENTITY: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// Moves the placement of an instance in one of the two layouts: `bounds`
/// when `transform` is the identity and `translate` is zero, `translate`
/// otherwise.
fn place(bounds: &mut [f32; 4], transform: [f32; 4], translate: &mut [f32; 4], by: PixelShift) {
    if transform == IDENTITY && translate[0] == 0.0 && translate[1] == 0.0 {
        by.apply(bounds);
    } else {
        by.apply(translate);
    }
}

impl Shift for RectInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        let Self {
            bounds,
            color: _,
            corner_radii: _,
            transform,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform_translate,
        } = &mut self;
        place(bounds, *transform, transform_translate, by);
        self
    }
}

impl Shift for CircleInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // The centre is always placed by the translation.
        let Self {
            center_radius: _,
            color: _,
            transform: _,
            transform_translate,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for ArcInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // The centre is always placed by the translation.
        let Self {
            center_radius: _,
            angles: _,
            color: _,
            transform: _,
            transform_translate,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for ShadowInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // Recorded with the transform baked in: bounds and rectangle are
        // device pixels, the offset relative to them.
        let Self {
            bounds,
            rect_pos,
            rect_size: _,
            corner_radius: _,
            padding1: _,
            shadow_offset: _,
            blur_sigma: _,
            padding2: _,
            shadow_color: _,
        } = &mut self;
        by.apply(bounds);
        by.apply(rect_pos);
        self
    }
}

impl Shift for LinearGradientInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // `with_transform` keeps bounds and the gradient line local and puts
        // the device placement in the translation.
        let Self {
            bounds: _,
            linear_parameter: _,
            corner_radii: _,
            stop_count: _,
            stop_offset: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform: _,
            transform_translate,
            padding: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for RadialGradientInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        let Self {
            bounds: _,
            center: _,
            radius: _,
            tile_mode: _,
            corner_radii: _,
            stop_count: _,
            stop_offset: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform: _,
            transform_translate,
            focal: _,
            padding2: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for SweepGradientInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        let Self {
            bounds: _,
            center: _,
            angles: _,
            corner_radii: _,
            stop_count: _,
            stop_offset: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform: _,
            transform_translate,
            padding: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for GlyphInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // A uniform CTM records the device rectangle with no origin; any
        // other records raster-space rectangles under a device origin.
        let Self {
            dst_rect,
            atlas: _,
            color: _,
            transform,
            origin,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
        } = &mut self;
        place(dst_rect, *transform, origin, by);
        self
    }
}

impl Shift for TextureInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // The origin is a device position; the axes and texel coordinates
        // are not.
        let Self {
            origin_and_x,
            src_uv: _,
            tint: _,
            axis_y: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            original_image_uv: _,
        } = &mut self;
        by.apply(origin_and_x);
        self
    }
}

impl Shift for Vertex {
    fn shifted(mut self, by: PixelShift) -> Self {
        // Tessellated with the transform baked in; `tex_coord` is a texture
        // coordinate.
        let Self {
            position,
            color: _,
            tex_coord: _,
        } = &mut self;
        by.apply(position);
        self
    }
}
