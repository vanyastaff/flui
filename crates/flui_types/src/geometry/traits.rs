//! Utility traits for geometry operations.
//!
//! This module provides helper traits that enable ergonomic operations on
//! geometry types. Inspired by GPUI's design patterns.

use super::{DevicePixels, Pixels, Radians, Rems, ScaledPixels};
use std::fmt::Debug;
use std::ops::{Add, Div, Mul, Neg, Sub};

// ============================================================================
// AXIS - 2D cartesian axes
// ============================================================================

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum Axis {
    /// The vertical axis (y, up and down).
    Vertical,
    /// The horizontal axis (x, left and right).
    Horizontal,
}

impl Axis {
    #[must_use]
    pub const fn invert(self) -> Self {
        match self {
            Axis::Vertical => Axis::Horizontal,
            Axis::Horizontal => Axis::Vertical,
        }
    }

    #[must_use]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Axis::Vertical)
    }

    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Axis::Horizontal)
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Axis::Horizontal => 0,
            Axis::Vertical => 1,
        }
    }

    #[must_use]
    pub fn select<T>(self, horizontal: T, vertical: T) -> T {
        match self {
            Axis::Horizontal => horizontal,
            Axis::Vertical => vertical,
        }
    }
}

// ============================================================================
// UNIT - Marker trait for unit types
// ============================================================================

/// Marker trait for all unit types (Pixels, DevicePixels, etc.).
///
/// This trait enables generic geometry types to work with different
/// coordinate systems in a type-safe manner.
///
/// **Note:** This trait requires `Eq` and `Hash` to prevent using raw floating-point
/// types (like `f32`) which don't implement these traits due to NaN semantics.
/// Use wrapper types like `Pixels` instead, which implement `Eq` and `Hash` manually.
pub trait Unit: Copy + Clone + Debug + Default + PartialEq + Eq + PartialOrd + std::hash::Hash {
    /// The underlying scalar type (f32, i32, etc.)
    type Scalar: Copy;

    /// Returns the zero value for this unit
    fn zero() -> Self {
        Self::default()
    }

    /// Returns the one value for this unit (useful for scaling)
    fn one() -> Self;

    /// Minimum representable value
    const MIN: Self;

    /// Maximum representable value
    const MAX: Self;
}

/// Units that support arithmetic operations.
///
/// This trait enables math operations on unit types while maintaining
/// type safety. All operations preserve the unit type.
///
/// **Note:** This trait requires standard operator traits (`Add`, `Sub`)
/// and provides additional utility methods (`abs`, `min`, `max`) for common operations.
/// Prefer using operators directly for clarity; utility methods are provided for
/// generic programming contexts.
pub trait NumericUnit: Unit + Add<Output = Self> + Sub<Output = Self> {
    /// Returns the absolute value.
    fn abs(self) -> Self;

    /// Returns the minimum of two values.
    fn min(self, other: Self) -> Self;

    /// Returns the maximum of two values.
    fn max(self, other: Self) -> Self;
}

// Note: f32 impl removed - f32 cannot implement Unit due to Eq + Hash requirements.
// Use wrapper types like Pixels, PixelDelta, etc. instead.

// ============================================================================
// ALONG - Axis-based value access
// ============================================================================

/// Access values along a specific axis.
///
/// This trait provides a unified interface for accessing components
/// of geometry types along the horizontal (x/width) or vertical (y/height) axis.
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
/// use flui_types::geometry::{Pixels, Half, px};
///
/// let width = px(100.0);
/// assert_eq!(width.half(), px(50.0));
/// ```
pub trait Half {
    #[must_use]
    fn half(&self) -> Self;
}

impl Half for f32 {
    #[inline]
    fn half(&self) -> Self {
        self * 0.5
    }
}

impl Half for f64 {
    #[inline]
    fn half(&self) -> Self {
        self * 0.5
    }
}

impl Half for Pixels {
    #[inline]
    fn half(&self) -> Self {
        Pixels(self.get() * 0.5)
    }
}

impl Half for Rems {
    #[inline]
    fn half(&self) -> Self {
        Rems(self.get() * 0.5)
    }
}

