//! Point types for coordinates in 2D space.
//!
//! This module provides type-safe point types representing absolute positions in 2D space.

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Represents a point in 2D space with absolute coordinates.
///
/// Semantic distinction:
/// - `Point`: Absolute position in coordinate system (x, y)
/// - `Offset`: Relative displacement/translation (dx, dy) - in flui_rendering
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X coordinate
    pub x: f32,
    /// Y coordinate
    pub y: f32,
}

impl Point {
    /// Origin point at (0, 0).
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    /// Point at positive infinity.
    pub const INFINITY: Point = Point {
        x: f32::INFINITY,
        y: f32::INFINITY,
    };

    /// Create a new point at the given coordinates.
    #[inline]
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Calculate distance from this point to another.
    #[inline]
    #[must_use]
    pub fn distance_to(&self, other: impl Into<Point>) -> f32 {
        let other = other.into();
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Calculate squared distance (faster, avoids sqrt).
    #[inline]
    #[must_use]
    pub fn distance_squared_to(&self, other: impl Into<Point>) -> f32 {
        let other = other.into();
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        dx * dx + dy * dy
    }

    /// Calculate the midpoint between this point and another.
    #[inline]
    #[must_use]
    pub fn midpoint(&self, other: impl Into<Point>) -> Point {
        let other = other.into();
        Point::new((self.x + other.x) * 0.5, (self.y + other.y) * 0.5)
    }

    /// Linear interpolation between two points.
    #[inline]
    #[must_use]
    pub fn lerp(a: impl Into<Point>, b: impl Into<Point>, t: f32) -> Point {
        let a = a.into();
        let b = b.into();
        Point {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }

    /// Clamp point to a rectangular region.
    #[inline]
    #[must_use]
    pub fn clamp(&self, min: impl Into<Point>, max: impl Into<Point>) -> Point {
        let min = min.into();
        let max = max.into();
        Point {
            x: self.x.clamp(min.x, max.x),
            y: self.y.clamp(min.y, max.y),
        }
    }

    /// Check if point is finite (not NaN or infinity).
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Get the minimum of two points (component-wise).
    #[inline]
    #[must_use]
    pub fn min(a: impl Into<Point>, b: impl Into<Point>) -> Point {
        let a = a.into();
        let b = b.into();
        Point {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
        }
    }

    /// Get the maximum of two points (component-wise).
    #[inline]
    #[must_use]
    pub fn max(a: impl Into<Point>, b: impl Into<Point>) -> Point {
        let a = a.into();
        let b = b.into();
        Point {
            x: a.x.max(b.x),
            y: a.y.max(b.y),
        }
    }

    /// Round coordinates to nearest integer.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Point {
        Point {
            x: self.x.round(),
            y: self.y.round(),
        }
    }

    /// Floor coordinates to integer.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Point {
        Point {
            x: self.x.floor(),
            y: self.y.floor(),
        }
    }

    /// Ceil coordinates to integer.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Point {
        Point {
            x: self.x.ceil(),
            y: self.y.ceil(),
        }
    }

    /// Returns the magnitude (length) of the vector from origin to this point.
    #[inline]
    #[must_use]
    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Returns the squared magnitude (avoids sqrt for performance).
    #[inline]
    #[must_use]
    pub fn magnitude_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// Normalizes this point as a vector (returns unit vector).
    ///
    /// Returns `Point::ZERO` if magnitude is zero.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> Point {
        let mag = self.magnitude();
        if mag > f32::EPSILON {
            Point::new(self.x / mag, self.y / mag)
        } else {
            Point::ZERO
        }
    }

    /// Returns the dot product with another point (treating as vectors).
    #[inline]
    #[must_use]
    pub fn dot(&self, other: impl Into<Point>) -> f32 {
        let other = other.into();
        self.x * other.x + self.y * other.y
    }
}

impl Default for Point {
    fn default() -> Self {
        Self::ZERO
    }
}

// Conversions from primitives
impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

impl From<[f32; 2]> for Point {
    fn from([x, y]: [f32; 2]) -> Self {
        Self::new(x, y)
    }
}

impl From<Point> for (f32, f32) {
    fn from(point: Point) -> Self {
        (point.x, point.y)
    }
}

impl From<Point> for [f32; 2] {
    fn from(point: Point) -> Self {
        [point.x, point.y]
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:.1}, {:.1})", self.x, self.y)
    }
}

// Math operators
impl Add for Point {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Point {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f32> for Point {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Mul<Point> for f32 {
    type Output = Point;

    #[inline]
    fn mul(self, rhs: Point) -> Self::Output {
        Point::new(rhs.x * self, rhs.y * self)
    }
}

impl Div<f32> for Point {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl Neg for Point {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_creation() {
        let point = Point::new(10.0, 20.0);
        assert_eq!(point.x, 10.0);
        assert_eq!(point.y, 20.0);

