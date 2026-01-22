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

use std::fmt::{self, Debug, Display};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use super::traits::{Along, Axis, NumericUnit, Unit};
use super::Point;

/// A 2D vector representing direction and magnitude.
///
/// Generic over unit type `T`. Common usage:
/// - `Vec2<Pixels>` - UI displacement
/// - `Vec2<f32>` - Normalized/dimensionless vector
///
/// This represents a displacement or direction, not an absolute position.
/// For positions, use [`Point`].
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Vec2, px, Pixels};
///
/// let velocity = Vec2::<Pixels>::new(px(10.0), px(5.0));
/// let normalized = Vec2::<f32>::new(0.6, 0.8);
/// ```
#[derive(Clone, Copy, PartialEq)]
pub struct Vec2<T: Unit> {
    /// X component.
    pub x: T,
    /// Y component.
    pub y: T,
}

// ============================================================================
// Constants (f32 only for backwards compatibility)
// ============================================================================

impl Vec2<f32> {
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

    /// Checks if this vector is approximately equal to another.
    #[inline]
    #[must_use]
    pub fn approx_eq(self, other: Self) -> bool {
        (self.x - other.x).abs() < f32::EPSILON
            && (self.y - other.y).abs() < f32::EPSILON
    }

    /// Checks if this vector is valid (finite).
    #[inline]
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.is_finite()
    }

    /// Returns the Manhattan distance (|x| + |y|).
    #[inline]
    #[must_use]
    pub fn manhattan_length(self) -> f32 {
        self.x.abs() + self.y.abs()
    }

    /// Returns the Chebyshev distance (max(|x|, |y|)).
    #[inline]
    #[must_use]
    pub fn chebyshev_length(self) -> f32 {
        self.x.abs().max(self.y.abs())
    }
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Vec2<T> {
    /// Creates a new vector (fast, no validation).
    #[inline]
    #[must_use]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    /// Creates a vector with both components set to the same value.
    #[inline]
    #[must_use]
    pub fn splat(value: T) -> Self {
        Self { x: value, y: value }
    }
}

// ============================================================================
// Array/Tuple Constructors (NumericUnit with Into<f32> + From<f32>)
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: From<f32>
{
    /// Creates a vector from an array.
    #[inline]
    #[must_use]
    pub fn from_array(a: [f32; 2]) -> Self {
        Self::new(T::from(a[0]), T::from(a[1]))
    }

    /// Creates a vector from a tuple.
    #[inline]
    #[must_use]
    pub fn from_tuple(t: (f32, f32)) -> Self {
        Self::new(T::from(t.0), T::from(t.1))
    }
}

// ============================================================================
// Angle Constructors (f32 only)
// ============================================================================

impl Vec2<f32> {
    /// Creates a unit vector from an angle in radians.
    ///
    /// - `angle = 0` → `(1, 0)` (pointing right)
    /// - `angle = π/2` → `(0, 1)` (pointing up)
    #[inline]
    #[must_use]
    pub fn from_angle(angle: f32) -> Self {
        Self::new(angle.cos(), angle.sin())
    }

    /// Creates a unit vector from an angle (type-safe version).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Vec2, radians, Radians};
    /// use std::f32::consts::PI;
    ///
    /// let v = Vec2::from_radians(Radians::from_degrees(90.0));
    /// assert!((v.y - 1.0).abs() < 0.001);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_radians(angle: crate::geometry::Radians) -> Self {
        Self::from_angle(angle.0)
    }
}

// ============================================================================
// Accessors & Conversion (generic)
// ============================================================================

impl<T: Unit> Vec2<T> {
    /// Returns a new vector with the x component replaced.
    #[inline]
    #[must_use]
    pub const fn with_x(self, x: T) -> Self {
        Self::new(x, self.y)
    }

    /// Returns a new vector with the y component replaced.
    #[inline]
    #[must_use]
    pub const fn with_y(self, y: T) -> Self {
        Self::new(self.x, y)
    }

