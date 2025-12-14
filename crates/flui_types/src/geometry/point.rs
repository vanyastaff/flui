//! Point type for coordinates in 2D space.
//!
//! API design inspired by kurbo, glam, and euclid.
//!
//! # Semantic Distinction
//!
//! - [`Point`]: Absolute position in coordinate system (location)
//! - [`Vec2`]: Direction and magnitude (displacement)
//!
//! # Operator Semantics
//!
//! ```text
//! Point - Point = Vec2  (displacement between positions)
//! Point + Vec2  = Point (translate position)
//! Point - Vec2  = Point (translate in opposite direction)
//! ```

use std::fmt;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use super::Vec2;

/// A point in 2D space.
///
/// This represents an absolute position, not a direction or displacement.
/// For vectors (direction + magnitude), use [`Vec2`].
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Point, Vec2};
///
/// let origin = Point::ORIGIN;
/// let target = Point::new(10.0, 20.0);
///
/// // Displacement between points
/// let displacement: Vec2 = target - origin;
///
/// // Move point by vector
/// let moved = origin + displacement;
/// assert_eq!(moved, target);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Point {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
}

// ============================================================================
// Constants
// ============================================================================

impl Point {
    /// The origin point (0, 0).
    pub const ORIGIN: Self = Self::new(0.0, 0.0);

    /// Alias for [`ORIGIN`](Self::ORIGIN).
    pub const ZERO: Self = Self::ORIGIN;

    /// Point at positive infinity.
    pub const INFINITY: Self = Self::new(f32::INFINITY, f32::INFINITY);

    /// Point at negative infinity.
    pub const NEG_INFINITY: Self = Self::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    /// Point with NaN coordinates.
    pub const NAN: Self = Self::new(f32::NAN, f32::NAN);
}

// ============================================================================
// Constructors
// ============================================================================

impl Point {
    /// Creates a new point.
    #[inline]
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Creates a point with both coordinates set to the same value.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p = Point::splat(5.0);
    /// assert_eq!(p, Point::new(5.0, 5.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn splat(v: f32) -> Self {
        Self::new(v, v)
    }

    /// Creates a point from an array.
    #[inline]
    #[must_use]
    pub const fn from_array(a: [f32; 2]) -> Self {
        Self::new(a[0], a[1])
    }

    /// Creates a point from a tuple.
    #[inline]
    #[must_use]
    pub const fn from_tuple(t: (f32, f32)) -> Self {
        Self::new(t.0, t.1)
    }
}

// ============================================================================
// Accessors & Conversion
// ============================================================================

impl Point {
    /// Returns the point as an array `[x, y]`.
    #[inline]
    #[must_use]
    pub const fn to_array(self) -> [f32; 2] {
        [self.x, self.y]
    }

    /// Returns the point as a tuple `(x, y)`.
    #[inline]
    #[must_use]
    pub const fn to_tuple(self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// Converts to a vector with same coordinates.
    ///
    /// This interprets the point coordinates as a displacement from origin.
    #[inline]
    #[must_use]
    pub const fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Returns a new point with the x coordinate replaced.
    #[inline]
    #[must_use]
    pub const fn with_x(self, x: f32) -> Self {
        Self::new(x, self.y)
    }

    /// Returns a new point with the y coordinate replaced.
    #[inline]
    #[must_use]
    pub const fn with_y(self, y: f32) -> Self {
        Self::new(self.x, y)
    }
}

// ============================================================================
// Geometric Operations
// ============================================================================

impl Point {
    /// Euclidean distance to another point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p1 = Point::new(0.0, 0.0);
    /// let p2 = Point::new(3.0, 4.0);
    /// assert_eq!(p1.distance(p2), 5.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        self.distance_squared(other).sqrt()
    }

    /// Squared euclidean distance to another point.
    ///
    /// This is faster than [`distance`](Self::distance) when you only need
    /// to compare distances (avoids the square root).
    #[inline]
    #[must_use]
    pub fn distance_squared(self, other: Self) -> f32 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        dx * dx + dy * dy
    }

    /// Midpoint between this point and another.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p1 = Point::new(0.0, 0.0);
    /// let p2 = Point::new(10.0, 20.0);
    /// assert_eq!(p1.midpoint(p2), Point::new(5.0, 10.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        Self::new((self.x + other.x) * 0.5, (self.y + other.y) * 0.5)
    }

    /// Linear interpolation between two points.
    ///
    /// - `t = 0.0` returns `self`
    /// - `t = 0.5` returns midpoint
    /// - `t = 1.0` returns `other`
    ///
    /// Values outside `[0, 1]` extrapolate beyond the points.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
}

