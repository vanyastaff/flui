//! Utility traits for geometry operations.
//!
//! This module provides helper traits that enable ergonomic operations on
//! geometry types. Inspired by GPUI's design patterns.

use crate::geometry::axis::Axis;
use std::{
    fmt::Debug,
    ops::{Add, Mul, Neg, Sub},
};

// ============================================================================
// UNIT - the scalar a geometry type is built from
// ============================================================================

/// The scalar a geometry type is built from: `f64` for logical coordinates, `f32` for the
/// display list and GPU, `i32` for the device-pixel grid.
///
/// There are no unit wrappers (ADR-0098). The scalar alone keeps logical (`f64`) and device
/// (`i32`) values apart, and float types are neither `Eq` nor `Hash`, so neither is any
/// geometry type built from them.
pub trait Unit: Copy + Clone + Debug + Default + PartialEq + PartialOrd {
    /// Returns zero.
    fn zero() -> Self {
        Self::default()
    }

    /// Returns one.
    fn one() -> Self;

    /// Minimum representable value.
    const MIN: Self;

    /// Maximum representable value.
    const MAX: Self;
}

/// A scalar with arithmetic.
pub trait NumericUnit: Unit + Add<Output = Self> + Sub<Output = Self> {
    /// Returns the absolute value.
    fn abs(self) -> Self;

    /// Returns the minimum of two values.
    fn min(self, other: Self) -> Self;

    /// Returns the maximum of two values.
    fn max(self, other: Self) -> Self;
}

/// A floating-point scalar, convertible to and from `f64` for shared math.
pub trait FloatUnit: NumericUnit + Into<f64> {
    /// Converts from `f64`, rounding to the nearest representable value.
    fn from_f64(value: f64) -> Self;

    /// Returns the average without overflowing finite operands.
    ///
    /// The default computes in `f64` before converting back. Primitive float
    /// implementations use their native standard-library midpoint and rounding.
    /// NaN operands or opposite infinities produce NaN.
    fn midpoint(self, other: Self) -> Self {
        Self::from_f64(f64::midpoint(self.into(), other.into()))
    }
}

macro_rules! impl_float_scalar {
    ($($ty:ty),+) => {
        $(
            impl Unit for $ty {
                fn one() -> Self {
                    1.0
                }
                const MIN: Self = <$ty>::MIN;
                const MAX: Self = <$ty>::MAX;
            }

            impl NumericUnit for $ty {
                #[inline]
                fn abs(self) -> Self {
                    <$ty>::abs(self)
                }

                #[inline]
                fn min(self, other: Self) -> Self {
                    <$ty>::min(self, other)
                }

                #[inline]
                fn max(self, other: Self) -> Self {
                    <$ty>::max(self, other)
                }
            }
        )+
    };
}

impl_float_scalar!(f32, f64);

impl Unit for i32 {
    fn one() -> Self {
        1
    }
    const MIN: Self = i32::MIN;
    const MAX: Self = i32::MAX;
}

impl NumericUnit for i32 {
    #[inline]
    fn abs(self) -> Self {
        i32::abs(self)
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        std::cmp::Ord::min(self, other)
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        std::cmp::Ord::max(self, other)
    }
}

impl FloatUnit for f32 {
    #[inline]
    fn from_f64(value: f64) -> Self {
        value as f32
    }

    #[inline]
    fn midpoint(self, other: Self) -> Self {
        f32::midpoint(self, other)
    }
}

impl FloatUnit for f64 {
    #[inline]
    fn from_f64(value: f64) -> Self {
        value
    }

    #[inline]
    fn midpoint(self, other: Self) -> Self {
        f64::midpoint(self, other)
    }
}

// ============================================================================
// ALONG - Axis-based value access
// ============================================================================

/// Access values along a specific axis.
///
/// This trait provides a unified interface for accessing components
/// of geometry types along the horizontal (x/width) or vertical (y/height)
/// axis.
///
/// # Examples
///
/// Implementations are provided by Point and Size types:
///
/// ```text
/// Example usage (implementations in point.rs and size.rs):
/// let p = point(10.0, 20.0);
/// assert_eq!(p.along(Axis::Horizontal), 10.0);
/// assert_eq!(p.along(Axis::Vertical), 20.0);
/// ```
pub trait Along {
    /// The type of value accessed along the axis.
    type Unit;

