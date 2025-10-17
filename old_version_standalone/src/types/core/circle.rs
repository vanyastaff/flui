//! Circle and arc types for circular geometry.
//!
//! This module provides type-safe circle and arc types for circular shapes and paths.

use super::point::Point;
use super::rect::Rect;
use super::rotation::Rotation;
use std::f32::consts::PI;

/// Circle defined by center point and radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    /// Center point of the circle.
    pub center: Point,
    /// Radius of the circle.
    pub radius: f32,
}

impl Circle {
    /// Unit circle at origin.
    pub const UNIT: Circle = Circle {
        center: Point::ZERO,
        radius: 1.0,
    };

    /// Create a new circle.
    pub const fn new(center: Point, radius: f32) -> Self {
        Self { center, radius }
    }

    /// Create a circle at the origin with the given radius.
    pub const fn from_radius(radius: f32) -> Self {
        Self {
            center: Point::ZERO,
            radius,
        }
    }

    /// Create a circle from diameter instead of radius.
    pub fn from_diameter(center: impl Into<Point>, diameter: f32) -> Self {
        Self {
            center: center.into(),
            radius: diameter * 0.5,
        }
    }

    /// Get the diameter of the circle.
    pub fn diameter(&self) -> f32 {
        self.radius * 2.0
    }

    /// Get the circumference of the circle.
    pub fn circumference(&self) -> f32 {
        2.0 * PI * self.radius
    }

    /// Get the area of the circle.
    pub fn area(&self) -> f32 {
        PI * self.radius * self.radius
    }

    /// Check if a point is inside the circle.
    pub fn contains(&self, point: impl Into<Point>) -> bool {
        self.center.distance_squared_to(point) <= self.radius * self.radius
    }

    /// Check if this circle intersects another circle.
    pub fn intersects(&self, other: &Circle) -> bool {
        let distance = self.center.distance_to(other.center);
        distance <= self.radius + other.radius
    }

    /// Get the bounding rectangle of the circle.
    pub fn bounding_rect(&self) -> Rect {
        Rect::from_center_size(
            self.center,
            (self.diameter(), self.diameter()),
        )
    }

    /// Get a point on the circle's circumference at the given angle.
    pub fn point_at_angle(&self, angle: impl Into<Rotation>) -> Point {
        let radians = angle.into().as_radians();
        Point::new(
            self.center.x + self.radius * radians.cos(),
            self.center.y + self.radius * radians.sin(),
        )
    }

    /// Scale the circle by a factor.
    pub fn scale(&self, factor: f32) -> Circle {
        Circle {
            center: self.center,
            radius: self.radius * factor,
        }
    }

    /// Check if the circle has a valid radius (positive and finite).
    pub fn is_valid(&self) -> bool {
        self.radius > 0.0 && self.radius.is_finite()
    }
}

impl Default for Circle {
    fn default() -> Self {
        Self::UNIT
    }
}

impl std::fmt::Display for Circle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Circle[center: {}, radius: {:.2}]",
            self.center, self.radius
        )
    }
}

/// Arc (portion of a circle) defined by circle, start angle, and sweep angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    /// The circle this arc is part of.
    pub circle: Circle,
    /// Start angle of the arc.
    pub start_angle: Rotation,
    /// Sweep angle (how much of the circle, can be negative for counter-clockwise).
    pub sweep_angle: Rotation,
}

impl Arc {
    /// Create a new arc.
    pub const fn new(circle: Circle, start_angle: Rotation, sweep_angle: Rotation) -> Self {
        Self {
            circle,
            start_angle,
            sweep_angle,
        }
    }

    /// Create an arc from center, radius, and angles.
    pub fn from_center_radius(
        center: impl Into<Point>,
        radius: f32,
        start_angle: impl Into<Rotation>,
        sweep_angle: impl Into<Rotation>,
    ) -> Self {
        Self {
            circle: Circle::new(center.into(), radius),
            start_angle: start_angle.into(),
            sweep_angle: sweep_angle.into(),
        }
    }

    /// Create an arc spanning from start to end angle.
    pub fn from_angles(
        circle: Circle,
        start_angle: impl Into<Rotation>,
        end_angle: impl Into<Rotation>,
    ) -> Self {
        let start = start_angle.into();
        let end = end_angle.into();
        let sweep = Rotation::radians(end.as_radians() - start.as_radians());
        Self {
            circle,
            start_angle: start,
            sweep_angle: sweep,
        }
    }

    /// Get the end angle of the arc.
    pub fn end_angle(&self) -> Rotation {
        Rotation::radians(self.start_angle.as_radians() + self.sweep_angle.as_radians())
    }

    /// Get the arc length.
    pub fn arc_length(&self) -> f32 {
        self.circle.radius * self.sweep_angle.as_radians().abs()
    }

