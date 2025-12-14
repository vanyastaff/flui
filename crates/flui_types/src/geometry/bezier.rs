//! Bézier curve types.
//!
//! Provides quadratic and cubic Bézier curves for smooth paths and connections.
//!
//! # Use Cases
//!
//! - Node editor connections (workflow builders like n8n)
//! - Smooth path animations
//! - Vector graphics
//! - UI transitions

use std::fmt;

use super::{Line, Point, Rect, Vec2};

// ============================================================================
// Quadratic Bézier
// ============================================================================

/// A quadratic Bézier curve segment.
///
/// Defined by start point, control point, and end point.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{QuadBez, Point};
///
/// let curve = QuadBez::new(
///     Point::new(0.0, 0.0),
///     Point::new(50.0, 100.0),
///     Point::new(100.0, 0.0),
/// );
///
/// let midpoint = curve.eval(0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct QuadBez {
    /// Start point.
    pub p0: Point,
    /// Control point.
    pub p1: Point,
    /// End point.
    pub p2: Point,
}

impl QuadBez {
    /// Creates a new quadratic Bézier curve.
    #[inline]
    #[must_use]
    pub const fn new(p0: Point, p1: Point, p2: Point) -> Self {
        Self { p0, p1, p2 }
    }

    /// Returns the point at parameter t (0.0 to 1.0).
    #[inline]
    #[must_use]
    pub fn eval(&self, t: f32) -> Point {
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let t2 = t * t;

        Point::new(
            mt2 * self.p0.x + 2.0 * mt * t * self.p1.x + t2 * self.p2.x,
            mt2 * self.p0.y + 2.0 * mt * t * self.p1.y + t2 * self.p2.y,
        )
    }

    /// Returns the tangent vector at parameter t.
    #[inline]
    #[must_use]
    pub fn tangent(&self, t: f32) -> Vec2 {
        let mt = 1.0 - t;

        Vec2::new(
            2.0 * mt * (self.p1.x - self.p0.x) + 2.0 * t * (self.p2.x - self.p1.x),
            2.0 * mt * (self.p1.y - self.p0.y) + 2.0 * t * (self.p2.y - self.p1.y),
        )
    }