    /// Returns a vector with x and y swapped.
    #[inline]
    #[must_use]
    pub fn swap(self) -> Self {
        Self::new(self.y, self.x)
    }

    /// Maps the vector components through a function.
    #[inline]
    #[must_use]
    pub fn map<U: Unit>(self, f: impl Fn(T) -> U) -> Vec2<U> {
        Vec2 {
            x: f(self.x),
            y: f(self.y),
        }
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32>
{
    /// Returns the vector as an array `[x, y]`.
    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f32; 2] {
        [self.x.into(), self.y.into()]
    }

    /// Returns the vector as a tuple `(x, y)`.
    #[inline]
    #[must_use]
    pub fn to_tuple(self) -> (f32, f32) {
        (self.x.into(), self.y.into())
    }

    /// Converts to a point with same coordinates.
    #[inline]
    #[must_use]
    pub fn to_point(self) -> Point<T> {
        Point::new(self.x, self.y)
    }
}

// ============================================================================
// Type Conversions
// ============================================================================

impl<T: Unit> Vec2<T> {
    /// Cast vector to different unit type.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Vec2, Pixels, px};
    ///
    /// let px_vec = Vec2::<Pixels>::new(px(10.0), px(20.0));
    /// let f32_vec: Vec2<f32> = px_vec.cast();
    /// ```
    #[inline]
    #[must_use]
    pub fn cast<U: Unit>(self) -> Vec2<U>
    where
        T: Into<U>
    {
        Vec2 {
            x: self.x.into(),
            y: self.y.into(),
        }
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32>
{
    /// Convert to f32 vector.
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Vec2<f32> {
        Vec2 {
            x: self.x.into(),
            y: self.y.into(),
        }
    }
}

// ============================================================================
// Length & Normalization
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32> + From<f32>
{
    /// Returns the length (magnitude) of the vector.
    ///
    /// Also known as `hypot` in kurbo.
    #[inline]
    #[must_use]
    pub fn length(&self) -> f32 {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        x.hypot(y)
    }

    /// Returns the squared length of the vector.
    ///
    /// Faster than [`length`](Self::length) when you only need to compare magnitudes.
    #[inline]
    #[must_use]
    pub fn length_squared(&self) -> f32 {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        x * x + y * y
    }

    /// Returns a normalized (unit length) vector.
    ///
    /// Returns `None` if the vector has zero or near-zero length.
    #[inline]
    #[must_use]
    pub fn try_normalize(&self) -> Option<Vec2<f32>> {
        let len = self.length();
        if len > f32::EPSILON {
            Some(Vec2::new(self.x.into() / len, self.y.into() / len))
        } else {
            None
        }
    }

    /// Returns a normalized vector, or zero if length is zero.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> Vec2<f32> {
        self.try_normalize().unwrap_or(Vec2::ZERO)
    }

    /// Returns a normalized vector, or the fallback if length is zero.
    #[inline]
    #[must_use]
    pub fn normalize_or(&self, fallback: Vec2<f32>) -> Vec2<f32> {
        self.try_normalize().unwrap_or(fallback)
    }

    /// Returns `true` if the vector is normalized (length ≈ 1).
    #[inline]
    #[must_use]
    pub fn is_normalized(&self) -> bool {
        (self.length_squared() - 1.0).abs() < 1e-4
    }
}

// ============================================================================
// Vector Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32>
{
    /// Dot product with another vector.
    ///
    /// Properties:
    /// - `a · b = |a| |b| cos(θ)`
    /// - `a · b = 0` when perpendicular
    /// - `a · a = |a|²`
    #[inline]
    #[must_use]
    pub fn dot(&self, other: &Self) -> f32 {
        let x1: f32 = self.x.into();
        let y1: f32 = self.y.into();
        let x2: f32 = other.x.into();
        let y2: f32 = other.y.into();
        x1 * x2 + y1 * y2
    }

    /// 2D cross product (also called "perp dot product").
    ///
    /// Returns the z-component of the 3D cross product if vectors were in XY plane.
    /// Positive when `other` is counter-clockwise from `self`.
    #[inline]
    #[must_use]
    pub fn cross(&self, other: &Self) -> f32 {
        let x1: f32 = self.x.into();
        let y1: f32 = self.y.into();
        let x2: f32 = other.x.into();
        let y2: f32 = other.y.into();
        x1 * y2 - y1 * x2
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32> + From<f32>
{
    /// Returns a perpendicular vector (rotated 90° counter-clockwise).
    ///
    /// Also known as `turn_90` or `perp`.
    #[inline]
    #[must_use]
    pub fn perp(&self) -> Self {
        Self::new(
            T::from(-(self.y.into())),
            T::from(self.x.into())
        )
    }

    /// Linear interpolation between two vectors.
    ///
    /// - `t = 0.0` → `self`
    /// - `t = 0.5` → midpoint
    /// - `t = 1.0` → `other`
    #[inline]
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let x1: f32 = self.x.into();
        let y1: f32 = self.y.into();
        let x2: f32 = other.x.into();
        let y2: f32 = other.y.into();

        Self::new(
            T::from(x1 + (x2 - x1) * t),
            T::from(y1 + (y2 - y1) * t),
        )
    }

    /// Projects this vector onto another vector.
    ///
    /// Returns the component of `self` in the direction of `onto`.
    #[inline]
    #[must_use]
    pub fn project(&self, onto: &Self) -> Self {
        let len_sq = onto.length_squared();
        if len_sq > f32::EPSILON {
            let scale = self.dot(onto) / len_sq;
            Self::new(
                T::from(onto.x.into() * scale),
                T::from(onto.y.into() * scale),
            )
        } else {
            Self::new(T::zero(), T::zero())
        }
    }

    /// Reflects this vector about a normal.
    ///
    /// The normal should be normalized for correct results.
    #[inline]
    #[must_use]
    pub fn reflect(&self, normal: &Self) -> Self {
        let dot = self.dot(normal);
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        let nx: f32 = normal.x.into();
        let ny: f32 = normal.y.into();

        Self::new(
            T::from(x - nx * (2.0 * dot)),
            T::from(y - ny * (2.0 * dot)),
        )
    }
}

// ============================================================================
// Angle Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32>
{
    /// Returns the angle from the positive X axis in radians.
    ///
    /// Result is in range `(-π, π]`.
    #[inline]
    #[must_use]
    pub fn angle(&self) -> f32 {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        y.atan2(x)
    }

    /// Returns the angle from the positive X axis (type-safe version).
    ///
    /// Result is in range `(-π, π]`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Vec2, Radians};
    /// use std::f32::consts::PI;
    ///
    /// let v = Vec2::new(0.0, 1.0);
    /// let angle = v.angle_radians();
    /// assert!((angle.0 - PI / 2.0).abs() < 0.001);
    /// ```
    #[inline]
    #[must_use]
    pub fn angle_radians(&self) -> crate::geometry::Radians {
        crate::geometry::radians(self.angle())
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32> + From<f32>
{
    /// Returns the angle between this vector and another in radians.
    ///
    /// Result is in range `[0, π]`.
    #[inline]
    #[must_use]
    pub fn angle_between(&self, other: &Self) -> f32 {
        let dot = self.dot(other);
        let mags = self.length() * other.length();
        if mags > f32::EPSILON {
            (dot / mags).clamp(-1.0, 1.0).acos()
        } else {
            0.0
        }
    }

    /// Returns the angle between this vector and another (type-safe version).
    ///
    /// Result is in range `[0, π]`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Vec2, Radians};
    /// use std::f32::consts::PI;
    ///
    /// let v1 = Vec2::new(1.0, 0.0);
    /// let v2 = Vec2::new(0.0, 1.0);
    /// let angle = v1.angle_between_radians(&v2);
    /// assert!((angle.0 - PI / 2.0).abs() < 0.001);
    /// ```
    #[inline]
    #[must_use]
    pub fn angle_between_radians(&self, other: &Self) -> crate::geometry::Radians {
        crate::geometry::radians(self.angle_between(other))
    }
    /// Rotates the vector by an angle in radians.
    #[inline]
    #[must_use]
    pub fn rotate(&self, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();

        Self::new(
            T::from(x * cos - y * sin),
            T::from(x * sin + y * cos)
        )
    }

    /// Rotates the vector by an angle (type-safe version).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Vec2, Radians};
    /// use std::f32::consts::PI;
    ///
    /// let v = Vec2::new(1.0, 0.0);
    /// let rotated = v.rotate_radians(Radians::from_degrees(90.0));
    /// assert!((rotated.y - 1.0).abs() < 0.001);
    /// ```
    #[inline]
    #[must_use]
    pub fn rotate_radians(&self, angle: crate::geometry::Radians) -> Self {
        self.rotate(angle.0)
    }
}

// ============================================================================
// Component-wise Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32> + From<f32>
{
    /// Component-wise minimum.
    #[inline]
    #[must_use]
    pub fn min(&self, other: &Self) -> Self {
        let x1: f32 = self.x.into();
        let y1: f32 = self.y.into();
        let x2: f32 = other.x.into();
        let y2: f32 = other.y.into();

        Self::new(T::from(x1.min(x2)), T::from(y1.min(y2)))
    }

    /// Component-wise maximum.
    #[inline]
    #[must_use]
    pub fn max(&self, other: &Self) -> Self {
        let x1: f32 = self.x.into();
        let y1: f32 = self.y.into();
        let x2: f32 = other.x.into();
        let y2: f32 = other.y.into();

        Self::new(T::from(x1.max(x2)), T::from(y1.max(y2)))
    }

    /// Component-wise clamping.
    #[inline]
    #[must_use]
    pub fn clamp(&self, min: &Self, max: &Self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        let min_x: f32 = min.x.into();
        let min_y: f32 = min.y.into();
        let max_x: f32 = max.x.into();
        let max_y: f32 = max.y.into();

        Self::new(
            T::from(x.clamp(min_x, max_x)),
            T::from(y.clamp(min_y, max_y))
        )
    }

    /// Clamps the length of the vector.
    #[inline]
    #[must_use]
    pub fn clamp_length(&self, min: f32, max: f32) -> Self {
        let len = self.length();
        if len < f32::EPSILON {
            Self::new(T::zero(), T::zero())
        } else if len < min {
            let scale = min / len;
            Self::new(
                T::from(self.x.into() * scale),
                T::from(self.y.into() * scale),
            )
        } else if len > max {
            let scale = max / len;
            Self::new(
                T::from(self.x.into() * scale),
                T::from(self.y.into() * scale),
            )
        } else {
            *self
        }
    }

    /// Component-wise absolute value.
    #[inline]
    #[must_use]
    pub fn abs(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        Self::new(T::from(x.abs()), T::from(y.abs()))
    }

    /// Component-wise signum.
    #[inline]
    #[must_use]
    pub fn signum(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        Self::new(T::from(x.signum()), T::from(y.signum()))
    }

    /// Smallest component.
    #[inline]
    #[must_use]
    pub fn min_element(&self) -> f32 {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        x.min(y)
    }

    /// Largest component.
    #[inline]
    #[must_use]
    pub fn max_element(&self) -> f32 {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        x.max(y)
    }
}

// ============================================================================
// Rounding Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32> + From<f32>
{
    /// Rounds components to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        Self::new(T::from(x.round()), T::from(y.round()))
    }

    /// Rounds components up (toward positive infinity).
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        Self::new(T::from(x.ceil()), T::from(y.ceil()))
    }

    /// Rounds components down (toward negative infinity).
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        Self::new(T::from(x.floor()), T::from(y.floor()))
    }

    /// Rounds components toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        Self::new(T::from(x.trunc()), T::from(y.trunc()))
    }