        assert_eq!(Point::ZERO.x, 0.0);
        assert_eq!(Point::ZERO.y, 0.0);
    }

    #[test]
    fn test_point_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);

        assert_eq!(p1.distance_to(p2), 5.0);
        assert_eq!(p1.distance_squared_to(p2), 25.0);
    }

    #[test]
    fn test_point_midpoint() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(10.0, 20.0);

        let mid = p1.midpoint(p2);
        assert_eq!(mid, Point::new(5.0, 10.0));
    }

    #[test]
    fn test_point_lerp() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(10.0, 20.0);

        let mid = Point::lerp(p1, p2, 0.5);
        assert_eq!(mid, Point::new(5.0, 10.0));

        let start = Point::lerp(p1, p2, 0.0);
        assert_eq!(start, p1);

        let end = Point::lerp(p1, p2, 1.0);
        assert_eq!(end, p2);
    }

    #[test]
    fn test_point_clamp() {
        let point = Point::new(15.0, -5.0);
        let min = Point::new(0.0, 0.0);
        let max = Point::new(10.0, 10.0);

        let clamped = point.clamp(min, max);
        assert_eq!(clamped, Point::new(10.0, 0.0));
    }

    #[test]
    fn test_point_min_max() {
        let p1 = Point::new(5.0, 15.0);
        let p2 = Point::new(10.0, 8.0);

        let min = Point::min(p1, p2);
        assert_eq!(min, Point::new(5.0, 8.0));

        let max = Point::max(p1, p2);
        assert_eq!(max, Point::new(10.0, 15.0));
    }

    #[test]
    fn test_point_rounding() {
        let point = Point::new(10.6, 20.3);

        assert_eq!(point.round(), Point::new(11.0, 20.0));
        assert_eq!(point.floor(), Point::new(10.0, 20.0));
        assert_eq!(point.ceil(), Point::new(11.0, 21.0));
    }

    #[test]
    fn test_point_conversions() {
        // From tuple
        let from_tuple: Point = (10.0, 20.0).into();
        assert_eq!(from_tuple, Point::new(10.0, 20.0));

        // To tuple
        let to_tuple: (f32, f32) = Point::new(10.0, 20.0).into();
        assert_eq!(to_tuple, (10.0, 20.0));

        // From array
        let from_array: Point = [10.0, 20.0].into();
        assert_eq!(from_array, Point::new(10.0, 20.0));

        // To array
        let to_array: [f32; 2] = Point::new(10.0, 20.0).into();
        assert_eq!(to_array, [10.0, 20.0]);
    }

    #[test]
    fn test_point_display() {
        let point = Point::new(10.5, 20.7);
        assert_eq!(format!("{}", point), "(10.5, 20.7)");
    }

    #[test]
    fn test_point_is_finite() {
        assert!(Point::new(10.0, 20.0).is_finite());
        assert!(!Point::INFINITY.is_finite());
        assert!(!Point::new(f32::NAN, 10.0).is_finite());
    }

    #[test]
    fn test_point_math_operators() {
        let p1 = Point::new(10.0, 20.0);
        let p2 = Point::new(5.0, 8.0);

        // Addition
        assert_eq!(p1 + p2, Point::new(15.0, 28.0));

        // Subtraction
        assert_eq!(p1 - p2, Point::new(5.0, 12.0));

        // Multiplication by scalar
        assert_eq!(p1 * 2.0, Point::new(20.0, 40.0));
        assert_eq!(2.0 * p1, Point::new(20.0, 40.0));

        // Division by scalar
        assert_eq!(p1 / 2.0, Point::new(5.0, 10.0));

        // Negation
        assert_eq!(-p1, Point::new(-10.0, -20.0));
    }

    #[test]
    fn test_point_magnitude() {
        let p = Point::new(3.0, 4.0);
        assert_eq!(p.magnitude(), 5.0);
        assert_eq!(p.magnitude_squared(), 25.0);
    }

    #[test]
    fn test_point_normalize() {
        let p = Point::new(3.0, 4.0);
        let normalized = p.normalize();

        assert!((normalized.magnitude() - 1.0).abs() < 0.0001);
        assert!((normalized.x - 0.6).abs() < 0.0001);
        assert!((normalized.y - 0.8).abs() < 0.0001);

        // Zero vector should normalize to zero
        assert_eq!(Point::ZERO.normalize(), Point::ZERO);
    }

    #[test]
    fn test_point_dot_product() {
        let p1 = Point::new(2.0, 3.0);
        let p2 = Point::new(4.0, 5.0);

        assert_eq!(p1.dot(p2), 23.0); // 2*4 + 3*5 = 23

        // Perpendicular vectors
        let p3 = Point::new(1.0, 0.0);
        let p4 = Point::new(0.0, 1.0);
        assert_eq!(p3.dot(p4), 0.0);
    }
}