    /// Get the sector area (area enclosed by arc and two radii).
    pub fn sector_area(&self) -> f32 {
        0.5 * self.circle.radius * self.circle.radius * self.sweep_angle.as_radians().abs()
    }

    /// Get the start point of the arc.
    pub fn start_point(&self) -> Point {
        self.circle.point_at_angle(self.start_angle)
    }

    /// Get the end point of the arc.
    pub fn end_point(&self) -> Point {
        self.circle.point_at_angle(self.end_angle())
    }

    /// Get the midpoint of the arc.
    pub fn midpoint(&self) -> Point {
        let mid_angle = Rotation::radians(
            self.start_angle.as_radians() + self.sweep_angle.as_radians() * 0.5
        );
        self.circle.point_at_angle(mid_angle)
    }

    /// Check if this arc is a full circle.
    pub fn is_full_circle(&self) -> bool {
        self.sweep_angle.as_radians().abs() >= 2.0 * PI - 1e-5
    }

    /// Get a point on the arc at parameter t (0.0 = start, 1.0 = end).
    pub fn point_at(&self, t: f32) -> Point {
        let angle = Rotation::radians(
            self.start_angle.as_radians() + self.sweep_angle.as_radians() * t
        );
        self.circle.point_at_angle(angle)
    }

    /// Reverse the arc direction (swap start and end).
    pub fn reverse(&self) -> Arc {
        Arc {
            circle: self.circle,
            start_angle: self.end_angle(),
            sweep_angle: -self.sweep_angle,
        }
    }

    /// Get the bounding rectangle of the arc.
    pub fn bounding_rect(&self) -> Rect {
        // For simplicity, return the bounding rect of the full circle
        // A precise implementation would check which quadrants the arc passes through
        self.circle.bounding_rect()
    }
}

