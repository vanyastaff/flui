//! `Alignment` — a 2D point in *normalized* rectangle coordinates.
//!
//! Visual coordinate system mirrors Flutter's
//! [`painting/alignment.dart`](https://api.flutter.dev/flutter/painting/Alignment-class.html):
//!
//! ```text
//!  (-1,-1)        (0,-1)        (1,-1)
//!     +-------------+-------------+
//!     |          top-center        |
//!     |                            |
//!  (-1, 0)        (0, 0)        (1, 0)
//!     +          center            +
//!     |                            |
//!     |        bottom-center       |
//!     +-------------+-------------+
//!  (-1, 1)        (0, 1)        (1, 1)
//! ```
//!
//! `x` runs from `-1.0` at the *left* edge to `+1.0` at the *right* edge;
//! `y` runs from `-1.0` at the *top* edge to `+1.0` at the *bottom* edge.
//! `(0.0, 0.0)` is the center. The range is **not** clamped — values outside
//! `[-1, 1]` are legal and place the point outside the rectangle, useful for
//! follower-layer anchoring against off-rectangle pivots.
//!
//! Flutter reference:
//! [`packages/flutter/lib/src/painting/alignment.dart`](../../../../.flutter/flutter-master/packages/flutter/lib/src/painting/alignment.dart)
//! (lines 275–310 for the constants; 222 for `lerp`).

use crate::geometry::{Offset, Pixels, Rect};

/// A point within (or outside) a rectangle, expressed in normalized
/// coordinates. See module docs for the coordinate convention.
///
/// This type carries no allocation and is `Copy`. It is the workspace-canonical
/// representation for follower-layer anchors, container alignment, and any
/// layout primitive that needs a "fraction-of-rectangle" point.
///
/// The struct is `#[non_exhaustive]` to leave room for future fields (e.g. a
/// reserved-space axis tag) without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
#[must_use]
pub struct Alignment {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Horizontal position. `-1.0` = left, `0.0` = center, `+1.0` = right.
    pub x: f32,
    /// Vertical position. `-1.0` = top, `0.0` = center, `+1.0` = bottom.
    pub y: f32,
}

impl Alignment {
    // ---- Canonical 9-point grid (Flutter parity, alignment.dart:275-310) ----

    /// Top-left corner: `(-1.0, -1.0)`.
    pub const TOP_LEFT: Self = Self { x: -1.0, y: -1.0 };
    /// Top-center edge midpoint: `(0.0, -1.0)`.
    pub const TOP_CENTER: Self = Self { x: 0.0, y: -1.0 };
    /// Top-right corner: `(1.0, -1.0)`.
    pub const TOP_RIGHT: Self = Self { x: 1.0, y: -1.0 };
    /// Center-left edge midpoint: `(-1.0, 0.0)`.
    pub const CENTER_LEFT: Self = Self { x: -1.0, y: 0.0 };
    /// Rectangle center: `(0.0, 0.0)`.
    pub const CENTER: Self = Self { x: 0.0, y: 0.0 };
    /// Center-right edge midpoint: `(1.0, 0.0)`.
    pub const CENTER_RIGHT: Self = Self { x: 1.0, y: 0.0 };
    /// Bottom-left corner: `(-1.0, 1.0)`.
    pub const BOTTOM_LEFT: Self = Self { x: -1.0, y: 1.0 };
    /// Bottom-center edge midpoint: `(0.0, 1.0)`.
    pub const BOTTOM_CENTER: Self = Self { x: 0.0, y: 1.0 };
    /// Bottom-right corner: `(1.0, 1.0)`.
    pub const BOTTOM_RIGHT: Self = Self { x: 1.0, y: 1.0 };

    /// Constructs an `Alignment` from raw `(x, y)` coordinates. Inputs are not
    /// clamped — values outside `[-1, 1]` are legal.
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Linearly interpolates between two alignments.
    ///
    /// `t == 0.0` returns `a`; `t == 1.0` returns `b`. Values outside `[0, 1]`
    /// extrapolate. Flutter parity:
    /// [`alignment.dart:222`](../../../../.flutter/flutter-master/packages/flutter/lib/src/painting/alignment.dart).
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }

    /// Maps this normalized alignment to a pixel-coordinate `Offset` inside
    /// `rect`.
    ///
    /// - `Alignment::TOP_LEFT.align_within(rect) == rect.top_left()`
    /// - `Alignment::CENTER.align_within(rect) == rect.center()`
    /// - `Alignment::BOTTOM_RIGHT.align_within(rect) == rect.bottom_right()`
    ///
    /// Note: returns a position, not a delta — the caller adds this to nothing.
    pub fn align_within(self, rect: Rect<Pixels>) -> Offset<Pixels> {
        let half_w = rect.width().get() * 0.5;
        let half_h = rect.height().get() * 0.5;
        let left = rect.left().get();
        let top = rect.top().get();
        let cx = left + half_w;
        let cy = top + half_h;
        Offset::new(
            Pixels::new(cx + self.x * half_w),
            Pixels::new(cy + self.y * half_h),
        )
    }
}

