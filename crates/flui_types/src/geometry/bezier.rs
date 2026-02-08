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

use super::Pixels;
use std::fmt;

use super::traits::{NumericUnit, Unit};
use super::{Line, Point, Rect, Vec2};

// ============================================================================
// Quadratic Bézier
// ============================================================================

/// Quadratic Bézier curve with generic unit type.
///
/// A quadratic Bézier curve is defined by three points: start (p0), control (p1), and end (p2).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadBez<T: Unit> {
    /// Start point.
    pub p0: Point<T>,
    /// Control point.
    pub p1: Point<T>,
    /// End point.
    pub p2: Point<T>,
}

impl<T: Unit> Default for QuadBez<T> {
    fn default() -> Self {
        Self {
            p0: Point::default(),
            p1: Point::default(),
            p2: Point::default(),
        }
    }
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> QuadBez<T> {
    /// Creates a new quadratic Bézier curve from three points.
    #[inline]
    #[must_use]
    pub const fn new(p0: Point<T>, p1: Point<T>, p2: Point<T>) -> Self {
        Self { p0, p1, p2 }
    }

    /// Returns the start point of the curve.
    #[inline]
    #[must_use]
    pub const fn start(&self) -> Point<T> {
        self.p0
    }

    /// Returns the end point of the curve.
    #[inline]
    #[must_use]
    pub const fn end(&self) -> Point<T> {
        self.p2
    }

    /// Returns the control point of the curve.
    #[inline]
    #[must_use]
    pub const fn control(&self) -> Point<T> {
        self.p1
    }
}

// ============================================================================
// Numeric Operations (NumericUnit with Into<f32> + From<f32>)
// ============================================================================

impl<T: NumericUnit> QuadBez<T>
where
    T: Into<f32> + From<f32>,
{
    /// Evaluates the curve at parameter t ∈ [0, 1].
    #[inline]
    #[must_use]
    pub fn eval(&self, t: f32) -> Point<T> {
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let t2 = t * t;

        let p0x: f32 = self.p0.x.into();
        let p0y: f32 = self.p0.y.into();
        let p1x: f32 = self.p1.x.into();
        let p1y: f32 = self.p1.y.into();
        let p2x: f32 = self.p2.x.into();
        let p2y: f32 = self.p2.y.into();

        Point::new(
            T::from(mt2 * p0x + 2.0 * mt * t * p1x + t2 * p2x),
            T::from(mt2 * p0y + 2.0 * mt * t * p1y + t2 * p2y),
        )
    }

    /// Returns the tangent vector at parameter t.
    #[inline]
    #[must_use]
    pub fn tangent(&self, t: f32) -> Vec2<T> {
        let mt = 1.0 - t;

        let p0x: f32 = self.p0.x.into();
        let p0y: f32 = self.p0.y.into();
        let p1x: f32 = self.p1.x.into();
        let p1y: f32 = self.p1.y.into();
        let p2x: f32 = self.p2.x.into();
        let p2y: f32 = self.p2.y.into();

        Vec2::new(
            T::from(2.0 * mt * (p1x - p0x) + 2.0 * t * (p2x - p1x)),
            T::from(2.0 * mt * (p1y - p0y) + 2.0 * t * (p2y - p1y)),
        )
    }

    /// Splits the curve at parameter t into two curves.
    /// Splits the curve at parameter t into two curves.
    #[inline]
    #[must_use]
    pub fn split(&self, t: f32) -> (Self, Self) {
        let p01 = self.p0.lerp(self.p1, t);
        let p12 = self.p1.lerp(self.p2, t);
        let p012 = p01.lerp(p12, t);

        (Self::new(self.p0, p01, p012), Self::new(p012, p12, self.p2))
    }

    /// Translates the curve by the given offset.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2<T>) -> Self {
        Self {
            p0: self.p0 + offset,
            p1: self.p1 + offset,
            p2: self.p2 + offset,
        }
    }

