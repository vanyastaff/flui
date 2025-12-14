//! 2D offset (position/translation) type
//!
//! This module provides an immutable 2D offset type, similar to Flutter's Offset.

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use crate::{Point, Size};

/// An immutable 2D offset in Cartesian coordinates.
///
/// This represents a translation or displacement in 2D space.
/// Similar to Flutter's `Offset`.
///
/// # Examples
///
/// ```
/// use flui_types::Offset;
///
/// let offset = Offset::new(10.0, 20.0);
/// assert_eq!(offset.dx, 10.0);
/// assert_eq!(offset.dy, 20.0);
///
/// let scaled = offset * 2.0;
/// assert_eq!(scaled, Offset::new(20.0, 40.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Offset {
    /// The horizontal component.
    pub dx: f32,

    /// The vertical component.
    pub dy: f32,
}

impl Offset {
    /// Create a new offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// assert_eq!(offset.dx, 10.0);
    /// assert_eq!(offset.dy, 20.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    /// An offset with zero displacement.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// An offset with infinite displacement.
    pub const INFINITE: Self = Self::new(f32::INFINITY, f32::INFINITY);

    /// Create an offset from a direction (in radians) and distance.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let offset = Offset::from_direction(0.0, 10.0);
    /// assert!((offset.dx - 10.0).abs() < 0.001);
    /// assert!(offset.dy.abs() < 0.001);
    /// ```
    pub fn from_direction(direction: f32, distance: f32) -> Self {
        Self::new(distance * direction.cos(), distance * direction.sin())
    }

    /// Check if this offset is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// assert!(Offset::ZERO.is_zero());
    /// assert!(!Offset::new(1.0, 0.0).is_zero());
    /// ```
    pub fn is_zero(&self) -> bool {
        self.dx == 0.0 && self.dy == 0.0
    }

    /// Get the magnitude (distance) of this offset from the origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let offset = Offset::new(3.0, 4.0);
    /// assert_eq!(offset.distance(), 5.0); // 3-4-5 triangle
    /// ```
    #[inline]
    #[must_use]
    pub fn distance(&self) -> f32 {
        (self.dx * self.dx + self.dy * self.dy).sqrt()
    }