    /// Rounds components away from zero.
    #[inline]
    #[must_use]
    pub fn expand(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();

        Self::new(
            T::from(if x >= 0.0 { x.ceil() } else { x.floor() }),
            T::from(if y >= 0.0 { y.ceil() } else { y.floor() }),
        )
    }

    /// Returns the fractional part of components.
    #[inline]
    #[must_use]
    pub fn fract(&self) -> Self {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        Self::new(T::from(x.fract()), T::from(y.fract()))
    }
}

// ============================================================================
// Validation
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32>
{
    /// Returns `true` if both components are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        x.is_finite() && y.is_finite()
    }

    /// Returns `true` if either component is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(&self) -> bool {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        x.is_nan() || y.is_nan()
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f32> + From<f32>
{
    /// Returns `true` if the vector is zero (or very close to zero).
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.length_squared() < f32::EPSILON * f32::EPSILON
    }
}

// ============================================================================
// Operators: Vec2 ± Vec2
// ============================================================================

impl<T: NumericUnit> Add for Vec2<T> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x.add(rhs.x), self.y.add(rhs.y))
    }
}

impl<T: NumericUnit> AddAssign for Vec2<T> {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x = self.x.add(rhs.x);
        self.y = self.y.add(rhs.y);
    }
}

impl<T: NumericUnit> Sub for Vec2<T> {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x.sub(rhs.x), self.y.sub(rhs.y))
    }
}

