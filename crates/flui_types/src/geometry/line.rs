//! Line segment type.
//!
//! A line segment defined by two endpoints.
//!
//! # Type Safety
//!
//! `Line<T>` is generic over unit type `T`, preventing accidental mixing
//! of coordinate systems.

use std::fmt;

use super::traits::{NumericUnit, Unit};
use super::{Point, Rect, Vec2};

/// A line segment defined by two endpoints.
///
/// Generic over unit type `T`. Common usage:
/// - `Line<f32>` - Raw coordinates (GPU-ready)
/// - `Line<Pixels>` - Logical pixel coordinates
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Line, point};
///
/// let line = Line::new(point(0.0, 0.0), point(100.0, 100.0));
/// assert_eq!(line.length(), 100.0 * std::f32::consts::SQRT_2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Line<T: Unit = f32> {
    /// Start point.
    pub p0: Point<T>,
    /// End point.
    pub p1: Point<T>,
}

impl<T: Unit> Default for Line<T> {
    fn default() -> Self {
        Self {
            p0: Point::new(T::zero(), T::zero()),
            p1: Point::new(T::zero(), T::zero()),
        }
    }
}

// ============================================================================
// Generic Constructors
// ============================================================================

impl<T: Unit> Line<T> {
    /// Creates a new line from two points.
    #[inline]
    #[must_use]
    pub const fn new(p0: Point<T>, p1: Point<T>) -> Self {
        Self { p0, p1 }
    }

    /// Returns the start point.
    #[inline]
    #[must_use]
    pub const fn start(&self) -> Point<T> {
        self.p0
    }

    /// Returns the end point.
    #[inline]
    #[must_use]
    pub const fn end(&self) -> Point<T> {
        self.p1
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

    /// Maps the line through a function.
    #[inline]
    #[must_use]
    pub fn map<U: Unit>(&self, f: impl Fn(T) -> U + Copy) -> Line<U>
    where
        T: Clone + fmt::Debug + Default + PartialEq,
    {
        Line {
            p0: self.p0.map(f),
            p1: self.p1.map(f),
        }
    }
}

// ============================================================================
// f32-specific Constructors
// ============================================================================

impl Line<f32> {
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
// Accessors (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Line<T>
where
    T: Into<f32> + From<f32> + std::ops::Sub<Output = T>,
{
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
    pub fn to_vec(&self) -> Vec2<T>
    where
        T: std::ops::Sub<Output = T> + Copy,
    {
        Vec2::new(self.p1.x - self.p0.x, self.p1.y - self.p0.y)
    }

    /// Returns the normalized direction vector.
    ///
    /// Returns zero vector if line has zero length.
    #[inline]
    #[must_use]
    pub fn direction(&self) -> Vec2<f32> {
        self.to_vec().normalize_or(Vec2::ZERO)
    }

    /// Returns the midpoint of the line.
    #[inline]
    #[must_use]
    pub fn midpoint(&self) -> Point<T> {
        self.p0.midpoint(self.p1)
    }

    /// Returns the point at parameter t (0.0 = p0, 1.0 = p1).
    #[inline]
    #[must_use]
    pub fn eval(&self, t: f32) -> Point<T> {
        self.p0.lerp(self.p1, t)
    }
}

// ============================================================================
// Queries (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Line<T>
where
    T: Into<f32> + From<f32> + PartialEq + std::ops::Sub<Output = T> + Clone + fmt::Debug + Default,
{
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
        let y0: f32 = self.p0.y.into();
        let y1: f32 = self.p1.y.into();
        (y0 - y1).abs() < f32::EPSILON
    }

    /// Returns `true` if the line is vertical (within epsilon).
    #[inline]
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        let x0: f32 = self.p0.x.into();
        let x1: f32 = self.p1.x.into();
        (x0 - x1).abs() < f32::EPSILON
    }

    /// Returns the closest point on the line segment to the given point.
    #[must_use]
    pub fn nearest_point(&self, point: Point<T>) -> Point<T> {
        // Convert to f32 for calculation since Point<f32> - Point<f32> = Vec2<f32>
        let point_f32 = point.map(|v| v.into());
        let p0_f32 = self.p0.map(|v| v.into());
        let p1_f32 = self.p1.map(|v| v.into());

        let d = p1_f32 - p0_f32;
        let length_sq = d.length_squared();

        if length_sq < f32::EPSILON {
            return self.p0;
        }

        let v = point_f32 - p0_f32;
        let t = (v.dot(&d) / length_sq).clamp(0.0, 1.0);
        self.eval(t)
    }

