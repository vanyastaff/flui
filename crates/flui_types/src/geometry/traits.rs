//! Utility traits for geometry operations.
//!
//! This module provides helper traits that enable ergonomic operations on
//! geometry types. Inspired by GPUI's design patterns.

use super::{DevicePixels, Pixels, Rems, ScaledPixels};
use std::fmt::Debug;

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
}

// ============================================================================
// UNIT - Marker trait for unit types
// ============================================================================

/// Marker trait for all unit types (Pixels, DevicePixels, etc.).
///
/// This trait enables generic geometry types to work with different
/// coordinate systems in a type-safe manner.
pub trait Unit: Copy + Clone + Debug {
    /// The underlying scalar type (f32, i32, etc.)
    type Scalar: Copy;

    /// Returns the zero value for this unit
    fn zero() -> Self;
}

/// Units that support arithmetic operations.
///
/// This trait enables math operations on unit types while maintaining
/// type safety. All operations preserve the unit type.
pub trait NumericUnit: Unit {
    /// Add two values of the same unit
    fn add(self, other: Self) -> Self;

    /// Subtract two values of the same unit
    fn sub(self, other: Self) -> Self;

    /// Multiply by a scalar (dimensionless)
    fn mul(self, scalar: f32) -> Self;

    /// Divide by a scalar (dimensionless)
    fn div(self, scalar: f32) -> Self;
}

impl Unit for f32 {
    type Scalar = f32;

    #[inline]
    fn zero() -> Self {
        0.0
    }
}

impl NumericUnit for f32 {
    #[inline]
    fn add(self, other: Self) -> Self {
        self + other
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        self - other
    }

    #[inline]
    fn mul(self, scalar: f32) -> Self {
        self * scalar
    }

    #[inline]
    fn div(self, scalar: f32) -> Self {
        self / scalar
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
/// ```rust
/// use flui_types::geometry::{Point, Size, Axis, Along, point, size};
///
/// let p = point(10.0, 20.0);
/// assert_eq!(p.along(Axis::Horizontal), 10.0);
/// assert_eq!(p.along(Axis::Vertical), 20.0);
///
/// let s = size(100.0, 200.0);
/// assert_eq!(s.along(Axis::Horizontal), 100.0);
/// assert_eq!(s.along(Axis::Vertical), 200.0);
///
/// // Modify along axis
/// let modified = p.apply_along(Axis::Horizontal, |x| x * 2.0);
/// assert_eq!(modified.x, 20.0);
/// assert_eq!(modified.y, 20.0);
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

// ============================================================================
// NEGATE - Semantic negation
// ============================================================================

/// Negate a value (semantic alternative to `-` operator).
///
/// This trait provides a semantic method for negating values, which can be
/// more readable than the unary minus operator in some contexts.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Pixels, Negate, px};
///
/// let positive = px(100.0);
/// assert_eq!(positive.negate(), px(-100.0));
/// ```
pub trait Negate {
    /// Returns the negated value.
    #[must_use]
    fn negate(self) -> Self;
}

impl Negate for f32 {
    #[inline]
    fn negate(self) -> Self {
        -self
    }
}

impl Negate for f64 {
    #[inline]
    fn negate(self) -> Self {
        -self
    }
}

impl Negate for Pixels {
    #[inline]
    fn negate(self) -> Self {
        -self
    }
}

impl Negate for Rems {
    #[inline]
    fn negate(self) -> Self {
        Rems(-self.get())
    }
}

impl Negate for ScaledPixels {
    #[inline]
    fn negate(self) -> Self {
        -self
    }
}

impl Negate for DevicePixels {
    #[inline]
    fn negate(self) -> Self {
        DevicePixels(-self.get())
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
        *self == 0.0
    }
}

impl IsZero for f64 {
    #[inline]
    fn is_zero(&self) -> bool {
        *self == 0.0
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
        self.get() == 0.0
    }
}

impl IsZero for Rems {
    #[inline]
    fn is_zero(&self) -> bool {
        self.get() == 0.0
    }
}

impl IsZero for ScaledPixels {
    #[inline]
    fn is_zero(&self) -> bool {
        self.get() == 0.0
    }
}

impl IsZero for DevicePixels {
    #[inline]
    fn is_zero(&self) -> bool {
        self.get() == 0
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::px;

    #[test]
    fn test_axis_invert() {
        assert_eq!(Axis::Horizontal.invert(), Axis::Vertical);
        assert_eq!(Axis::Vertical.invert(), Axis::Horizontal);
    }

    #[test]
    fn test_half() {
        assert_eq!(px(100.0).half(), px(50.0));
        assert_eq!(100.0_f32.half(), 50.0);
    }

    #[test]
    fn test_negate() {
        assert_eq!(px(100.0).negate(), px(-100.0));
        assert_eq!(100.0_f32.negate(), -100.0);
    }

    #[test]
    fn test_is_zero() {
        assert!(px(0.0).is_zero());
        assert!(!px(1.0).is_zero());
        assert!(0.0_f32.is_zero());
        assert!(!1.0_f32.is_zero());
    }

    #[test]
    fn test_f32_unit() {
        assert_eq!(f32::zero(), 0.0);
        assert_eq!(NumericUnit::add(1.0, 2.0), 3.0);
        assert_eq!(NumericUnit::mul(2.0, 3.0), 6.0);
    }
}
