//! Circle type.
//!
//! A circle defined by center point and radius.
//!
//! # Type Safety
//!
//! `Circle<T>` is generic over unit type `T`, preventing accidental mixing
//! of coordinate systems:
//!
//! ```ignore
//! use flui_types::geometry::{Circle, Point, Pixels, px};
//!
//! let ui_circle = Circle::<Pixels>::new(
//!     Point::new(px(50.0), px(50.0)),
//!     px(25.0)
//! );
//!
//! // Convert to f32 for GPU
//! let gpu_circle: Circle<f32> = ui_circle.to_f32();
//! ```

use std::fmt;

use super::traits::{NumericUnit, Unit};
use super::{Point, Radians, Rect, Size, Vec2};

/// A circle defined by center and radius.
///
/// Generic over unit type `T`. Common usage:
/// - `Circle<f32>` - Raw coordinates (GPU-ready)
/// - `Circle<Pixels>` - Logical pixel coordinates
/// - `Circle<DevicePixels>` - Physical pixel coordinates
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Circle, point};
///
/// let circle = Circle::new(point(50.0, 50.0), 25.0);
/// assert!(circle.contains(point(50.0, 50.0)));
/// assert!(!circle.contains(point(100.0, 100.0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Circle<T: Unit = f32> {
    /// Center point.
    pub center: Point<T>,
    /// Radius (must be non-negative).
    pub radius: T,
}

// ============================================================================
// Constants (f32 only)
// ============================================================================

impl Circle<f32> {
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
// Generic Constructors
// ============================================================================

impl<T: Unit> Circle<T> {
    /// Creates a new circle.
    #[inline]
    #[must_use]
    pub const fn new(center: Point<T>, radius: T) -> Self {
        Self { center, radius }
    }

    /// Returns a new circle with the center at the given point.
    #[inline]
    #[must_use]
    pub const fn with_center(&self, center: Point<T>) -> Self {
        Self {
            center,
            radius: self.radius,
        }
    }

    /// Returns a new circle with the given radius.
    #[inline]
    #[must_use]
    pub const fn with_radius(&self, radius: T) -> Self {
        Self {
            center: self.center,
            radius,
        }
    }

    /// Maps the circle through a function.
    #[inline]
    #[must_use]
    pub fn map<U: Unit>(&self, f: impl Fn(T) -> U + Copy) -> Circle<U>
    where
        T: fmt::Debug + Default + PartialEq,
    {
        Circle {
            center: self.center.map(f),
            radius: f(self.radius),
        }
    }
}

// ============================================================================
// f32-specific Constructors
// ============================================================================

impl Circle<f32> {
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
    pub fn from_three_points(p1: Point<f32>, p2: Point<f32>, p3: Point<f32>) -> Option<Self> {
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
// Accessors (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32> + From<f32>,
{
    /// Returns the diameter.
    #[inline]
    #[must_use]
    pub fn diameter(&self) -> T {
        T::mul(self.radius, 2.0)
    }

    /// Returns the circumference (perimeter).
    #[inline]
    #[must_use]
    pub fn circumference(&self) -> f32 {
        std::f32::consts::TAU * self.radius.into()
    }

    /// Returns the area.
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        let r: f32 = self.radius.into();
        std::f32::consts::PI * r * r
    }

    /// Returns the bounding box.
    #[inline]
    #[must_use]
    pub fn bounding_box(&self) -> super::Bounds<T>
    where
        T: std::ops::Add<T, Output = T> + std::ops::Sub<T, Output = T> + std::ops::Div<f32, Output = T>,
    {
        let diameter = self.diameter();
        super::Bounds::centered_at(self.center, Size::new(diameter, diameter))
    }
}

// ============================================================================
// Queries (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32> + From<f32> + PartialOrd,
{
    /// Returns `true` if the circle has zero radius.
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.radius.into() == 0.0
    }

    /// Returns `true` if the radius is finite and non-negative.
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let r: f32 = self.radius.into();
        r >= 0.0 && r.is_finite() && self.center.is_finite()
    }

    /// Returns `true` if the point is inside the circle (including boundary).
    #[inline]
    #[must_use]
    pub fn contains(&self, point: Point<T>) -> bool {
        let r: f32 = self.radius.into();
        self.center.distance_squared(point) <= r * r
    }

    /// Returns `true` if the point is strictly inside the circle (excluding boundary).
    #[inline]
    #[must_use]
    pub fn contains_strict(&self, point: Point<T>) -> bool {
        let r: f32 = self.radius.into();
        self.center.distance_squared(point) < r * r
    }

