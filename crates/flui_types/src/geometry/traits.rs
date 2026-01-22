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

/// Axis in a 2D cartesian space.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum Axis {
    /// The vertical axis (y, up and down).
    Vertical,
    /// The horizontal axis (x, left and right).
    Horizontal,
}

impl Axis {
    /// Returns the opposite axis.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Axis;
    ///
    /// assert_eq!(Axis::Horizontal.invert(), Axis::Vertical);
    /// assert_eq!(Axis::Vertical.invert(), Axis::Horizontal);
    /// ```
    #[inline]
    #[must_use]
    pub const fn invert(self) -> Self {
        match self {
            Axis::Vertical => Axis::Horizontal,
            Axis::Horizontal => Axis::Vertical,
        }
    }

    /// Checks if this axis is vertical.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Axis;
    ///
    /// assert!(Axis::Vertical.is_vertical());
    /// assert!(!Axis::Horizontal.is_vertical());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Axis::Vertical)
    }

    /// Checks if this axis is horizontal.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Axis;
    ///
    /// assert!(Axis::Horizontal.is_horizontal());
    /// assert!(!Axis::Vertical.is_horizontal());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Axis::Horizontal)
    }

    /// Returns array index for this axis (0 for Horizontal, 1 for Vertical).
    ///
    /// Useful for accessing axis-specific data in arrays.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Axis;
    ///
    /// let values = [100.0, 200.0]; // [x, y]
    /// assert_eq!(values[Axis::Horizontal.index()], 100.0);
    /// assert_eq!(values[Axis::Vertical.index()], 200.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Axis::Horizontal => 0,
            Axis::Vertical => 1,
        }
    }

    /// Selects a value based on the axis.
    ///
    /// Returns `horizontal` for `Axis::Horizontal`, `vertical` for `Axis::Vertical`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Axis;
    ///
    /// let width = 100.0;
    /// let height = 200.0;
    ///
    /// assert_eq!(Axis::Horizontal.select(width, height), 100.0);
    /// assert_eq!(Axis::Vertical.select(width, height), 200.0);
    /// ```
    #[inline]
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
pub trait Unit: Copy + Clone + Debug + Default + PartialEq + PartialOrd {
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
/// **Note:** This trait requires standard operator traits (`Add`, `Sub`, `Mul`, `Div`)
/// and provides additional utility methods (`abs`, `min`, `max`) for common operations.
/// Prefer using operators directly for clarity; utility methods are provided for
/// generic programming contexts.
pub trait NumericUnit:
    Unit
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<f32, Output = Self>
    + Div<f32, Output = Self>
{
    /// Returns the absolute value.
    fn abs(self) -> Self;

    /// Returns the minimum of two values.
    fn min(self, other: Self) -> Self;

    /// Returns the maximum of two values.
    fn max(self, other: Self) -> Self;
}

impl Unit for f32 {
    type Scalar = f32;

    #[inline]
    fn one() -> Self {
        1.0
    }

    const MIN: Self = f32::MIN;
    const MAX: Self = f32::MAX;
}

impl NumericUnit for f32 {
    #[inline]
    fn abs(self) -> Self {
        self.abs()
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        self.min(other)
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        self.max(other)
    }
}

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
    /// Returns half of this value.
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
    /// Returns double of this value.
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
    /// Returns true if the absolute value is less than EPSILON.
    ///
    /// Note: This uses epsilon-based comparison for floating-point safety.
    /// Exact zero and values within epsilon range are considered zero.
    #[inline]
    fn is_zero(&self) -> bool {
        self.abs() < f32::EPSILON
    }
}

impl IsZero for f64 {
    /// Returns true if the absolute value is less than EPSILON.
    ///
    /// Note: This uses epsilon-based comparison for floating-point safety.
    /// Exact zero and values within epsilon range are considered zero.
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

    /// Returns -1 for negative, 0 for zero, 1 for positive.
    ///
    /// Unlike `signum()`, this explicitly handles zero and returns an integer.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Sign;
    ///
    /// assert_eq!(100.0_f32.abs_sign(), 1);
    /// assert_eq!((-50.0_f32).abs_sign(), -1);
    /// assert_eq!(0.0_f32.abs_sign(), 0);
    /// ```
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
    T: NumericUnit,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{device_px, px};

    #[test]
    fn test_axis_invert() {
        assert_eq!(Axis::Horizontal.invert(), Axis::Vertical);
        assert_eq!(Axis::Vertical.invert(), Axis::Horizontal);
    }

