//! 2D vector type for direction and magnitude.
//!
//! API design inspired by kurbo, glam, and euclid.
//!
//! # Semantic Distinction
//!
//! - [`Point`]: Absolute position in coordinate system (location)
//! - [`Vec2`]: Direction and magnitude (displacement)
//!
//! # Operator Semantics
//!
//! ```text
//! Vec2 + Vec2 = Vec2  (add displacements)
//! Vec2 - Vec2 = Vec2  (subtract displacements)
//! Vec2 * f32  = Vec2  (scale)
//! Vec2 · Vec2 = f32   (dot product)
//! Vec2 × Vec2 = f32   (2D cross product)
//! ```

use std::fmt;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use super::Point;

/// A 2D vector representing direction and magnitude.
///
/// This represents a displacement or direction, not an absolute position.
/// For positions, use [`Point`].
///
/// # Examples
///
/// ```
/// use flui_types::geometry::Vec2;
///
/// let velocity = Vec2::new(10.0, 5.0);
/// let scaled = velocity * 2.0;
/// let length = velocity.length();
/// let unit = velocity.normalize();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

// ============================================================================
// Constants
// ============================================================================

impl Vec2 {
    /// Zero vector (0, 0).
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// All ones (1, 1).
    pub const ONE: Self = Self::new(1.0, 1.0);

    /// Unit vector pointing right (+X).
    pub const X: Self = Self::new(1.0, 0.0);

    /// Unit vector pointing up (+Y).
    pub const Y: Self = Self::new(0.0, 1.0);

    /// Negative X unit vector.
    pub const NEG_X: Self = Self::new(-1.0, 0.0);

    /// Negative Y unit vector.
    pub const NEG_Y: Self = Self::new(0.0, -1.0);

    /// Vector with positive infinity components.
    pub const INFINITY: Self = Self::new(f32::INFINITY, f32::INFINITY);

    /// Vector with negative infinity components.
    pub const NEG_INFINITY: Self = Self::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    /// Vector with NaN components.
    pub const NAN: Self = Self::new(f32::NAN, f32::NAN);
}

// ============================================================================
// Constructors
// ============================================================================

impl Vec2 {
    /// Creates a new vector.
    #[inline]
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Creates a vector with both components set to the same value.
    #[inline]
    #[must_use]
    pub const fn splat(v: f32) -> Self {
        Self::new(v, v)
    }

    /// Creates a vector from an array.
    #[inline]
    #[must_use]
    pub const fn from_array(a: [f32; 2]) -> Self {
        Self::new(a[0], a[1])
    }

    /// Creates a vector from a tuple.
    #[inline]
    #[must_use]
    pub const fn from_tuple(t: (f32, f32)) -> Self {
        Self::new(t.0, t.1)
    }

    /// Creates a unit vector from an angle in radians.
    ///
    /// - `angle = 0` → `(1, 0)` (pointing right)
    /// - `angle = π/2` → `(0, 1)` (pointing up)
    #[inline]
    #[must_use]
    pub fn from_angle(angle: f32) -> Self {
        Self::new(angle.cos(), angle.sin())
    }
}

// ============================================================================
// Accessors & Conversion
// ============================================================================

impl Vec2 {
    /// Returns the vector as an array `[x, y]`.
    #[inline]
    #[must_use]
    pub const fn to_array(self) -> [f32; 2] {
        [self.x, self.y]
    }