    /// Returns the normalized tangent (direction) at parameter t.
    #[inline]
    #[must_use]
    pub fn direction(&self, t: f32) -> Vec2 {
        self.tangent(t).normalize_or(Vec2::X)
    }

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
        self.p2
    }

    /// Returns the control point.
    #[inline]
    #[must_use]
    pub const fn control(&self) -> Point {
        self.p1
    }

    /// Returns the bounding box (conservative estimate using control points).
    #[must_use]
    pub fn bounding_box(&self) -> Rect {
        let min_x = self.p0.x.min(self.p1.x).min(self.p2.x);
        let min_y = self.p0.y.min(self.p1.y).min(self.p2.y);
        let max_x = self.p0.x.max(self.p1.x).max(self.p2.x);
        let max_y = self.p0.y.max(self.p1.y).max(self.p2.y);

        Rect::new(min_x, min_y, max_x, max_y)
    }

    /// Returns the approximate length of the curve.
    ///
    /// Uses subdivision for estimation.
    #[must_use]
    pub fn arc_length(&self, tolerance: f32) -> f32 {
        self.arc_length_recursive(0.0, 1.0, tolerance)
    }

    fn arc_length_recursive(&self, t0: f32, t1: f32, tolerance: f32) -> f32 {
        let p0 = self.eval(t0);
        let p1 = self.eval(t1);
        let pm = self.eval((t0 + t1) / 2.0);

        let chord = p0.distance(p1);
        let arc = p0.distance(pm) + pm.distance(p1);

        if (arc - chord) < tolerance {
            arc
        } else {
            let mid = (t0 + t1) / 2.0;
            self.arc_length_recursive(t0, mid, tolerance)
                + self.arc_length_recursive(mid, t1, tolerance)
        }
    }

    /// Splits the curve at parameter t into two curves.
    #[must_use]
    pub fn split(&self, t: f32) -> (Self, Self) {
        let p01 = self.p0.lerp(self.p1, t);
        let p12 = self.p1.lerp(self.p2, t);
        let p012 = p01.lerp(p12, t);

        (Self::new(self.p0, p01, p012), Self::new(p012, p12, self.p2))
    }

    /// Returns the nearest point on the curve to the given point.
    ///
    /// Uses iterative refinement.
    #[must_use]
    pub fn nearest_point(&self, point: Point) -> Point {
        self.nearest_t(point, 0.0, 1.0, 10)
    }

    fn nearest_t(&self, point: Point, t0: f32, t1: f32, iterations: u32) -> Point {
        if iterations == 0 {
            let p0 = self.eval(t0);
            let p1 = self.eval(t1);
            if point.distance_squared(p0) < point.distance_squared(p1) {
                return p0;
            }
            return p1;
        }

        let t_mid = (t0 + t1) / 2.0;

        let d0 = point.distance_squared(self.eval(t0));
        let d1 = point.distance_squared(self.eval(t1));

        if d0 < d1 {
            self.nearest_t(point, t0, t_mid, iterations - 1)
        } else {
            self.nearest_t(point, t_mid, t1, iterations - 1)
        }
    }

    /// Returns the distance from the point to the curve.
    #[inline]
    #[must_use]
    pub fn distance_to_point(&self, point: Point) -> f32 {
        point.distance(self.nearest_point(point))
    }

    /// Checks if a point is within the given distance of the curve.
    #[inline]
    #[must_use]
    pub fn is_point_near(&self, point: Point, tolerance: f32) -> bool {
        self.distance_to_point(point) <= tolerance
    }

    /// Converts to a cubic Bézier curve.
    #[inline]
    #[must_use]
    pub fn to_cubic(&self) -> CubicBez {
        // Q(t) = C(t) when:
        // C.p1 = Q.p0 + 2/3 * (Q.p1 - Q.p0)
        // C.p2 = Q.p2 + 2/3 * (Q.p1 - Q.p2)
        let c1 = Point::new(
            self.p0.x + (2.0 / 3.0) * (self.p1.x - self.p0.x),
            self.p0.y + (2.0 / 3.0) * (self.p1.y - self.p0.y),
        );
        let c2 = Point::new(
            self.p2.x + (2.0 / 3.0) * (self.p1.x - self.p2.x),
            self.p2.y + (2.0 / 3.0) * (self.p1.y - self.p2.y),
        );

        CubicBez::new(self.p0, c1, c2, self.p2)
    }

    /// Returns a new curve translated by the given vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2) -> Self {
        Self {
            p0: self.p0 + offset,
            p1: self.p1 + offset,
            p2: self.p2 + offset,
        }
    }

    /// Linear interpolation between two curves.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            p0: self.p0.lerp(other.p0, t),
            p1: self.p1.lerp(other.p1, t),
            p2: self.p2.lerp(other.p2, t),
        }
    }
}

impl fmt::Display for QuadBez {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "QuadBez({} -> {} -> {})", self.p0, self.p1, self.p2)
    }
}

// ============================================================================
// Cubic Bézier
// ============================================================================

/// A cubic Bézier curve segment.
///
/// Defined by start point, two control points, and end point.
/// Most commonly used for smooth connections in node editors.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{CubicBez, Point};
///
/// // Horizontal S-curve (common for node connections)
/// let curve = CubicBez::horizontal_s(
///     Point::new(0.0, 0.0),
///     Point::new(100.0, 50.0),
/// );
///
/// let midpoint = curve.eval(0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct CubicBez {
    /// Start point.
    pub p0: Point,
    /// First control point.
    pub p1: Point,
    /// Second control point.
    pub p2: Point,
    /// End point.
    pub p3: Point,
}

impl CubicBez {
    /// Creates a new cubic Bézier curve.
    #[inline]
    #[must_use]
    pub const fn new(p0: Point, p1: Point, p2: Point, p3: Point) -> Self {
        Self { p0, p1, p2, p3 }
    }

    /// Creates a horizontal S-curve between two points.
    ///
    /// Control points are placed horizontally for smooth left-to-right connections.
    /// Ideal for node editor connections.
    #[inline]
    #[must_use]
    pub fn horizontal_s(start: Point, end: Point) -> Self {
        let dx = (end.x - start.x).abs() * 0.5;
        Self {
            p0: start,
            p1: Point::new(start.x + dx, start.y),
            p2: Point::new(end.x - dx, end.y),
            p3: end,
        }
    }

    /// Creates a vertical S-curve between two points.
    ///
    /// Control points are placed vertically for smooth top-to-bottom connections.
    #[inline]
    #[must_use]
    pub fn vertical_s(start: Point, end: Point) -> Self {
        let dy = (end.y - start.y).abs() * 0.5;
        Self {
            p0: start,
            p1: Point::new(start.x, start.y + dy),
            p2: Point::new(end.x, end.y - dy),
            p3: end,
        }
    }