impl<T: NumericUnit> SubAssign for Vec2<T> {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x = self.x.sub(rhs.x);
        self.y = self.y.sub(rhs.y);
    }
}

// ============================================================================
// Operators: Scalar multiplication/division
// ============================================================================

impl<T: NumericUnit> Mul<f32> for Vec2<T> {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x.mul(rhs), self.y.mul(rhs))
    }
}

impl<T: NumericUnit> Mul<Vec2<T>> for f32 {
    type Output = Vec2<T>;

    #[inline]
    fn mul(self, rhs: Vec2<T>) -> Vec2<T> {
        Vec2::new(rhs.x.mul(self), rhs.y.mul(self))
    }
}

impl<T: NumericUnit> MulAssign<f32> for Vec2<T> {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.x = self.x.mul(rhs);
        self.y = self.y.mul(rhs);
    }
}

impl<T: NumericUnit> Div<f32> for Vec2<T> {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x.div(rhs), self.y.div(rhs))
    }
}

impl<T: NumericUnit> DivAssign<f32> for Vec2<T> {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.x = self.x.div(rhs);
        self.y = self.y.div(rhs);
    }
}

impl<T: NumericUnit + Neg<Output = T>> Neg for Vec2<T> {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl<T: NumericUnit> From<(f32, f32)> for Vec2<T>
where
    T: From<f32>
{
    #[inline]
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(T::from(x), T::from(y))
    }
}

