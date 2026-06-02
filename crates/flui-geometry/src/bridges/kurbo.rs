//! Bridge between flui typed geometry and [`kurbo`] 2D-curve primitives (U8).
//!
//! [`kurbo`] is the `f64`-backed curve library that Core.2 render objects use
//! for Bézier paths, curve flattening, and hit-testing. This module is the
//! single sanctioned conversion point so object-by-object code never grows
//! inline ad-hoc `as f64` / `as f32` casts.
//!
//! # Type map
//!
//! | flui (`Pixels`, `f32`) | kurbo (`f64`)   |
//! |------------------------|-----------------|
//! | [`Point<Pixels>`]      | [`kurbo::Point`]  |
//! | [`Size<Pixels>`]       | [`kurbo::Size`]   |
//! | [`Rect<Pixels>`]       | [`kurbo::Rect`]   |
//! | [`Transform2D<Pixels>`]| [`kurbo::Affine`] |
//!
//! # Why no `Matrix4` bridge
//!
//! [`kurbo::Affine`] is a 2×3 (2D) affine; flui's [`Matrix4`](crate::Matrix4)
//! is a 4×4 (3D) matrix. There is no lossless, unambiguous mapping between
//! them. For 2D interop with kurbo, convert through [`Transform2D`] — the
//! genuine 2×3 analog — not `Matrix4`.

use crate::{Pixels, Point, Rect, Size, Transform2D, px};

/// Error raised when a `kurbo` (`f64`) value cannot be represented as flui's
/// `f32`-backed geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum KurboBridgeError {
    /// A finite `f64` coordinate overflowed the `f32` range during narrowing.
    OutOfRange {
        /// The offending source value.
        value: f64,
    },
}

impl core::fmt::Display for KurboBridgeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange { value } => write!(
                f,
                "kurbo f64 value {value} is outside the representable f32 range"
            ),
        }
    }
}

impl std::error::Error for KurboBridgeError {}

/// Narrows a `kurbo` `f64` to flui `f32`, failing when a finite value overflows.
///
/// Non-finite inputs (`±inf`, `NaN`) are preserved as-is — they are already
/// representable in `f32` and carry meaning the caller may rely on.
#[inline]
fn narrow(value: f64) -> Result<f32, KurboBridgeError> {
    // PORT-CHECK-OK-SP3: deliberate fallible f64→f32 narrowing; the range
    // check below rejects finite-to-infinite overflow.
    let narrowed = value as f32;
    if value.is_finite() && !narrowed.is_finite() {
        return Err(KurboBridgeError::OutOfRange { value });
    }
    Ok(narrowed)
}

// ============================================================================
// Point
// ============================================================================

impl From<Point<Pixels>> for kurbo::Point {
    #[inline]
    fn from(p: Point<Pixels>) -> Self {
        // f32 → f64 is lossless widening.
        Self::new(f64::from(p.x.0), f64::from(p.y.0))
    }
}

impl TryFrom<kurbo::Point> for Point<Pixels> {
    type Error = KurboBridgeError;

    #[inline]
    fn try_from(p: kurbo::Point) -> Result<Self, Self::Error> {
        Ok(Self::new(px(narrow(p.x)?), px(narrow(p.y)?)))
    }
}

// ============================================================================
// Size
// ============================================================================

impl From<Size<Pixels>> for kurbo::Size {
    #[inline]
    fn from(s: Size<Pixels>) -> Self {
        Self::new(f64::from(s.width.0), f64::from(s.height.0))
    }
}

impl TryFrom<kurbo::Size> for Size<Pixels> {
    type Error = KurboBridgeError;

    #[inline]
    fn try_from(s: kurbo::Size) -> Result<Self, Self::Error> {
        Ok(Self::new(px(narrow(s.width)?), px(narrow(s.height)?)))
    }
}

// ============================================================================
// Rect
// ============================================================================

impl From<Rect<Pixels>> for kurbo::Rect {
    #[inline]
    fn from(r: Rect<Pixels>) -> Self {
        Self::new(
            f64::from(r.min.x.0),
            f64::from(r.min.y.0),
            f64::from(r.max.x.0),
            f64::from(r.max.y.0),
        )
    }
}

impl TryFrom<kurbo::Rect> for Rect<Pixels> {
    type Error = KurboBridgeError;

