//! Circle type.
//!
//! A circle defined by center point and radius.

use std::fmt;

use super::{Point, Rect, Vec2};

/// A circle defined by center and radius.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Circle, Point};
///
/// let circle = Circle::new(Point::new(50.0, 50.0), 25.0);
/// assert!(circle.contains(Point::new(50.0, 50.0)));
/// assert!(!circle.contains(Point::new(100.0, 100.0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Circle {
    /// Center point.
    pub center: Point,
    /// Radius (must be non-negative).
    pub radius: f32,
}

// ============================================================================
// Constants
// ============================================================================

impl Circle {
    /// A zero-sized circle at the origin.
    pub const ZERO: Self = Self {
        center: Point::ORIGIN,
        radius: 0.0,
    };

    /// A unit circle (radius 1) at the origin.
    pub const UNIT: Self = Self {
        center: Point::ORIGIN,
        radius: 1.0,
    };
}

// ============================================================================
// Constructors
// ============================================================================

impl Circle {
    /// Creates a new circle.
    #[inline]
    #[must_use]
    pub const fn new(center: Point, radius: f32) -> Self {
        Self { center, radius }
    }

    /// Creates a circle at the origin with the given radius.
    #[inline]
    #[must_use]
    pub const fn from_radius(radius: f32) -> Self {
        Self {
            center: Point::ORIGIN,
            radius,
        }
    }

    /// Creates a circle from center coordinates and radius.
    #[inline]
    #[must_use]
    pub const fn from_coords(cx: f32, cy: f32, radius: f32) -> Self {
        Self {
            center: Point::new(cx, cy),
            radius,
        }
    }

    /// Creates the smallest circle containing three points.
    ///
    /// Returns `None` if points are collinear.
    #[must_use]
    pub fn from_three_points(p1: Point, p2: Point, p3: Point) -> Option<Self> {
        // Using circumcircle formula
        let ax = p1.x;
        let ay = p1.y;
        let bx = p2.x;
        let by = p2.y;
        let cx = p3.x;
        let cy = p3.y;

        let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
        if d.abs() < f32::EPSILON {
            return None; // Collinear
        }

        let ax2_ay2 = ax * ax + ay * ay;
        let bx2_by2 = bx * bx + by * by;
        let cx2_cy2 = cx * cx + cy * cy;

        let ux = (ax2_ay2 * (by - cy) + bx2_by2 * (cy - ay) + cx2_cy2 * (ay - by)) / d;
        let uy = (ax2_ay2 * (cx - bx) + bx2_by2 * (ax - cx) + cx2_cy2 * (bx - ax)) / d;

        let center = Point::new(ux, uy);
        let radius = center.distance(p1);

        Some(Self { center, radius })
    }

    /// Creates a circle from a bounding rect (inscribed circle).
    ///
    /// The circle is inscribed in the rect, touching all four sides
    /// if the rect is square.
    #[inline]
    #[must_use]
    pub fn inscribed_in_rect(rect: Rect) -> Self {
        Self {
            center: rect.center(),
            radius: rect.width().min(rect.height()) / 2.0,
        }
    }

    /// Creates a circle from a bounding rect (circumscribed circle).
    ///
    /// The circle circumscribes the rect, passing through all four corners.
    #[inline]
    #[must_use]
    pub fn circumscribed_around_rect(rect: Rect) -> Self {
        let center = rect.center();
        let radius = center.distance(rect.min);
        Self { center, radius }
    }
}

// ============================================================================
// Accessors
// ============================================================================

impl Circle {
    /// Returns the diameter.
    #[inline]
    #[must_use]
    pub fn diameter(&self) -> f32 {
        self.radius * 2.0
    }

    /// Returns the circumference (perimeter).
    #[inline]
    #[must_use]
    pub fn circumference(&self) -> f32 {
        std::f32::consts::TAU * self.radius
    }

    /// Returns the area.
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        std::f32::consts::PI * self.radius * self.radius
    }

    /// Returns the bounding box.
    #[inline]
    #[must_use]
    pub fn bounding_box(&self) -> Rect {
        Rect::from_center_size(self.center, super::Size::splat(self.diameter()))
    }
}

// ============================================================================
// Queries
// ============================================================================

