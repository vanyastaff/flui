//! Vector types for direction and magnitude in 2D and 3D space.
//!
//! This module provides vector types for representing directions, velocities, and forces.

use egui::Vec2 as EguiVec2;

/// 2D vector representing direction and magnitude.
///
/// Semantic distinction:
/// - `Vector2`: Direction + magnitude (physics, velocities, forces)
/// - `Offset`: Translation/displacement (UI positioning)
/// - `Point`: Absolute position (coordinates)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector2 {
    pub x: f32,
    pub y: f32,
}

impl Vector2 {
    /// Zero vector.
    pub const ZERO: Vector2 = Vector2 { x: 0.0, y: 0.0 };

    /// Unit vector pointing right (1, 0).
    pub const RIGHT: Vector2 = Vector2 { x: 1.0, y: 0.0 };

    /// Unit vector pointing left (-1, 0).
    pub const LEFT: Vector2 = Vector2 { x: -1.0, y: 0.0 };

    /// Unit vector pointing up (0, 1).
    pub const UP: Vector2 = Vector2 { x: 0.0, y: 1.0 };

    /// Unit vector pointing down (0, -1).
    pub const DOWN: Vector2 = Vector2 { x: 0.0, y: -1.0 };

    /// Unit vector (1, 1).
    pub const ONE: Vector2 = Vector2 { x: 1.0, y: 1.0 };

    /// Create a new 2D vector.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Calculate the length (magnitude) of this vector.
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Calculate the squared length (faster, avoids sqrt).
    pub fn length_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// Normalize this vector to unit length.
    pub fn normalize(&self) -> Vector2 {
        let len = self.length();
        if len > 0.0 {
            Vector2 {
                x: self.x / len,
                y: self.y / len,
            }
        } else {
            *self
        }
    }

    /// Check if this is a normalized (unit) vector.
    pub fn is_normalized(&self) -> bool {
        (self.length_squared() - 1.0).abs() < 1e-5
    }

    /// Dot product with another vector.
    pub fn dot(&self, other: impl Into<Vector2>) -> f32 {
        let other = other.into();
        self.x * other.x + self.y * other.y
    }

    /// Cross product (returns scalar in 2D).
    pub fn cross(&self, other: impl Into<Vector2>) -> f32 {
        let other = other.into();
        self.x * other.y - self.y * other.x
    }

    /// Calculate angle between two vectors in radians.
    pub fn angle_between(&self, other: impl Into<Vector2>) -> f32 {
        let other = other.into();
        let dot = self.dot(other);
        let lengths = self.length() * other.length();
        if lengths > 0.0 {
            (dot / lengths).acos()
        } else {
            0.0
        }
    }

    /// Get angle of this vector from X axis in radians.
    pub fn angle(&self) -> f32 {
        self.y.atan2(self.x)
    }

    /// Create vector from angle and length.
    pub fn from_angle(angle: f32, length: f32) -> Vector2 {
        Vector2 {
            x: angle.cos() * length,
            y: angle.sin() * length,
        }
    }

    /// Rotate this vector by an angle in radians.
    pub fn rotate(&self, angle: f32) -> Vector2 {
        let cos = angle.cos();
        let sin = angle.sin();
        Vector2 {
            x: self.x * cos - self.y * sin,
            y: self.x * sin + self.y * cos,
        }
    }

    /// Reflect this vector across a normal.
    pub fn reflect(&self, normal: impl Into<Vector2>) -> Vector2 {
        let normal = normal.into();
        *self - normal * (2.0 * self.dot(normal))
    }

    /// Project this vector onto another vector.
    pub fn project_onto(&self, other: impl Into<Vector2>) -> Vector2 {
        let other = other.into();
        let dot = self.dot(other);
        let len_sq = other.length_squared();
        if len_sq > 0.0 {
            other * (dot / len_sq)
        } else {
            Vector2::ZERO
        }
    }