impl<T: NumericUnit> From<[f32; 2]> for Vec2<T>
where
    T: From<f32>
{
    #[inline]
    fn from([x, y]: [f32; 2]) -> Self {
        Self::new(T::from(x), T::from(y))
    }
}

impl<T: NumericUnit> From<Vec2<T>> for (f32, f32)
where
    T: Into<f32>
{
    #[inline]
    fn from(v: Vec2<T>) -> Self {
        (v.x.into(), v.y.into())
    }
}

impl<T: NumericUnit> From<Vec2<T>> for [f32; 2]
where
    T: Into<f32>
{
    #[inline]
    fn from(v: Vec2<T>) -> Self {
        [v.x.into(), v.y.into()]
    }
}

impl<T: Unit> From<Point<T>> for Vec2<T> {
    #[inline]
    fn from(p: Point<T>) -> Self {
        Self::new(p.x, p.y)
    }
}

// ============================================================================
// Debug & Display
// ============================================================================

impl<T: Unit + Debug> Debug for Vec2<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vec2")
            .field("x", &self.x)
            .field("y", &self.y)
            .finish()
    }
}

impl<T: NumericUnit> Display for Vec2<T>
where
    T: Into<f32>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let x: f32 = self.x.into();
        let y: f32 = self.y.into();
        write!(f, "({}, {})", x, y)
    }
}