    /// Get the squared magnitude (avoids sqrt for performance).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let offset = Offset::new(3.0, 4.0);
    /// assert_eq!(offset.distance_squared(), 25.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn distance_squared(&self) -> f32 {
        self.dx * self.dx + self.dy * self.dy
    }

    /// Get the direction of this offset in radians.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let right = Offset::new(1.0, 0.0);
    /// assert!((right.direction() - 0.0).abs() < 0.001);
    /// ```
    pub fn direction(&self) -> f32 {
        self.dy.atan2(self.dx)
    }

    /// Check if this offset is finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// assert!(Offset::ZERO.is_finite());
    /// assert!(!Offset::INFINITE.is_finite());
    /// ```
    pub fn is_finite(&self) -> bool {
        self.dx.is_finite() && self.dy.is_finite()
    }

    /// Check if this offset is infinite.
    pub fn is_infinite(&self) -> bool {
        !self.is_finite()
    }

    /// Scale the offset by a factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let scaled = offset.scale(2.0);
    /// assert_eq!(scaled, Offset::new(20.0, 40.0));
    /// ```
    pub fn scale(&self, factor: f32) -> Self {
        Self::new(self.dx * factor, self.dy * factor)
    }

    /// Translate an offset by another offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let a = Offset::new(10.0, 20.0);
    /// let b = Offset::new(5.0, 10.0);
    /// let c = a.translate(b);
    /// assert_eq!(c, Offset::new(15.0, 30.0));
    /// ```
    pub fn translate(self, other: impl Into<Offset>) -> Self {
        let other = other.into();
        Self::new(self.dx + other.dx, self.dy + other.dy)
    }

    /// Linear interpolation between two offsets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let a = Offset::new(0.0, 0.0);
    /// let b = Offset::new(10.0, 10.0);
    /// let mid = a.lerp(b, 0.5);
    /// assert_eq!(mid, Offset::new(5.0, 5.0));
    /// ```
    pub fn lerp(self, other: impl Into<Offset>, t: f32) -> Offset {
        let other = other.into();
        let t = t.clamp(0.0, 1.0);
        Offset::new(
            self.dx + (other.dx - self.dx) * t,
            self.dy + (other.dy - self.dy) * t,
        )
    }

    /// Convert to a Point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Offset, Point};
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let point = offset.to_point();
    /// assert_eq!(point, Point::new(10.0, 20.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn to_point(self) -> Point {
        Point::new(self.dx, self.dy)
    }

    /// Convert to a Size (treating offset as width/height).
    ///
    /// Negative components are clamped to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Offset, Size};
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let size = offset.to_size();
    /// assert_eq!(size, Size::new(10.0, 20.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn to_size(self) -> Size {
        Size::new(self.dx.max(0.0), self.dy.max(0.0))
    }

    // ===== Helper methods for rendering =====

    /// Normalize this offset to a unit vector.
    ///
    /// Returns `Offset::ZERO` if magnitude is zero.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> Offset {
        let dist = self.distance();
        if dist > f32::EPSILON {
            Offset::new(self.dx / dist, self.dy / dist)
        } else {
            Offset::ZERO
        }
    }

    /// Dot product with another offset.
    #[inline]
    #[must_use]
    pub const fn dot(&self, other: Offset) -> f32 {
        self.dx * other.dx + self.dy * other.dy
    }

    /// Cross product magnitude (2D cross is a scalar).
    #[inline]
    #[must_use]
    pub const fn cross(&self, other: Offset) -> f32 {
        self.dx * other.dy - self.dy * other.dx
    }

    /// Rotate this offset by an angle (in radians).
    #[must_use]
    pub fn rotate(&self, angle: f32) -> Offset {
        let (sin, cos) = angle.sin_cos();
        Offset::new(self.dx * cos - self.dy * sin, self.dx * sin + self.dy * cos)
    }

    /// Round components to nearest integer.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Offset {
        Offset::new(self.dx.round(), self.dy.round())
    }

    /// Floor components.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Offset {
        Offset::new(self.dx.floor(), self.dy.floor())
    }

    /// Ceil components.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Offset {
        Offset::new(self.dx.ceil(), self.dy.ceil())
    }

    /// Clamp components between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(&self, min: Offset, max: Offset) -> Offset {
        Offset::new(self.dx.clamp(min.dx, max.dx), self.dy.clamp(min.dy, max.dy))
    }

    /// Get absolute values of components.
    #[inline]
    #[must_use]
    pub const fn abs(&self) -> Offset {
        Offset::new(
            if self.dx >= 0.0 { self.dx } else { -self.dx },
            if self.dy >= 0.0 { self.dy } else { -self.dy },
        )
    }

    /// Clamp the magnitude (length) of this offset to a maximum value.
    ///
    /// If the magnitude exceeds `max`, returns a scaled version with magnitude `max`.
    /// Direction is preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let offset = Offset::new(30.0, 40.0); // magnitude = 50
    /// let clamped = offset.clamp_magnitude(25.0);
    ///
    /// assert!((clamped.distance() - 25.0).abs() < 0.1);
    /// // Direction preserved: still pointing in same direction
    /// assert!((clamped.direction() - offset.direction()).abs() < 0.01);
    /// ```
    #[inline]
    #[must_use]
    pub fn clamp_magnitude(&self, max: f32) -> Offset {
        let magnitude = self.distance();
        if magnitude > max && magnitude > f32::EPSILON {
            let scale = max / magnitude;
            Offset::new(self.dx * scale, self.dy * scale)
        } else {
            *self
        }
    }

    /// Move towards another offset by a specific distance.
    ///
    /// If the distance to target is less than `max_distance`, returns the target.
    /// Otherwise, moves `max_distance` units towards the target.
    ///
    /// Useful for smooth following behavior and lerping with fixed step size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    ///
    /// let start = Offset::new(0.0, 0.0);
    /// let target = Offset::new(10.0, 0.0);
    ///
    /// // Move 3 units towards target
    /// let moved = start.move_towards(target, 3.0);
    /// assert_eq!(moved, Offset::new(3.0, 0.0));
    ///
    /// // Moving beyond target distance returns target
    /// let at_target = start.move_towards(target, 20.0);
    /// assert_eq!(at_target, target);
    /// ```
    #[inline]
    #[must_use]
    pub fn move_towards(&self, target: impl Into<Offset>, max_distance: f32) -> Offset {
        let target = target.into();
        let delta = target - *self;
        let distance = delta.distance();

        if distance <= max_distance || distance < f32::EPSILON {
            target
        } else {
            let direction = delta.normalize();
            *self + direction * max_distance
        }
    }

    /// Calculate the angle between this offset and another, in radians.
    ///
    /// Returns the absolute angle difference in range [0, π].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Offset;
    /// use std::f32::consts::PI;
    ///
    /// let right = Offset::new(1.0, 0.0);
    /// let up = Offset::new(0.0, 1.0);
    ///
    /// let angle = right.angle_to(up);
    /// assert!((angle - PI / 2.0).abs() < 0.01);
    /// ```
    #[inline]
    #[must_use]
    pub fn angle_to(&self, other: impl Into<Offset>) -> f32 {
        let other = other.into();
        let dot = self.dot(other);
        let det = self.cross(other);
        det.atan2(dot).abs()
    }
}

impl From<(f32, f32)> for Offset {
    fn from((dx, dy): (f32, f32)) -> Self {
        Offset::new(dx, dy)
    }
}

impl From<[f32; 2]> for Offset {
    fn from([dx, dy]: [f32; 2]) -> Self {
        Offset::new(dx, dy)
    }
}

