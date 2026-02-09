//! Line segment type.
//!
//! A line segment defined by two endpoints.
//!
//! # Type Safety
//!
//! `Line<T>` is generic over unit type `T`, preventing accidental mixing
//! of coordinate systems.

use super::{px, Pixels};
use std::fmt;

use super::traits::{NumericUnit, Unit};
use super::{Point, Vec2};

/// A line segment defined by two endpoints with generic unit type.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line<T: Unit> {
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
    /// Creates a new line segment from two points.
    #[inline]
    #[must_use]
    pub const fn new(p0: Point<T>, p1: Point<T>) -> Self {
        Self { p0, p1 }
    }

    /// Returns the start point of the line segment.
    #[inline]
    #[must_use]
    pub const fn start(&self) -> Point<T> {
        self.p0
    }

    /// Returns the end point of the line segment.
    #[inline]
    #[must_use]
    pub const fn end(&self) -> Point<T> {
        self.p1
    }

    /// Returns a line with reversed direction (swaps start and end points).
    #[inline]
    #[must_use]
    pub const fn reverse(&self) -> Self {
        Self {
            p0: self.p1,
            p1: self.p0,
        }
    }

    /// Maps this line to a different unit type using the provided function.
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
// Accessors (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Line<T>
where
    T: Into<f32> + From<f32> + std::ops::Sub<Output = T>,
{
    /// Returns the length of the line segment.
    #[inline]
    #[must_use]
    pub fn length(&self) -> f32 {
        self.p0.distance(self.p1)
    }

    /// Returns the squared length of the line segment (faster than length).
    #[inline]
    #[must_use]
    pub fn length_squared(&self) -> f32 {
        self.p0.distance_squared(self.p1)
    }

    /// Converts the line segment to a direction vector.
    #[inline]
    #[must_use]
    pub fn to_vec(&self) -> Vec2<T>
    where
        T: std::ops::Sub<Output = T> + Copy,
    {
        Vec2::new(self.p1.x - self.p0.x, self.p1.y - self.p0.y)
    }

    /// Returns the normalized direction vector of the line segment.
    #[inline]
    #[must_use]
    pub fn direction(&self) -> Vec2<Pixels> {
        self.to_vec().normalize_or(Vec2::ZERO)
    }

    /// Returns the midpoint of the line segment.
    #[inline]
    #[must_use]
    pub fn midpoint(&self) -> Point<T> {
        self.p0.midpoint(self.p1)
    }

    /// Evaluates a point along the line segment at parameter t ∈ [0, 1].
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
    /// Returns true if the line segment has zero length (both points are equal).
    #[inline]
    #[must_use]
    pub fn is_zero_length(&self) -> bool {
        self.p0 == self.p1
    }

    /// Returns true if the line segment is horizontal (y-coordinates are equal within epsilon).
    #[inline]
    #[must_use]
    pub fn is_horizontal(&self) -> bool {
        let y0: f32 = self.p0.y.into();
        let y1: f32 = self.p1.y.into();
        (y0 - y1).abs() < f32::EPSILON
    }

    /// Returns true if the line segment is vertical (x-coordinates are equal within epsilon).
    #[inline]
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        let x0: f32 = self.p0.x.into();
        let x1: f32 = self.p1.x.into();
        (x0 - x1).abs() < f32::EPSILON
    }

    /// Returns the nearest point on the line segment to the given point.
    ///
    /// If the nearest point falls outside the segment, returns the closest endpoint.
    #[inline]
    #[must_use]
    pub fn nearest_point(&self, point: Point<T>) -> Point<T>
    where
        T: Into<f32> + Copy,
    {
        // Extract f32 values for calculation
        let px: f32 = point.x.into();
        let py: f32 = point.y.into();
        let p0x: f32 = self.p0.x.into();
        let p0y: f32 = self.p0.y.into();
        let p1x: f32 = self.p1.x.into();
        let p1y: f32 = self.p1.y.into();

        let dx = p1x - p0x;
        let dy = p1y - p0y;
        let length_sq = dx * dx + dy * dy;

        if length_sq < f32::EPSILON {
            return self.p0;
        }

        let vx = px - p0x;
        let vy = py - p0y;
        let dot = vx * dx + vy * dy;
        let t = (dot / length_sq).clamp(0.0, 1.0);
        self.eval(t)
    }

    /// Returns the shortest distance from a point to the line segment.
    #[inline]
    #[must_use]
    pub fn distance_to_point(&self, point: Point<T>) -> f32 {
        point.distance(self.nearest_point(point))
    }

    /// Returns the squared distance from a point to the line segment (faster than distance_to_point).
    #[inline]
    #[must_use]
    pub fn distance_squared_to_point(&self, point: Point<T>) -> f32 {
        point.distance_squared(self.nearest_point(point))
    }

    /// Returns true if a point is within the specified tolerance distance of the line segment.
    #[inline]
    #[must_use]
    pub fn is_point_near(&self, point: Point<T>, tolerance: f32) -> bool {
        self.distance_squared_to_point(point) <= tolerance * tolerance
    }
}