    /// Linearly interpolates between this curve and another at parameter t.
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

// ============================================================================
// Pixels-specific operations
// ============================================================================

impl QuadBez<Pixels> {
    /// Returns the normalized direction vector at parameter t.
    #[inline]
    #[must_use]
    pub fn direction(&self, t: f32) -> Vec2<Pixels> {
        self.tangent(t).normalize_or(Vec2::X)
    }

    /// Computes an approximate bounding box for the curve.
    /// Computes an approximate bounding box for the curve.
    #[inline]
    #[must_use]
    pub fn bounding_box(&self) -> Rect<Pixels> {
        let min_x = self.p0.x.min(self.p1.x).min(self.p2.x);
        let min_y = self.p0.y.min(self.p1.y).min(self.p2.y);
        let max_x = self.p0.x.max(self.p1.x).max(self.p2.x);
        let max_y = self.p0.y.max(self.p1.y).max(self.p2.y);

        Rect::from_ltrb(min_x, min_y, max_x, max_y)
    }

    /// Computes the arc length of the curve using recursive subdivision.
    #[inline]
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

    /// Finds the nearest point on the curve to the given point using binary search.
    /// Reserved for future curve operations API.
    #[allow(dead_code)]
    #[must_use]
    fn nearest_t(&self, point: Point<Pixels>, t0: f32, t1: f32, iterations: u32) -> Point<Pixels> {
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

    /// Converts this quadratic Bézier to a cubic Bézier curve.
    #[inline]
    #[must_use]
    pub fn to_cubic(&self) -> CubicBez<Pixels> {
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
}

impl<T: Unit> fmt::Display for QuadBez<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "QuadBez({} -> {} -> {})", self.p0, self.p1, self.p2)
    }
}

// ============================================================================
// Cubic Bézier
// ============================================================================

/// Cubic Bézier curve with generic unit type.
///
/// A cubic Bézier curve is defined by four points: start (p0), control points (p1, p2), and end (p3).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBez<T: Unit> {
    /// Start point.
    pub p0: Point<T>,
    /// First control point.
    pub p1: Point<T>,
    /// Second control point.
    pub p2: Point<T>,
    /// End point.
    pub p3: Point<T>,
}

impl<T: Unit> Default for CubicBez<T> {
    fn default() -> Self {
        Self {
            p0: Point::default(),
            p1: Point::default(),
            p2: Point::default(),
            p3: Point::default(),
        }
    }
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> CubicBez<T> {
    /// Creates a new cubic Bézier curve from four points.
    #[inline]
    #[must_use]
    pub const fn new(p0: Point<T>, p1: Point<T>, p2: Point<T>, p3: Point<T>) -> Self {
        Self { p0, p1, p2, p3 }
    }

    /// Returns the start point of the curve.
    #[inline]
    #[must_use]
    pub const fn start(&self) -> Point<T> {
        self.p0
    }

    /// Returns the end point of the curve.
    #[inline]
    #[must_use]
    pub const fn end(&self) -> Point<T> {
        self.p3
    }

    /// Returns the first control point.
    #[inline]
    #[must_use]
    pub const fn control1(&self) -> Point<T> {
        self.p1
    }

    /// Returns the second control point.
    #[inline]
    #[must_use]
    pub const fn control2(&self) -> Point<T> {
        self.p2
    }
}

// ============================================================================
// Numeric Operations (NumericUnit with Into<f32> + From<f32>)
// ============================================================================

impl<T: NumericUnit> CubicBez<T>
where
    T: Into<f32> + From<f32>,
{
    /// Evaluates the curve at parameter t ∈ [0, 1].
    #[inline]
    #[must_use]
    pub fn eval(&self, t: f32) -> Point<T> {
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;
        let t2 = t * t;
        let t3 = t2 * t;

        let p0x: f32 = self.p0.x.into();
        let p0y: f32 = self.p0.y.into();
        let p1x: f32 = self.p1.x.into();
        let p1y: f32 = self.p1.y.into();
        let p2x: f32 = self.p2.x.into();
        let p2y: f32 = self.p2.y.into();
        let p3x: f32 = self.p3.x.into();
        let p3y: f32 = self.p3.y.into();

        Point::new(
            T::from(mt3 * p0x + 3.0 * mt2 * t * p1x + 3.0 * mt * t2 * p2x + t3 * p3x),
            T::from(mt3 * p0y + 3.0 * mt2 * t * p1y + 3.0 * mt * t2 * p2y + t3 * p3y),
        )
    }

    /// Returns the tangent vector at parameter t.
    #[inline]
    #[must_use]
    pub fn tangent(&self, t: f32) -> Vec2<T> {
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let t2 = t * t;

        let p0x: f32 = self.p0.x.into();
        let p0y: f32 = self.p0.y.into();
        let p1x: f32 = self.p1.x.into();
        let p1y: f32 = self.p1.y.into();
        let p2x: f32 = self.p2.x.into();
        let p2y: f32 = self.p2.y.into();
        let p3x: f32 = self.p3.x.into();
        let p3y: f32 = self.p3.y.into();

        Vec2::new(
            T::from(3.0 * mt2 * (p1x - p0x) + 6.0 * mt * t * (p2x - p1x) + 3.0 * t2 * (p3x - p2x)),
            T::from(3.0 * mt2 * (p1y - p0y) + 6.0 * mt * t * (p2y - p1y) + 3.0 * t2 * (p3y - p2y)),
        )
    }

    /// Splits the curve at parameter t into two curves.
    #[inline]
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

    /// Translates the curve by the given offset.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2<T>) -> Self {
        Self {
            p0: self.p0 + offset,
            p1: self.p1 + offset,
            p2: self.p2 + offset,
            p3: self.p3 + offset,
        }
    }