// ============================================================================
// Component-wise Operations
// ============================================================================

impl Point {
    /// Component-wise minimum.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }

    /// Component-wise maximum.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
    }

    /// Clamp point coordinates between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(self.x.clamp(min.x, max.x), self.y.clamp(min.y, max.y))
    }

    /// Component-wise absolute value.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs())
    }

    /// Smallest component.
    #[inline]
    #[must_use]
    pub fn min_element(self) -> f32 {
        self.x.min(self.y)
    }

    /// Largest component.
    #[inline]
    #[must_use]
    pub fn max_element(self) -> f32 {
        self.x.max(self.y)
    }
}

// ============================================================================
// Rounding Operations (kurbo-style)
// ============================================================================

impl Point {
    /// Rounds coordinates to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    /// Rounds coordinates up (toward positive infinity).
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil())
    }

    /// Rounds coordinates down (toward negative infinity).
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    /// Rounds coordinates toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self::new(self.x.trunc(), self.y.trunc())
    }

    /// Rounds coordinates away from zero.
    #[inline]
    #[must_use]
    pub fn expand(self) -> Self {
        Self::new(
            if self.x >= 0.0 {
                self.x.ceil()
            } else {
                self.x.floor()
            },
            if self.y >= 0.0 {
                self.y.ceil()
            } else {
                self.y.floor()
            },
        )
    }

    /// Returns the fractional part of coordinates.
    #[inline]
    #[must_use]
    pub fn fract(self) -> Self {
        Self::new(self.x.fract(), self.y.fract())
    }
}

// ============================================================================
// Validation
// ============================================================================

impl Point {
    /// Returns `true` if both coordinates are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Returns `true` if either coordinate is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(self) -> bool {
        self.x.is_nan() || self.y.is_nan()
    }
}

// ============================================================================
// Operators: Point - Point = Vec2
// ============================================================================

impl Sub for Point {
    type Output = Vec2;