    /// Returns the vector as a tuple `(x, y)`.
    #[inline]
    #[must_use]
    pub const fn to_tuple(self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// Converts to a point with same coordinates.
    #[inline]
    #[must_use]
    pub const fn to_point(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// Returns a new vector with the x component replaced.
    #[inline]
    #[must_use]
    pub const fn with_x(self, x: f32) -> Self {
        Self::new(x, self.y)
    }

    /// Returns a new vector with the y component replaced.
    #[inline]
    #[must_use]
    pub const fn with_y(self, y: f32) -> Self {
        Self::new(self.x, y)
    }
}

// ============================================================================
// Length & Normalization
// ============================================================================

impl Vec2 {
    /// Returns the length (magnitude) of the vector.
    ///
    /// Also known as `hypot` in kurbo.
    #[inline]
    #[must_use]
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    /// Returns the squared length of the vector.
    ///
    /// Faster than [`length`](Self::length) when you only need to compare magnitudes.
    #[inline]
    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// Returns a normalized (unit length) vector.
    ///
    /// Returns `None` if the vector has zero or near-zero length.
    #[inline]
    #[must_use]
    pub fn try_normalize(self) -> Option<Self> {
        let len = self.length();
        if len > f32::EPSILON {
            Some(Self::new(self.x / len, self.y / len))
        } else {
            None
        }
    }

    /// Returns a normalized vector, or zero if length is zero.
    #[inline]
    #[must_use]
    pub fn normalize(self) -> Self {
        self.try_normalize().unwrap_or(Self::ZERO)
    }

    /// Returns a normalized vector, or the fallback if length is zero.
    #[inline]
    #[must_use]
    pub fn normalize_or(self, fallback: Self) -> Self {
        self.try_normalize().unwrap_or(fallback)
    }

    /// Returns `true` if the vector is normalized (length ≈ 1).
    #[inline]
    #[must_use]
    pub fn is_normalized(self) -> bool {
        (self.length_squared() - 1.0).abs() < 1e-4
    }
}

// ============================================================================
// Vector Operations
// ============================================================================

impl Vec2 {
    /// Dot product with another vector.
    ///
    /// Properties:
    /// - `a · b = |a| |b| cos(θ)`
    /// - `a · b = 0` when perpendicular
    /// - `a · a = |a|²`
    #[inline]
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// 2D cross product (also called "perp dot product").
    ///
    /// Returns the z-component of the 3D cross product if vectors were in XY plane.
    /// Positive when `other` is counter-clockwise from `self`.
    #[inline]
    #[must_use]
    pub fn cross(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    /// Returns a perpendicular vector (rotated 90° counter-clockwise).
    ///
    /// Also known as `turn_90` or `perp`.
    #[inline]
    #[must_use]
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// Linear interpolation between two vectors.
    ///
    /// - `t = 0.0` → `self`
    /// - `t = 0.5` → midpoint
    /// - `t = 1.0` → `other`
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }

    /// Projects this vector onto another vector.
    ///
    /// Returns the component of `self` in the direction of `onto`.
    #[inline]
    #[must_use]
    pub fn project(self, onto: Self) -> Self {
        let len_sq = onto.length_squared();
        if len_sq > f32::EPSILON {
            onto * (self.dot(onto) / len_sq)
        } else {
            Self::ZERO
        }
    }

    /// Reflects this vector about a normal.
    ///
    /// The normal should be normalized for correct results.
    #[inline]
    #[must_use]
    pub fn reflect(self, normal: Self) -> Self {
        self - normal * (2.0 * self.dot(normal))
    }
}

// ============================================================================
// Angle Operations
// ============================================================================

impl Vec2 {
    /// Returns the angle from the positive X axis in radians.
    ///
    /// Result is in range `(-π, π]`.
    #[inline]
    #[must_use]
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    /// Returns the angle between this vector and another in radians.
    ///
    /// Result is in range `[0, π]`.
    #[inline]
    #[must_use]
    pub fn angle_between(self, other: Self) -> f32 {
        let dot = self.dot(other);
        let mags = self.length() * other.length();
        if mags > f32::EPSILON {
            (dot / mags).clamp(-1.0, 1.0).acos()
        } else {
            0.0
        }
    }

    /// Rotates the vector by an angle in radians.
    #[inline]
    #[must_use]
    pub fn rotate(self, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }
}

// ============================================================================
// Component-wise Operations
// ============================================================================

impl Vec2 {
    /// Component-wise minimum.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }

    /// Component-wise maximum.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
    }

    /// Component-wise clamping.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(self.x.clamp(min.x, max.x), self.y.clamp(min.y, max.y))
    }