// ============================================================================
// Transformations (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Line<T>
where
    T: Into<f32> + From<f32>,
{
    /// Translates the line segment by the given offset vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2<T>) -> Self {
        Self {
            p0: self.p0 + offset,
            p1: self.p1 + offset,
        }
    }

    /// Linearly interpolates between two line segments.
    ///
    /// - `t = 0.0` returns `self`
    /// - `t = 1.0` returns `other`
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
    /// Converts the line segment to use `Pixels` units.
    #[inline]
    #[must_use]
    pub fn to_f32(&self) -> Line<Pixels> {
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
    /// Converts the line segment to an array `[x0, y0, x1, y1]` for GPU buffers.
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

impl Line<Pixels> {
    /// Computes the intersection point of this line with another line segment.
    ///
    /// Returns `Some(point)` if the lines intersect, `None` otherwise.
    /// This only works for `Line<Pixels>` due to the complex mathematical operations.
    #[inline]
    #[must_use]
    pub fn intersect_segment(&self, other: &Line<Pixels>) -> Option<Point<Pixels>> {
        let p0_x = self.p0.x.0;
        let p0_y = self.p0.y.0;
        let p1_x = self.p1.x.0;
        let p1_y = self.p1.y.0;

        let p2_x = other.p0.x.0;
        let p2_y = other.p0.y.0;
        let p3_x = other.p1.x.0;
        let p3_y = other.p1.y.0;

        let s1_x = p1_x - p0_x;
        let s1_y = p1_y - p0_y;
        let s2_x = p3_x - p2_x;
        let s2_y = p3_y - p2_y;

        let denom = -s2_x * s1_y + s1_x * s2_y;

        if denom.abs() < f32::EPSILON {
            return None; // Lines are parallel or coincident
        }

        let s = (-s1_y * (p0_x - p2_x) + s1_x * (p0_y - p2_y)) / denom;
        let t = (s2_x * (p0_y - p2_y) - s2_y * (p0_x - p2_x)) / denom;

        if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
            Some(Point::new(px(p0_x + t * s1_x), px(p0_y + t * s1_y)))
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
    T: super::traits::IsZero,
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.p0.is_zero() && self.p1.is_zero()
    }
}

impl<T: Unit> super::traits::ApproxEq for Line<T>
where
    T: super::traits::ApproxEq,
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        self.p0.approx_eq_eps(&other.p0, epsilon) && self.p1.approx_eq_eps(&other.p1, epsilon)
    }
}

// ============================================================================
// Additional Generic Methods
// ============================================================================

impl<T: Unit> Line<T> {
    /// Converts the line segment to a different unit type.
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

    /// Alias for `reverse()` - swaps the start and end points.
    #[inline]
    #[must_use]
    pub fn swap(self) -> Self {
        self.reverse()
    }
}

impl Line<Pixels> {
    /// A zero-length line at the origin.
    pub const ZERO: Self = Self {
        p0: Point::ORIGIN,
        p1: Point::ORIGIN,
    };

    /// Returns true if both endpoints have valid (finite) coordinates.
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.p0.is_valid() && self.p1.is_valid()
    }

    /// Linearly interpolates between two line segments (alias for `lerp`).
    #[inline]
    #[must_use]
    pub fn lerp_line(&self, other: &Self, t: f32) -> Self {
        Self {
            p0: self.p0.lerp(other.p0, t),
            p1: self.p1.lerp(other.p1, t),
        }
    }

    /// Returns a perpendicular line segment of the same length, centered at the midpoint.
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

    /// Extends the line segment in both directions by the specified amount.
    #[inline]
    #[must_use]
    pub fn extend(&self, amount: f32) -> Self {
        let dir = self.direction();
        Self {
            p0: self.p0 - dir * amount,
            p1: self.p1 + dir * amount,
        }
    }

    /// Shrinks the line segment from both ends by the specified amount.
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
