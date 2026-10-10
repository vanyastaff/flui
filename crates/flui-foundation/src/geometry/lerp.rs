//! `Lerp` and `MaybeLerp` — the interpolation substrate for the animation system.
//!
//! A single `Lerp` trait lets one generic `Tween<V: Lerp>` interpolate every
//! animatable value type, so no per-type tween structs are needed.
//!
//! # Extrapolation contract
//!
//! Implementations MUST extrapolate for `t` outside `[0, 1]` — they must NOT
//! clamp `t`. Overshoot is a feature: bouncy, elastic, and spring curves emit
//! `t > 1` (or `t < 0`), and clamping would silently flatten that motion.
//!
//! # Domains belong to the property
//!
//! An extrapolated value can leave the domain its consumer accepts: a padding
//! tweened from 16 to 0 along a back-out curve passes through −1.6, and a size
//! shrinking with overshoot goes negative. Neither `Lerp` nor `Tween` clamps it,
//! because only the property knows its domain (a padding or a size is
//! non-negative, an `Offset` or an `Alignment` is not). The consumer that applies
//! the value to a property clamps it there, at the point of use.
//!
//! # NaN
//!
//! Types with a NaN representation (the `f64` geometry, [`Angle`]) carry a NaN `t`
//! or a NaN endpoint through intermediate samples. Exact endpoint samples
//! retain the selected endpoint, including its signed zero or NaN payload.
//! Types without one decide for
//! themselves and document it (`Color` and the integer tweens return their
//! beginning value).
//!
//! [`Angle`]: crate::geometry::Angle

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
        if t == 0.0 {
            return *self;
        }
        if t == 1.0 {
            return *other;
        }
        let value = self + (other - self) * t;
        if value.is_finite() {
            value
        } else {
            // Opposite finite endpoints can overflow their difference while
            // their weighted sum remains representable. Genuine overflow and
            // invalid input still remain visible to the consuming property.
            self * (1.0 - t) + other * t
        }
    }
}

impl Lerp for Offset<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        // Computed manually rather than via `Offset::lerp`, which clamps `t` and
        // would flatten spring/elastic overshoot.
        Offset::new(self.dx.lerp_to(&other.dx, t), self.dy.lerp_to(&other.dy, t))
    }
}

impl Lerp for Size<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Size::new(
            self.width.lerp_to(&other.width, t),
            self.height.lerp_to(&other.height, t),
        )
    }
}

impl Lerp for Rect<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Rect::from_ltrb(
            self.min.x.lerp_to(&other.min.x, t),
            self.min.y.lerp_to(&other.min.y, t),
            self.max.x.lerp_to(&other.max.x, t),
            self.max.y.lerp_to(&other.max.y, t),
        )
    }
}

impl Lerp for Edges<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Edges {
            top: self.top.lerp_to(&other.top, t),
            right: self.right.lerp_to(&other.right, t),
            bottom: self.bottom.lerp_to(&other.bottom, t),
            left: self.left.lerp_to(&other.left, t),
        }
    }
}

impl Lerp for Radius<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Radius::new(self.x.lerp_to(&other.x, t), self.y.lerp_to(&other.y, t))
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
        // Decompose, interpolate the parts, recompose; see `Matrix4::lerp`.
        Matrix4::lerp(*self, *other, t)
    }
}