    /// Creates a curve with custom control point offset.
    ///
    /// `factor` controls how far control points extend (0.0 to 1.0).
    /// Higher values create more pronounced curves.
    #[inline]
    #[must_use]
    pub fn horizontal_s_with_factor(start: Point, end: Point, factor: f32) -> Self {
        let dx = (end.x - start.x).abs() * factor;
        Self {
            p0: start,
            p1: Point::new(start.x + dx, start.y),
            p2: Point::new(end.x - dx, end.y),
            p3: end,
        }
    }

    /// Creates a straight line as a degenerate cubic Bézier.
    #[inline]
    #[must_use]
    pub fn line(start: Point, end: Point) -> Self {
        let p1 = start.lerp(end, 1.0 / 3.0);
        let p2 = start.lerp(end, 2.0 / 3.0);
        Self::new(start, p1, p2, end)
    }

    /// Returns the point at parameter t (0.0 to 1.0).
    #[inline]
    #[must_use]
    pub fn eval(&self, t: f32) -> Point {
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;
        let t2 = t * t;
        let t3 = t2 * t;

        Point::new(
            mt3 * self.p0.x
                + 3.0 * mt2 * t * self.p1.x
                + 3.0 * mt * t2 * self.p2.x
                + t3 * self.p3.x,
            mt3 * self.p0.y
                + 3.0 * mt2 * t * self.p1.y
                + 3.0 * mt * t2 * self.p2.y
                + t3 * self.p3.y,
        )
    }

    /// Returns the tangent vector at parameter t.
    #[inline]
    #[must_use]
    pub fn tangent(&self, t: f32) -> Vec2 {
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let t2 = t * t;

        Vec2::new(
            3.0 * mt2 * (self.p1.x - self.p0.x)
                + 6.0 * mt * t * (self.p2.x - self.p1.x)
                + 3.0 * t2 * (self.p3.x - self.p2.x),
            3.0 * mt2 * (self.p1.y - self.p0.y)
                + 6.0 * mt * t * (self.p2.y - self.p1.y)
                + 3.0 * t2 * (self.p3.y - self.p2.y),
        )
    }

