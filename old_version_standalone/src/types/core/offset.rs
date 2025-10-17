//! Offset and position types
//!
//! This module provides types for representing 2D offsets and positions,
//! similar to Flutter's Offset system.

use egui::{Pos2, Vec2};

/// An immutable 2D offset in Cartesian coordinates.
///
/// This represents a translation or displacement in 2D space.
/// Similar to Flutter's `Offset`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Offset {
    /// The horizontal component.
    pub dx: f32,

    /// The vertical component.
    pub dy: f32,
}

impl Offset {
    /// Create a new offset.
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    /// An offset with zero displacement.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// An offset with infinite displacement.
    pub const INFINITE: Self = Self::new(f32::INFINITY, f32::INFINITY);

    /// Create an offset from a direction and distance.
    pub fn from_direction(direction: f32, distance: f32) -> Self {
        Self::new(distance * direction.cos(), distance * direction.sin())
    }

    /// Get the magnitude (distance) of this offset.
    pub fn distance(&self) -> f32 {
        (self.dx * self.dx + self.dy * self.dy).sqrt()
    }

    /// Get the squared magnitude (avoids sqrt for performance).
    pub fn distance_squared(&self) -> f32 {
        self.dx * self.dx + self.dy * self.dy
    }

    /// Get the direction of this offset in radians.
    pub fn direction(&self) -> f32 {
        self.dy.atan2(self.dx)
    }

    /// Check if this offset is finite.
    pub fn is_finite(&self) -> bool {
        self.dx.is_finite() && self.dy.is_finite()
    }

    /// Check if this offset is infinite.
    pub fn is_infinite(&self) -> bool {
        !self.is_finite()
    }

    /// Scale the offset by a factor.
    pub fn scale(&self, factor: f32) -> Self {
        Self::new(self.dx * factor, self.dy * factor)
    }

    /// Translate an offset by another offset.
    pub fn translate(&self, other: Offset) -> Self {
        Self::new(self.dx + other.dx, self.dy + other.dy)
    }

    /// Linear interpolation between two offsets.
    pub fn lerp(&self, other: Offset, t: f32) -> Offset {
        let t = t.clamp(0.0, 1.0);
        Offset::new(
            self.dx + (other.dx - self.dx) * t,
            self.dy + (other.dy - self.dy) * t,
        )
    }

    /// Convert to egui::Vec2.
    pub fn to_vec2(&self) -> Vec2 {
        Vec2::new(self.dx, self.dy)
    }

    /// Convert to egui::Pos2 (treating offset as absolute position).
    pub fn to_pos2(&self) -> Pos2 {
        Pos2::new(self.dx, self.dy)
    }

    /// Create from egui::Vec2.
    pub fn from_vec2(vec: Vec2) -> Self {
        Self::new(vec.x, vec.y)
    }

    /// Create from egui::Pos2.
    pub fn from_pos2(pos: Pos2) -> Self {
        Self::new(pos.x, pos.y)
    }
}

impl From<Vec2> for Offset {
    fn from(vec: Vec2) -> Self {
        Offset::from_vec2(vec)
    }
}

impl From<Offset> for Vec2 {
    fn from(offset: Offset) -> Self {
        offset.to_vec2()
    }
}

impl From<Pos2> for Offset {
    fn from(pos: Pos2) -> Self {
        Offset::from_pos2(pos)
    }
}

impl From<Offset> for Pos2 {
    fn from(offset: Offset) -> Self {
        offset.to_pos2()
    }
}

impl From<(f32, f32)> for Offset {
    fn from((dx, dy): (f32, f32)) -> Self {
        Offset::new(dx, dy)
    }
}

impl std::ops::Add for Offset {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.dx + rhs.dx, self.dy + rhs.dy)
    }
}

impl std::ops::Sub for Offset {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.dx - rhs.dx, self.dy - rhs.dy)
    }
}

impl std::ops::Mul<f32> for Offset {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

impl std::ops::Div<f32> for Offset {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.dx / rhs, self.dy / rhs)
    }
}

impl std::ops::Neg for Offset {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.dx, -self.dy)
    }
}

