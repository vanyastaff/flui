//! kurbo bridge (N-geom PR 3, U8) — gated behind `feature = "kurbo"`.
//!
//! kurbo is the Linebender curve/path crate (also Vello's geometry layer) and
//! is **`f64`**, whereas flui-geometry is **`f32`**. The two directions are
//! therefore asymmetric:
//!
//! - **flui → kurbo** (`From`): `f32 → f64` is a *lossless* widening — every
//!   `f32` is exactly representable in `f64` — so these are infallible.
//! - **kurbo → flui** (`TryFrom`): `f64 → f32` is *lossy* and can overflow or
//!   carry non-finite values, so it is fallible and returns
//!   [`KurboBridgeError::OutOfRange`].
//!
//! Only flui's typed primitives need an explicit boundary here; under Option D
//! the engine's `glam` types bridge to kurbo for free via `mint`. Every scalar
//! cast is marked `PORT-CHECK-OK-SP3` (sanctioned cross-representation cast).

use thiserror::Error;

use crate::{Matrix4, Offset, Point, Rect, Size, px};

/// Error returned when converting a `kurbo` (`f64`) value into a flui (`f32`)
/// primitive whose magnitude is non-finite or outside the `f32` range.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum KurboBridgeError {
    /// A coordinate was `NaN`/`±inf` or outside the representable `f32` range.
    #[error("kurbo value {0} is not finite or is outside the f32 range")]
    OutOfRange(f64),
}

/// Range-checked `f64 → f32` narrowing used by every `TryFrom<kurbo …>` impl.
#[inline]
fn narrow(value: f64) -> Result<f32, KurboBridgeError> {
    if value.is_finite() && value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX) {
        // PORT-CHECK-OK-SP3: range-checked f64 -> f32 boundary narrowing.
        Ok(value as f32)
    } else {
        Err(KurboBridgeError::OutOfRange(value))
    }
}

/// Lossless `f32 → f64` widening helper (the blessed flui → kurbo direction).
#[inline]
fn widen(value: f32) -> f64 {
    // PORT-CHECK-OK-SP3: lossless f32 -> f64 widening.
    f64::from(value)
}

// ============================================================================
// Point <-> kurbo::Point
// ============================================================================

impl From<Point> for kurbo::Point {
    #[inline]
    fn from(p: Point) -> Self {
        kurbo::Point::new(widen(p.x.get()), widen(p.y.get()))
    }
}

impl TryFrom<kurbo::Point> for Point {
    type Error = KurboBridgeError;
    #[inline]
    fn try_from(p: kurbo::Point) -> Result<Self, Self::Error> {
        Ok(Point::new(px(narrow(p.x)?), px(narrow(p.y)?)))
    }
}

// ============================================================================
// Offset <-> kurbo::Vec2 (displacement, not a location)
// ============================================================================

impl From<Offset> for kurbo::Vec2 {
    #[inline]
    fn from(o: Offset) -> Self {
        kurbo::Vec2::new(widen(o.dx.get()), widen(o.dy.get()))
    }
}

impl TryFrom<kurbo::Vec2> for Offset {
    type Error = KurboBridgeError;
    #[inline]
    fn try_from(v: kurbo::Vec2) -> Result<Self, Self::Error> {
        Ok(Offset::new(px(narrow(v.x)?), px(narrow(v.y)?)))
    }
}

// ============================================================================
// Size <-> kurbo::Size
// ============================================================================

impl From<Size> for kurbo::Size {
    #[inline]
    fn from(s: Size) -> Self {
        kurbo::Size::new(widen(s.width.get()), widen(s.height.get()))
    }
}

impl TryFrom<kurbo::Size> for Size {
    type Error = KurboBridgeError;
    #[inline]
    fn try_from(s: kurbo::Size) -> Result<Self, Self::Error> {
        Ok(Size::new(px(narrow(s.width)?), px(narrow(s.height)?)))
    }
}

// ============================================================================
// Rect <-> kurbo::Rect
// ============================================================================

impl From<Rect> for kurbo::Rect {
    #[inline]
    fn from(r: Rect) -> Self {
        kurbo::Rect::new(
            widen(r.min.x.get()),
            widen(r.min.y.get()),
            widen(r.max.x.get()),
            widen(r.max.y.get()),
        )
    }
}