    /// Returns `true` if this circle completely contains another circle.
    #[inline]
    #[must_use]
    pub fn contains_circle(&self, other: &Circle<T>) -> bool {
        let dist = self.center.distance(other.center);
        let my_r: f32 = self.radius.into();
        let other_r: f32 = other.radius.into();
        dist + other_r <= my_r
    }

    /// Returns `true` if this circle overlaps with another circle.
    #[inline]
    #[must_use]
    pub fn overlaps(&self, other: &Circle<T>) -> bool {
        let dist_sq = self.center.distance_squared(other.center);
        let my_r: f32 = self.radius.into();
        let other_r: f32 = other.radius.into();
        let radii_sum = my_r + other_r;
        dist_sq < radii_sum * radii_sum
    }

    /// Returns the distance from the point to the circle boundary.
    ///
    /// Negative if inside, positive if outside.
    #[inline]
    #[must_use]
    pub fn signed_distance(&self, point: Point<T>) -> f32 {
        self.center.distance(point) - self.radius.into()
    }

    /// Returns the distance from the point to the circle boundary (always positive).
    #[inline]
    #[must_use]
    pub fn distance_to_point(&self, point: Point<T>) -> f32 {
        self.signed_distance(point).abs()
    }

    /// Returns the point on the circle boundary closest to the given point.
    #[must_use]
    pub fn nearest_point(&self, point: Point<T>) -> Point<T> {
        let center_f32 = self.center.to_f32();
        let point_f32 = point.to_f32();

        if point_f32 == center_f32 {
            // Any point on boundary is equally close
            let r: f32 = self.radius.into();
            return Point::new(
                T::from(center_f32.x + r),
                T::from(center_f32.y),
            );
        }

        let dir = (point_f32 - center_f32).normalize_or(Vec2::ZERO);
        let r: f32 = self.radius.into();
        let result = center_f32 + dir * r;
        Point::new(T::from(result.x), T::from(result.y))
    }

    /// Returns the point on the circle at the given angle.
    ///
    /// 0 = right, π/2 = top, π = left, 3π/2 = bottom
    #[inline]
    #[must_use]
    pub fn point_at_angle(&self, angle: Radians) -> Point<T> {
        let center_f32 = self.center.to_f32();
        let r: f32 = self.radius.into();
        Point::new(
            T::from(center_f32.x + r * angle.get().cos()),
            T::from(center_f32.y + r * angle.get().sin()),
        )
    }

    /// Returns the angle from center to the given point.
    #[inline]
    #[must_use]
    pub fn angle_to(&self, point: Point<T>) -> Radians {
        let center_f32 = self.center.to_f32();
        let point_f32 = point.to_f32();
        Radians::new((point_f32.y - center_f32.y).atan2(point_f32.x - center_f32.x))
    }
}

// ============================================================================
// f32-specific Queries (Rect operations)
// ============================================================================

impl Circle<f32> {
    /// Returns `true` if this circle overlaps with a rectangle.
    #[must_use]
    pub fn overlaps_rect(&self, rect: Rect) -> bool {
        // Find closest point on rect to circle center
        let closest_x = self.center.x.clamp(rect.min.x, rect.max.x);
        let closest_y = self.center.y.clamp(rect.min.y, rect.max.y);
        let closest = Point::new(closest_x, closest_y);

        self.contains(closest)
    }
}

// ============================================================================
// Transformations (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32> + From<f32>,
{
    /// Returns a new circle translated by the given vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2<T>) -> Self {
        Self {
            center: self.center + offset,
            radius: self.radius,
        }
    }

    /// Returns a new circle scaled by the given factor.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            center: self.center,
            radius: T::from(self.radius.into() * factor),
        }
    }

    /// Returns a circle expanded by the given amount.
    #[inline]
    #[must_use]
    pub fn inflate(&self, amount: T) -> Self {
        let new_radius = (self.radius.into() + amount.into()).max(0.0);
        Self {
            center: self.center,
            radius: T::from(new_radius),
        }
    }

    /// Returns a circle contracted by the given amount.
    #[inline]
    #[must_use]
    pub fn deflate(&self, amount: T) -> Self
    where
        T: std::ops::Neg<Output = T>,
    {
        self.inflate(-amount)
    }

    /// Linear interpolation between two circles.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let r1: f32 = self.radius.into();
        let r2: f32 = other.radius.into();
        Self {
            center: self.center.lerp(other.center, t),
            radius: T::from(r1 + (r2 - r1) * t),
        }
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl<T: NumericUnit> Circle<T>
where
    T: Into<f32>,
{
    /// Converts to `Circle<f32>`.
    #[inline]
    #[must_use]
    pub fn to_f32(&self) -> Circle<f32> {
        Circle {
            center: self.center.to_f32(),
            radius: self.radius.into(),
        }
    }
}