impl Default for Alignment {
    /// Default alignment is the rectangle center.
    fn default() -> Self {
        Self::CENTER
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Pixels, Rect};

    fn px(v: f32) -> Pixels {
        Pixels::new(v)
    }

    #[test]
    fn canonical_constants_match_quadrant() {
        assert_eq!(Alignment::TOP_LEFT, Alignment::new(-1.0, -1.0));
        assert_eq!(Alignment::CENTER, Alignment::new(0.0, 0.0));
        assert_eq!(Alignment::BOTTOM_RIGHT, Alignment::new(1.0, 1.0));
        assert_eq!(Alignment::CENTER_LEFT.x, -1.0);
        assert_eq!(Alignment::TOP_CENTER.y, -1.0);
    }

    #[test]
    fn default_is_center() {
        assert_eq!(Alignment::default(), Alignment::CENTER);
    }

    #[test]
    fn lerp_endpoints_are_exact() {
        let a = Alignment::TOP_LEFT;
        let b = Alignment::BOTTOM_RIGHT;
        assert_eq!(Alignment::lerp(a, b, 0.0), a);
        assert_eq!(Alignment::lerp(a, b, 1.0), b);
    }

    #[test]
    fn lerp_midpoint_is_center() {
        let mid = Alignment::lerp(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT, 0.5);
        assert_eq!(mid, Alignment::CENTER);
    }

    #[test]
    fn lerp_extrapolates_outside_unit_interval() {
        let out = Alignment::lerp(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT, 1.5);
        assert_eq!(out, Alignment::new(2.0, 2.0));
    }

    #[test]
    fn align_within_corner_constants_match_rect_corners() {
        let r = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(200.0));
        let tl: f32 = Alignment::TOP_LEFT.align_within(r).dx.into();
        let tl_y: f32 = Alignment::TOP_LEFT.align_within(r).dy.into();
        assert_eq!(tl, 0.0);
        assert_eq!(tl_y, 0.0);

        let br_x: f32 = Alignment::BOTTOM_RIGHT.align_within(r).dx.into();
        let br_y: f32 = Alignment::BOTTOM_RIGHT.align_within(r).dy.into();
        assert_eq!(br_x, 100.0);
        assert_eq!(br_y, 200.0);

        let c_x: f32 = Alignment::CENTER.align_within(r).dx.into();
        let c_y: f32 = Alignment::CENTER.align_within(r).dy.into();
        assert_eq!(c_x, 50.0);
        assert_eq!(c_y, 100.0);
    }

    #[test]
    fn align_within_handles_offset_rect() {
        // 200×100 rect anchored at (10, 20).
        let r = Rect::from_ltwh(px(10.0), px(20.0), px(200.0), px(100.0));
        let tl_x: f32 = Alignment::TOP_LEFT.align_within(r).dx.into();
        let tl_y: f32 = Alignment::TOP_LEFT.align_within(r).dy.into();
        assert_eq!(tl_x, 10.0);
        assert_eq!(tl_y, 20.0);

        let c_x: f32 = Alignment::CENTER.align_within(r).dx.into();
        let c_y: f32 = Alignment::CENTER.align_within(r).dy.into();
        assert_eq!(c_x, 110.0);
        assert_eq!(c_y, 70.0);
    }

    #[test]
    fn align_within_accepts_values_outside_unit_range() {
        // Off-rectangle anchor — a follower-layer use case.
        let r = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        let a = Alignment::new(2.0, 0.0); // one full width to the right of right edge
        let off: f32 = a.align_within(r).dx.into();
        assert_eq!(off, 150.0);
    }

    #[test]
    fn alignment_is_copy_and_lightweight() {
        // Compile-time sanity that Alignment is Copy + small.
        const fn requires_copy<T: Copy>() {}
        requires_copy::<Alignment>();
        assert_eq!(std::mem::size_of::<Alignment>(), 8);
    }

    #[test]
    fn edge_midpoint_constants_lie_on_correct_edge() {
        let r = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(200.0));

        let cl_x: f32 = Alignment::CENTER_LEFT.align_within(r).dx.into();
        let cl_y: f32 = Alignment::CENTER_LEFT.align_within(r).dy.into();
        assert_eq!(cl_x, 0.0);
        assert_eq!(cl_y, 100.0);

        let tc_x: f32 = Alignment::TOP_CENTER.align_within(r).dx.into();
        let tc_y: f32 = Alignment::TOP_CENTER.align_within(r).dy.into();
        assert_eq!(tc_x, 50.0);
        assert_eq!(tc_y, 0.0);
    }
}