    /// Returns the value along the given axis.
    fn along(&self, axis: Axis) -> Self::Unit;

    /// Applies a function to the value along the given axis.
    fn apply_along(&self, axis: Axis, f: impl FnOnce(Self::Unit) -> Self::Unit) -> Self;
}

// ============================================================================
// HALF - Compute half value
// ============================================================================

/// Compute half of a value.
///
/// This trait provides a semantic method for halving values, commonly
/// used for centering calculations.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::Half;
///
/// let width = 100.0;
/// assert_eq!(width.half(), 50.0);
/// ```
pub trait Half {
    /// Returns half of this value.
    #[must_use]
    fn half(self) -> Self;
}

impl Half for f32 {
    #[inline]
    fn half(self) -> Self {
        self * 0.5
    }
}

impl Half for f64 {
    #[inline]
    fn half(self) -> Self {
        self * 0.5
    }
}

impl Half for i32 {
    #[inline]
    fn half(self) -> Self {
        self / 2
    }
}

// ============================================================================
// DOUBLE - Compute double value
// ============================================================================

/// Compute double of a value.
///
/// This trait provides a semantic method for doubling values, complementing
/// the [`Half`] trait.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::Double;
///
/// let width = 50.0;
/// assert_eq!(width.double(), 100.0);
/// ```
pub trait Double {
    /// Returns double of this value.
    #[must_use]
    fn double(self) -> Self;
}

impl Double for f32 {
    #[inline]
    fn double(self) -> Self {
        self * 2.0
    }
}

impl Double for f64 {
    #[inline]
    fn double(self) -> Self {
        self * 2.0
    }
}

impl Double for i32 {
    #[inline]
    fn double(self) -> Self {
        self * 2
    }
}

// ============================================================================
// ISZERO - Zero check
// ============================================================================

/// Check if a value is zero.
///
/// This trait provides a semantic method for checking if a value is zero,
/// which can be clearer than direct comparison in some contexts.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::IsZero;
///
/// assert!(0.0.is_zero());
/// assert!(!1.0.is_zero());
/// ```
pub trait IsZero {
    /// Returns true if this value is zero.
    fn is_zero(&self) -> bool;
}

impl IsZero for f32 {
    #[inline]
    fn is_zero(&self) -> bool {
        self.abs() < f32::EPSILON
    }
}

impl IsZero for f64 {
    #[inline]
    fn is_zero(&self) -> bool {
        self.abs() < f64::EPSILON
    }
}

impl IsZero for i32 {
    #[inline]
    fn is_zero(&self) -> bool {
        *self == 0
    }
}

impl IsZero for usize {
    #[inline]
    fn is_zero(&self) -> bool {
        *self == 0
    }
}

// ============================================================================
// SIGN - Sign operations
// ============================================================================

/// Sign-related operations for numeric types.
///
/// This trait provides methods for checking and manipulating the sign
/// of a numeric value.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::Sign;
///
/// let positive = 100.0;
/// assert!(positive.is_positive());
/// assert!(!positive.is_negative());
/// assert_eq!(Sign::signum(positive), 1.0);
///
/// let negative = -50.0;
/// assert!(negative.is_negative());
/// assert_eq!(Sign::signum(negative), -1.0);
/// ```
pub trait Sign: Neg<Output = Self> + Sized {
    /// Returns true if the value is positive.
    fn is_positive(&self) -> bool;

    /// Returns true if the value is negative.
    fn is_negative(&self) -> bool;

    /// Returns the sign of the value (-1, 0, or 1).
    fn signum(self) -> Self;

    /// Returns the sign as an integer: 1, -1, or 0.
    #[inline]
    fn abs_sign(&self) -> i32 {
        if self.is_positive() {
            1
        } else if self.is_negative() {
            -1
        } else {
            0
        }
    }
}

// Primitive impls use fully qualified syntax to avoid infinite recursion
// with the trait method shadowing the inherent method.

impl Sign for f32 {
    #[inline]
    fn is_positive(&self) -> bool {
        *self > 0.0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        *self < 0.0
    }

    #[inline]
    fn signum(self) -> Self {
        f32::signum(self)
    }
}

impl Sign for f64 {
    #[inline]
    fn is_positive(&self) -> bool {
        *self > 0.0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        *self < 0.0
    }