    /// Returns the distance from the point to the line segment.
    #[inline]
    #[must_use]
    pub fn distance_to_point(&self, point: Point<T>) -> f32 {
        point.distance(self.nearest_point(point))
    }

    /// Returns the squared distance from the point to the line segment.
    #[inline]
    #[must_use]
    pub fn distance_squared_to_point(&self, point: Point<T>) -> f32 {
        point.distance_squared(self.nearest_point(point))
    }

    /// Checks if a point is within the given distance of the line segment.
    ///
    /// Useful for hit testing with a tolerance.
    #[inline]
    #[must_use]
    pub fn is_point_near(&self, point: Point<T>, tolerance: f32) -> bool {
        self.distance_squared_to_point(point) <= tolerance * tolerance
    }
}

// ============================================================================
// f32-specific Queries (Rect operations)
// ============================================================================

impl Line<f32> {
    /// Returns the bounding box of the line.
    #[inline]
    #[must_use]
    pub fn bounding_box(&self) -> Rect {
        Rect::from_points(self.p0, self.p1)
    }
}

// ============================================================================
// Transformations (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Line<T>
where
    T: Into<f32> + From<f32>,
{
    /// Returns a new line translated by the given vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2<T>) -> Self {
        Self {
            p0: self.p0 + offset,
            p1: self.p1 + offset,
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
// Conversions
// ============================================================================

impl<T: NumericUnit> Line<T>
where
    T: Into<f32>,
{
    /// Converts to `Line<f32>`.
    #[inline]
    #[must_use]
    pub fn to_f32(&self) -> Line<f32> {
        Line {
            p0: self.p0.to_f32(),
            p1: self.p1.to_f32(),
        }
    }
}

impl<T: Unit> Line<T>
where
    T: Into<f32>,
{
    /// Converts to array `[x0, y0, x1, y1]` for GPU usage.
    #[inline]
    #[must_use]
    pub fn to_array(&self) -> [f32; 4] {
        [
            self.p0.x.into(),
            self.p0.y.into(),
            self.p1.x.into(),
            self.p1.y.into(),
        ]
    }
}

// ============================================================================
// Intersections (f32 only - complex math)
// ============================================================================

impl Line<f32> {
    /// Returns the intersection point of two lines (as infinite lines).
    ///
    /// Returns `None` if lines are parallel.
    #[must_use]
    pub fn intersect_line(&self, other: &Line<f32>) -> Option<Point<f32>> {
        let d1 = self.to_vec();
        let d2 = other.to_vec();

        let cross = d1.cross(&d2);
        if cross.abs() < f32::EPSILON {
            return None; // Parallel
        }

        let d = other.p0 - self.p0;
        let t = d.cross(&d2) / cross;

        Some(self.eval(t))
    }

    /// Returns the intersection point of two line segments.
    ///
    /// Returns `None` if segments don't intersect.
    #[must_use]
    pub fn intersect_segment(&self, other: &Line<f32>) -> Option<Point<f32>> {
        let d1 = self.to_vec();
        let d2 = other.to_vec();

        let cross = d1.cross(&d2);
        if cross.abs() < f32::EPSILON {
            return None; // Parallel
        }

        let d = other.p0 - self.p0;
        let t1 = d.cross(&d2) / cross;
        let t2 = d.cross(&d1) / cross;

        if (0.0..=1.0).contains(&t1) && (0.0..=1.0).contains(&t2) {
            Some(self.eval(t1))
        } else {
            None
        }
    }
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl<T: Unit> super::traits::IsZero for Line<T>
where
    T: super::traits::IsZero
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.p0.is_zero() && self.p1.is_zero()
    }
}

impl<T: Unit> super::traits::ApproxEq for Line<T>
where
    T: super::traits::ApproxEq
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        self.p0.approx_eq_eps(&other.p0, epsilon)
            && self.p1.approx_eq_eps(&other.p1, epsilon)
    }
}

// ============================================================================
// Additional Generic Methods
// ============================================================================