    #[test]
    fn test_axis_is_vertical() {
        assert!(Axis::Vertical.is_vertical());
        assert!(!Axis::Horizontal.is_vertical());
    }

    #[test]
    fn test_axis_is_horizontal() {
        assert!(Axis::Horizontal.is_horizontal());
        assert!(!Axis::Vertical.is_horizontal());
    }

    #[test]
    fn test_half() {
        assert_eq!(px(100.0).half(), px(50.0));
        assert_eq!(100.0_f32.half(), 50.0);
        assert_eq!(device_px(100).half(), device_px(50));
    }

    #[test]
    fn test_is_zero_float() {
        // Test epsilon-based comparison for floats
        assert!(0.0_f32.is_zero());
        assert!(!f32::EPSILON.is_zero()); // Exactly epsilon is NOT < epsilon
        assert!((f32::EPSILON * 0.5).is_zero()); // Half epsilon is within threshold
        assert!(!1.0_f32.is_zero());

        assert!(px(0.0).is_zero());
        assert!(px(f32::EPSILON * 0.5).is_zero()); // Within epsilon
        assert!(!px(1.0).is_zero());
    }

    #[test]
    fn test_is_zero_integer() {
        assert!(0_i32.is_zero());
        assert!(!1_i32.is_zero());

        assert!(device_px(0).is_zero());
        assert!(!device_px(1).is_zero());
    }

    #[test]
    fn test_sign() {
        // f32
        assert!(100.0_f32.is_positive());
        assert!((-100.0_f32).is_negative());
        assert_eq!(100.0_f32.signum(), 1.0);
        assert_eq!((-100.0_f32).signum(), -1.0);

        // Pixels
        let positive = px(100.0);
        assert!(positive.is_positive());
        assert!(!positive.is_negative());
        // Note: Pixels has an inherent signum() method that returns f32
        assert_eq!(Sign::signum(positive), px(1.0));

        let negative = px(-50.0);
        assert!(negative.is_negative());
        assert!(!negative.is_positive());
        assert_eq!(Sign::signum(negative), px(-1.0));

        // DevicePixels
        assert!(device_px(100).is_positive());
        assert!(device_px(-100).is_negative());
        // Note: DevicePixels has inherent signum() -> i32, use Sign trait for DevicePixels
        assert_eq!(Sign::signum(device_px(100)), device_px(1));
        assert_eq!(Sign::signum(device_px(-100)), device_px(-1));
    }

    #[test]
    fn test_geometry_ops() {
        let a = px(100.0);
        let b = px(50.0);

        assert_eq!(a.abs(), px(100.0));
        assert_eq!(px(-100.0).abs(), px(100.0));

        assert_eq!(a.min(b), px(50.0));
        assert_eq!(a.max(b), px(100.0));

        assert_eq!(b.clamp(px(60.0), px(140.0)), px(60.0));
        assert_eq!(a.clamp(px(60.0), px(140.0)), px(100.0));
        assert_eq!(px(150.0).clamp(px(60.0), px(140.0)), px(140.0));
    }

    #[test]
    fn test_lerp() {
        let start = px(0.0);
        let end = px(100.0);

        assert_eq!(start.lerp(end, 0.0), px(0.0));
        assert_eq!(start.lerp(end, 0.5), px(50.0));
        assert_eq!(start.lerp(end, 1.0), px(100.0));

        // Beyond range
        assert_eq!(start.lerp(end, 1.5), px(150.0));
        assert_eq!(start.lerp(end, -0.5), px(-50.0));
    }

    #[test]
    fn test_f32_unit() {
        assert_eq!(f32::zero(), 0.0);
        assert_eq!(f32::one(), 1.0);
        assert_eq!(f32::MIN, f32::MIN);
        assert_eq!(f32::MAX, f32::MAX);
    }

    #[test]
    fn test_numeric_unit_trait() {
        let a = 10.0_f32;
        let b = 20.0_f32;

        assert_eq!(a.abs(), 10.0);
        assert_eq!((-10.0_f32).abs(), 10.0);
        assert_eq!(a.min(b), 10.0);
        assert_eq!(a.max(b), 20.0);
    }