    #[inline]
    fn try_from(r: kurbo::Rect) -> Result<Self, Self::Error> {
        let min = Point::new(px(narrow(r.x0)?), px(narrow(r.y0)?));
        let max = Point::new(px(narrow(r.x1)?), px(narrow(r.y1)?));
        Ok(Self::from_min_max(min, max))
    }
}

// ============================================================================
// Transform2D ↔ kurbo::Affine
// ============================================================================
//
// kurbo::Affine stores coefficients [a, b, c, d, e, f] applied as:
//     x' = a·x + c·y + e
//     y' = b·x + d·y + f
//
// flui Transform2D is row-major [m11 m12 m31 / m21 m22 m32] applied as:
//     x' = m11·x + m12·y + m31
//     y' = m21·x + m22·y + m32
//
// so the coefficient mapping is [m11, m21, m12, m22, m31, m32].

impl From<Transform2D<Pixels>> for kurbo::Affine {
    #[inline]
    fn from(t: Transform2D<Pixels>) -> Self {
        Self::new([
            f64::from(t.m11),
            f64::from(t.m21),
            f64::from(t.m12),
            f64::from(t.m22),
            f64::from(t.m31),
            f64::from(t.m32),
        ])
    }
}

impl TryFrom<kurbo::Affine> for Transform2D<Pixels> {
    type Error = KurboBridgeError;

    #[inline]
    fn try_from(a: kurbo::Affine) -> Result<Self, Self::Error> {
        let [a, b, c, d, e, f] = a.as_coeffs();
        Ok(Self::from_components(
            narrow(a)?, // m11
            narrow(c)?, // m12
            narrow(b)?, // m21
            narrow(d)?, // m22
            narrow(e)?, // m31
            narrow(f)?, // m32
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_round_trip() {
        let p = Point::<Pixels>::new(px(10.5), px(-3.25));
        let k: kurbo::Point = p.into();
        assert_eq!(k.x, 10.5_f64);
        assert_eq!(k.y, -3.25_f64);
        let back: Point<Pixels> = k.try_into().unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn size_round_trip() {
        let s = Size::<Pixels>::new(px(640.0), px(480.0));
        let k: kurbo::Size = s.into();
        assert_eq!(k.width, 640.0_f64);
        let back: Size<Pixels> = k.try_into().unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn rect_round_trip() {
        let r = Rect::<Pixels>::from_min_max(
            Point::new(px(1.0), px(2.0)),
            Point::new(px(11.0), px(22.0)),
        );
        let k: kurbo::Rect = r.into();
        assert_eq!((k.x0, k.y0, k.x1, k.y1), (1.0, 2.0, 11.0, 22.0));
        let back: Rect<Pixels> = k.try_into().unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn affine_round_trip_translation() {
        let t = Transform2D::<Pixels>::translation(50.0, 100.0);
        let k: kurbo::Affine = t.into();
        // kurbo translation puts tx/ty in coeffs[4]/coeffs[5].
        assert_eq!(k.as_coeffs(), [1.0, 0.0, 0.0, 1.0, 50.0, 100.0]);
        let back: Transform2D<Pixels> = k.try_into().unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn affine_maps_point_consistently() {
        // Applying the transform in flui must equal applying the bridged
        // affine in kurbo, for the same input point.
        let t = Transform2D::<Pixels>::scale_xy(2.0, 3.0);
        let p = Point::<Pixels>::new(px(4.0), px(5.0));
        let flui_out = t.transform_point(p);

        let k_affine: kurbo::Affine = t.into();
        let k_point: kurbo::Point = p.into();
        let k_out = k_affine * k_point;

        assert_eq!(f64::from(flui_out.x.0), k_out.x);
        assert_eq!(f64::from(flui_out.y.0), k_out.y);
    }

    #[test]
    fn narrow_rejects_overflow() {
        let huge = kurbo::Point::new(f64::MAX, 0.0);
        let err = Point::<Pixels>::try_from(huge).unwrap_err();
        assert!(matches!(err, KurboBridgeError::OutOfRange { .. }));
    }

    #[test]
    fn narrow_preserves_infinity() {
        let inf = kurbo::Point::new(f64::INFINITY, 0.0);
        let back = Point::<Pixels>::try_from(inf).unwrap();
        assert!(back.x.0.is_infinite());
    }
}
