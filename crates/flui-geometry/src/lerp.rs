//! `Lerp` and `MaybeLerp` — the interpolation substrate for the animation system.
//!
//! A single `Lerp` trait lets one generic `Tween<V: Lerp>` interpolate every
//! animatable value type, replacing the per-type tween structs Flutter needs
//! because Dart dispatches `begin + (end - begin) * t` dynamically.
//!
//! # Extrapolation contract
//!
//! Implementations MUST extrapolate for `t` outside `[0, 1]` — they must NOT
//! clamp `t`. Overshoot is a feature: bouncy, elastic, and spring curves emit
//! `t > 1` (or `t < 0`), and clamping would silently flatten that motion.

use crate::{Edges, Offset, Pixels, Rect, Size};

/// Linear interpolation between two values of the same type.
///
/// `t == 0.0` yields `self`, `t == 1.0` yields `other`, and values outside
/// `[0, 1]` extrapolate (see the module-level extrapolation contract).
///
/// The method is named `lerp_to` rather than `lerp` deliberately: several
/// geometry primitives already carry an inherent `lerp` with a different
/// signature (and clamping), which would shadow a trait method named `lerp` on
/// concrete types. `lerp_to` is unambiguous in both generic and concrete code.
pub trait Lerp: Clone {
    /// Interpolate from `self` toward `other` by `t`, extrapolating outside `[0, 1]`.
    fn lerp_to(&self, other: &Self, t: f32) -> Self;
}

/// Fallible interpolation for types that interpolate only when compatible — for
/// example decorations or gradients of differing shape, which return `None`
/// when the two values cannot be blended.
pub trait MaybeLerp: Clone {
    /// Interpolate `a` toward `b` by `t`, or `None` if the two are incompatible.
    fn maybe_lerp(a: &Self, b: &Self, t: f32) -> Option<Self>;
}

/// Every total [`Lerp`] type is trivially a [`MaybeLerp`] that always succeeds.
impl<T: Lerp> MaybeLerp for T {
    #[inline]
    fn maybe_lerp(a: &Self, b: &Self, t: f32) -> Option<Self> {
        Some(a.lerp_to(b, t))
    }
}

impl Lerp for f32 {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for Offset<Pixels> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        // Computed manually rather than via `Offset::lerp`, which clamps `t` and
        // would flatten spring/elastic overshoot.
        Offset::new(
            self.dx + (other.dx - self.dx) * t,
            self.dy + (other.dy - self.dy) * t,
        )
    }
}

impl Lerp for Size<Pixels> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        // `Size::lerp` already extrapolates (no clamp).
        Size::lerp(*self, *other, t)
    }
}

impl Lerp for Rect<Pixels> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        // `Rect::lerp` already extrapolates (no clamp).
        Rect::lerp(*self, *other, t)
    }
}

impl Lerp for Edges<Pixels> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        Edges {
            top: self.top + (other.top - self.top) * t,
            right: self.right + (other.right - self.right) * t,
            bottom: self.bottom + (other.bottom - self.bottom) * t,
            left: self.left + (other.left - self.left) * t,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::px;

    #[test]
    fn f32_lerp_extrapolates() {
        assert_eq!(0.0_f32.lerp_to(&10.0, 0.5), 5.0);
        assert_eq!(0.0_f32.lerp_to(&10.0, 0.0), 0.0);
        assert_eq!(0.0_f32.lerp_to(&10.0, 1.0), 10.0);
        // Overshoot must NOT be clamped.
        assert_eq!(0.0_f32.lerp_to(&10.0, 1.5), 15.0);
        assert_eq!(0.0_f32.lerp_to(&10.0, -0.5), -5.0);
    }

    #[test]
    fn offset_lerp_extrapolates() {
        let a = Offset::new(px(0.0), px(0.0));
        let b = Offset::new(px(10.0), px(20.0));
        let mid = a.lerp_to(&b, 0.5);
        assert_eq!(mid.dx, px(5.0));
        assert_eq!(mid.dy, px(10.0));
        // Overshoot preserved (unlike the clamping inherent Offset::lerp).
        let over = a.lerp_to(&b, 1.5);
        assert_eq!(over.dx, px(15.0));
        assert_eq!(over.dy, px(30.0));
    }

    #[test]
    fn edges_lerp_extrapolates() {
        let a = Edges {
            top: px(0.0),
            right: px(0.0),
            bottom: px(0.0),
            left: px(0.0),
        };
        let b = Edges {
            top: px(4.0),
            right: px(8.0),
            bottom: px(12.0),
            left: px(16.0),
        };
        let mid = a.lerp_to(&b, 0.5);
        assert_eq!(mid.top, px(2.0));
        assert_eq!(mid.left, px(8.0));
    }

    #[test]
    fn maybe_lerp_blankets_lerp() {
        assert_eq!(f32::maybe_lerp(&0.0, &10.0, 0.25), Some(2.5));
    }
}