    /// Returns the displacement vector from `rhs` to `self`.
    #[inline]
    fn sub(self, rhs: Self) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

// ============================================================================
// Operators: Point ± Vec2 = Point
// ============================================================================

impl Add<Vec2> for Point {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Vec2) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign<Vec2> for Point {
    #[inline]
    fn add_assign(&mut self, rhs: Vec2) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub<Vec2> for Point {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Vec2) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign<Vec2> for Point {
    #[inline]
    fn sub_assign(&mut self, rhs: Vec2) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

// ============================================================================
// Operators: Scalar multiplication/division
// ============================================================================

impl Mul<f32> for Point {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Mul<Point> for f32 {
    type Output = Point;

    #[inline]
    fn mul(self, rhs: Point) -> Point {
        Point::new(self * rhs.x, self * rhs.y)
    }
}

impl MulAssign<f32> for Point {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<f32> for Point {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl DivAssign<f32> for Point {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Neg for Point {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl From<(f32, f32)> for Point {
    #[inline]
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

impl From<[f32; 2]> for Point {
    #[inline]
    fn from([x, y]: [f32; 2]) -> Self {
        Self::new(x, y)
    }
}

impl From<Point> for (f32, f32) {
    #[inline]
    fn from(p: Point) -> Self {
        (p.x, p.y)
    }
}

impl From<Point> for [f32; 2] {
    #[inline]
    fn from(p: Point) -> Self {
        [p.x, p.y]
    }
}

impl From<Vec2> for Point {
    #[inline]
    fn from(v: Vec2) -> Self {
        Self::new(v.x, v.y)
    }
}

// ============================================================================
// Display
// ============================================================================

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Point::new(x, y)`.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::point;
///
/// let p = point(10.0, 20.0);
/// ```
#[inline]
#[must_use]
pub const fn point(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construction() {
        let p = Point::new(10.0, 20.0);
        assert_eq!(p.x, 10.0);
        assert_eq!(p.y, 20.0);

        assert_eq!(Point::splat(5.0), Point::new(5.0, 5.0));
        assert_eq!(Point::from_array([1.0, 2.0]), Point::new(1.0, 2.0));
        assert_eq!(Point::from_tuple((3.0, 4.0)), Point::new(3.0, 4.0));
    }

    #[test]
    fn test_constants() {
        assert_eq!(Point::ORIGIN, Point::new(0.0, 0.0));
        assert_eq!(Point::ZERO, Point::ORIGIN);
        assert!(Point::INFINITY.x.is_infinite());
        assert!(Point::NAN.is_nan());
    }

    #[test]
    fn test_accessors() {
        let p = Point::new(10.0, 20.0);
        assert_eq!(p.to_array(), [10.0, 20.0]);
        assert_eq!(p.to_tuple(), (10.0, 20.0));
        assert_eq!(p.with_x(5.0), Point::new(5.0, 20.0));
        assert_eq!(p.with_y(5.0), Point::new(10.0, 5.0));
    }

    #[test]
    fn test_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);
        assert_eq!(p1.distance(p2), 5.0);
        assert_eq!(p1.distance_squared(p2), 25.0);
    }

    #[test]
    fn test_midpoint() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(10.0, 20.0);
        assert_eq!(p1.midpoint(p2), Point::new(5.0, 10.0));
    }

    #[test]
    fn test_lerp() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(10.0, 20.0);

        assert_eq!(p1.lerp(p2, 0.0), p1);
        assert_eq!(p1.lerp(p2, 0.5), Point::new(5.0, 10.0));
        assert_eq!(p1.lerp(p2, 1.0), p2);
    }

    #[test]
    fn test_min_max_clamp() {
        let p1 = Point::new(5.0, 15.0);
        let p2 = Point::new(10.0, 8.0);

        assert_eq!(p1.min(p2), Point::new(5.0, 8.0));
        assert_eq!(p1.max(p2), Point::new(10.0, 15.0));

        let p = Point::new(15.0, -5.0);
        let min = Point::ZERO;
        let max = Point::splat(10.0);
        assert_eq!(p.clamp(min, max), Point::new(10.0, 0.0));
    }

    #[test]
    fn test_rounding() {
        let p = Point::new(10.6, -3.3);
        assert_eq!(p.round(), Point::new(11.0, -3.0));
        assert_eq!(p.ceil(), Point::new(11.0, -3.0));
        assert_eq!(p.floor(), Point::new(10.0, -4.0));
        assert_eq!(p.trunc(), Point::new(10.0, -3.0));
        assert_eq!(p.expand(), Point::new(11.0, -4.0));
    }

    #[test]
    fn test_validation() {
        assert!(Point::new(1.0, 2.0).is_finite());
        assert!(!Point::INFINITY.is_finite());
        assert!(!Point::NAN.is_finite());
        assert!(Point::NAN.is_nan());
        assert!(!Point::ZERO.is_nan());
    }

    #[test]
    fn test_point_minus_point() {
        let p1 = Point::new(10.0, 20.0);
        let p2 = Point::new(3.0, 5.0);
        let v: Vec2 = p1 - p2;
        assert_eq!(v, Vec2::new(7.0, 15.0));
    }

    #[test]
    fn test_point_vec_ops() {
        let p = Point::new(10.0, 20.0);
        let v = Vec2::new(5.0, 10.0);

        assert_eq!(p + v, Point::new(15.0, 30.0));
        assert_eq!(p - v, Point::new(5.0, 10.0));

        let mut p2 = p;
        p2 += v;
        assert_eq!(p2, Point::new(15.0, 30.0));

        let mut p3 = p;
        p3 -= v;
        assert_eq!(p3, Point::new(5.0, 10.0));
    }

    #[test]
    fn test_scalar_ops() {
        let p = Point::new(10.0, 20.0);

        assert_eq!(p * 2.0, Point::new(20.0, 40.0));
        assert_eq!(2.0 * p, Point::new(20.0, 40.0));
        assert_eq!(p / 2.0, Point::new(5.0, 10.0));
        assert_eq!(-p, Point::new(-10.0, -20.0));
    }

    #[test]
    fn test_conversions() {
        let p = Point::new(10.0, 20.0);

        let from_tuple: Point = (10.0, 20.0).into();
        let from_array: Point = [10.0, 20.0].into();
        assert_eq!(from_tuple, p);
        assert_eq!(from_array, p);

        let to_tuple: (f32, f32) = p.into();
        let to_array: [f32; 2] = p.into();
        assert_eq!(to_tuple, (10.0, 20.0));
        assert_eq!(to_array, [10.0, 20.0]);

        let v = Vec2::new(5.0, 10.0);
        let p_from_v: Point = v.into();
        assert_eq!(p_from_v, Point::new(5.0, 10.0));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Point::new(10.5, 20.5)), "(10.5, 20.5)");
    }

    #[test]
    fn test_convenience_fn() {
        assert_eq!(point(1.0, 2.0), Point::new(1.0, 2.0));
    }
}
