//! Size type for 2D dimensions.
//!
//! API design inspired by kurbo, glam, and Flutter.

use std::fmt;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

use super::Vec2;

/// A 2D size with width and height.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::Size;
///
/// let size = Size::new(800.0, 600.0);
/// assert_eq!(size.area(), 480000.0);
/// assert_eq!(size.aspect_ratio(), 800.0 / 600.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Size {
    /// Width dimension.
    pub width: f32,
    /// Height dimension.
    pub height: f32,
}

// ============================================================================
// Constants
// ============================================================================

impl Size {
    /// Zero size (0, 0).
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Infinite size.
    pub const INFINITY: Self = Self::new(f32::INFINITY, f32::INFINITY);

    /// NaN size.
    pub const NAN: Self = Self::new(f32::NAN, f32::NAN);
}

// ============================================================================
// Constructors
// ============================================================================

impl Size {
    /// Creates a new size.
    #[inline]
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Creates a square size (width == height).
    #[inline]
    #[must_use]
    pub const fn splat(v: f32) -> Self {
        Self::new(v, v)
    }

    /// Creates a size from an array.
    #[inline]
    #[must_use]
    pub const fn from_array(a: [f32; 2]) -> Self {
        Self::new(a[0], a[1])
    }

    /// Creates a size from a tuple.
    #[inline]
    #[must_use]
    pub const fn from_tuple(t: (f32, f32)) -> Self {
        Self::new(t.0, t.1)
    }
}

// ============================================================================
// Accessors & Conversion
// ============================================================================

impl Size {
    /// Returns the size as an array `[width, height]`.
    #[inline]
    #[must_use]
    pub const fn to_array(self) -> [f32; 2] {
        [self.width, self.height]
    }

    /// Returns the size as a tuple `(width, height)`.
    #[inline]
    #[must_use]
    pub const fn to_tuple(self) -> (f32, f32) {
        (self.width, self.height)
    }

    /// Converts to a vector.
    #[inline]
    #[must_use]
    pub const fn to_vec2(self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    /// Returns a new size with the width replaced.
    #[inline]
    #[must_use]
    pub const fn with_width(self, width: f32) -> Self {
        Self::new(width, self.height)
    }

    /// Returns a new size with the height replaced.
    #[inline]
    #[must_use]
    pub const fn with_height(self, height: f32) -> Self {
        Self::new(self.width, height)
    }
}

// ============================================================================
// Dimensions
// ============================================================================

impl Size {
    /// Returns the area (width × height).
    #[inline]
    #[must_use]
    pub fn area(self) -> f32 {
        self.width * self.height
    }

    /// Returns `true` if the area is zero or negative.
    #[inline]
    #[must_use]
    pub fn is_zero_area(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Returns `true` if both dimensions are zero (or very close).
    #[inline]
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.width.abs() < f32::EPSILON && self.height.abs() < f32::EPSILON
    }

    /// Returns the smaller dimension.
    #[inline]
    #[must_use]
    pub fn min_side(self) -> f32 {
        self.width.min(self.height)
    }

    /// Returns the larger dimension.
    #[inline]
    #[must_use]
    pub fn max_side(self) -> f32 {
        self.width.max(self.height)
    }

    /// Returns the aspect ratio (width / height).
    ///
    /// Returns 0.0 if height is zero.
    #[inline]
    #[must_use]
    pub fn aspect_ratio(self) -> f32 {
        if self.height.abs() < f32::EPSILON {
            0.0
        } else {
            self.width / self.height
        }
    }
}

// ============================================================================
// Component-wise Operations
// ============================================================================

impl Size {
    /// Component-wise minimum.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self::new(self.width.min(other.width), self.height.min(other.height))
    }

    /// Component-wise maximum.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self::new(self.width.max(other.width), self.height.max(other.height))
    }

    /// Clamp size between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(
            self.width.clamp(min.width, max.width),
            self.height.clamp(min.height, max.height),
        )
    }

    /// Returns size with width and height swapped.
    #[inline]
    #[must_use]
    pub const fn transpose(self) -> Self {
        Self::new(self.height, self.width)
    }
}

// ============================================================================
// Rounding Operations
// ============================================================================

impl Size {
    /// Rounds dimensions to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.width.round(), self.height.round())
    }

    /// Rounds dimensions up.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.width.ceil(), self.height.ceil())
    }

    /// Rounds dimensions down.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.width.floor(), self.height.floor())
    }

    /// Rounds dimensions toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self::new(self.width.trunc(), self.height.trunc())
    }

    /// Rounds dimensions away from zero.
    #[inline]
    #[must_use]
    pub fn expand(self) -> Self {
        Self::new(
            if self.width >= 0.0 {
                self.width.ceil()
            } else {
                self.width.floor()
            },
            if self.height >= 0.0 {
                self.height.ceil()
            } else {
                self.height.floor()
            },
        )
    }
}

