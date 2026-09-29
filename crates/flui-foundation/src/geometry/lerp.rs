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

use crate::geometry::{Corners, Edges, Matrix4, Offset, Radius, Rect, Size};

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
    fn lerp_to(&self, other: &Self, t: f64) -> Self;
}

/// Fallible interpolation for types that interpolate only when compatible — for
/// example decorations or gradients of differing shape, which return `None`
/// when the two values cannot be blended.
pub trait MaybeLerp: Clone {
    /// Interpolate `a` toward `b` by `t`, or `None` if the two are incompatible.
    fn maybe_lerp(a: &Self, b: &Self, t: f64) -> Option<Self>;
}

/// Every total [`Lerp`] type is trivially a [`MaybeLerp`] that always succeeds.
impl<T: Lerp> MaybeLerp for T {
    #[inline]
    fn maybe_lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        Some(a.lerp_to(b, t))
    }
}

impl Lerp for f64 {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for Offset<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        // Computed manually rather than via `Offset::lerp`, which clamps `t` and
        // would flatten spring/elastic overshoot.
        Offset::new(
            self.dx + (other.dx - self.dx) * t,
            self.dy + (other.dy - self.dy) * t,
        )
    }
}

impl Lerp for Size<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        // `Size::lerp` already extrapolates (no clamp).
        Size::lerp(*self, *other, t)
    }
}

impl Lerp for Rect<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        // `Rect::lerp` already extrapolates (no clamp).
        Rect::lerp(*self, *other, t)
    }
}

impl Lerp for Edges<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Edges {
            top: self.top + (other.top - self.top) * t,
            right: self.right + (other.right - self.right) * t,
            bottom: self.bottom + (other.bottom - self.bottom) * t,
            left: self.left + (other.left - self.left) * t,
        }
    }
}

impl Lerp for Radius<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Radius::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
}

/// Interpolates each corner independently. With `Radius: Lerp` this
/// makes `BorderRadius` (= `Corners<Radius>`) animatable, collapsing
/// the bespoke border-radius tween into the generic `Tween<V>`.
impl<T: Lerp> Lerp for Corners<T> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Corners {
            top_left: self.top_left.lerp_to(&other.top_left, t),
            top_right: self.top_right.lerp_to(&other.top_right, t),
            bottom_right: self.bottom_right.lerp_to(&other.bottom_right, t),
            bottom_left: self.bottom_left.lerp_to(&other.bottom_left, t),
        }
    }
}

impl Lerp for Matrix4 {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        // Decompose -> slerp rotation -> recompose; see `Matrix4::lerp`.
        Matrix4::lerp(*self, *other, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix4_lerp_rotation_slerps_not_collapses() {
        // A naive element-wise lerp of a rotation passes through a degenerate
        // (non-orthonormal, det < 1) matrix at t = 0.5; decompose+slerp keeps it
        // a proper rotation. Same-axis slerp is exact angle interpolation, so the
        // half-way point equals the half-angle rotation.
        let mid = Matrix4::rotation_z(0.0).lerp_to(&Matrix4::rotation_z(0.6), 0.5);
        let expected = Matrix4::rotation_z(0.3);
        for i in 0..16 {
            assert!(
                (mid.m[i] - expected.m[i]).abs() < 1e-3,
                "element {i}: {} vs expected {}",
                mid.m[i],
                expected.m[i],
            );
        }
    }
}