// ============================================================================
// Default
// ============================================================================

impl<T: Unit> Default for Vec2<T> {
    fn default() -> Self {
        Self::new(T::zero(), T::zero())
    }
}

// ============================================================================
// Convenience function (f32 only for backwards compatibility)
// ============================================================================

/// Shorthand for `Vec2::new(x, y)`.
#[inline]
#[must_use]
pub const fn vec2(x: f32, y: f32) -> Vec2<f32> {
    Vec2::new(x, y)
}

// ============================================================================
// Along trait - Axis-based access
// ============================================================================

impl<T: NumericUnit> Along for Vec2<T> {
    type Unit = T;

    #[inline]
    fn along(&self, axis: Axis) -> Self::Unit {
        match axis {
            Axis::Horizontal => self.x,
            Axis::Vertical => self.y,
        }
    }

    #[inline]
    fn apply_along(&self, axis: Axis, f: impl FnOnce(Self::Unit) -> Self::Unit) -> Self {
        match axis {
            Axis::Horizontal => Self::new(f(self.x), self.y),
            Axis::Vertical => Self::new(self.x, f(self.y)),
        }
    }
}

// ============================================================================
// Half trait - Compute half value
// ============================================================================

impl<T: Unit> super::traits::Half for Vec2<T>
where
    T: super::traits::Half
{
    #[inline]
    fn half(&self) -> Self {
        Self { x: self.x.half(), y: self.y.half() }
    }
}

// Negate is now replaced by std::ops::Neg (see Neg impl above)

// ============================================================================
// IsZero trait - Zero check
// ============================================================================

impl<T: Unit> super::traits::IsZero for Vec2<T>
where
    T: super::traits::IsZero
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.x.is_zero() && self.y.is_zero()
    }
}

// ============================================================================
// Double trait - Double the value
// ============================================================================

impl<T: Unit> super::traits::Double for Vec2<T>
where
    T: super::traits::Double
{
    #[inline]
    fn double(&self) -> Self {
        Self {
            x: self.x.double(),
            y: self.y.double(),
        }
    }
}

// ============================================================================
// ApproxEq trait - Approximate equality
// ============================================================================

impl<T: Unit> super::traits::ApproxEq for Vec2<T>
where
    T: super::traits::ApproxEq
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        self.x.approx_eq_eps(&other.x, epsilon)
            && self.y.approx_eq_eps(&other.y, epsilon)
    }
}

// ============================================================================
// Sign trait - Signum operations
// ============================================================================

impl<T: NumericUnit> super::traits::Sign for Vec2<T>
where
    T: super::traits::Sign
{
    #[inline]
    fn signum(self) -> Self {
        Self {
            x: self.x.signum(),
            y: self.y.signum(),
        }
    }

    #[inline]
    fn is_positive(&self) -> bool {
        self.x.is_positive() && self.y.is_positive()
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.x.is_negative() || self.y.is_negative()
    }
}

// ============================================================================
// Sum trait - Iterator summing
// ============================================================================

impl<T> std::iter::Sum for Vec2<T>
where
    T: NumericUnit,
{
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Vec2::new(T::zero(), T::zero()), |acc, v| Vec2::new(
            T::add(acc.x, v.x),
            T::add(acc.y, v.y),
        ))
    }
}