    #[test]
    fn test_axis_index() {
        let values = [100.0, 200.0]; // [x, y]
        assert_eq!(values[Axis::Horizontal.index()], 100.0);
        assert_eq!(values[Axis::Vertical.index()], 200.0);

        // Test inversion
        assert_eq!(Axis::Horizontal.invert().index(), 1);
        assert_eq!(Axis::Vertical.invert().index(), 0);
    }

    #[test]
    fn test_sign_zero() {
        // Rust's f32::signum() returns 1.0 for positive zero (IEEE 754)
        assert_eq!(0.0_f32.signum(), 1.0);
        // Note: Pixels has inherent signum() -> f32, use trait method
        assert_eq!(Sign::signum(px(0.0)).get(), 1.0);

        // Positive zero is considered positive by Rust
        assert!(0.0_f32.is_sign_positive());
        assert!(!0.0_f32.is_sign_negative());

        // abs_sign should return 0 for zero (special case)
        assert_eq!(0.0_f32.abs_sign(), 0);
        assert_eq!(px(0.0).abs_sign(), 0);
    }

    #[test]
    fn test_abs_sign() {
        // Positive
        assert_eq!(100.0_f32.abs_sign(), 1);
        assert_eq!(px(100.0).abs_sign(), 1);
        assert_eq!(device_px(100).abs_sign(), 1);

        // Negative
        assert_eq!((-50.0_f32).abs_sign(), -1);
        assert_eq!(px(-50.0).abs_sign(), -1);
        assert_eq!(device_px(-50).abs_sign(), -1);

        // Zero
        assert_eq!(0.0_f32.abs_sign(), 0);
        assert_eq!(px(0.0).abs_sign(), 0);
        assert_eq!(device_px(0).abs_sign(), 0);
    }

    #[test]
    fn test_lerp_edge_cases() {
        let start = px(100.0);
        let end = px(100.0); // Same values

        // Should handle same values gracefully
        assert_eq!(start.lerp(end, 0.5), px(100.0));
        assert_eq!(start.lerp(end, 0.0), px(100.0));
        assert_eq!(start.lerp(end, 1.0), px(100.0));

        // Negative lerp (extrapolation)
        let a = px(0.0);
        let b = px(100.0);
        assert_eq!(a.lerp(b, -1.0), px(-100.0));

        // Beyond 1.0 (extrapolation)
        assert_eq!(a.lerp(b, 2.0), px(200.0));

        // Zero distance lerp
        assert_eq!(px(0.0).lerp(px(0.0), 0.5), px(0.0));
    }

    #[test]
    fn test_saturating_lerp() {
        let start = px(0.0);
        let end = px(100.0);

        // Normal range
        assert_eq!(start.saturating_lerp(end, 0.0), px(0.0));
        assert_eq!(start.saturating_lerp(end, 0.5), px(50.0));
        assert_eq!(start.saturating_lerp(end, 1.0), px(100.0));

        // Clamped below
        assert_eq!(start.saturating_lerp(end, -0.5), px(0.0));
        assert_eq!(start.saturating_lerp(end, -10.0), px(0.0));

        // Clamped above
        assert_eq!(start.saturating_lerp(end, 1.5), px(100.0));
        assert_eq!(start.saturating_lerp(end, 100.0), px(100.0));

        // Edge cases with same values
        assert_eq!(px(50.0).saturating_lerp(px(50.0), 2.0), px(50.0));
    }

    #[test]
    fn test_clamp_edge_cases() {
        // Normal clamp
        let val = px(50.0);
        assert_eq!(val.clamp(px(0.0), px(100.0)), px(50.0));
        assert_eq!(val.clamp(px(60.0), px(100.0)), px(60.0));
        assert_eq!(val.clamp(px(0.0), px(40.0)), px(40.0));

        // Note: Rust's clamp panics when min > max in debug mode
        // This is correct behavior - don't test invalid input

        // Exact boundaries
        assert_eq!(px(0.0).clamp(px(0.0), px(100.0)), px(0.0));
        assert_eq!(px(100.0).clamp(px(0.0), px(100.0)), px(100.0));

        // Same min and max
        assert_eq!(px(50.0).clamp(px(75.0), px(75.0)), px(75.0));
    }

    #[test]
    fn test_geometry_ops_with_device_pixels() {
        let a = device_px(10);
        let b = device_px(20);

        assert_eq!(a.clamp(device_px(5), device_px(15)), device_px(10));
        assert_eq!(a.clamp(device_px(15), device_px(25)), device_px(15));

        // DevicePixels lerp with rounding
        assert_eq!(a.lerp(b, 0.5), device_px(15));
    }