impl std::fmt::Display for Arc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Arc[{}, {}° to {}°]",
            self.circle,
            self.start_angle.as_degrees(),
            self.end_angle().as_degrees()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circle_creation() {
        let circle = Circle::new(Point::new(10.0, 20.0), 5.0);
        assert_eq!(circle.center, Point::new(10.0, 20.0));
        assert_eq!(circle.radius, 5.0);

        let from_radius = Circle::from_radius(7.0);
        assert_eq!(from_radius.center, Point::ZERO);
        assert_eq!(from_radius.radius, 7.0);
    }

    #[test]
    fn test_circle_from_diameter() {
        let circle = Circle::from_diameter(Point::ZERO, 10.0);
        assert_eq!(circle.radius, 5.0);
        assert_eq!(circle.diameter(), 10.0);
    }

    #[test]
    fn test_circle_measurements() {
        let circle = Circle::from_radius(5.0);
        assert_eq!(circle.diameter(), 10.0);
        assert!((circle.circumference() - (2.0 * PI * 5.0)).abs() < 1e-5);
        assert!((circle.area() - (PI * 25.0)).abs() < 1e-5);
    }

    #[test]
    fn test_circle_contains() {
        let circle = Circle::new(Point::new(0.0, 0.0), 5.0);

        assert!(circle.contains(Point::new(0.0, 0.0))); // center
        assert!(circle.contains(Point::new(3.0, 4.0))); // inside (3-4-5 triangle)
        assert!(circle.contains(Point::new(5.0, 0.0))); // on edge
        assert!(!circle.contains(Point::new(6.0, 0.0))); // outside
    }

    #[test]
    fn test_circle_intersects() {
        let c1 = Circle::new(Point::new(0.0, 0.0), 5.0);
        let c2 = Circle::new(Point::new(8.0, 0.0), 5.0); // touching
        let c3 = Circle::new(Point::new(15.0, 0.0), 5.0); // separate

        assert!(c1.intersects(&c2));
        assert!(!c1.intersects(&c3));
    }

    #[test]
    fn test_circle_bounding_rect() {
        let circle = Circle::new(Point::new(10.0, 20.0), 5.0);
        let rect = circle.bounding_rect();

        assert_eq!(rect.center(), Point::new(10.0, 20.0));
        assert_eq!(rect.width(), 10.0);
        assert_eq!(rect.height(), 10.0);
    }

    #[test]
    fn test_circle_point_at_angle() {
        let circle = Circle::new(Point::new(0.0, 0.0), 5.0);

        let right = circle.point_at_angle(Rotation::radians(0.0));
        assert!((right.x - 5.0).abs() < 1e-5);
        assert!(right.y.abs() < 1e-5);

        let top = circle.point_at_angle(Rotation::radians(PI / 2.0));
        assert!(top.x.abs() < 1e-5);
        assert!((top.y - 5.0).abs() < 1e-5);
    }

    #[test]
    fn test_circle_scale() {
        let circle = Circle::from_radius(5.0);
        let scaled = circle.scale(2.0);
        assert_eq!(scaled.radius, 10.0);
        assert_eq!(scaled.center, circle.center);
    }

    #[test]
    fn test_circle_is_valid() {
        assert!(Circle::from_radius(5.0).is_valid());
        assert!(!Circle::from_radius(0.0).is_valid());
        assert!(!Circle::from_radius(-5.0).is_valid());
        assert!(!Circle::from_radius(f32::INFINITY).is_valid());
    }

    #[test]
    fn test_arc_creation() {
        let circle = Circle::from_radius(5.0);
        let arc = Arc::new(
            circle,
            Rotation::degrees(0.0),
            Rotation::degrees(90.0),
        );

        assert_eq!(arc.circle, circle);
        assert_eq!(arc.start_angle.as_degrees(), 0.0);
        assert_eq!(arc.sweep_angle.as_degrees(), 90.0);
    }

    #[test]
    fn test_arc_end_angle() {
        let arc = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::degrees(45.0),
            Rotation::degrees(90.0),
        );

        assert_eq!(arc.end_angle().as_degrees(), 135.0);
    }

    #[test]
    fn test_arc_from_angles() {
        let circle = Circle::from_radius(5.0);
        let arc = Arc::from_angles(
            circle,
            Rotation::degrees(30.0),
            Rotation::degrees(120.0),
        );

        assert!((arc.start_angle.as_degrees() - 30.0).abs() < 1e-5);
        assert!((arc.sweep_angle.as_degrees() - 90.0).abs() < 1e-5);
        assert!((arc.end_angle().as_degrees() - 120.0).abs() < 1e-5);
    }

    #[test]
    fn test_arc_length() {
        let arc = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::radians(0.0),
            Rotation::radians(PI / 2.0), // quarter circle
        );

        let expected = 5.0 * (PI / 2.0); // r * theta
        assert!((arc.arc_length() - expected).abs() < 1e-5);
    }

    #[test]
    fn test_arc_sector_area() {
        let arc = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::radians(0.0),
            Rotation::radians(PI / 2.0), // quarter circle
        );

        let expected = 0.5 * 25.0 * (PI / 2.0); // 0.5 * r² * theta
        assert!((arc.sector_area() - expected).abs() < 1e-5);
    }

    #[test]
    fn test_arc_points() {
        let arc = Arc::from_center_radius(
            Point::new(0.0, 0.0),
            5.0,
            Rotation::degrees(0.0),
            Rotation::degrees(90.0),
        );

        let start = arc.start_point();
        assert!((start.x - 5.0).abs() < 1e-5);
        assert!(start.y.abs() < 1e-5);

        let end = arc.end_point();
        assert!(end.x.abs() < 1e-5);
        assert!((end.y - 5.0).abs() < 1e-5);
    }

    #[test]
    fn test_arc_midpoint() {
        let arc = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::degrees(0.0),
            Rotation::degrees(90.0),
        );

        let mid = arc.midpoint();
        // At 45°, both x and y should be ~3.536 (5 / sqrt(2))
        let expected = 5.0 / 2.0_f32.sqrt();
        assert!((mid.x - expected).abs() < 1e-2);
        assert!((mid.y - expected).abs() < 1e-2);
    }

    #[test]
    fn test_arc_is_full_circle() {
        let full = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::degrees(0.0),
            Rotation::degrees(360.0),
        );
        assert!(full.is_full_circle());

        let half = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::degrees(0.0),
            Rotation::degrees(180.0),
        );
        assert!(!half.is_full_circle());
    }

    #[test]
    fn test_arc_point_at() {
        let arc = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::degrees(0.0),
            Rotation::degrees(90.0),
        );

        let start = arc.point_at(0.0);
        assert!((start.x - 5.0).abs() < 1e-5);

        let end = arc.point_at(1.0);
        assert!((end.y - 5.0).abs() < 1e-5);

        let mid = arc.point_at(0.5);
        let expected = 5.0 / 2.0_f32.sqrt();
        assert!((mid.x - expected).abs() < 1e-2);
    }

    #[test]
    fn test_arc_reverse() {
        let arc = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::degrees(0.0),
            Rotation::degrees(90.0),
        );

        let reversed = arc.reverse();
        assert_eq!(reversed.start_angle.as_degrees(), 90.0);
        assert_eq!(reversed.sweep_angle.as_degrees(), -90.0);
        assert_eq!(reversed.end_angle().as_degrees(), 0.0);
    }

    #[test]
    fn test_arc_display() {
        let arc = Arc::from_center_radius(
            Point::ZERO,
            5.0,
            Rotation::degrees(45.0),
            Rotation::degrees(90.0),
        );

        let display = format!("{}", arc);
        assert!(display.contains("45"));
        assert!(display.contains("135"));
    }
}
