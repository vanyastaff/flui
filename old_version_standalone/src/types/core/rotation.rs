//! Rotation type for angles
//!
//! This module provides a type-safe wrapper for rotation angles.

use std::f32::consts::PI;

/// Represents a rotation angle.
///
/// Type-safe wrapper that stores angle in radians internally.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Rotation(f32);

impl Rotation {
    /// No rotation (0 radians).
    pub const ZERO: Rotation = Rotation(0.0);

    /// Quarter turn (90 degrees / π/2 radians).
    pub const QUARTER: Rotation = Rotation(PI / 2.0);

    /// Half turn (180 degrees / π radians).
    pub const HALF: Rotation = Rotation(PI);

    /// Three quarter turn (270 degrees / 3π/2 radians).
    pub const THREE_QUARTERS: Rotation = Rotation(3.0 * PI / 2.0);

    /// Full turn (360 degrees / 2π radians).
    pub const FULL: Rotation = Rotation(2.0 * PI);

    /// Create rotation from radians.
    pub const fn radians(radians: f32) -> Self {
        Self(radians)
    }

    /// Create rotation from degrees.
    pub fn degrees(degrees: f32) -> Self {
        Self(degrees.to_radians())
    }

    /// Create rotation from turns (1 turn = 360 degrees).
    pub fn turns(turns: f32) -> Self {
        Self(turns * 2.0 * PI)
    }

    /// Get rotation in radians.
    pub fn as_radians(&self) -> f32 {
        self.0
    }

    /// Get rotation in degrees.
    pub fn as_degrees(&self) -> f32 {
        self.0.to_degrees()
    }

    /// Get rotation in turns.
    pub fn as_turns(&self) -> f32 {
        self.0 / (2.0 * PI)
    }

    /// Normalize rotation to [0, 2π) range.
    pub fn normalize(&self) -> Self {
        let normalized = self.0.rem_euclid(2.0 * PI);
        Self(normalized)
    }

    /// Get the opposite rotation (add 180 degrees).
    pub fn opposite(&self) -> Self {
        Self(self.0 + PI)
    }

    /// Negate the rotation (reverse direction).
    pub fn negate(&self) -> Self {
        Self(-self.0)
    }

    /// Get sine of this rotation.
    pub fn sin(&self) -> f32 {
        self.0.sin()
    }

    /// Get cosine of this rotation.
    pub fn cos(&self) -> f32 {
        self.0.cos()
    }

    /// Get tangent of this rotation.
    pub fn tan(&self) -> f32 {
        self.0.tan()
    }

    /// Linear interpolation between two rotations.
    pub fn lerp(a: Rotation, b: Rotation, t: f32) -> Rotation {
        Rotation(a.0 + (b.0 - a.0) * t)
    }

    /// Shortest path interpolation (takes shorter arc).
    pub fn slerp(a: Rotation, b: Rotation, t: f32) -> Rotation {
        let mut diff = b.0 - a.0;
        // Normalize to [-π, π]
        while diff > PI {
            diff -= 2.0 * PI;
        }
        while diff < -PI {
            diff += 2.0 * PI;
        }
        Rotation(a.0 + diff * t)
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<f32> for Rotation {
    /// Create from radians.
    fn from(radians: f32) -> Self {
        Self::radians(radians)
    }
}

impl From<Rotation> for f32 {
    fn from(rotation: Rotation) -> Self {
        rotation.0
    }
}

impl std::ops::Add for Rotation {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Rotation(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Rotation {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Rotation(self.0 - rhs.0)
    }
}

impl std::ops::Mul<f32> for Rotation {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Rotation(self.0 * rhs)
    }
}

impl std::ops::Div<f32> for Rotation {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Rotation(self.0 / rhs)
    }
}

impl std::ops::Neg for Rotation {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Rotation(-self.0)
    }
}

impl std::fmt::Display for Rotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}°", self.as_degrees())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotation_creation() {
        let rot = Rotation::radians(PI);
        assert_eq!(rot.as_radians(), PI);

        let rot = Rotation::degrees(180.0);
        assert!((rot.as_radians() - PI).abs() < 0.001);

        let rot = Rotation::turns(0.5);
        assert!((rot.as_radians() - PI).abs() < 0.001);
    }

    #[test]
    fn test_rotation_constants() {
        assert_eq!(Rotation::ZERO.as_degrees(), 0.0);
        assert!((Rotation::QUARTER.as_degrees() - 90.0).abs() < 0.001);
        assert!((Rotation::HALF.as_degrees() - 180.0).abs() < 0.001);
        assert!((Rotation::FULL.as_degrees() - 360.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_conversions() {
        let rot = Rotation::degrees(90.0);
        assert!((rot.as_degrees() - 90.0).abs() < 0.001);
        assert!((rot.as_radians() - PI / 2.0).abs() < 0.001);
        assert!((rot.as_turns() - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_rotation_normalize() {
        let rot = Rotation::degrees(450.0); // 360 + 90
        let normalized = rot.normalize();
        assert!((normalized.as_degrees() - 90.0).abs() < 0.001);

        let rot = Rotation::degrees(-90.0);
        let normalized = rot.normalize();
        assert!((normalized.as_degrees() - 270.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_opposite() {
        let rot = Rotation::degrees(45.0);
        let opposite = rot.opposite();
        assert!((opposite.as_degrees() - 225.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_arithmetic() {
        let a = Rotation::degrees(45.0);
        let b = Rotation::degrees(30.0);

        let sum = a + b;
        assert!((sum.as_degrees() - 75.0).abs() < 0.001);

        let diff = a - b;
        assert!((diff.as_degrees() - 15.0).abs() < 0.001);

        let scaled = a * 2.0;
        assert!((scaled.as_degrees() - 90.0).abs() < 0.001);

        let neg = -a;
        assert!((neg.as_degrees() + 45.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_lerp() {
        let a = Rotation::degrees(0.0);
        let b = Rotation::degrees(90.0);

        let mid = Rotation::lerp(a, b, 0.5);
        assert!((mid.as_degrees() - 45.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_slerp() {
        let a = Rotation::degrees(10.0);
        let b = Rotation::degrees(350.0); // -10 degrees

        // Should take shorter path (20 degrees total)
        let mid = Rotation::slerp(a, b, 0.5);
        assert!((mid.as_degrees() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_rotation_trig() {
        let rot = Rotation::degrees(90.0);
        assert!((rot.sin() - 1.0).abs() < 0.001);
        assert!(rot.cos().abs() < 0.001);
    }

    #[test]
    fn test_rotation_display() {
        let rot = Rotation::degrees(45.5);
        let display = format!("{}", rot);
        assert!(display.contains("45.5"));
        assert!(display.contains("°"));
    }
}
