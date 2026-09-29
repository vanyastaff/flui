//! From continuous coordinates to the device-pixel grid (ADR-0098 §6).
//!
//! Every raster decision that turns a float coordinate into a whole device pixel goes
//! through these functions, so one rule decides it everywhere:
//!
//! - [`snap`] rounds to the nearest integer with ties toward +∞, the rule Skia's
//!   `sk_float_round` uses. Ties toward +∞ keep a shape the same width wherever it lands:
//!   `[-0.5, 0.5)` and `[0.5, 1.5)` both snap to one pixel, where ties away from zero
//!   (`f64::round`) would make the first two pixels wide.
//! - [`snap_edges`] snaps a rectangle's edges, not its size, so rectangles that abut
//!   before snapping still abut after it.
//! - [`cover`] takes the floor of the minimum and the ceiling of the maximum: a scissor,
//!   damage, layer or blur bound never loses a partly covered pixel.
//! - [`resolve_stroke_width`] turns a border or stroke width into a whole number of device
//!   pixels without ever making a non-zero width disappear.
//!
//! The functions work in whatever space their input is in; the caller converts to
//! raster-target coordinates first (ADR-0098 §6 "Where to snap").

use crate::geometry::{DevicePoint, DeviceRect, DeviceSize, Point, Rect, Size};

/// The ratio between device pixels and logical pixels.
///
/// Finite and positive by construction: a zero, negative or non-finite ratio would turn every
/// conversion into zero, a mirror image or NaN.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct DevicePixelRatio(f64);

impl DevicePixelRatio {
    /// One device pixel per logical pixel.
    pub const ONE: Self = Self(1.0);

    /// The ratio, or `None` when `ratio` is zero, negative or not finite.
    #[must_use]
    pub fn new(ratio: f64) -> Option<Self> {
        (ratio.is_finite() && ratio > 0.0).then_some(Self(ratio))
    }

    /// Device pixels per logical pixel.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// `logical` in device pixels, unrounded.
    #[must_use]
    pub fn to_device(self, logical: f64) -> f64 {
        logical * self.0
    }

    /// `device` in logical pixels.
    #[must_use]
    pub fn to_logical(self, device: f64) -> f64 {
        device / self.0
    }
}

impl Default for DevicePixelRatio {
    fn default() -> Self {
        Self::ONE
    }
}

/// `x` rounded to the nearest integer, ties toward +∞.
///
/// Computed from the distance to the floor, so the largest float below one half rounds to
/// 0 (`(x + 0.5).floor()` rounds it to 1). NaN stays NaN and infinities stay infinite.
#[must_use]
pub fn snap(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `point` with each coordinate snapped.
#[must_use]
pub fn snap_point(point: Point) -> Point {
    Point::new(snap(point.x), snap(point.y))
}

/// `rect` with each edge snapped on its own, so abutting rectangles stay abutting.
#[must_use]
pub fn snap_edges(rect: Rect) -> Rect {
    Rect::from_ltrb(
        snap(rect.left()),
        snap(rect.top()),
        snap(rect.right()),
        snap(rect.bottom()),
    )
}

/// The smallest pixel-aligned rectangle containing `rect`: floor of the minimum, ceiling
/// of the maximum.
#[must_use]
pub fn cover(rect: Rect) -> Rect {
    Rect::from_ltrb(
        rect.left().floor(),
        rect.top().floor(),
        rect.right().ceil(),
        rect.bottom().ceil(),
    )
}

/// `value`, already whole, as an `i32`, saturating at the `i32` range (NaN becomes 0).
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is whole and `as` saturates at the i32 range, which is the intent"
)]
fn to_i32(value: f64) -> i32 {
    value as i32
}

/// The device-pixel rectangle covering the logical `rect` at `ratio`.
#[must_use]
pub fn device_rect_covering(rect: Rect, ratio: DevicePixelRatio) -> DeviceRect {
    let device = cover(Rect::from_ltrb(
        ratio.to_device(rect.left()),
        ratio.to_device(rect.top()),
        ratio.to_device(rect.right()),
        ratio.to_device(rect.bottom()),
    ));
    DeviceRect::from_min_max(
        DevicePoint::new(to_i32(device.left()), to_i32(device.top())),
        DevicePoint::new(to_i32(device.right()), to_i32(device.bottom())),
    )
}

/// The device-pixel size of a logical `size` at `ratio`: each extent snapped, never
/// negative.
#[must_use]
pub fn device_size(size: Size, ratio: DevicePixelRatio) -> DeviceSize {
    let extent = |logical: f64| to_i32(snap(ratio.to_device(logical)).max(0.0));
    DeviceSize::new(extent(size.width), extent(size.height))
}