    #[inline]
    fn signum(self) -> Self {
        f64::signum(self)
    }
}

impl Sign for i32 {
    #[inline]
    fn is_positive(&self) -> bool {
        *self > 0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        *self < 0
    }

    #[inline]
    fn signum(self) -> Self {
        i32::signum(self)
    }
}

// ============================================================================
// APPROXEQ - Approximate equality for floating-point values
// ============================================================================

/// Approximate equality for floating-point values.
///
/// This trait provides epsilon-based comparison for unit types that wrap
/// floating-point values. Useful for geometry calculations where exact
/// equality is often problematic due to floating-point precision.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::ApproxEq;
///
/// let a = 100.0;
/// let b = (100.0 + 1e-8); // Very close but not exactly equal
///
/// assert!(a.approx_eq(&b));
/// assert!(a.approx_eq_eps(&b, 1e-6));
///
/// let c = 100.1;
/// assert!(!a.approx_eq(&c));
/// ```
pub trait ApproxEq {
    /// Default epsilon for approximate equality.
    const DEFAULT_EPSILON: f64 = 1e-6;

    /// Returns true if self and other are approximately equal using the default
    /// epsilon.
    fn approx_eq(&self, other: &Self) -> bool {
        self.approx_eq_eps(other, Self::DEFAULT_EPSILON)
    }

    /// Returns true if self and other are approximately equal using the given
    /// epsilon.
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool;
}

impl ApproxEq for f32 {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        f64::from((self - other).abs()) < epsilon
    }
}

impl ApproxEq for f64 {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        (self - other).abs() < epsilon
    }
}

impl ApproxEq for i32 {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        f64::from(self.abs_diff(*other)) <= epsilon
    }
}

// ============================================================================
// GEOMETRYOPS - Common geometry operations
// ============================================================================

/// Common geometry operations combining arithmetic with useful utilities.
///
/// This trait provides a unified interface for operations commonly needed
/// in geometry calculations: absolute values, min/max, clamping, and
/// linear interpolation.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::GeometryOps;
///
/// let a = -100.0_f64;
/// assert_eq!(a.abs(), 100.0);
///
/// let b = 50.0_f64;
/// let c = 150.0_f64;
/// assert_eq!(b.min(c), 50.0);
/// assert_eq!(b.max(c), 150.0);
/// assert_eq!(b.clamp(60.0, 140.0), 60.0);
///
/// // Linear interpolation
/// let start = 0.0;
/// let end = 100.0;
/// assert_eq!(start.lerp(end, 0.5), 50.0);
///
/// // Safe interpolation with clamping
/// assert_eq!(start.saturating_lerp(end, 1.5), 100.0);
/// ```
pub trait GeometryOps: NumericUnit {
    /// Clamps the value between min and max.
    fn clamp(self, min: Self, max: Self) -> Self;

    /// Linear interpolation between self and other.
    ///
    /// When `t = 0.0`, returns `self`.
    /// When `t = 1.0`, returns `other`.
    /// Values between interpolate linearly.
    /// Values outside [0.0, 1.0] extrapolate beyond the range.
    fn lerp(self, other: Self, t: f64) -> Self;

    /// Safe linear interpolation with clamping to [0.0, 1.0] range.
    ///
    /// This clamps `t` to ensure the result stays between `self` and `other`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::geometry::GeometryOps;
    ///
    /// let start = 0.0;
    /// let end = 100.0;
    ///
    /// // t clamped to [0.0, 1.0]
    /// assert_eq!(start.saturating_lerp(end, 1.5), 100.0);
    /// assert_eq!(start.saturating_lerp(end, -0.5), 0.0);
    /// ```
    fn saturating_lerp(self, other: Self, t: f64) -> Self;
}

impl<T> GeometryOps for T
where
    T: FloatUnit + Mul<Output = T>,
{
    #[inline]
    fn clamp(self, min: Self, max: Self) -> Self {
        NumericUnit::min(NumericUnit::max(self, min), max)
    }

    #[inline]
    fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * T::from_f64(t)
    }

    #[inline]
    fn saturating_lerp(self, other: Self, t: f64) -> Self {
        let clamped_t = t.clamp(0.0, 1.0);
        self.lerp(other, clamped_t)
    }
}

// ============================================================================
// TESTS
// ============================================================================