// ============================================================================
// Validation
// ============================================================================

impl Size {
    /// Returns `true` if both dimensions are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.width.is_finite() && self.height.is_finite()
    }

    /// Returns `true` if either dimension is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(self) -> bool {
        self.width.is_nan() || self.height.is_nan()
    }

    /// Returns `true` if both dimensions are positive.
    #[inline]
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

// ============================================================================
// Interpolation & Fitting
// ============================================================================

impl Size {
    /// Linear interpolation between two sizes.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.width + (other.width - self.width) * t,
            self.height + (other.height - self.height) * t,
        )
    }

    /// Scales to fit within bounds while maintaining aspect ratio.
    ///
    /// Returns the largest size that fits completely within `bounds`.
    /// Equivalent to `BoxFit::contain`.
    #[must_use]
    pub fn fit_within(self, bounds: Self) -> Self {
        if self.is_zero_area() || bounds.is_zero_area() {
            return Self::ZERO;
        }
        let scale = (bounds.width / self.width).min(bounds.height / self.height);
        Self::new(self.width * scale, self.height * scale)
    }

    /// Scales to fill bounds while maintaining aspect ratio.
    ///
    /// Returns the smallest size that completely covers `bounds`.
    /// Equivalent to `BoxFit::cover`.
    #[must_use]
    pub fn fill_bounds(self, bounds: Self) -> Self {
        if self.is_zero_area() || bounds.is_zero_area() {
            return Self::ZERO;
        }
        let scale = (bounds.width / self.width).max(bounds.height / self.height);
        Self::new(self.width * scale, self.height * scale)
    }

    /// Adjusts height to match a specific aspect ratio.
    #[inline]
    #[must_use]
    pub fn with_aspect_ratio(self, ratio: f32) -> Self {
        if ratio <= 0.0 {
            self
        } else {
            Self::new(self.width, self.width / ratio)
        }
    }
}

// ============================================================================
// Operators
// ============================================================================

impl Add for Size {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.width + rhs.width, self.height + rhs.height)
    }
}

impl AddAssign for Size {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.width += rhs.width;
        self.height += rhs.height;
    }
}

impl Sub for Size {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.width - rhs.width, self.height - rhs.height)
    }
}

impl SubAssign for Size {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.width -= rhs.width;
        self.height -= rhs.height;
    }
}

impl Mul<f32> for Size {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.width * rhs, self.height * rhs)
    }
}

impl Mul<Size> for f32 {
    type Output = Size;

    #[inline]
    fn mul(self, rhs: Size) -> Size {
        Size::new(self * rhs.width, self * rhs.height)
    }
}

impl MulAssign<f32> for Size {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.width *= rhs;
        self.height *= rhs;
    }
}

impl Div<f32> for Size {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self {
        Self::new(self.width / rhs, self.height / rhs)
    }
}