impl<'a, T> std::iter::Sum<&'a Vec2<T>> for Vec2<T>
where
    T: NumericUnit,
{
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Vec2::new(T::zero(), T::zero()), |acc, v| Vec2::new(
            T::add(acc.x, v.x),
            T::add(acc.y, v.y),
        ))
    }
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

        assert_eq!(v1.dot(&v2), 23.0); // 2*4 + 3*5
        assert_eq!(v1.cross(&v2), -2.0); // 2*5 - 3*4

        // Perpendicular vectors
        assert_eq!(Vec2::X.dot(&Vec2::Y), 0.0);
        assert_eq!(Vec2::X.cross(&Vec2::Y), 1.0);
    }

    #[test]
    fn test_perp() {
        let v = Vec2::new(1.0, 0.0);
        assert_eq!(v.perp(), Vec2::new(0.0, 1.0));
        assert_eq!(v.dot(&v.perp()), 0.0);
    }

    #[test]
    fn test_lerp() {
        let v1 = Vec2::ZERO;
        let v2 = Vec2::new(10.0, 20.0);

        assert_eq!(v1.lerp(&v2, 0.0), v1);
        assert_eq!(v1.lerp(&v2, 0.5), Vec2::new(5.0, 10.0));
        assert_eq!(v1.lerp(&v2, 1.0), v2);
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
        assert_eq!(v.project(&onto), Vec2::new(3.0, 0.0));

        let incoming = Vec2::new(1.0, -1.0);
        let normal = Vec2::Y;
        let reflected = incoming.reflect(&normal);
        assert!((reflected.x - 1.0).abs() < 1e-6);
        assert!((reflected.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_min_max_clamp() {
        let v1 = Vec2::new(5.0, 15.0);
        let v2 = Vec2::new(10.0, 8.0);

        assert_eq!(v1.min(&v2), Vec2::new(5.0, 8.0));
        assert_eq!(v1.max(&v2), Vec2::new(10.0, 15.0));

        let v = Vec2::new(15.0, -5.0);
        let clamped = v.clamp(&Vec2::ZERO, &Vec2::splat(10.0));
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

        let from_tuple: Vec2<f32> = (10.0, 20.0).into();
        let from_array: Vec2<f32> = [10.0, 20.0].into();
        assert_eq!(from_tuple, v);
        assert_eq!(from_array, v);

        let to_tuple: (f32, f32) = v.into();
        let to_array: [f32; 2] = v.into();
        assert_eq!(to_tuple, (10.0, 20.0));
        assert_eq!(to_array, [10.0, 20.0]);

        let p = Point::new(5.0, 10.0);
        let v_from_p: Vec2<f32> = p.into();
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

// ============================================================================
// Typed Generic Tests
// ============================================================================

#[cfg(test)]
mod typed_tests {
    use super::*;
    use crate::geometry::{Pixels, px};

    #[test]
    fn test_vec2_new() {
        let v = Vec2::<Pixels>::new(px(3.0), px(4.0));
        assert_eq!(v.x.get(), 3.0);
        assert_eq!(v.y.get(), 4.0);
    }

    #[test]
    fn test_vec2_length() {
        let v = Vec2::<f32>::new(3.0, 4.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(v.length_squared(), 25.0);
    }

    #[test]
    fn test_vec2_normalize() {
        let v = Vec2::<f32>::new(3.0, 4.0);
        let n = v.normalize();
        assert!((n.length() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_dot_cross() {
        let v1 = Vec2::<f32>::new(1.0, 0.0);
        let v2 = Vec2::<f32>::new(0.0, 1.0);
        assert_eq!(v1.dot(&v2), 0.0);
        assert_eq!(v1.cross(&v2), 1.0);
    }

    #[test]
    fn test_vec2_arithmetic() {
        let v1 = Vec2::<Pixels>::new(px(10.0), px(20.0));
        let v2 = Vec2::<Pixels>::new(px(5.0), px(10.0));

        let v3 = v1 + v2;
        assert_eq!(v3.x.get(), 15.0);

        let v4 = v1 * 2.0;
        assert_eq!(v4.x.get(), 20.0);
    }

    #[test]
    fn test_vec2_cast() {
        let px_vec = Vec2::<Pixels>::new(px(10.0), px(20.0));
        let f32_vec: Vec2<f32> = px_vec.cast();
        assert_eq!(f32_vec.x, 10.0);
        assert_eq!(f32_vec.y, 20.0);
    }

    #[test]
    fn test_vec2_rotate() {
        let v = Vec2::<Pixels>::new(px(1.0), px(0.0));
        let rotated = v.rotate(std::f32::consts::PI / 2.0);
        assert!((rotated.x.get()).abs() < 0.001);
        assert!((rotated.y.get() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_utility_traits() {
        use crate::geometry::{Axis, Along, Half, IsZero, Double, ApproxEq, Sign};

        // Test Along trait
        let v = Vec2::<Pixels>::new(px(10.0), px(20.0));
        assert_eq!(v.along(Axis::Horizontal).0, 10.0);
        assert_eq!(v.along(Axis::Vertical).0, 20.0);

        let modified = v.apply_along(Axis::Horizontal, |x| px(x.0 * 2.0));
        assert_eq!(modified.x.0, 20.0);
        assert_eq!(modified.y.0, 20.0);

        // Test Half trait
        let half_v = v.half();
        assert_eq!(half_v.x.0, 5.0);
        assert_eq!(half_v.y.0, 10.0);

        // Test negation (using std::ops::Neg)
        let neg_v = -v;
        assert_eq!(neg_v.x.0, -10.0);
        assert_eq!(neg_v.y.0, -20.0);

        // Test IsZero trait
        let zero = Vec2::<Pixels>::new(px(0.0), px(0.0));
        assert!(zero.is_zero());
        assert!(!v.is_zero());

        // Test Double trait
        let doubled = v.double();
        assert_eq!(doubled.x.0, 20.0);
        assert_eq!(doubled.y.0, 40.0);

        // Test ApproxEq trait
        let v2 = Vec2::<Pixels>::new(px(10.0 + 1e-8), px(20.0 - 1e-8));
        assert!(v.approx_eq_eps(&v2, 1e-6));

        // Test Sign trait
        let v_f32 = Vec2::<f32>::new(-10.0, 20.0);
        let signum_v: Vec2<f32> = Sign::signum(v_f32);
        assert_eq!(signum_v.x, -1.0);
        assert_eq!(signum_v.y, 1.0);
    }

    #[test]
    fn test_vec2_swap() {
        let v = Vec2::<f32>::new(10.0, 20.0);
        let swapped = v.swap();
        assert_eq!(swapped.x, 20.0);
        assert_eq!(swapped.y, 10.0);
    }

    #[test]
    fn test_vec2_map() {
        let v = Vec2::<f32>::new(2.0, 3.0);
        let mapped = v.map(|c| c * 2.0);
        assert_eq!(mapped.x, 4.0);
        assert_eq!(mapped.y, 6.0);
    }

    #[test]
    fn test_vec2_distance_metrics() {
        let v = Vec2::<f32>::new(3.0, 4.0);
        assert_eq!(v.manhattan_length(), 7.0);  // |3| + |4|
        assert_eq!(v.chebyshev_length(), 4.0);  // max(|3|, |4|)
        assert_eq!(v.length(), 5.0);            // sqrt(3^2 + 4^2)
    }

    #[test]
    fn test_vec2_sum_iterator() {
        let vectors = vec![
            Vec2::<f32>::new(1.0, 2.0),
            Vec2::<f32>::new(3.0, 4.0),
            Vec2::<f32>::new(5.0, 6.0),
        ];
        let total: Vec2<f32> = vectors.iter().sum();
        assert_eq!(total.x, 9.0);
        assert_eq!(total.y, 12.0);

        let total_owned: Vec2<f32> = vectors.into_iter().sum();
        assert_eq!(total_owned.x, 9.0);
        assert_eq!(total_owned.y, 12.0);
    }

    #[test]
    fn test_vec2_is_valid() {
        assert!(Vec2::<f32>::new(1.0, 2.0).is_valid());
        assert!(!Vec2::<f32>::INFINITY.is_valid());
        assert!(!Vec2::<f32>::NAN.is_valid());
    }
}