impl Half for ScaledPixels {
    #[inline]
    fn half(&self) -> Self {
        ScaledPixels(self.get() * 0.5)
    }
}

impl Half for DevicePixels {
    #[inline]
    fn half(&self) -> Self {
        DevicePixels(self.get() / 2)
    }
}

impl Half for Radians {
    #[inline]
    fn half(&self) -> Self {
        Radians(self.get() * 0.5)
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
/// use flui_types::geometry::{Pixels, Double, px};
///
/// let width = px(50.0);
/// assert_eq!(width.double(), px(100.0));
/// ```
pub trait Double {
    #[must_use]
    fn double(&self) -> Self;
}

impl Double for f32 {
    #[inline]
    fn double(&self) -> Self {
        self * 2.0
    }
}

impl Double for f64 {
    #[inline]
    fn double(&self) -> Self {
        self * 2.0
    }
}

impl Double for Pixels {
    #[inline]
    fn double(&self) -> Self {
        Pixels(self.get() * 2.0)
    }
}

impl Double for Rems {
    #[inline]
    fn double(&self) -> Self {
        Rems(self.get() * 2.0)
    }
}

impl Double for ScaledPixels {
    #[inline]
    fn double(&self) -> Self {
        ScaledPixels(self.get() * 2.0)
    }
}

impl Double for DevicePixels {
    #[inline]
    fn double(&self) -> Self {
        DevicePixels(self.get() * 2)
    }
}

impl Double for Radians {
    #[inline]
    fn double(&self) -> Self {
        Radians(self.get() * 2.0)
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
/// use flui_types::geometry::{Pixels, IsZero, px};
///
/// assert!(px(0.0).is_zero());
/// assert!(!px(1.0).is_zero());
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

impl IsZero for Pixels {
    #[inline]
    fn is_zero(&self) -> bool {
        self.get().abs() < f32::EPSILON
    }
}

impl IsZero for Rems {
    #[inline]
    fn is_zero(&self) -> bool {
        self.get().abs() < f32::EPSILON
    }
}

impl IsZero for ScaledPixels {
    #[inline]
    fn is_zero(&self) -> bool {
        self.get().abs() < f32::EPSILON
    }
}

impl IsZero for DevicePixels {
    #[inline]
    fn is_zero(&self) -> bool {
        self.get() == 0
    }
}

impl IsZero for Radians {
    #[inline]
    fn is_zero(&self) -> bool {
        self.0.abs() < f32::EPSILON
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
/// # Note on `signum()` method conflicts
///
/// **Important:** Some types (like `Pixels`) also have inherent `signum_raw()` methods
/// that return `f32` instead of `Self`. Use the trait method `Sign::signum(value)` when
/// you need the result in the same type.
///
/// ```rust
/// use flui_types::geometry::{Pixels, Sign, px};
///
/// let value = px(100.0);
///
/// // Inherent method signum_raw() returns f32
/// let sign_f32: f32 = value.signum_raw();
///
/// // Trait method (returns Pixels) - use qualified syntax
/// let sign_px: Pixels = Sign::signum(value);
/// ```
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Pixels, Sign, px};
///
/// let positive = px(100.0);
/// assert!(positive.is_positive());
/// assert!(!positive.is_negative());
/// assert_eq!(Sign::signum(positive), px(1.0));
///
/// let negative = px(-50.0);
/// assert!(negative.is_negative());
/// assert_eq!(Sign::signum(negative), px(-1.0));
/// ```
pub trait Sign: Neg<Output = Self> + Sized {
    /// Returns true if the value is positive.
    fn is_positive(&self) -> bool;

    /// Returns true if the value is negative.
    fn is_negative(&self) -> bool;

    /// Returns the sign of the value (-1, 0, or 1).
    fn signum(self) -> Self;

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
        self.signum()
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
        self.signum()
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
        self.signum()
    }
}

impl Sign for Pixels {
    #[inline]
    fn is_positive(&self) -> bool {
        self.get() > 0.0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.get() < 0.0
    }

    #[inline]
    fn signum(self) -> Self {
        Pixels(self.get().signum())
    }
}

impl Sign for Rems {
    #[inline]
    fn is_positive(&self) -> bool {
        self.get() > 0.0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.get() < 0.0
    }

    #[inline]
    fn signum(self) -> Self {
        Rems(self.get().signum())
    }
}

impl Sign for ScaledPixels {
    #[inline]
    fn is_positive(&self) -> bool {
        self.get() > 0.0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.get() < 0.0
    }

    #[inline]
    fn signum(self) -> Self {
        ScaledPixels(self.get().signum())
    }
}

impl Sign for DevicePixels {
    #[inline]
    fn is_positive(&self) -> bool {
        self.get() > 0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.get() < 0
    }

    #[inline]
    fn signum(self) -> Self {
        DevicePixels(self.get().signum())
    }
}

impl Sign for Radians {
    #[inline]
    fn is_positive(&self) -> bool {
        self.get() > 0.0
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.get() < 0.0
    }

    #[inline]
    fn signum(self) -> Self {
        Radians(self.get().signum())
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
/// use flui_types::geometry::{Pixels, ApproxEq, px};
///
/// let a = px(100.0);
/// let b = px(100.0 + 1e-8);  // Very close but not exactly equal
///
/// assert!(a.approx_eq(&b));
/// assert!(a.approx_eq_eps(&b, 1e-6));
///
/// let c = px(100.1);
/// assert!(!a.approx_eq(&c));
/// ```
pub trait ApproxEq {
    /// Default epsilon for approximate equality.
    const DEFAULT_EPSILON: f32 = 1e-6;

    /// Returns true if self and other are approximately equal using the default epsilon.
    fn approx_eq(&self, other: &Self) -> bool {
        self.approx_eq_eps(other, Self::DEFAULT_EPSILON)
    }

    /// Returns true if self and other are approximately equal using the given epsilon.
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool;
}

impl ApproxEq for f32 {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        (self - other).abs() < epsilon
    }
}

impl ApproxEq for f64 {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        (self - other).abs() < epsilon as f64
    }
}

impl ApproxEq for Pixels {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        (self.get() - other.get()).abs() < epsilon
    }
}

impl ApproxEq for Rems {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        (self.get() - other.get()).abs() < epsilon
    }
}

impl ApproxEq for ScaledPixels {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        (self.get() - other.get()).abs() < epsilon
    }
}

impl ApproxEq for Radians {
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        (self.get() - other.get()).abs() < epsilon
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
/// use flui_types::geometry::{Pixels, GeometryOps, px};
///
/// let a = px(-100.0);
/// assert_eq!(a.abs(), px(100.0));
///
/// let b = px(50.0);
/// let c = px(150.0);
/// assert_eq!(b.min(c), px(50.0));
/// assert_eq!(b.max(c), px(150.0));
/// assert_eq!(b.clamp(px(60.0), px(140.0)), px(60.0));
///
/// // Linear interpolation
/// let start = px(0.0);
/// let end = px(100.0);
/// assert_eq!(start.lerp(end, 0.5), px(50.0));
///
/// // Safe interpolation with clamping
/// assert_eq!(start.saturating_lerp(end, 1.5), px(100.0));
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
    fn lerp(self, other: Self, t: f32) -> Self;

    /// Safe linear interpolation with clamping to [0.0, 1.0] range.
    ///
    /// This clamps `t` to ensure the result stays between `self` and `other`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Pixels, GeometryOps, px};
    ///
    /// let start = px(0.0);
    /// let end = px(100.0);
    ///
    /// // t clamped to [0.0, 1.0]
    /// assert_eq!(start.saturating_lerp(end, 1.5), px(100.0));
    /// assert_eq!(start.saturating_lerp(end, -0.5), px(0.0));
    /// ```
    fn saturating_lerp(self, other: Self, t: f32) -> Self;
}

impl<T> GeometryOps for T
where
    T: NumericUnit + Mul<f32, Output = T>,
{
    #[inline(always)]
    fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }

    #[inline(always)]
    fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    #[inline(always)]
    fn saturating_lerp(self, other: Self, t: f32) -> Self {
        let clamped_t = t.clamp(0.0, 1.0);
        self.lerp(other, clamped_t)
    }
}

// ============================================================================
// TESTS
// ============================================================================