impl DivAssign<f32> for Size {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.width /= rhs;
        self.height /= rhs;
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl From<(f32, f32)> for Size {
    #[inline]
    fn from((width, height): (f32, f32)) -> Self {
        Self::new(width, height)
    }
}

impl From<[f32; 2]> for Size {
    #[inline]
    fn from([width, height]: [f32; 2]) -> Self {
        Self::new(width, height)
    }
}

impl From<Size> for (f32, f32) {
    #[inline]
    fn from(s: Size) -> Self {
        (s.width, s.height)
    }
}

impl From<Size> for [f32; 2] {
    #[inline]
    fn from(s: Size) -> Self {
        [s.width, s.height]
    }
}

impl From<Vec2> for Size {
    #[inline]
    fn from(v: Vec2) -> Self {
        Self::new(v.x, v.y)
    }
}

// ============================================================================
// Display
// ============================================================================

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Size::new(width, height)`.
#[inline]
#[must_use]
pub const fn size(width: f32, height: f32) -> Size {
    Size::new(width, height)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construction() {
        let s = Size::new(800.0, 600.0);
        assert_eq!(s.width, 800.0);
        assert_eq!(s.height, 600.0);

        assert_eq!(Size::splat(10.0), Size::new(10.0, 10.0));
        assert_eq!(Size::from_array([1.0, 2.0]), Size::new(1.0, 2.0));
        assert_eq!(Size::from_tuple((3.0, 4.0)), Size::new(3.0, 4.0));
    }

    #[test]
    fn test_constants() {
        assert_eq!(Size::ZERO, Size::new(0.0, 0.0));
        assert!(Size::INFINITY.width.is_infinite());
        assert!(Size::NAN.is_nan());
    }

    #[test]
    fn test_dimensions() {
        let s = Size::new(100.0, 50.0);
        assert_eq!(s.area(), 5000.0);
        assert_eq!(s.min_side(), 50.0);
        assert_eq!(s.max_side(), 100.0);
        assert_eq!(s.aspect_ratio(), 2.0);
    }

    #[test]
    fn test_zero_checks() {
        assert!(Size::ZERO.is_zero());
        assert!(Size::ZERO.is_zero_area());
        assert!(Size::new(0.0, 100.0).is_zero_area());
        assert!(Size::new(-5.0, 100.0).is_zero_area());
        assert!(!Size::new(10.0, 20.0).is_zero_area());
    }

    #[test]
    fn test_min_max_clamp() {
        let s1 = Size::new(100.0, 50.0);
        let s2 = Size::new(80.0, 60.0);

        assert_eq!(s1.min(s2), Size::new(80.0, 50.0));
        assert_eq!(s1.max(s2), Size::new(100.0, 60.0));

        let s = Size::new(150.0, 30.0);
        let clamped = s.clamp(Size::splat(50.0), Size::splat(100.0));
        assert_eq!(clamped, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_transpose() {
        let s = Size::new(100.0, 50.0);
        assert_eq!(s.transpose(), Size::new(50.0, 100.0));
    }

    #[test]
    fn test_rounding() {
        let s = Size::new(10.6, 20.3);
        assert_eq!(s.round(), Size::new(11.0, 20.0));
        assert_eq!(s.ceil(), Size::new(11.0, 21.0));
        assert_eq!(s.floor(), Size::new(10.0, 20.0));
    }

    #[test]
    fn test_validation() {
        assert!(Size::new(10.0, 20.0).is_finite());
        assert!(!Size::INFINITY.is_finite());
        assert!(Size::NAN.is_nan());
        assert!(Size::new(10.0, 20.0).is_positive());
        assert!(!Size::new(-5.0, 20.0).is_positive());
    }

    #[test]
    fn test_lerp() {
        let a = Size::ZERO;
        let b = Size::new(100.0, 200.0);

        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 0.5), Size::new(50.0, 100.0));
        assert_eq!(a.lerp(b, 1.0), b);
    }

    #[test]
    fn test_fit_fill() {
        let image = Size::new(1920.0, 1080.0); // 16:9
        let bounds = Size::new(800.0, 600.0); // 4:3

        let fitted = image.fit_within(bounds);
        assert!(fitted.width <= bounds.width + 0.01);
        assert!(fitted.height <= bounds.height + 0.01);

        let filled = image.fill_bounds(bounds);
        assert!(filled.width >= bounds.width - 0.01);
        assert!(filled.height >= bounds.height - 0.01);
    }

    #[test]
    fn test_aspect_ratio_set() {
        let s = Size::new(1920.0, 0.0);
        let adjusted = s.with_aspect_ratio(16.0 / 9.0);
        assert_eq!(adjusted.width, 1920.0);
        assert!((adjusted.height - 1080.0).abs() < 0.1);
    }

    #[test]
    fn test_operators() {
        let s1 = Size::new(100.0, 50.0);
        let s2 = Size::new(30.0, 20.0);

        assert_eq!(s1 + s2, Size::new(130.0, 70.0));
        assert_eq!(s1 - s2, Size::new(70.0, 30.0));
        assert_eq!(s1 * 2.0, Size::new(200.0, 100.0));
        assert_eq!(2.0 * s1, Size::new(200.0, 100.0));
        assert_eq!(s1 / 2.0, Size::new(50.0, 25.0));
    }

    #[test]
    fn test_assign_operators() {
        let mut s = Size::new(100.0, 50.0);

        s += Size::new(10.0, 5.0);
        assert_eq!(s, Size::new(110.0, 55.0));

        s -= Size::new(10.0, 5.0);
        assert_eq!(s, Size::new(100.0, 50.0));

        s *= 2.0;
        assert_eq!(s, Size::new(200.0, 100.0));

        s /= 2.0;
        assert_eq!(s, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_conversions() {
        let s = Size::new(100.0, 50.0);

        let from_tuple: Size = (100.0, 50.0).into();
        let from_array: Size = [100.0, 50.0].into();
        assert_eq!(from_tuple, s);
        assert_eq!(from_array, s);

        let to_tuple: (f32, f32) = s.into();
        let to_array: [f32; 2] = s.into();
        assert_eq!(to_tuple, (100.0, 50.0));
        assert_eq!(to_array, [100.0, 50.0]);

        let v = Vec2::new(10.0, 20.0);
        let s_from_v: Size = v.into();
        assert_eq!(s_from_v, Size::new(10.0, 20.0));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Size::new(800.0, 600.0)), "800×600");
    }

    #[test]
    fn test_convenience_fn() {
        assert_eq!(size(100.0, 50.0), Size::new(100.0, 50.0));
    }
}
