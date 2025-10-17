//! Point types for coordinates in 2D space.
//!
//! This module provides type-safe point types representing absolute positions in 2D space.

use super::offset::Offset;
use egui::{Pos2, Vec2};

/// Represents a point in 2D space with absolute coordinates.
///
/// Semantic distinction:
/// - `Point`: Absolute position in coordinate system (x, y)
/// - `Offset`: Relative displacement/translation (dx, dy)
/// - `Position`: CSS-like positioning with optional edges
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
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
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Calculate distance from this point to another.
    pub fn distance_to(&self, other: impl Into<Point>) -> f32 {
        let other = other.into();
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Calculate squared distance (faster, avoids sqrt).
    pub fn distance_squared_to(&self, other: impl Into<Point>) -> f32 {
        let other = other.into();
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        dx * dx + dy * dy
    }

    /// Calculate the offset/displacement from this point to another.
    pub fn offset_to(&self, other: impl Into<Point>) -> Offset {
        let other = other.into();
        Offset::new(other.x - self.x, other.y - self.y)
    }

    /// Calculate the midpoint between this point and another.
    pub fn midpoint(&self, other: impl Into<Point>) -> Point {
        let other = other.into();
        Point::new((self.x + other.x) * 0.5, (self.y + other.y) * 0.5)
    }

    /// Linear interpolation between two points.
    pub fn lerp(a: impl Into<Point>, b: impl Into<Point>, t: f32) -> Point {
        let a = a.into();
        let b = b.into();
        Point {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }

    /// Clamp point to a rectangular region.
    pub fn clamp(&self, min: impl Into<Point>, max: impl Into<Point>) -> Point {
        let min = min.into();
        let max = max.into();
        Point {
            x: self.x.clamp(min.x, max.x),
            y: self.y.clamp(min.y, max.y),
        }
    }

    /// Check if point is finite (not NaN or infinity).
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Get the minimum of two points (component-wise).
    pub fn min(a: impl Into<Point>, b: impl Into<Point>) -> Point {
        let a = a.into();
        let b = b.into();
        Point {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
        }
    }

    /// Get the maximum of two points (component-wise).
    pub fn max(a: impl Into<Point>, b: impl Into<Point>) -> Point {
        let a = a.into();
        let b = b.into();
        Point {
            x: a.x.max(b.x),
            y: a.y.max(b.y),
        }
    }

    /// Round coordinates to nearest integer.
    pub fn round(&self) -> Point {
        Point {
            x: self.x.round(),
            y: self.y.round(),
        }
    }

    /// Floor coordinates to integer.
    pub fn floor(&self) -> Point {
        Point {
            x: self.x.floor(),
            y: self.y.floor(),
        }
    }

    /// Ceil coordinates to integer.
    pub fn ceil(&self) -> Point {
        Point {
            x: self.x.ceil(),
            y: self.y.ceil(),
        }
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

// Conversions with egui types
impl From<Pos2> for Point {
    fn from(pos: Pos2) -> Self {
        Self::new(pos.x, pos.y)
    }
}

impl From<Point> for Pos2 {
    fn from(point: Point) -> Self {
        Pos2::new(point.x, point.y)
    }
}

impl From<Vec2> for Point {
    fn from(vec: Vec2) -> Self {
        Self::new(vec.x, vec.y)
    }
}

impl From<Point> for Vec2 {
    fn from(point: Point) -> Self {
        Vec2::new(point.x, point.y)
    }
}

// Point + Offset = Point (translate point by offset)
impl std::ops::Add<Offset> for Point {
    type Output = Point;

    fn add(self, offset: Offset) -> Self::Output {
        Point {
            x: self.x + offset.dx,
            y: self.y + offset.dy,
        }
    }
}

impl std::ops::AddAssign<Offset> for Point {
    fn add_assign(&mut self, offset: Offset) {
        self.x += offset.dx;
        self.y += offset.dy;
    }
}

// Point - Offset = Point (translate point by negative offset)
impl std::ops::Sub<Offset> for Point {
    type Output = Point;

    fn sub(self, offset: Offset) -> Self::Output {
        Point {
            x: self.x - offset.dx,
            y: self.y - offset.dy,
        }
    }
}

impl std::ops::SubAssign<Offset> for Point {
    fn sub_assign(&mut self, offset: Offset) {
        self.x -= offset.dx;
        self.y -= offset.dy;
    }
}

// Point - Point = Offset (displacement between two points)
impl std::ops::Sub<Point> for Point {
    type Output = Offset;

    fn sub(self, other: Point) -> Self::Output {
        Offset::new(self.x - other.x, self.y - other.y)
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.1}, {:.1})", self.x, self.y)
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
    fn test_point_offset_to() {
        let p1 = Point::new(10.0, 20.0);
        let p2 = Point::new(15.0, 25.0);

        let offset = p1.offset_to(p2);
        assert_eq!(offset, Offset::new(5.0, 5.0));
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
    fn test_point_arithmetic() {
        let point = Point::new(10.0, 20.0);
        let offset = Offset::new(5.0, 3.0);

        // Addition
        let translated = point + offset;
        assert_eq!(translated, Point::new(15.0, 23.0));

        // Subtraction
        let back = translated - offset;
        assert_eq!(back, point);

        // Point - Point = Offset
        let diff = translated - point;
        assert_eq!(diff, offset);
    }

    #[test]
    fn test_point_add_assign() {
        let mut point = Point::new(10.0, 20.0);
        point += Offset::new(5.0, 3.0);
        assert_eq!(point, Point::new(15.0, 23.0));
    }

    #[test]
    fn test_point_sub_assign() {
        let mut point = Point::new(10.0, 20.0);
        point -= Offset::new(5.0, 3.0);
        assert_eq!(point, Point::new(5.0, 17.0));
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
    fn test_point_egui_conversions() {
        use egui::{Pos2, Vec2};

        // From Pos2
        let pos = Pos2::new(10.0, 20.0);
        let point: Point = pos.into();
        assert_eq!(point, Point::new(10.0, 20.0));

        // To Pos2
        let back: Pos2 = point.into();
        assert_eq!(back, pos);

        // From Vec2
        let vec = Vec2::new(5.0, 7.0);
        let point2: Point = vec.into();
        assert_eq!(point2, Point::new(5.0, 7.0));
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
}
