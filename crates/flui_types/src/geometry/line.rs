//! Line segment type.
//!
//! A line segment defined by two endpoints.

use std::fmt;

use super::{Point, Rect, Vec2};

/// A line segment defined by two endpoints.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Line, Point};
///
/// let line = Line::new(Point::new(0.0, 0.0), Point::new(100.0, 100.0));
/// assert_eq!(line.length(), 100.0 * std::f32::consts::SQRT_2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Line {
    /// Start point.
    pub p0: Point,
    /// End point.
    pub p1: Point,
}

// ============================================================================
// Constructors
// ============================================================================

impl Line {
    /// Creates a new line from two points.
    #[inline]
    #[must_use]
    pub const fn new(p0: Point, p1: Point) -> Self {
        Self { p0, p1 }
    }

    /// Creates a line from coordinates.
    #[inline]
    #[must_use]
    pub const fn from_coords(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            p0: Point::new(x0, y0),
            p1: Point::new(x1, y1),
        }
    }

    /// Creates a horizontal line.
    #[inline]
    #[must_use]
    pub const fn horizontal(y: f32, x0: f32, x1: f32) -> Self {
        Self::from_coords(x0, y, x1, y)
    }

    /// Creates a vertical line.
    #[inline]
    #[must_use]
    pub const fn vertical(x: f32, y0: f32, y1: f32) -> Self {
        Self::from_coords(x, y0, x, y1)
    }
}

// ============================================================================
// Accessors
// ============================================================================

impl Line {
    /// Returns the start point.
    #[inline]
    #[must_use]
    pub const fn start(&self) -> Point {
        self.p0
    }

    /// Returns the end point.
    #[inline]
    #[must_use]
    pub const fn end(&self) -> Point {
        self.p1
    }

    /// Returns the length of the line.
    #[inline]
    #[must_use]
    pub fn length(&self) -> f32 {
        self.p0.distance(self.p1)
    }

    /// Returns the squared length of the line.
    #[inline]
    #[must_use]
    pub fn length_squared(&self) -> f32 {
        self.p0.distance_squared(self.p1)
    }

    /// Returns the direction vector (unnormalized).
    #[inline]
    #[must_use]
    pub fn to_vec(&self) -> Vec2 {
        self.p1 - self.p0
    }

    /// Returns the normalized direction vector.
    ///
    /// Returns zero vector if line has zero length.
    #[inline]
    #[must_use]
    pub fn direction(&self) -> Vec2 {
        self.to_vec().normalize_or(Vec2::ZERO)
    }

    /// Returns the midpoint of the line.
    #[inline]
    #[must_use]
    pub fn midpoint(&self) -> Point {
        self.p0.midpoint(self.p1)
    }

    /// Returns the bounding box of the line.
    #[inline]
    #[must_use]
    pub fn bounding_box(&self) -> Rect {
        Rect::from_points(self.p0, self.p1)
    }
}

// ============================================================================
// Queries
// ============================================================================

impl Line {
    /// Returns `true` if the line has zero length.
    #[inline]
    #[must_use]
    pub fn is_zero_length(&self) -> bool {
        self.p0 == self.p1
    }

    /// Returns `true` if the line is horizontal (within epsilon).
    #[inline]
    #[must_use]
    pub fn is_horizontal(&self) -> bool {
        (self.p0.y - self.p1.y).abs() < f32::EPSILON
    }

    /// Returns `true` if the line is vertical (within epsilon).
    #[inline]
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        (self.p0.x - self.p1.x).abs() < f32::EPSILON
    }

    /// Returns the point at parameter t (0.0 = p0, 1.0 = p1).
    #[inline]
    #[must_use]
    pub fn eval(&self, t: f32) -> Point {
        self.p0.lerp(self.p1, t)
    }

    /// Returns the closest point on the line segment to the given point.
    #[must_use]
    pub fn nearest_point(&self, point: Point) -> Point {
        let d = self.to_vec();
        let length_sq = d.length_squared();

        if length_sq < f32::EPSILON {
            return self.p0;
        }

        let t = ((point - self.p0).dot(d) / length_sq).clamp(0.0, 1.0);
        self.eval(t)
    }

    /// Returns the distance from the point to the line segment.
    #[inline]
    #[must_use]
    pub fn distance_to_point(&self, point: Point) -> f32 {
        point.distance(self.nearest_point(point))
    }

    /// Returns the squared distance from the point to the line segment.
    #[inline]
    #[must_use]
    pub fn distance_squared_to_point(&self, point: Point) -> f32 {
        point.distance_squared(self.nearest_point(point))
    }

    /// Checks if a point is within the given distance of the line segment.
    ///
    /// Useful for hit testing with a tolerance.
    #[inline]
    #[must_use]
    pub fn is_point_near(&self, point: Point, tolerance: f32) -> bool {
        self.distance_squared_to_point(point) <= tolerance * tolerance
    }
}

// ============================================================================
// Transformations
// ============================================================================

impl Line {
    /// Returns a new line translated by the given vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2) -> Self {
        Self {
            p0: self.p0 + offset,
            p1: self.p1 + offset,
        }
    }

    /// Returns a new line with reversed direction.
    #[inline]
    #[must_use]
    pub const fn reverse(&self) -> Self {
        Self {
            p0: self.p1,
            p1: self.p0,
        }
    }

    /// Linear interpolation between two lines.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            p0: self.p0.lerp(other.p0, t),
            p1: self.p1.lerp(other.p1, t),
        }
    }
}

// ============================================================================
// Intersections
// ============================================================================