/// A fractional offset from a reference point.
///
/// Similar to Flutter's `FractionalOffset` but more general.
/// Values are in the range [0.0, 1.0] relative to a size.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FractionalOffset {
    /// The horizontal fraction (0.0 to 1.0).
    pub dx: f32,

    /// The vertical fraction (0.0 to 1.0).
    pub dy: f32,
}

impl FractionalOffset {
    /// Create a new fractional offset.
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    /// Top-left corner (0.0, 0.0).
    pub const TOP_LEFT: Self = Self::new(0.0, 0.0);

    /// Top-center (0.5, 0.0).
    pub const TOP_CENTER: Self = Self::new(0.5, 0.0);

    /// Top-right corner (1.0, 0.0).
    pub const TOP_RIGHT: Self = Self::new(1.0, 0.0);

    /// Center-left (0.0, 0.5).
    pub const CENTER_LEFT: Self = Self::new(0.0, 0.5);

    /// Center (0.5, 0.5).
    pub const CENTER: Self = Self::new(0.5, 0.5);

    /// Center-right (1.0, 0.5).
    pub const CENTER_RIGHT: Self = Self::new(1.0, 0.5);

    /// Bottom-left corner (0.0, 1.0).
    pub const BOTTOM_LEFT: Self = Self::new(0.0, 1.0);

    /// Bottom-center (0.5, 1.0).
    pub const BOTTOM_CENTER: Self = Self::new(0.5, 1.0);

    /// Bottom-right corner (1.0, 1.0).
    pub const BOTTOM_RIGHT: Self = Self::new(1.0, 1.0);

    /// Resolve to an absolute offset given a size.
    pub fn resolve(&self, size: Vec2) -> Offset {
        Offset::new(self.dx * size.x, self.dy * size.y)
    }

    /// Linear interpolation between two fractional offsets.
    pub fn lerp(&self, other: FractionalOffset, t: f32) -> FractionalOffset {
        let t = t.clamp(0.0, 1.0);
        FractionalOffset::new(
            self.dx + (other.dx - self.dx) * t,
            self.dy + (other.dy - self.dy) * t,
        )
    }
}

impl From<(f32, f32)> for FractionalOffset {
    fn from((dx, dy): (f32, f32)) -> Self {
        FractionalOffset::new(dx, dy)
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

        let vec2 = offset.to_vec2();
        assert_eq!(vec2.x, 10.0);
        assert_eq!(vec2.y, 20.0);

        let back = Offset::from_vec2(vec2);
        assert_eq!(back, offset);

        let from_tuple: Offset = (10.0, 20.0).into();
        assert_eq!(from_tuple, offset);
    }

    #[test]
    fn test_offset_finite() {
        assert!(Offset::ZERO.is_finite());
        assert!(!Offset::ZERO.is_infinite());

        assert!(!Offset::INFINITE.is_finite());
        assert!(Offset::INFINITE.is_infinite());
    }

    #[test]
    fn test_fractional_offset_constants() {
        assert_eq!(FractionalOffset::TOP_LEFT.dx, 0.0);
        assert_eq!(FractionalOffset::TOP_LEFT.dy, 0.0);

        assert_eq!(FractionalOffset::CENTER.dx, 0.5);
        assert_eq!(FractionalOffset::CENTER.dy, 0.5);

        assert_eq!(FractionalOffset::BOTTOM_RIGHT.dx, 1.0);
        assert_eq!(FractionalOffset::BOTTOM_RIGHT.dy, 1.0);
    }

    #[test]
    fn test_fractional_offset_resolve() {
        let frac = FractionalOffset::new(0.5, 0.5);
        let size = Vec2::new(100.0, 200.0);

        let offset = frac.resolve(size);
        assert_eq!(offset.dx, 50.0);
        assert_eq!(offset.dy, 100.0);
    }

    #[test]
    fn test_fractional_offset_lerp() {
        let a = FractionalOffset::TOP_LEFT;
        let b = FractionalOffset::BOTTOM_RIGHT;

        let mid = a.lerp(b, 0.5);
        assert_eq!(mid.dx, 0.5);
        assert_eq!(mid.dy, 0.5);
    }

    #[test]
    fn test_fractional_offset_conversions() {
        let from_tuple: FractionalOffset = (0.5, 0.5).into();
        assert_eq!(from_tuple, FractionalOffset::CENTER);
    }
}