impl From<Point> for Offset {
    fn from(point: Point) -> Self {
        Offset::new(point.x, point.y)
    }
}

impl From<Offset> for Point {
    fn from(offset: Offset) -> Self {
        offset.to_point()
    }
}

impl Add for Offset {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.dx + rhs.dx, self.dy + rhs.dy)
    }
}

impl Sub for Offset {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.dx - rhs.dx, self.dy - rhs.dy)
    }
}

impl Mul<f32> for Offset {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

impl Div<f32> for Offset {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.dx / rhs, self.dy / rhs)
    }
}

impl Neg for Offset {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.dx, -self.dy)
    }
}

impl fmt::Display for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Offset({}, {})", self.dx, self.dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_creation() {
        let offset = Offset::new(10.0, 20.0);
        assert_eq!(offset.dx, 10.0);
        assert_eq!(offset.dy, 20.0);

        assert_eq!(Offset::ZERO.dx, 0.0);
        assert_eq!(Offset::ZERO.dy, 0.0);
        assert!(Offset::ZERO.is_zero());
    }

    #[test]
    fn test_offset_distance() {
        let offset = Offset::new(3.0, 4.0);
        assert_eq!(offset.distance(), 5.0); // 3-4-5 triangle
        assert_eq!(offset.distance_squared(), 25.0);
    }

    #[test]
    fn test_offset_direction() {
        let right = Offset::new(1.0, 0.0);
        assert!((right.direction() - 0.0).abs() < 0.001);

        let up = Offset::new(0.0, 1.0);
        assert!((up.direction() - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    }

    #[test]
    fn test_offset_from_direction() {
        let offset = Offset::from_direction(0.0, 10.0); // Right direction
        assert!((offset.dx - 10.0).abs() < 0.001);
        assert!(offset.dy.abs() < 0.001);
    }

    #[test]
    fn test_offset_arithmetic() {
        let a = Offset::new(10.0, 20.0);
        let b = Offset::new(5.0, 10.0);

        let sum = a + b;
        assert_eq!(sum.dx, 15.0);
        assert_eq!(sum.dy, 30.0);

        let diff = a - b;
        assert_eq!(diff.dx, 5.0);
        assert_eq!(diff.dy, 10.0);

        let scaled = a * 2.0;
        assert_eq!(scaled.dx, 20.0);
        assert_eq!(scaled.dy, 40.0);

        let divided = a / 2.0;
        assert_eq!(divided.dx, 5.0);
        assert_eq!(divided.dy, 10.0);

        let negated = -a;
        assert_eq!(negated.dx, -10.0);
        assert_eq!(negated.dy, -20.0);
    }

    #[test]
    fn test_offset_lerp() {
        let a = Offset::new(0.0, 0.0);
        let b = Offset::new(10.0, 10.0);

        let mid = a.lerp(b, 0.5);
        assert_eq!(mid.dx, 5.0);
        assert_eq!(mid.dy, 5.0);

        let start = a.lerp(b, 0.0);
        assert_eq!(start, a);

        let end = a.lerp(b, 1.0);
        assert_eq!(end, b);
    }

    #[test]
    fn test_offset_conversions() {
        let offset = Offset::new(10.0, 20.0);

        let from_tuple: Offset = (10.0, 20.0).into();
        assert_eq!(from_tuple, offset);

        let from_array: Offset = [10.0, 20.0].into();
        assert_eq!(from_array, offset);

        let point = offset.to_point();
        assert_eq!(point.x, 10.0);
        assert_eq!(point.y, 20.0);

        let from_point: Offset = point.into();
        assert_eq!(from_point, offset);
    }

    #[test]
    fn test_offset_finite() {
        assert!(Offset::ZERO.is_finite());
        assert!(!Offset::ZERO.is_infinite());

        assert!(!Offset::INFINITE.is_finite());
        assert!(Offset::INFINITE.is_infinite());
    }

    #[test]
    fn test_offset_scale() {
        let offset = Offset::new(10.0, 20.0);
        let scaled = offset.scale(3.0);
        assert_eq!(scaled, Offset::new(30.0, 60.0));
    }

    #[test]
    fn test_offset_translate() {
        let a = Offset::new(10.0, 20.0);
        let b = Offset::new(5.0, 3.0);
        let translated = a.translate(b);
        assert_eq!(translated, Offset::new(15.0, 23.0));
    }

    #[test]
    fn test_offset_to_size() {
        let offset = Offset::new(10.0, 20.0);
        let size = offset.to_size();
        assert_eq!(size.width, 10.0);
        assert_eq!(size.height, 20.0);

        // Negative components should be clamped
        let negative = Offset::new(-5.0, 10.0);
        let size2 = negative.to_size();
        assert_eq!(size2.width, 0.0);
        assert_eq!(size2.height, 10.0);
    }
}