impl Circle {
    /// Returns `true` if the circle has zero radius.
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.radius == 0.0
    }

    /// Returns `true` if the radius is finite and non-negative.
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.radius >= 0.0 && self.radius.is_finite() && self.center.is_finite()
    }

    /// Returns `true` if the point is inside the circle (including boundary).
    #[inline]
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        self.center.distance_squared(point) <= self.radius * self.radius
    }

    /// Returns `true` if the point is strictly inside the circle (excluding boundary).
    #[inline]
    #[must_use]
    pub fn contains_strict(&self, point: Point) -> bool {
        self.center.distance_squared(point) < self.radius * self.radius
    }

    /// Returns `true` if this circle completely contains another circle.
    #[inline]
    #[must_use]
    pub fn contains_circle(&self, other: &Circle) -> bool {
        let dist = self.center.distance(other.center);
        dist + other.radius <= self.radius
    }

    /// Returns `true` if this circle overlaps with another circle.
    #[inline]
    #[must_use]
    pub fn overlaps(&self, other: &Circle) -> bool {
        let dist_sq = self.center.distance_squared(other.center);
        let radii_sum = self.radius + other.radius;
        dist_sq < radii_sum * radii_sum
    }

    /// Returns `true` if this circle overlaps with a rectangle.
    #[must_use]
    pub fn overlaps_rect(&self, rect: Rect) -> bool {
        // Find closest point on rect to circle center
        let closest_x = self.center.x.clamp(rect.min.x, rect.max.x);
        let closest_y = self.center.y.clamp(rect.min.y, rect.max.y);
        let closest = Point::new(closest_x, closest_y);

        self.contains(closest)
    }

    /// Returns the distance from the point to the circle boundary.
    ///
    /// Negative if inside, positive if outside.
    #[inline]
    #[must_use]
    pub fn signed_distance(&self, point: Point) -> f32 {
        self.center.distance(point) - self.radius
    }

    /// Returns the distance from the point to the circle boundary (always positive).
    #[inline]
    #[must_use]
    pub fn distance_to_point(&self, point: Point) -> f32 {
        self.signed_distance(point).abs()
    }

    /// Returns the point on the circle boundary closest to the given point.
    #[must_use]
    pub fn nearest_point(&self, point: Point) -> Point {
        if point == self.center {
            // Any point on boundary is equally close
            return Point::new(self.center.x + self.radius, self.center.y);
        }

        let dir = (point - self.center).normalize_or(Vec2::ZERO);
        self.center + dir * self.radius
    }

    /// Returns the point on the circle at the given angle (radians).
    ///
    /// 0 = right, PI/2 = top, PI = left, 3PI/2 = bottom
    #[inline]
    #[must_use]
    pub fn point_at_angle(&self, angle: f32) -> Point {
        Point::new(
            self.center.x + self.radius * angle.cos(),
            self.center.y + self.radius * angle.sin(),
        )
    }

    /// Returns the angle (radians) from center to the given point.
    #[inline]
    #[must_use]
    pub fn angle_to(&self, point: Point) -> f32 {
        (point.y - self.center.y).atan2(point.x - self.center.x)
    }
}

// ============================================================================
// Transformations
// ============================================================================

impl Circle {
    /// Returns a new circle translated by the given vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2) -> Self {
        Self {
            center: self.center + offset,
            radius: self.radius,
        }
    }

    /// Returns a new circle with the center at the given point.
    #[inline]
    #[must_use]
    pub const fn with_center(&self, center: Point) -> Self {
        Self {
            center,
            radius: self.radius,
        }
    }

    /// Returns a new circle with the given radius.
    #[inline]
    #[must_use]
    pub const fn with_radius(&self, radius: f32) -> Self {
        Self {
            center: self.center,
            radius,
        }
    }

    /// Returns a new circle scaled by the given factor.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            center: self.center,
            radius: self.radius * factor,
        }
    }

    /// Returns a circle expanded by the given amount.
    #[inline]
    #[must_use]
    pub fn inflate(&self, amount: f32) -> Self {
        Self {
            center: self.center,
            radius: (self.radius + amount).max(0.0),
        }
    }

    /// Returns a circle contracted by the given amount.
    #[inline]
    #[must_use]
    pub fn deflate(&self, amount: f32) -> Self {
        self.inflate(-amount)
    }

    /// Linear interpolation between two circles.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            center: self.center.lerp(other.center, t),
            radius: self.radius + (other.radius - self.radius) * t,
        }
    }
}

// ============================================================================
// Intersections
// ============================================================================