    /// Returns the normalized tangent (direction) at parameter t.
    #[inline]
    #[must_use]
    pub fn direction(&self, t: f32) -> Vec2 {
        self.tangent(t).normalize_or(Vec2::X)
    }

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
        self.p3
    }

    /// Returns the first control point.
    #[inline]
    #[must_use]
    pub const fn control1(&self) -> Point {
        self.p1
    }

    /// Returns the second control point.
    #[inline]
    #[must_use]
    pub const fn control2(&self) -> Point {
        self.p2
    }

    /// Returns the bounding box (conservative estimate using control points).
    #[must_use]
    pub fn bounding_box(&self) -> Rect {
        let min_x = self.p0.x.min(self.p1.x).min(self.p2.x).min(self.p3.x);
        let min_y = self.p0.y.min(self.p1.y).min(self.p2.y).min(self.p3.y);
        let max_x = self.p0.x.max(self.p1.x).max(self.p2.x).max(self.p3.x);
        let max_y = self.p0.y.max(self.p1.y).max(self.p2.y).max(self.p3.y);

        Rect::new(min_x, min_y, max_x, max_y)
    }

    /// Returns the approximate length of the curve.
    #[must_use]
    pub fn arc_length(&self, tolerance: f32) -> f32 {
        self.arc_length_recursive(0.0, 1.0, tolerance)
    }

    fn arc_length_recursive(&self, t0: f32, t1: f32, tolerance: f32) -> f32 {
        let p0 = self.eval(t0);
        let p1 = self.eval(t1);
        let pm = self.eval((t0 + t1) / 2.0);

        let chord = p0.distance(p1);
        let arc = p0.distance(pm) + pm.distance(p1);

        if (arc - chord) < tolerance {
            arc
        } else {
            let mid = (t0 + t1) / 2.0;
            self.arc_length_recursive(t0, mid, tolerance)
                + self.arc_length_recursive(mid, t1, tolerance)
        }
    }

    /// Splits the curve at parameter t into two curves (de Casteljau).
    #[must_use]
    pub fn split(&self, t: f32) -> (Self, Self) {
        let p01 = self.p0.lerp(self.p1, t);
        let p12 = self.p1.lerp(self.p2, t);
        let p23 = self.p2.lerp(self.p3, t);

        let p012 = p01.lerp(p12, t);
        let p123 = p12.lerp(p23, t);

        let p0123 = p012.lerp(p123, t);

        (
            Self::new(self.p0, p01, p012, p0123),
            Self::new(p0123, p123, p23, self.p3),
        )
    }

    /// Returns the nearest point on the curve to the given point.
    #[must_use]
    pub fn nearest_point(&self, point: Point) -> Point {
        // Newton-Raphson with fallback to subdivision
        self.nearest_point_subdivision(point, 0.0, 1.0, 16)
    }

    fn nearest_point_subdivision(
        &self,
        point: Point,
        t0: f32,
        t1: f32,
        subdivisions: u32,
    ) -> Point {
        if subdivisions == 0 {
            let p0 = self.eval(t0);
            let p1 = self.eval(t1);
            if point.distance_squared(p0) < point.distance_squared(p1) {
                return p0;
            }
            return p1;
        }

        let steps = 4;
        let mut best_t = t0;
        let mut best_dist = f32::MAX;

        for i in 0..=steps {
            let t = t0 + (t1 - t0) * (i as f32 / steps as f32);
            let p = self.eval(t);
            let dist = point.distance_squared(p);
            if dist < best_dist {
                best_dist = dist;
                best_t = t;
            }
        }

        let dt = (t1 - t0) / steps as f32;
        let new_t0 = (best_t - dt).max(t0);
        let new_t1 = (best_t + dt).min(t1);

        self.nearest_point_subdivision(point, new_t0, new_t1, subdivisions - 1)
    }

    /// Returns the distance from the point to the curve.
    #[inline]
    #[must_use]
    pub fn distance_to_point(&self, point: Point) -> f32 {
        point.distance(self.nearest_point(point))
    }

    /// Checks if a point is within the given distance of the curve.
    #[inline]
    #[must_use]
    pub fn is_point_near(&self, point: Point, tolerance: f32) -> bool {
        self.distance_to_point(point) <= tolerance
    }

    /// Returns a new curve translated by the given vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2) -> Self {
        Self {
            p0: self.p0 + offset,
            p1: self.p1 + offset,
            p2: self.p2 + offset,
            p3: self.p3 + offset,
        }
    }

    /// Returns a new curve with start and end points swapped.
    #[inline]
    #[must_use]
    pub const fn reverse(&self) -> Self {
        Self {
            p0: self.p3,
            p1: self.p2,
            p2: self.p1,
            p3: self.p0,
        }
    }

    /// Linear interpolation between two curves.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            p0: self.p0.lerp(other.p0, t),
            p1: self.p1.lerp(other.p1, t),
            p2: self.p2.lerp(other.p2, t),
            p3: self.p3.lerp(other.p3, t),
        }
    }

    /// Flattens the curve into line segments.
    ///
    /// Returns points along the curve with the given tolerance.
    #[must_use]
    pub fn flatten(&self, tolerance: f32) -> Vec<Point> {
        let mut points = vec![self.p0];
        self.flatten_recursive(0.0, 1.0, tolerance, &mut points);
        points
    }

    fn flatten_recursive(&self, t0: f32, t1: f32, tolerance: f32, points: &mut Vec<Point>) {
        let p0 = self.eval(t0);
        let p1 = self.eval(t1);
        let pm = self.eval((t0 + t1) / 2.0);

        // Check if the midpoint is close enough to the line
        let line = Line::new(p0, p1);
        let dist = line.distance_to_point(pm);

        if dist <= tolerance {
            points.push(p1);
        } else {
            let mid = (t0 + t1) / 2.0;
            self.flatten_recursive(t0, mid, tolerance, points);
            self.flatten_recursive(mid, t1, tolerance, points);
        }
    }

    /// Returns intersection points with a line.
    #[must_use]
    pub fn intersect_line(&self, line: &Line) -> Vec<Point> {
        // Simplified: flatten and check segments
        let points = self.flatten(1.0);
        let mut intersections = Vec::new();

        for i in 0..points.len() - 1 {
            let segment = Line::new(points[i], points[i + 1]);
            if let Some(p) = segment.intersect_segment(line) {
                intersections.push(p);
            }
        }

        intersections
    }
}