    #[test]
    fn test_is_zero_epsilon() {
        // Values strictly less than epsilon are considered zero
        assert!((f32::EPSILON * 0.5).is_zero());
        assert!((f32::EPSILON * 0.9).is_zero());

        // Exactly epsilon and greater are NOT zero (strict < comparison)
        assert!(!f32::EPSILON.is_zero());
        assert!(!(f32::EPSILON * 2.0).is_zero());
        assert!(!0.001_f32.is_zero());

        // Exact zero
        assert!(0.0_f32.is_zero());
        assert!((-0.0_f32).is_zero());
    }

    // ========================================================================
    // RADIANS TRAIT TESTS
    // ========================================================================

    #[test]
    fn test_radians_half() {
        use crate::geometry::radians;
        use std::f32::consts::PI;

        assert_eq!(radians(PI).half(), radians(PI / 2.0));
        assert_eq!(radians(2.0).half(), radians(1.0));
        assert_eq!(radians(0.0).half(), radians(0.0));
    }

    #[test]
    fn test_radians_is_zero() {
        use crate::geometry::radians;

        assert!(radians(0.0).is_zero());
        assert!(radians(f32::EPSILON * 0.5).is_zero());
        assert!(!radians(1.0).is_zero());
        assert!(!radians(-1.0).is_zero());
    }

    #[test]
    fn test_radians_sign() {
        use crate::geometry::radians;
        use std::f32::consts::PI;

        // Positive
        let pos = radians(PI);
        assert!(pos.is_positive());
        assert!(!pos.is_negative());
        assert_eq!(Sign::signum(pos), radians(1.0));

        // Negative
        let neg = radians(-PI);
        assert!(neg.is_negative());
        assert!(!neg.is_positive());
        assert_eq!(Sign::signum(neg), radians(-1.0));

        // Zero (Rust's signum returns 1.0 for positive zero)
        let zero = radians(0.0);
        assert!(zero.0.is_sign_positive());
        assert!(!zero.0.is_sign_negative());
        assert_eq!(Sign::signum(zero).get(), 1.0);

        // abs_sign
        assert_eq!(radians(PI).abs_sign(), 1);
        assert_eq!(radians(-PI).abs_sign(), -1);
        assert_eq!(radians(0.0).abs_sign(), 0);
    }

    // ========================================================================
    // DOUBLE TRAIT TESTS
    // ========================================================================

    #[test]
    fn test_double() {
        // f32
        assert_eq!(50.0_f32.double(), 100.0);
        assert_eq!((-25.0_f32).double(), -50.0);

        // Pixels
        assert_eq!(px(50.0).double(), px(100.0));
        assert_eq!(px(-25.0).double(), px(-50.0));

        // DevicePixels
        assert_eq!(device_px(50).double(), device_px(100));
        assert_eq!(device_px(-25).double(), device_px(-50));
    }

    #[test]
    fn test_double_radians() {
        use crate::geometry::radians;
        use std::f32::consts::PI;

        assert_eq!(radians(PI).double(), radians(PI * 2.0));
        assert_eq!(radians(PI / 2.0).double(), radians(PI));
    }

    // ========================================================================
    // APPROXEQ TRAIT TESTS
    // ========================================================================

    #[test]
    fn test_approx_eq_f32() {
        let a = 100.0_f32;
        let b = 100.0 + 1e-8;
        let c = 100.1;

        assert!(a.approx_eq(&b));
        assert!(!a.approx_eq(&c));

        // Custom epsilon
        assert!(a.approx_eq_eps(&c, 0.2));
        assert!(!a.approx_eq_eps(&c, 0.05));
    }

    #[test]
    fn test_approx_eq_pixels() {
        let a = px(100.0);
        let b = px(100.0 + 1e-8);
        let c = px(100.1);

        assert!(a.approx_eq(&b));
        assert!(!a.approx_eq(&c));

        // Custom epsilon
        assert!(a.approx_eq_eps(&c, 0.2));
    }

    #[test]
    fn test_approx_eq_radians() {
        use crate::geometry::radians;
        use std::f32::consts::PI;

        let a = radians(PI);
        let b = radians(PI + 1e-8);
        let c = radians(PI + 0.1);

        assert!(a.approx_eq(&b));
        assert!(!a.approx_eq(&c));
    }
}