    /// Clamps the length of the vector.
    #[inline]
    #[must_use]
    pub fn clamp_length(self, min: f32, max: f32) -> Self {
        let len = self.length();
        if len < f32::EPSILON {
            Self::ZERO
        } else if len < min {
            self * (min / len)
        } else if len > max {
            self * (max / len)
        } else {
            self
        }
    }

    /// Component-wise absolute value.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs())
    }

    /// Component-wise signum.
    #[inline]
    #[must_use]
    pub fn signum(self) -> Self {
        Self::new(self.x.signum(), self.y.signum())
    }

    /// Smallest component.
    #[inline]
    #[must_use]
    pub fn min_element(self) -> f32 {
        self.x.min(self.y)
    }

    /// Largest component.
    #[inline]
    #[must_use]
    pub fn max_element(self) -> f32 {
        self.x.max(self.y)
    }
}

// ============================================================================
// Rounding Operations
// ============================================================================

impl Vec2 {
    /// Rounds components to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    /// Rounds components up (toward positive infinity).
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil())
    }

    /// Rounds components down (toward negative infinity).
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    /// Rounds components toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self::new(self.x.trunc(), self.y.trunc())
    }

    /// Rounds components away from zero.
    #[inline]
    #[must_use]
    pub fn expand(self) -> Self {
        Self::new(
            if self.x >= 0.0 {
                self.x.ceil()
            } else {
                self.x.floor()
            },
            if self.y >= 0.0 {
                self.y.ceil()
            } else {
                self.y.floor()
            },
        )
    }

    /// Returns the fractional part of components.
    #[inline]
    #[must_use]
    pub fn fract(self) -> Self {
        Self::new(self.x.fract(), self.y.fract())
    }
}

// ============================================================================
// Validation
// ============================================================================

impl Vec2 {
    /// Returns `true` if both components are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Returns `true` if either component is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(self) -> bool {
        self.x.is_nan() || self.y.is_nan()
    }

    /// Returns `true` if the vector is zero (or very close to zero).
    #[inline]
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.length_squared() < f32::EPSILON * f32::EPSILON
    }
}

// ============================================================================
// Operators: Vec2 ± Vec2
// ============================================================================

impl Add for Vec2 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Vec2 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

// ============================================================================
// Operators: Scalar multiplication/division
// ============================================================================

impl Mul<f32> for Vec2 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;

    #[inline]
    fn mul(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self * rhs.x, self * rhs.y)
    }
}