/// A border or stroke width resolved to whole device pixels, returned in logical pixels.
///
/// A width in `(0, 1]` device pixel becomes one device pixel and anything wider takes the
/// floor, so a non-zero width never disappears and a border never straddles a pixel it only
/// partly covers. Zero is a hairline: one device pixel. A negative or non-finite width is
/// treated as zero.
#[must_use]
pub fn resolve_stroke_width(width: f64, ratio: DevicePixelRatio) -> f64 {
    let device = ratio.to_device(width);
    let whole = if !device.is_finite() || device <= 1.0 {
        1.0
    } else {
        device.floor()
    };
    ratio.to_logical(whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_rounds_half_way_toward_positive_infinity() {
        for (x, expected) in [
            (0.5, 1.0),
            (1.5, 2.0),
            (2.5, 3.0),
            (-0.5, 0.0),
            (-1.5, -1.0),
            (-2.5, -2.0),
        ] {
            assert_eq!(snap(x), expected, "snap({x})");
        }
    }

    #[test]
    fn snap_rounds_to_the_nearest_integer_off_the_half() {
        for (x, expected) in [
            (0.0, 0.0),
            (0.49, 0.0),
            (0.51, 1.0),
            (-0.49, 0.0),
            (-0.51, -1.0),
            (-1.49, -1.0),
            (-1.51, -2.0),
            (1e9 + 0.4, 1e9),
        ] {
            assert_eq!(snap(x), expected, "snap({x})");
        }
    }

    /// `(x + 0.5).floor()` rounds the largest float below one half up to 1.
    #[test]
    fn snap_keeps_the_float_just_below_one_half_at_zero() {
        let below_half = 0.5_f64.next_down();
        assert_eq!(snap(below_half), 0.0);
        assert_eq!(
            (below_half + 0.5).floor(),
            1.0,
            "the naive formula this avoids"
        );
    }

    #[test]
    fn snap_passes_non_finite_values_through() {
        assert!(snap(f64::NAN).is_nan());
        assert_eq!(snap(f64::INFINITY), f64::INFINITY);
        assert_eq!(snap(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }

    /// A one-pixel-wide rectangle is one pixel wide wherever its half-way edges land.
    #[test]
    fn half_way_edges_keep_the_width() {
        for left in [-2.5, -1.5, -0.5, 0.5, 1.5] {
            let rect = snap_edges(Rect::from_ltrb(left, 0.0, left + 1.0, 1.0));
            assert_eq!(rect.width(), 1.0, "left edge {left}");
        }
    }

    /// Snapping edges, not sizes: two rectangles sharing an edge share it after snapping.
    #[test]
    fn abutting_rectangles_stay_abutting() {
        let a = snap_edges(Rect::from_ltrb(0.3, 0.0, 10.4, 1.0));
        let b = snap_edges(Rect::from_ltrb(10.4, 0.0, 20.6, 1.0));
        assert_eq!(a.right(), b.left());
    }

    #[test]
    fn cover_never_loses_a_partly_covered_pixel() {
        let rect = cover(Rect::from_ltrb(-0.2, 0.7, 3.1, 4.0));
        assert_eq!(rect, Rect::from_ltrb(-1.0, 0.0, 4.0, 4.0));
    }

    #[test]
    fn device_rect_covers_after_scaling() {
        let ratio = DevicePixelRatio::new(1.5).expect("a valid ratio");
        let rect = device_rect_covering(Rect::from_ltrb(0.5, -0.5, 2.0, 1.0), ratio);
        // 0.75 → 0, -0.75 → -1, 3.0 → 3, 1.5 → 2.
        assert_eq!(
            rect,
            DeviceRect::from_min_max(DevicePoint::new(0, -1), DevicePoint::new(3, 2))
        );
    }

    #[test]
    fn device_size_snaps_each_extent_and_clamps_at_zero() {
        let ratio = DevicePixelRatio::new(1.25).expect("a valid ratio");
        assert_eq!(
            device_size(Size::new(10.0, 2.0), ratio),
            DeviceSize::new(13, 3)
        );
        assert_eq!(
            device_size(Size::new(-4.0, 0.0), ratio),
            DeviceSize::new(0, 0)
        );
    }

    #[test]
    fn a_ratio_must_be_finite_and_positive() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(DevicePixelRatio::new(bad).is_none(), "{bad}");
        }
        assert_eq!(
            DevicePixelRatio::new(2.0).map(DevicePixelRatio::get),
            Some(2.0)
        );
    }

    #[test]
    fn stroke_widths_resolve_to_whole_device_pixels() {
        let ratio = DevicePixelRatio::new(1.5).expect("a valid ratio");
        for (logical, device) in [
            (0.0, 1.0),  // hairline
            (0.1, 1.0),  // thin but visible
            (0.5, 1.0),  // 0.75 device px
            (1.0, 1.0),  // 1.5 device px: the floor
            (2.0, 3.0),  // exactly 3 device px
            (2.1, 3.0),  // 3.15 device px
            (-1.0, 1.0), // treated as zero
        ] {
            let resolved = resolve_stroke_width(logical, ratio);
            assert_eq!(ratio.to_device(resolved), device, "width {logical}");
        }
    }
}