impl<T: Unit> Circle<T>
where
    T: Into<f32>,
{
    /// Converts to array `[cx, cy, r]` for GPU usage.
    #[inline]
    #[must_use]
    pub fn to_array(&self) -> [f32; 3] {
        [
            self.center.x.into(),
            self.center.y.into(),
            self.radius.into(),
        ]
    }
}

// ============================================================================
// Intersections (f32 only - complex math)
// ============================================================================

impl Circle<f32> {
    /// Returns the intersection points with another circle.
    ///
    /// Returns:
    /// - `None` if circles don't intersect or are identical
    /// - `Some((p1, p2))` for two intersection points (may be the same for tangent)
    #[must_use]
    pub fn intersect_circle(&self, other: &Circle<f32>) -> Option<(Point<f32>, Point<f32>)> {
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
    pub fn intersect_line(&self, line: &super::Line) -> Option<(Point<f32>, Point<f32>)> {
        let d = line.to_vec();
        let f = line.p0 - self.center;

        let a = d.dot(&d);
        let b = 2.0 * f.dot(&d);
        let c = f.dot(&f) - self.radius * self.radius;

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
// Specialized implementations for Pixels
// ============================================================================

impl Circle<super::Pixels> {
    /// Scales the circle by a given factor, producing `Circle<ScaledPixels>`.
    #[inline]
    #[must_use]
    pub fn scale_to_scaled(&self, factor: f32) -> Circle<super::ScaledPixels> {
        Circle {
            center: self.center.scale(factor),
            radius: super::ScaledPixels(self.radius.get() * factor),
        }
    }
}

// ============================================================================
// Display
// ============================================================================

impl<T> fmt::Display for Circle<T>
where
    T: Unit + fmt::Display + Clone + fmt::Debug + Default + PartialEq,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Circle({}, r={})", self.center, self.radius)
    }
}

// ============================================================================
// Default
// ============================================================================

impl<T: Unit> Default for Circle<T> {
    fn default() -> Self {
        Self {
            center: Point::new(T::zero(), T::zero()),
            radius: T::zero(),
        }
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Circle::new(center, radius)`.
#[inline]
#[must_use]
pub fn circle(center: Point<f32>, radius: f32) -> Circle<f32> {
    Circle::new(center, radius)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{point, px, radians, Pixels};
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn test_construction() {
        let c = Circle::new(point(10.0, 20.0), 5.0);
        assert_eq!(c.center, point(10.0, 20.0));
        assert_eq!(c.radius, 5.0);
    }

    #[test]
    fn test_generic_construction() {
        let c = Circle::<Pixels>::new(Point::new(px(10.0), px(20.0)), px(5.0));
        assert_eq!(c.center, Point::new(px(10.0), px(20.0)));
        assert_eq!(c.radius, px(5.0));
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
        let c = Circle::new(point(10.0, 10.0), 5.0);
        let bbox = c.bounding_box();
        assert_eq!(bbox.origin, point(5.0, 5.0));
        assert_eq!(bbox.size.width, 10.0);
        assert_eq!(bbox.size.height, 10.0);
    }

    #[test]
    fn test_contains() {
        let c = Circle::new(point(0.0, 0.0), 10.0);

        assert!(c.contains(point(0.0, 0.0))); // center
        assert!(c.contains(point(10.0, 0.0))); // on boundary
        assert!(c.contains(point(5.0, 5.0))); // inside
        assert!(!c.contains(point(10.0, 10.0))); // outside
    }

    #[test]
    fn test_contains_circle() {
        let big = Circle::new(Point::ORIGIN, 10.0);
        let small = Circle::new(point(2.0, 0.0), 3.0);
        let outside = Circle::new(point(20.0, 0.0), 3.0);

        assert!(big.contains_circle(&small));
        assert!(!small.contains_circle(&big));
        assert!(!big.contains_circle(&outside));
    }

    #[test]
    fn test_overlaps() {
        let c1 = Circle::new(point(0.0, 0.0), 5.0);
        let c2 = Circle::new(point(8.0, 0.0), 5.0);
        let c3 = Circle::new(point(20.0, 0.0), 5.0);

        assert!(c1.overlaps(&c2)); // overlapping
        assert!(!c1.overlaps(&c3)); // too far
    }

    #[test]
    fn test_overlaps_rect() {
        let c = Circle::new(point(0.0, 0.0), 5.0);

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
        assert_eq!(c.signed_distance(point(10.0, 0.0)), 0.0); // boundary
        assert_eq!(c.signed_distance(point(15.0, 0.0)), 5.0); // outside
    }

    #[test]
    fn test_nearest_point() {
        let c = Circle::new(Point::ORIGIN, 10.0);

        let p = c.nearest_point(point(20.0, 0.0));
        assert!((p.x - 10.0).abs() < 0.001);
        assert!(p.y.abs() < 0.001);
    }

    #[test]
    fn test_point_at_angle() {
        let c = Circle::from_radius(10.0);

        let p = c.point_at_angle(radians(0.0));
        assert!((p.x - 10.0).abs() < 0.001);
        assert!(p.y.abs() < 0.001);

        let p = c.point_at_angle(radians(FRAC_PI_2));
        assert!(p.x.abs() < 0.001);
        assert!((p.y - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_angle_to() {
        let c = Circle::from_radius(10.0);

        let angle = c.angle_to(point(10.0, 0.0));
        assert!(angle.get().abs() < 0.001);

        let angle = c.angle_to(point(0.0, 10.0));
        assert!((angle.get() - FRAC_PI_2).abs() < 0.001);
    }

    #[test]
    fn test_translate() {
        let c = Circle::new(Point::ORIGIN, 5.0);
        let translated = c.translate(Vec2::new(10.0, 10.0));
        assert_eq!(translated.center, point(10.0, 10.0));
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
        let c1 = Circle::new(point(0.0, 0.0), 10.0);
        let c2 = Circle::new(point(10.0, 10.0), 20.0);
        let mid = c1.lerp(c2, 0.5);
        assert_eq!(mid.center, point(5.0, 5.0));
        assert_eq!(mid.radius, 15.0);
    }

    #[test]
    fn test_intersect_circles() {
        let c1 = Circle::new(point(0.0, 0.0), 5.0);
        let c2 = Circle::new(point(6.0, 0.0), 5.0);

        let result = c1.intersect_circle(&c2);
        assert!(result.is_some());
    }

    #[test]
    fn test_intersect_circles_no_intersection() {
        let c1 = Circle::new(point(0.0, 0.0), 5.0);
        let c2 = Circle::new(point(20.0, 0.0), 5.0);

        assert!(c1.intersect_circle(&c2).is_none());
    }

    #[test]
    fn test_inscribed_in_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 10.0, 20.0);
        let c = Circle::inscribed_in_rect(rect);
        assert_eq!(c.center, point(5.0, 10.0));
        assert_eq!(c.radius, 5.0); // min(10, 20) / 2
    }

    #[test]
    fn test_circumscribed_around_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 6.0, 8.0);
        let c = Circle::circumscribed_around_rect(rect);
        assert_eq!(c.center, point(3.0, 4.0));
        assert_eq!(c.radius, 5.0); // distance from center to corner
    }

    #[test]
    fn test_convenience_fn() {
        let c = circle(point(1.0, 2.0), 3.0);
        assert_eq!(c.center, point(1.0, 2.0));
        assert_eq!(c.radius, 3.0);
    }

    #[test]
    fn test_to_f32() {
        let c = Circle::<Pixels>::new(Point::new(px(10.0), px(20.0)), px(5.0));
        let f = c.to_f32();
        assert_eq!(f.center, point(10.0, 20.0));
        assert_eq!(f.radius, 5.0);
    }

    #[test]
    fn test_to_array() {
        let c = circle(point(10.0, 20.0), 5.0);
        let arr = c.to_array();
        assert_eq!(arr, [10.0, 20.0, 5.0]);
    }

    #[test]
    fn test_map() {
        let c = circle(point(10.0, 20.0), 5.0);
        let doubled = c.map(|x| x * 2.0);
        assert_eq!(doubled.center, point(20.0, 40.0));
        assert_eq!(doubled.radius, 10.0);
    }

    #[test]
    fn test_default() {
        let c: Circle<f32> = Circle::default();
        assert_eq!(c.center, Point::ORIGIN);
        assert_eq!(c.radius, 0.0);
    }
}