impl<T: Unit> Line<T> {
    /// Casts the line to a different unit type.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Line, Point, Pixels, px};
    ///
    /// let px_line = Line::<Pixels>::new(
    ///     Point::new(px(0.0), px(0.0)),
    ///     Point::new(px(10.0), px(10.0))
    /// );
    /// let f32_line: Line<f32> = px_line.cast();
    /// assert_eq!(f32_line.p0.x, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn cast<U: Unit>(self) -> Line<U>
    where
        T: Into<U>,
    {
        Line {
            p0: self.p0.cast(),
            p1: self.p1.cast(),
        }
    }

    /// Swaps the start and end points (alias for reverse).
    #[inline]
    #[must_use]
    pub fn swap(self) -> Self {
        self.reverse()
    }
}

impl Line<f32> {
    /// A zero-length line at the origin.
    pub const ZERO: Self = Self {
        p0: Point::ORIGIN,
        p1: Point::ORIGIN,
    };

    /// Returns true if the line is valid (finite coordinates).
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.p0.is_valid() && self.p1.is_valid()
    }

    /// Linear interpolation between two lines.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Line, point};
    ///
    /// let a = Line::new(point(0.0, 0.0), point(10.0, 0.0));
    /// let b = Line::new(point(0.0, 10.0), point(10.0, 10.0));
    /// let mid = a.lerp_line(&b, 0.5);
    /// assert_eq!(mid.p0.y, 5.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn lerp_line(&self, other: &Self, t: f32) -> Self {
        Self {
            p0: self.p0.lerp(other.p0, t),
            p1: self.p1.lerp(other.p1, t),
        }
    }

    /// Returns the perpendicular line at the midpoint.
    #[inline]
    #[must_use]
    pub fn perpendicular(&self) -> Self {
        let mid = self.midpoint();
        let dir = self.to_vec();
        let perp = Vec2::new(-dir.y, dir.x);
        let half_len = self.length() * 0.5;
        let normalized = if half_len > 0.0 {
            perp * (half_len / perp.length())
        } else {
            Vec2::ZERO
        };
        Self {
            p0: mid - normalized,
            p1: mid + normalized,
        }
    }

    /// Extends the line by a given amount at both ends.
    #[inline]
    #[must_use]
    pub fn extend(&self, amount: f32) -> Self {
        let dir = self.direction();
        Self {
            p0: self.p0 - dir * amount,
            p1: self.p1 + dir * amount,
        }
    }

    /// Shrinks the line by a given amount at both ends.
    #[inline]
    #[must_use]
    pub fn shrink(&self, amount: f32) -> Self {
        self.extend(-amount)
    }
}

// ============================================================================
// Display
// ============================================================================