impl fmt::Display for CubicBez {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CubicBez({} -> {} -> {} -> {})",
            self.p0, self.p1, self.p2, self.p3
        )
    }
}

// ============================================================================
// Convenience functions
// ============================================================================

/// Shorthand for `QuadBez::new(p0, p1, p2)`.
#[inline]
#[must_use]
pub fn quad_bez(p0: Point, p1: Point, p2: Point) -> QuadBez {
    QuadBez::new(p0, p1, p2)
}

/// Shorthand for `CubicBez::new(p0, p1, p2, p3)`.
#[inline]
#[must_use]
pub fn cubic_bez(p0: Point, p1: Point, p2: Point, p3: Point) -> CubicBez {
    CubicBez::new(p0, p1, p2, p3)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // QuadBez tests

    #[test]
    fn test_quad_construction() {
        let q = QuadBez::new(
            Point::new(0.0, 0.0),
            Point::new(50.0, 100.0),
            Point::new(100.0, 0.0),
        );
        assert_eq!(q.start(), Point::new(0.0, 0.0));
        assert_eq!(q.end(), Point::new(100.0, 0.0));
        assert_eq!(q.control(), Point::new(50.0, 100.0));
    }

    #[test]
    fn test_quad_eval() {
        let q = QuadBez::new(
            Point::new(0.0, 0.0),
            Point::new(50.0, 100.0),
            Point::new(100.0, 0.0),
        );

        assert_eq!(q.eval(0.0), Point::new(0.0, 0.0));
        assert_eq!(q.eval(1.0), Point::new(100.0, 0.0));

        let mid = q.eval(0.5);
        assert!((mid.x - 50.0).abs() < 0.001);
        assert!((mid.y - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_quad_split() {
        let q = QuadBez::new(
            Point::new(0.0, 0.0),
            Point::new(50.0, 100.0),
            Point::new(100.0, 0.0),
        );

        let (left, right) = q.split(0.5);
        assert_eq!(left.start(), q.start());
        assert_eq!(right.end(), q.end());

        // Both should evaluate to same point at split
        let p1 = left.eval(1.0);
        let p2 = right.eval(0.0);
        assert!((p1.x - p2.x).abs() < 0.001);
        assert!((p1.y - p2.y).abs() < 0.001);
    }

    #[test]
    fn test_quad_bounding_box() {
        let q = QuadBez::new(
            Point::new(0.0, 0.0),
            Point::new(50.0, 100.0),
            Point::new(100.0, 0.0),
        );

        let bbox = q.bounding_box();
        assert_eq!(bbox.min.x, 0.0);
        assert_eq!(bbox.min.y, 0.0);
        assert_eq!(bbox.max.x, 100.0);
        assert_eq!(bbox.max.y, 100.0);
    }

    #[test]
    fn test_quad_to_cubic() {
        let q = QuadBez::new(
            Point::new(0.0, 0.0),
            Point::new(50.0, 100.0),
            Point::new(100.0, 0.0),
        );

        let c = q.to_cubic();

        // Should evaluate to same points
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let pq = q.eval(t);
            let pc = c.eval(t);
            assert!((pq.x - pc.x).abs() < 0.01);
            assert!((pq.y - pc.y).abs() < 0.01);
        }
    }

    // CubicBez tests

    #[test]
    fn test_cubic_construction() {
        let c = CubicBez::new(
            Point::new(0.0, 0.0),
            Point::new(33.0, 100.0),
            Point::new(66.0, 100.0),
            Point::new(100.0, 0.0),
        );

        assert_eq!(c.start(), Point::new(0.0, 0.0));
        assert_eq!(c.end(), Point::new(100.0, 0.0));
        assert_eq!(c.control1(), Point::new(33.0, 100.0));
        assert_eq!(c.control2(), Point::new(66.0, 100.0));
    }

    #[test]
    fn test_cubic_eval() {
        let c = CubicBez::new(
            Point::new(0.0, 0.0),
            Point::new(0.0, 100.0),
            Point::new(100.0, 100.0),
            Point::new(100.0, 0.0),
        );

        assert_eq!(c.eval(0.0), Point::new(0.0, 0.0));
        assert_eq!(c.eval(1.0), Point::new(100.0, 0.0));

        let mid = c.eval(0.5);
        assert!((mid.x - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_cubic_horizontal_s() {
        let c = CubicBez::horizontal_s(Point::new(0.0, 0.0), Point::new(100.0, 50.0));

        assert_eq!(c.start(), Point::new(0.0, 0.0));
        assert_eq!(c.end(), Point::new(100.0, 50.0));

        // Control points should be horizontal from endpoints
        assert_eq!(c.control1().y, 0.0);
        assert_eq!(c.control2().y, 50.0);
    }

    #[test]
    fn test_cubic_vertical_s() {
        let c = CubicBez::vertical_s(Point::new(0.0, 0.0), Point::new(50.0, 100.0));

        assert_eq!(c.start(), Point::new(0.0, 0.0));
        assert_eq!(c.end(), Point::new(50.0, 100.0));

        // Control points should be vertical from endpoints
        assert_eq!(c.control1().x, 0.0);
        assert_eq!(c.control2().x, 50.0);
    }

    #[test]
    fn test_cubic_split() {
        let c = CubicBez::horizontal_s(Point::new(0.0, 0.0), Point::new(100.0, 0.0));

        let (left, right) = c.split(0.5);
        assert_eq!(left.start(), c.start());
        assert_eq!(right.end(), c.end());

        // Both should meet at split point
        let p1 = left.eval(1.0);
        let p2 = right.eval(0.0);
        assert!((p1.x - p2.x).abs() < 0.001);
        assert!((p1.y - p2.y).abs() < 0.001);
    }

    #[test]
    fn test_cubic_reverse() {
        let c = CubicBez::new(
            Point::new(0.0, 0.0),
            Point::new(33.0, 100.0),
            Point::new(66.0, 100.0),
            Point::new(100.0, 0.0),
        );

        let r = c.reverse();
        assert_eq!(r.start(), c.end());
        assert_eq!(r.end(), c.start());
    }

    #[test]
    fn test_cubic_flatten() {
        let c = CubicBez::horizontal_s(Point::new(0.0, 0.0), Point::new(100.0, 50.0));

        let points = c.flatten(1.0);
        assert!(points.len() >= 2);
        assert_eq!(points[0], c.start());
        assert_eq!(*points.last().unwrap(), c.end());
    }

    #[test]
    fn test_cubic_nearest_point() {
        let c = CubicBez::horizontal_s(Point::new(0.0, 0.0), Point::new(100.0, 0.0));

        // Point on curve
        let near = c.nearest_point(Point::new(50.0, 0.0));
        assert!(near.distance(Point::new(50.0, 0.0)) < 1.0);

        // Point above curve
        let near = c.nearest_point(Point::new(50.0, 50.0));
        assert!(near.y < 50.0);
    }

    #[test]
    fn test_cubic_is_point_near() {
        let c = CubicBez::horizontal_s(Point::new(0.0, 0.0), Point::new(100.0, 0.0));

        assert!(c.is_point_near(Point::new(50.0, 5.0), 10.0));
        assert!(!c.is_point_near(Point::new(50.0, 50.0), 10.0));
    }

    #[test]
    fn test_cubic_translate() {
        let c = CubicBez::horizontal_s(Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        let translated = c.translate(Vec2::new(10.0, 20.0));

        assert_eq!(translated.start(), Point::new(10.0, 20.0));
        assert_eq!(translated.end(), Point::new(110.0, 20.0));
    }

    #[test]
    fn test_cubic_lerp() {
        let c1 = CubicBez::horizontal_s(Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        let c2 = CubicBez::horizontal_s(Point::new(0.0, 100.0), Point::new(100.0, 100.0));

        let mid = c1.lerp(c2, 0.5);
        assert_eq!(mid.start(), Point::new(0.0, 50.0));
        assert_eq!(mid.end(), Point::new(100.0, 50.0));
    }

    #[test]
    fn test_cubic_line() {
        let c = CubicBez::line(Point::new(0.0, 0.0), Point::new(100.0, 100.0));

        // Should be a straight line
        let mid = c.eval(0.5);
        assert!((mid.x - 50.0).abs() < 0.001);
        assert!((mid.y - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_convenience_functions() {
        let q = quad_bez(
            Point::ORIGIN,
            Point::new(50.0, 100.0),
            Point::new(100.0, 0.0),
        );
        assert_eq!(q.start(), Point::ORIGIN);

        let c = cubic_bez(
            Point::ORIGIN,
            Point::new(33.0, 100.0),
            Point::new(66.0, 100.0),
            Point::new(100.0, 0.0),
        );
        assert_eq!(c.start(), Point::ORIGIN);
    }
}