impl TryFrom<kurbo::Rect> for Rect {
    type Error = KurboBridgeError;
    #[inline]
    fn try_from(r: kurbo::Rect) -> Result<Self, Self::Error> {
        Ok(Rect::from_ltrb(
            px(narrow(r.x0)?),
            px(narrow(r.y0)?),
            px(narrow(r.x1)?),
            px(narrow(r.y1)?),
        ))
    }
}

// ============================================================================
// Matrix4 (2D affine subset) <-> kurbo::Affine
// ============================================================================
//
// kurbo::Affine is the 6-coefficient 2D affine `[a, b, c, d, e, f]` mapping
// `(x, y) -> (a·x + c·y + e,  b·x + d·y + f)`. flui's column-major `Matrix4`
// maps `(x, y) -> (m0·x + m4·y + m12,  m1·x + m5·y + m13)`, so:
//   a = m0, b = m1, c = m4, d = m5, e = m12, f = m13.
// The 3D/perspective rows of a `Matrix4` are dropped on the way to kurbo, and
// reconstructed as identity on the way back.

impl From<Matrix4> for kurbo::Affine {
    #[inline]
    fn from(m: Matrix4) -> Self {
        let v = &m.m;
        kurbo::Affine::new([
            widen(v[0]),
            widen(v[1]),
            widen(v[4]),
            widen(v[5]),
            widen(v[12]),
            widen(v[13]),
        ])
    }
}

impl TryFrom<kurbo::Affine> for Matrix4 {
    type Error = KurboBridgeError;
    #[inline]
    fn try_from(a: kurbo::Affine) -> Result<Self, Self::Error> {
        let [a0, b0, c0, d0, e0, f0] = a.as_coeffs();
        let (a, b, c, d, e, f) = (
            narrow(a0)?,
            narrow(b0)?,
            narrow(c0)?,
            narrow(d0)?,
            narrow(e0)?,
            narrow(f0)?,
        );
        // Column-major 4x4 with the 2D affine in the x/y rows, identity in z/w.
        Ok(Matrix4::new(
            a, b, 0.0, 0.0, // col 0
            c, d, 0.0, 0.0, // col 1
            0.0, 0.0, 1.0, 0.0, // col 2
            e, f, 0.0, 1.0, // col 3
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_round_trip() {
        let p = Point::new(px(12.5), px(-3.25));
        let k: kurbo::Point = p.into();
        assert_eq!(k.x, 12.5);
        assert_eq!(k.y, -3.25);
        let back = Point::try_from(k).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn offset_and_size_round_trip() {
        let o = Offset::new(px(4.0), px(8.0));
        assert_eq!(Offset::try_from(kurbo::Vec2::from(o)).unwrap(), o);
        let s = Size::new(px(100.0), px(50.0));
        assert_eq!(Size::try_from(kurbo::Size::from(s)).unwrap(), s);
    }

    #[test]
    fn rect_round_trip() {
        let r = Rect::from_ltrb(px(1.0), px(2.0), px(30.0), px(40.0));
        let k: kurbo::Rect = r.into();
        assert_eq!((k.x0, k.y0, k.x1, k.y1), (1.0, 2.0, 30.0, 40.0));
        assert_eq!(Rect::try_from(k).unwrap(), r);
    }

    #[test]
    fn affine_matches_point_transform() {
        // A flui Matrix4 and its kurbo::Affine must transform a point identically.
        let m = Matrix4::translation(10.0, 20.0, 0.0) * Matrix4::scaling(2.0, 3.0, 1.0);
        let affine: kurbo::Affine = m.into();

        let (fx, fy) = m.transform_point(px(5.0), px(7.0));
        let kp = affine * kurbo::Point::new(5.0, 7.0);
        assert!((f64::from(fx.get()) - kp.x).abs() < 1e-4);
        assert!((f64::from(fy.get()) - kp.y).abs() < 1e-4);

        // Round-trip back to Matrix4 preserves the 2D affine.
        let back = Matrix4::try_from(affine).unwrap();
        let (bx, by) = back.transform_point(px(5.0), px(7.0));
        assert!((bx.get() - fx.get()).abs() < 1e-4);
        assert!((by.get() - fy.get()).abs() < 1e-4);
    }

    #[test]
    fn non_finite_kurbo_value_is_rejected() {
        assert_eq!(
            Point::try_from(kurbo::Point::new(f64::INFINITY, 0.0)),
            Err(KurboBridgeError::OutOfRange(f64::INFINITY))
        );
        assert!(Point::try_from(kurbo::Point::new(1e300, 0.0)).is_err());
    }
}