impl Circle {
    /// Returns the intersection points with another circle.
    ///
    /// Returns:
    /// - `None` if circles don't intersect or are identical
    /// - `Some((p1, p2))` for two intersection points (may be the same for tangent)
    #[must_use]
    pub fn intersect_circle(&self, other: &Circle) -> Option<(Point, Point)> {
        let d = self.center.distance(other.center);

        // No intersection if too far apart or one contains the other
        if d > self.radius + other.radius || d < (self.radius - other.radius).abs() {
            return None;
        }

        // Identical circles
        if d < f32::EPSILON && (self.radius - other.radius).abs() < f32::EPSILON {
            return None;
        }

        let a = (self.radius * self.radius - other.radius * other.radius + d * d) / (2.0 * d);
        let h_sq = self.radius * self.radius - a * a;
        if h_sq < 0.0 {
            return None;
        }
        let h = h_sq.sqrt();

        let dir = (other.center - self.center) / d;
        let p = self.center + dir * a;

        let perp = Vec2::new(-dir.y, dir.x);

        Some((p + perp * h, p - perp * h))
    }

    /// Returns the intersection points with a line.
    ///
    /// Returns:
    /// - `None` if line doesn't intersect circle
    /// - `Some((p1, p2))` for two intersection points (may be the same for tangent)
    #[must_use]
    pub fn intersect_line(&self, line: &super::Line) -> Option<(Point, Point)> {
        let d = line.to_vec();
        let f = line.p0 - self.center;

        let a = d.dot(d);
        let b = 2.0 * f.dot(d);
        let c = f.dot(f) - self.radius * self.radius;

        let discriminant = b * b - 4.0 * a * c;

        if discriminant < 0.0 {
            return None;
        }

        let sqrt_disc = discriminant.sqrt();
        let t1 = (-b - sqrt_disc) / (2.0 * a);
        let t2 = (-b + sqrt_disc) / (2.0 * a);

        Some((line.eval(t1), line.eval(t2)))
    }
}

// ============================================================================
// Display
// ============================================================================

impl fmt::Display for Circle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Circle({}, r={})", self.center, self.radius)
    }
}

// ============================================================================
// Default
// ============================================================================