impl<T> fmt::Display for Line<T>
where
    T: Unit + fmt::Display + Clone + fmt::Debug + Default + PartialEq,
{
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
pub fn line(p0: Point<f32>, p1: Point<f32>) -> Line<f32> {
    Line::new(p0, p1)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{point, px, Pixels};

    #[test]
    fn test_construction() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0));
        assert_eq!(line.p0, point(0.0, 0.0));
        assert_eq!(line.p1, point(10.0, 0.0));

        let line2 = Line::from_coords(0.0, 0.0, 10.0, 0.0);
        assert_eq!(line, line2);
    }

    #[test]
    fn test_generic_construction() {
        let line = Line::<Pixels>::new(
            Point::new(px(0.0), px(0.0)),
            Point::new(px(10.0), px(0.0)),
        );
        assert_eq!(line.p0, Point::new(px(0.0), px(0.0)));
        assert_eq!(line.p1, Point::new(px(10.0), px(0.0)));
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
        let line = Line::new(point(0.0, 0.0), point(3.0, 4.0));
        assert_eq!(line.length(), 5.0);
        assert_eq!(line.length_squared(), 25.0);
    }

    #[test]
    fn test_midpoint() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 10.0));
        assert_eq!(line.midpoint(), point(5.0, 5.0));
    }

    #[test]
    fn test_eval() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0));
        assert_eq!(line.eval(0.0), point(0.0, 0.0));
        assert_eq!(line.eval(0.5), point(5.0, 0.0));
        assert_eq!(line.eval(1.0), point(10.0, 0.0));
    }

    #[test]
    fn test_nearest_point() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0));

        // Point on line
        assert_eq!(
            line.nearest_point(point(5.0, 0.0)),
            point(5.0, 0.0)
        );

        // Point above line
        assert_eq!(
            line.nearest_point(point(5.0, 5.0)),
            point(5.0, 0.0)
        );

        // Point before start
        assert_eq!(
            line.nearest_point(point(-5.0, 0.0)),
            point(0.0, 0.0)
        );

        // Point after end
        assert_eq!(
            line.nearest_point(point(15.0, 0.0)),
            point(10.0, 0.0)
        );
    }

    #[test]
    fn test_distance_to_point() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0));
        assert_eq!(line.distance_to_point(point(5.0, 3.0)), 3.0);
    }

    #[test]
    fn test_is_point_near() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0));
        assert!(line.is_point_near(point(5.0, 2.0), 3.0));
        assert!(!line.is_point_near(point(5.0, 5.0), 3.0));
    }

    #[test]
    fn test_bounding_box() {
        let line = Line::new(point(10.0, 20.0), point(30.0, 40.0));
        let bbox = line.bounding_box();
        assert_eq!(bbox.min, point(10.0, 20.0));
        assert_eq!(bbox.max, point(30.0, 40.0));
    }

    #[test]
    fn test_translate() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 10.0));
        let translated = line.translate(Vec2::new(5.0, 5.0));
        assert_eq!(translated.p0, point(5.0, 5.0));
        assert_eq!(translated.p1, point(15.0, 15.0));
    }

    #[test]
    fn test_reverse() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 10.0));
        let reversed = line.reverse();
        assert_eq!(reversed.p0, point(10.0, 10.0));
        assert_eq!(reversed.p1, point(0.0, 0.0));
    }

    #[test]
    fn test_intersect_segment() {
        let line1 = Line::new(point(0.0, 0.0), point(10.0, 10.0));
        let line2 = Line::new(point(0.0, 10.0), point(10.0, 0.0));

        let intersection = line1.intersect_segment(&line2);
        assert!(intersection.is_some());
        let p = intersection.unwrap();
        assert!((p.x - 5.0).abs() < 0.001);
        assert!((p.y - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_intersect_parallel() {
        let line1 = Line::new(point(0.0, 0.0), point(10.0, 0.0));
        let line2 = Line::new(point(0.0, 5.0), point(10.0, 5.0));

        assert!(line1.intersect_segment(&line2).is_none());
    }

    #[test]
    fn test_direction() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0));
        let dir = line.direction();
        assert!((dir.x - 1.0).abs() < f32::EPSILON);
        assert!(dir.y.abs() < f32::EPSILON);
    }

    #[test]
    fn test_lerp() {
        let line1 = Line::new(point(0.0, 0.0), point(10.0, 0.0));
        let line2 = Line::new(point(0.0, 10.0), point(10.0, 10.0));
        let mid = line1.lerp(line2, 0.5);
        assert_eq!(mid.p0, point(0.0, 5.0));
        assert_eq!(mid.p1, point(10.0, 5.0));
    }

    #[test]
    fn test_convenience_fn() {
        let l = line(point(0.0, 0.0), point(10.0, 10.0));
        assert_eq!(l, Line::new(point(0.0, 0.0), point(10.0, 10.0)));
    }

    #[test]
    fn test_to_f32() {
        let l = Line::<Pixels>::new(
            Point::new(px(0.0), px(0.0)),
            Point::new(px(10.0), px(20.0)),
        );
        let f = l.to_f32();
        assert_eq!(f.p0, point(0.0, 0.0));
        assert_eq!(f.p1, point(10.0, 20.0));
    }

    #[test]
    fn test_to_array() {
        let l = line(point(10.0, 20.0), point(30.0, 40.0));
        let arr = l.to_array();
        assert_eq!(arr, [10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn test_map() {
        let l = line(point(10.0, 20.0), point(30.0, 40.0));
        let doubled = l.map(|x| x * 2.0);
        assert_eq!(doubled.p0, point(20.0, 40.0));
        assert_eq!(doubled.p1, point(60.0, 80.0));
    }

    #[test]
    fn test_default() {
        let l: Line<f32> = Line::default();
        assert_eq!(l.p0, Point::ORIGIN);
        assert_eq!(l.p1, Point::ORIGIN);
    }
}