    /// Linear interpolation between two vectors.
    pub fn lerp(a: impl Into<Vector2>, b: impl Into<Vector2>, t: f32) -> Vector2 {
        let a = a.into();
        let b = b.into();
        Vector2 {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }

    /// Clamp vector components to a range.
    pub fn clamp(&self, min: f32, max: f32) -> Vector2 {
        Vector2 {
            x: self.x.clamp(min, max),
            y: self.y.clamp(min, max),
        }
    }

    /// Get absolute values of components.
    pub fn abs(&self) -> Vector2 {
        Vector2 {
            x: self.x.abs(),
            y: self.y.abs(),
        }
    }

    /// Component-wise min.
    pub fn min(a: impl Into<Vector2>, b: impl Into<Vector2>) -> Vector2 {
        let a = a.into();
        let b = b.into();
        Vector2 {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
        }
    }

    /// Component-wise max.
    pub fn max(a: impl Into<Vector2>, b: impl Into<Vector2>) -> Vector2 {
        let a = a.into();
        let b = b.into();
        Vector2 {
            x: a.x.max(b.x),
            y: a.y.max(b.y),
        }
    }
}

impl Default for Vector2 {
    fn default() -> Self {
        Self::ZERO
    }
}

// Conversions
impl From<(f32, f32)> for Vector2 {
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

impl From<[f32; 2]> for Vector2 {
    fn from([x, y]: [f32; 2]) -> Self {
        Self::new(x, y)
    }
}

impl From<Vector2> for (f32, f32) {
    fn from(v: Vector2) -> Self {
        (v.x, v.y)
    }
}

impl From<Vector2> for [f32; 2] {
    fn from(v: Vector2) -> Self {
        [v.x, v.y]
    }
}

impl From<EguiVec2> for Vector2 {
    fn from(v: EguiVec2) -> Self {
        Self::new(v.x, v.y)
    }
}

impl From<Vector2> for EguiVec2 {
    fn from(v: Vector2) -> Self {
        EguiVec2::new(v.x, v.y)
    }
}

// Arithmetic operations
impl std::ops::Add for Vector2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Vector2 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::AddAssign for Vector2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl std::ops::Sub for Vector2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Vector2 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl std::ops::SubAssign for Vector2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl std::ops::Mul<f32> for Vector2 {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self::Output {
        Vector2 {
            x: self.x * scalar,
            y: self.y * scalar,
        }
    }
}

impl std::ops::MulAssign<f32> for Vector2 {
    fn mul_assign(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
    }
}

impl std::ops::Div<f32> for Vector2 {
    type Output = Self;