impl Default for Circle {
    fn default() -> Self {
        Self::ZERO
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Circle::new(center, radius)`.
#[inline]
#[must_use]
pub fn circle(center: Point, radius: f32) -> Circle {
    Circle::new(center, radius)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construction() {
        let c = Circle::new(Point::new(10.0, 20.0), 5.0);
        assert_eq!(c.center, Point::new(10.0, 20.0));
        assert_eq!(c.radius, 5.0);
    }

    #[test]
    fn test_from_radius() {
        let c = Circle::from_radius(10.0);
        assert_eq!(c.center, Point::ORIGIN);
        assert_eq!(c.radius, 10.0);
    }

    #[test]
    fn test_diameter_circumference_area() {
        let c = Circle::from_radius(1.0);
        assert_eq!(c.diameter(), 2.0);
        assert!((c.circumference() - std::f32::consts::TAU).abs() < 0.001);
        assert!((c.area() - std::f32::consts::PI).abs() < 0.001);
    }

    #[test]
    fn test_bounding_box() {
        let c = Circle::new(Point::new(10.0, 10.0), 5.0);
        let bbox = c.bounding_box();
        assert_eq!(bbox.min, Point::new(5.0, 5.0));
        assert_eq!(bbox.max, Point::new(15.0, 15.0));
    }

    #[test]
    fn test_contains() {
        let c = Circle::new(Point::new(0.0, 0.0), 10.0);

        assert!(c.contains(Point::new(0.0, 0.0))); // center
        assert!(c.contains(Point::new(10.0, 0.0))); // on boundary
        assert!(c.contains(Point::new(5.0, 5.0))); // inside
        assert!(!c.contains(Point::new(10.0, 10.0))); // outside
    }

    #[test]
    fn test_contains_circle() {
        let big = Circle::new(Point::ORIGIN, 10.0);
        let small = Circle::new(Point::new(2.0, 0.0), 3.0);
        let outside = Circle::new(Point::new(20.0, 0.0), 3.0);

        assert!(big.contains_circle(&small));
        assert!(!small.contains_circle(&big));
        assert!(!big.contains_circle(&outside));
    }

    #[test]
    fn test_overlaps() {
        let c1 = Circle::new(Point::new(0.0, 0.0), 5.0);
        let c2 = Circle::new(Point::new(8.0, 0.0), 5.0);
        let c3 = Circle::new(Point::new(20.0, 0.0), 5.0);

        assert!(c1.overlaps(&c2)); // overlapping
        assert!(!c1.overlaps(&c3)); // too far
    }

    #[test]
    fn test_overlaps_rect() {
        let c = Circle::new(Point::new(0.0, 0.0), 5.0);

        let inside = Rect::from_xywh(-2.0, -2.0, 4.0, 4.0);
        let overlapping = Rect::from_xywh(3.0, 3.0, 10.0, 10.0);
        let outside = Rect::from_xywh(20.0, 20.0, 10.0, 10.0);

        assert!(c.overlaps_rect(inside));
        assert!(c.overlaps_rect(overlapping));
        assert!(!c.overlaps_rect(outside));
    }

    #[test]
    fn test_signed_distance() {
        let c = Circle::from_radius(10.0);

        assert_eq!(c.signed_distance(Point::ORIGIN), -10.0); // center
        assert_eq!(c.signed_distance(Point::new(10.0, 0.0)), 0.0); // boundary
        assert_eq!(c.signed_distance(Point::new(15.0, 0.0)), 5.0); // outside
    }

    #[test]
    fn test_nearest_point() {
        let c = Circle::new(Point::ORIGIN, 10.0);

        let p = c.nearest_point(Point::new(20.0, 0.0));
        assert!((p.x - 10.0).abs() < 0.001);
        assert!(p.y.abs() < 0.001);
    }

    #[test]
    fn test_point_at_angle() {
        let c = Circle::from_radius(10.0);

        let p = c.point_at_angle(0.0);
        assert!((p.x - 10.0).abs() < 0.001);
        assert!(p.y.abs() < 0.001);

        let p = c.point_at_angle(std::f32::consts::FRAC_PI_2);
        assert!(p.x.abs() < 0.001);
        assert!((p.y - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_translate() {
        let c = Circle::new(Point::ORIGIN, 5.0);
        let translated = c.translate(Vec2::new(10.0, 10.0));
        assert_eq!(translated.center, Point::new(10.0, 10.0));
        assert_eq!(translated.radius, 5.0);
    }

    #[test]
    fn test_scale() {
        let c = Circle::from_radius(5.0);
        let scaled = c.scale(2.0);
        assert_eq!(scaled.radius, 10.0);
    }

    #[test]
    fn test_inflate_deflate() {
        let c = Circle::from_radius(10.0);
        assert_eq!(c.inflate(5.0).radius, 15.0);
        assert_eq!(c.deflate(5.0).radius, 5.0);
        assert_eq!(c.deflate(20.0).radius, 0.0); // clamped to 0
    }

    #[test]
    fn test_lerp() {
        let c1 = Circle::new(Point::new(0.0, 0.0), 10.0);
        let c2 = Circle::new(Point::new(10.0, 10.0), 20.0);
        let mid = c1.lerp(c2, 0.5);
        assert_eq!(mid.center, Point::new(5.0, 5.0));
        assert_eq!(mid.radius, 15.0);
    }

    #[test]
    fn test_intersect_circles() {
        let c1 = Circle::new(Point::new(0.0, 0.0), 5.0);
        let c2 = Circle::new(Point::new(6.0, 0.0), 5.0);

        let result = c1.intersect_circle(&c2);
        assert!(result.is_some());
    }

    #[test]
    fn test_intersect_circles_no_intersection() {
        let c1 = Circle::new(Point::new(0.0, 0.0), 5.0);
        let c2 = Circle::new(Point::new(20.0, 0.0), 5.0);

        assert!(c1.intersect_circle(&c2).is_none());
    }

    #[test]
    fn test_inscribed_in_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 10.0, 20.0);
        let c = Circle::inscribed_in_rect(rect);
        assert_eq!(c.center, Point::new(5.0, 10.0));
        assert_eq!(c.radius, 5.0); // min(10, 20) / 2
    }

    #[test]
    fn test_circumscribed_around_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 6.0, 8.0);
        let c = Circle::circumscribed_around_rect(rect);
        assert_eq!(c.center, Point::new(3.0, 4.0));
        assert_eq!(c.radius, 5.0); // distance from center to corner
    }

    #[test]
    fn test_convenience_fn() {
        let c = circle(Point::new(1.0, 2.0), 3.0);
        assert_eq!(c.center, Point::new(1.0, 2.0));
        assert_eq!(c.radius, 3.0);
    }
}