impl MulAssign<f32> for Vec2 {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl DivAssign<f32> for Vec2 {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Neg for Vec2 {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl From<(f32, f32)> for Vec2 {
    #[inline]
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

impl From<[f32; 2]> for Vec2 {
    #[inline]
    fn from([x, y]: [f32; 2]) -> Self {
        Self::new(x, y)
    }
}

impl From<Vec2> for (f32, f32) {
    #[inline]
    fn from(v: Vec2) -> Self {
        (v.x, v.y)
    }
}

impl From<Vec2> for [f32; 2] {
    #[inline]
    fn from(v: Vec2) -> Self {
        [v.x, v.y]
    }
}

impl From<Point> for Vec2 {
    #[inline]
    fn from(p: Point) -> Self {
        Self::new(p.x, p.y)
    }
}

// ============================================================================
// Display
// ============================================================================

impl fmt::Display for Vec2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Vec2::new(x, y)`.
#[inline]
#[must_use]
pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn test_construction() {
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(v.x, 3.0);
        assert_eq!(v.y, 4.0);

        assert_eq!(Vec2::splat(5.0), Vec2::new(5.0, 5.0));
        assert_eq!(Vec2::from_array([1.0, 2.0]), Vec2::new(1.0, 2.0));
        assert_eq!(Vec2::from_tuple((3.0, 4.0)), Vec2::new(3.0, 4.0));
    }

    #[test]
    fn test_constants() {
        assert_eq!(Vec2::ZERO, Vec2::new(0.0, 0.0));
        assert_eq!(Vec2::ONE, Vec2::new(1.0, 1.0));
        assert_eq!(Vec2::X, Vec2::new(1.0, 0.0));
        assert_eq!(Vec2::Y, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn test_length() {
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(v.length_squared(), 25.0);
    }

    #[test]
    fn test_normalize() {
        let v = Vec2::new(3.0, 4.0);
        let n = v.normalize();
        assert!((n.length() - 1.0).abs() < 1e-6);
        assert!((n.x - 0.6).abs() < 1e-6);
        assert!((n.y - 0.8).abs() < 1e-6);

        assert_eq!(Vec2::ZERO.normalize(), Vec2::ZERO);
        assert!(Vec2::X.is_normalized());
    }

    #[test]
    fn test_dot_cross() {
        let v1 = Vec2::new(2.0, 3.0);
        let v2 = Vec2::new(4.0, 5.0);

        assert_eq!(v1.dot(v2), 23.0); // 2*4 + 3*5
        assert_eq!(v1.cross(v2), -2.0); // 2*5 - 3*4

        // Perpendicular vectors
        assert_eq!(Vec2::X.dot(Vec2::Y), 0.0);
        assert_eq!(Vec2::X.cross(Vec2::Y), 1.0);
    }

    #[test]
    fn test_perp() {
        let v = Vec2::new(1.0, 0.0);
        assert_eq!(v.perp(), Vec2::new(0.0, 1.0));
        assert_eq!(v.dot(v.perp()), 0.0);
    }

    #[test]
    fn test_lerp() {
        let v1 = Vec2::ZERO;
        let v2 = Vec2::new(10.0, 20.0);

        assert_eq!(v1.lerp(v2, 0.0), v1);
        assert_eq!(v1.lerp(v2, 0.5), Vec2::new(5.0, 10.0));
        assert_eq!(v1.lerp(v2, 1.0), v2);
    }

    #[test]
    fn test_angle() {
        assert_eq!(Vec2::X.angle(), 0.0);
        assert!((Vec2::Y.angle() - PI / 2.0).abs() < 1e-6);
        assert!((Vec2::NEG_X.angle() - PI).abs() < 1e-6);
    }

    #[test]
    fn test_from_angle() {
        let v = Vec2::from_angle(PI / 4.0);
        let sqrt2_2 = std::f32::consts::FRAC_1_SQRT_2;
        assert!((v.x - sqrt2_2).abs() < 1e-6);
        assert!((v.y - sqrt2_2).abs() < 1e-6);
    }

    #[test]
    fn test_rotate() {
        let v = Vec2::X;
        let rotated = v.rotate(PI / 2.0);
        assert!((rotated.x).abs() < 1e-6);
        assert!((rotated.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_project_reflect() {
        let v = Vec2::new(3.0, 4.0);
        let onto = Vec2::X;
        assert_eq!(v.project(onto), Vec2::new(3.0, 0.0));

        let incoming = Vec2::new(1.0, -1.0);
        let normal = Vec2::Y;
        let reflected = incoming.reflect(normal);
        assert!((reflected.x - 1.0).abs() < 1e-6);
        assert!((reflected.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_min_max_clamp() {
        let v1 = Vec2::new(5.0, 15.0);
        let v2 = Vec2::new(10.0, 8.0);

        assert_eq!(v1.min(v2), Vec2::new(5.0, 8.0));
        assert_eq!(v1.max(v2), Vec2::new(10.0, 15.0));

        let v = Vec2::new(15.0, -5.0);
        let clamped = v.clamp(Vec2::ZERO, Vec2::splat(10.0));
        assert_eq!(clamped, Vec2::new(10.0, 0.0));
    }

    #[test]
    fn test_clamp_length() {
        let v = Vec2::new(3.0, 4.0); // length = 5

        let clamped_max = v.clamp_length(0.0, 2.0);
        assert!((clamped_max.length() - 2.0).abs() < 1e-6);

        let clamped_min = Vec2::new(0.3, 0.4).clamp_length(5.0, 10.0);
        assert!((clamped_min.length() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_component_ops() {
        let v = Vec2::new(-3.0, 4.0);
        assert_eq!(v.abs(), Vec2::new(3.0, 4.0));
        assert_eq!(v.signum(), Vec2::new(-1.0, 1.0));
        assert_eq!(v.min_element(), -3.0);
        assert_eq!(v.max_element(), 4.0);
    }

    #[test]
    fn test_rounding() {
        let v = Vec2::new(10.6, -3.3);
        assert_eq!(v.round(), Vec2::new(11.0, -3.0));
        assert_eq!(v.ceil(), Vec2::new(11.0, -3.0));
        assert_eq!(v.floor(), Vec2::new(10.0, -4.0));
        assert_eq!(v.trunc(), Vec2::new(10.0, -3.0));
        assert_eq!(v.expand(), Vec2::new(11.0, -4.0));
    }

    #[test]
    fn test_validation() {
        assert!(Vec2::new(1.0, 2.0).is_finite());
        assert!(!Vec2::INFINITY.is_finite());
        assert!(Vec2::NAN.is_nan());
        assert!(Vec2::ZERO.is_zero());
        assert!(!Vec2::ONE.is_zero());
    }

    #[test]
    fn test_operators() {
        let v1 = Vec2::new(10.0, 20.0);
        let v2 = Vec2::new(5.0, 8.0);

        assert_eq!(v1 + v2, Vec2::new(15.0, 28.0));
        assert_eq!(v1 - v2, Vec2::new(5.0, 12.0));
        assert_eq!(v1 * 2.0, Vec2::new(20.0, 40.0));
        assert_eq!(2.0 * v1, Vec2::new(20.0, 40.0));
        assert_eq!(v1 / 2.0, Vec2::new(5.0, 10.0));
        assert_eq!(-v1, Vec2::new(-10.0, -20.0));
    }

    #[test]
    fn test_assign_operators() {
        let mut v = Vec2::new(10.0, 20.0);

        v += Vec2::new(5.0, 5.0);
        assert_eq!(v, Vec2::new(15.0, 25.0));

        v -= Vec2::new(5.0, 5.0);
        assert_eq!(v, Vec2::new(10.0, 20.0));

        v *= 2.0;
        assert_eq!(v, Vec2::new(20.0, 40.0));

        v /= 2.0;
        assert_eq!(v, Vec2::new(10.0, 20.0));
    }

    #[test]
    fn test_conversions() {
        let v = Vec2::new(10.0, 20.0);

        let from_tuple: Vec2 = (10.0, 20.0).into();
        let from_array: Vec2 = [10.0, 20.0].into();
        assert_eq!(from_tuple, v);
        assert_eq!(from_array, v);

        let to_tuple: (f32, f32) = v.into();
        let to_array: [f32; 2] = v.into();
        assert_eq!(to_tuple, (10.0, 20.0));
        assert_eq!(to_array, [10.0, 20.0]);

        let p = Point::new(5.0, 10.0);
        let v_from_p: Vec2 = p.into();
        assert_eq!(v_from_p, Vec2::new(5.0, 10.0));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Vec2::new(10.5, 20.5)), "(10.5, 20.5)");
    }

    #[test]
    fn test_convenience_fn() {
        assert_eq!(vec2(1.0, 2.0), Vec2::new(1.0, 2.0));
    }
}