    fn div(self, scalar: f32) -> Self::Output {
        Vector2 {
            x: self.x / scalar,
            y: self.y / scalar,
        }
    }
}

impl std::ops::DivAssign<f32> for Vector2 {
    fn div_assign(&mut self, scalar: f32) {
        self.x /= scalar;
        self.y /= scalar;
    }
}

impl std::ops::Neg for Vector2 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Vector2 {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl std::fmt::Display for Vector2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vec2({:.2}, {:.2})", self.x, self.y)
    }
}

/// 3D vector representing direction and magnitude in 3D space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vector3 {
    /// Zero vector.
    pub const ZERO: Vector3 = Vector3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// Unit vector along X axis (1, 0, 0).
    pub const X: Vector3 = Vector3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };

    /// Unit vector along Y axis (0, 1, 0).
    pub const Y: Vector3 = Vector3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };

    /// Unit vector along Z axis (0, 0, 1).
    pub const Z: Vector3 = Vector3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    /// Unit vector (1, 1, 1).
    pub const ONE: Vector3 = Vector3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };

    /// Create a new 3D vector.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Calculate the length (magnitude) of this vector.
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Calculate the squared length (faster, avoids sqrt).
    pub fn length_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Normalize this vector to unit length.
    pub fn normalize(&self) -> Vector3 {
        let len = self.length();
        if len > 0.0 {
            Vector3 {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
            }
        } else {
            *self
        }
    }

    /// Dot product with another vector.
    pub fn dot(&self, other: impl Into<Vector3>) -> f32 {
        let other = other.into();
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product with another vector.
    pub fn cross(&self, other: impl Into<Vector3>) -> Vector3 {
        let other = other.into();
        Vector3 {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Linear interpolation between two vectors.
    pub fn lerp(a: impl Into<Vector3>, b: impl Into<Vector3>, t: f32) -> Vector3 {
        let a = a.into();
        let b = b.into();
        Vector3 {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
            z: a.z + (b.z - a.z) * t,
        }
    }

    /// Project onto XY plane (drop Z).
    pub fn xy(&self) -> Vector2 {
        Vector2::new(self.x, self.y)
    }

    /// Clamp vector components to a range.
    pub fn clamp(&self, min: f32, max: f32) -> Vector3 {
        Vector3 {
            x: self.x.clamp(min, max),
            y: self.y.clamp(min, max),
            z: self.z.clamp(min, max),
        }
    }
}

impl Default for Vector3 {
    fn default() -> Self {
        Self::ZERO
    }
}

// Conversions
impl From<(f32, f32, f32)> for Vector3 {
    fn from((x, y, z): (f32, f32, f32)) -> Self {
        Self::new(x, y, z)
    }
}

impl From<[f32; 3]> for Vector3 {
    fn from([x, y, z]: [f32; 3]) -> Self {
        Self::new(x, y, z)
    }
}

impl From<Vector3> for (f32, f32, f32) {
    fn from(v: Vector3) -> Self {
        (v.x, v.y, v.z)
    }
}

impl From<Vector3> for [f32; 3] {
    fn from(v: Vector3) -> Self {
        [v.x, v.y, v.z]
    }
}

// Extend Vector2 to Vector3
impl From<Vector2> for Vector3 {
    fn from(v: Vector2) -> Self {
        Self::new(v.x, v.y, 0.0)
    }
}

// Arithmetic operations
impl std::ops::Add for Vector3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Vector3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl std::ops::Sub for Vector3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Vector3 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl std::ops::Mul<f32> for Vector3 {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self::Output {
        Vector3 {
            x: self.x * scalar,
            y: self.y * scalar,
            z: self.z * scalar,
        }
    }
}

impl std::ops::Div<f32> for Vector3 {
    type Output = Self;

    fn div(self, scalar: f32) -> Self::Output {
        Vector3 {
            x: self.x / scalar,
            y: self.y / scalar,
            z: self.z / scalar,
        }
    }
}

impl std::ops::Neg for Vector3 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Vector3 {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

impl std::fmt::Display for Vector3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vec3({:.2}, {:.2}, {:.2})", self.x, self.y, self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector2_creation() {
        let v = Vector2::new(3.0, 4.0);
        assert_eq!(v.x, 3.0);
        assert_eq!(v.y, 4.0);
    }

    #[test]
    fn test_vector2_constants() {
        assert_eq!(Vector2::ZERO, Vector2::new(0.0, 0.0));
        assert_eq!(Vector2::RIGHT, Vector2::new(1.0, 0.0));
        assert_eq!(Vector2::UP, Vector2::new(0.0, 1.0));
    }

    #[test]
    fn test_vector2_length() {
        let v = Vector2::new(3.0, 4.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(v.length_squared(), 25.0);
    }

    #[test]
    fn test_vector2_normalize() {
        let v = Vector2::new(3.0, 4.0);
        let normalized = v.normalize();
        assert!((normalized.length() - 1.0).abs() < 1e-5);
        assert!(normalized.is_normalized());
    }

    #[test]
    fn test_vector2_dot_product() {
        let v1 = Vector2::new(1.0, 2.0);
        let v2 = Vector2::new(3.0, 4.0);
        assert_eq!(v1.dot(v2), 11.0); // 1*3 + 2*4 = 11
    }

    #[test]
    fn test_vector2_cross_product() {
        let v1 = Vector2::new(1.0, 0.0);
        let v2 = Vector2::new(0.0, 1.0);
        assert_eq!(v1.cross(v2), 1.0);
    }

    #[test]
    fn test_vector2_angle() {
        let right = Vector2::RIGHT;
        assert!((right.angle() - 0.0).abs() < 1e-5);

        let up = Vector2::UP;
        assert!((up.angle() - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn test_vector2_from_angle() {
        let v = Vector2::from_angle(0.0, 1.0);
        assert!((v.x - 1.0).abs() < 1e-5);
        assert!(v.y.abs() < 1e-5);
    }

    #[test]
    fn test_vector2_rotate() {
        use std::f32::consts::FRAC_PI_2;
        let v = Vector2::RIGHT;
        let rotated = v.rotate(FRAC_PI_2);
        assert!((rotated.x - 0.0).abs() < 1e-5);
        assert!((rotated.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_vector2_arithmetic() {
        let v1 = Vector2::new(1.0, 2.0);
        let v2 = Vector2::new(3.0, 4.0);

        assert_eq!(v1 + v2, Vector2::new(4.0, 6.0));
        assert_eq!(v2 - v1, Vector2::new(2.0, 2.0));
        assert_eq!(v1 * 2.0, Vector2::new(2.0, 4.0));
        assert_eq!(v2 / 2.0, Vector2::new(1.5, 2.0));
        assert_eq!(-v1, Vector2::new(-1.0, -2.0));
    }

    #[test]
    fn test_vector2_lerp() {
        let a = Vector2::ZERO;
        let b = Vector2::new(10.0, 20.0);
        let mid = Vector2::lerp(a, b, 0.5);
        assert_eq!(mid, Vector2::new(5.0, 10.0));
    }

    #[test]
    fn test_vector3_creation() {
        let v = Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn test_vector3_length() {
        let v = Vector3::new(1.0, 2.0, 2.0);
        assert_eq!(v.length(), 3.0);
        assert_eq!(v.length_squared(), 9.0);
    }

    #[test]
    fn test_vector3_cross_product() {
        let v1 = Vector3::X;
        let v2 = Vector3::Y;
        let cross = v1.cross(v2);
        assert_eq!(cross, Vector3::Z);
    }

    #[test]
    fn test_vector3_xy_projection() {
        let v3 = Vector3::new(1.0, 2.0, 3.0);
        let v2 = v3.xy();
        assert_eq!(v2, Vector2::new(1.0, 2.0));
    }

    #[test]
    fn test_vector2_conversions() {
        let v: Vector2 = (1.0, 2.0).into();
        assert_eq!(v, Vector2::new(1.0, 2.0));

        let tuple: (f32, f32) = v.into();
        assert_eq!(tuple, (1.0, 2.0));
    }

    #[test]
    fn test_vector3_conversions() {
        let v: Vector3 = (1.0, 2.0, 3.0).into();
        assert_eq!(v, Vector3::new(1.0, 2.0, 3.0));

        let v2 = Vector2::new(1.0, 2.0);
        let v3: Vector3 = v2.into();
        assert_eq!(v3, Vector3::new(1.0, 2.0, 0.0));
    }
}