    /// Reverses the direction of the curve.
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

    /// Linearly interpolates between this curve and another at parameter t.
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
}

// ============================================================================
// Pixels-specific operations
// ============================================================================

impl CubicBez<Pixels> {
    /// Returns the normalized direction vector at parameter t.
    #[inline]
    #[must_use]
    pub fn direction(&self, t: f32) -> Vec2<Pixels> {
        self.tangent(t).normalize_or(Vec2::X)
    }

    /// Computes an approximate bounding box for the curve.
    #[inline]
    #[must_use]
    pub fn bounding_box(&self) -> Rect<Pixels> {
        let min_x = self.p0.x.min(self.p1.x).min(self.p2.x).min(self.p3.x);
        let min_y = self.p0.y.min(self.p1.y).min(self.p2.y).min(self.p3.y);
        let max_x = self.p0.x.max(self.p1.x).max(self.p2.x).max(self.p3.x);
        let max_y = self.p0.y.max(self.p1.y).max(self.p2.y).max(self.p3.y);

        Rect::from_ltrb(min_x, min_y, max_x, max_y)
    }

    /// Computes the arc length of the curve using recursive subdivision.
    #[inline]
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

    /// Finds the nearest point on the curve using subdivision.
    /// Reserved for future curve operations API.
    #[allow(dead_code)]
    #[must_use]
    fn nearest_point_subdivision(
        &self,
        point: Point<Pixels>,
        t0: f32,
        t1: f32,
        subdivisions: u32,
    ) -> Point<Pixels> {
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

    /// Flattens the curve into a series of line segments within the given tolerance.
    #[inline]
    #[must_use]
    pub fn flatten(&self, tolerance: f32) -> Vec<Point<Pixels>> {
        let mut points = vec![self.p0];
        self.flatten_recursive(0.0, 1.0, tolerance, &mut points);
        points
    }

    fn flatten_recursive(&self, t0: f32, t1: f32, tolerance: f32, points: &mut Vec<Point<Pixels>>) {
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

    /// Finds approximate intersection points with a line segment.
    #[inline]
    #[must_use]
    pub fn intersect_line(&self, line: &Line<Pixels>) -> Vec<Point<Pixels>> {
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

impl<T: Unit> fmt::Display for CubicBez<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CubicBez({} -> {} -> {} -> {})",
            self.p0, self.p1, self.p2, self.p3
        )
    }
}

// ============================================================================
// Tests
// ============================================================================