impl Line {
    /// Returns the intersection point of two lines (as infinite lines).
    ///
    /// Returns `None` if lines are parallel.
    #[must_use]
    pub fn intersect_line(&self, other: &Line) -> Option<Point> {
        let d1 = self.to_vec();
        let d2 = other.to_vec();

        let cross = d1.cross(d2);
        if cross.abs() < f32::EPSILON {
            return None; // Parallel
        }

        let d = other.p0 - self.p0;
        let t = d.cross(d2) / cross;

        Some(self.eval(t))
    }

    /// Returns the intersection point of two line segments.
    ///
    /// Returns `None` if segments don't intersect.
    #[must_use]
    pub fn intersect_segment(&self, other: &Line) -> Option<Point> {
        let d1 = self.to_vec();
        let d2 = other.to_vec();

        let cross = d1.cross(d2);
        if cross.abs() < f32::EPSILON {
            return None; // Parallel
        }

        let d = other.p0 - self.p0;
        let t1 = d.cross(d2) / cross;
        let t2 = d.cross(d1) / cross;

        if t1 >= 0.0 && t1 <= 1.0 && t2 >= 0.0 && t2 <= 1.0 {
            Some(self.eval(t1))
        } else {
            None
        }
    }
}

// ============================================================================
// Display
// ============================================================================

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Line({} -> {})", self.p0, self.p1)
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Line::new(p0, p1)`.
#[inline]
#[must_use]
pub fn line(p0: Point, p1: Point) -> Line {
    Line::new(p0, p1)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construction() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert_eq!(line.p0, Point::new(0.0, 0.0));
        assert_eq!(line.p1, Point::new(10.0, 0.0));

        let line2 = Line::from_coords(0.0, 0.0, 10.0, 0.0);
        assert_eq!(line, line2);
    }

    #[test]
    fn test_horizontal_vertical() {
        let h = Line::horizontal(5.0, 0.0, 10.0);
        assert!(h.is_horizontal());
        assert!(!h.is_vertical());

        let v = Line::vertical(5.0, 0.0, 10.0);
        assert!(v.is_vertical());
        assert!(!v.is_horizontal());
    }

    #[test]
    fn test_length() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(3.0, 4.0));
        assert_eq!(line.length(), 5.0);
        assert_eq!(line.length_squared(), 25.0);
    }

    #[test]
    fn test_midpoint() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert_eq!(line.midpoint(), Point::new(5.0, 5.0));
    }

    #[test]
    fn test_eval() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert_eq!(line.eval(0.0), Point::new(0.0, 0.0));
        assert_eq!(line.eval(0.5), Point::new(5.0, 0.0));
        assert_eq!(line.eval(1.0), Point::new(10.0, 0.0));
    }

    #[test]
    fn test_nearest_point() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));

        // Point on line
        assert_eq!(
            line.nearest_point(Point::new(5.0, 0.0)),
            Point::new(5.0, 0.0)
        );

        // Point above line
        assert_eq!(
            line.nearest_point(Point::new(5.0, 5.0)),
            Point::new(5.0, 0.0)
        );

        // Point before start
        assert_eq!(
            line.nearest_point(Point::new(-5.0, 0.0)),
            Point::new(0.0, 0.0)
        );

        // Point after end
        assert_eq!(
            line.nearest_point(Point::new(15.0, 0.0)),
            Point::new(10.0, 0.0)
        );
    }

    #[test]
    fn test_distance_to_point() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert_eq!(line.distance_to_point(Point::new(5.0, 3.0)), 3.0);
    }

    #[test]
    fn test_is_point_near() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert!(line.is_point_near(Point::new(5.0, 2.0), 3.0));
        assert!(!line.is_point_near(Point::new(5.0, 5.0), 3.0));
    }

    #[test]
    fn test_bounding_box() {
        let line = Line::new(Point::new(10.0, 20.0), Point::new(30.0, 40.0));
        let bbox = line.bounding_box();
        assert_eq!(bbox.min, Point::new(10.0, 20.0));
        assert_eq!(bbox.max, Point::new(30.0, 40.0));
    }

    #[test]
    fn test_translate() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        let translated = line.translate(Vec2::new(5.0, 5.0));
        assert_eq!(translated.p0, Point::new(5.0, 5.0));
        assert_eq!(translated.p1, Point::new(15.0, 15.0));
    }

    #[test]
    fn test_reverse() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        let reversed = line.reverse();
        assert_eq!(reversed.p0, Point::new(10.0, 10.0));
        assert_eq!(reversed.p1, Point::new(0.0, 0.0));
    }

    #[test]
    fn test_intersect_segment() {
        let line1 = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        let line2 = Line::new(Point::new(0.0, 10.0), Point::new(10.0, 0.0));

        let intersection = line1.intersect_segment(&line2);
        assert!(intersection.is_some());
        let p = intersection.unwrap();
        assert!((p.x - 5.0).abs() < 0.001);
        assert!((p.y - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_intersect_parallel() {
        let line1 = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        let line2 = Line::new(Point::new(0.0, 5.0), Point::new(10.0, 5.0));

        assert!(line1.intersect_segment(&line2).is_none());
    }

    #[test]
    fn test_direction() {
        let line = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        let dir = line.direction();
        assert!((dir.x - 1.0).abs() < f32::EPSILON);
        assert!(dir.y.abs() < f32::EPSILON);
    }

    #[test]
    fn test_lerp() {
        let line1 = Line::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        let line2 = Line::new(Point::new(0.0, 10.0), Point::new(10.0, 10.0));
        let mid = line1.lerp(line2, 0.5);
        assert_eq!(mid.p0, Point::new(0.0, 5.0));
        assert_eq!(mid.p1, Point::new(10.0, 5.0));
    }

    #[test]
    fn test_convenience_fn() {
        let l = line(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert_eq!(l, Line::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0)));
    }
}
