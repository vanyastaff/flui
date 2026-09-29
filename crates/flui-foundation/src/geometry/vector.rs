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
//! Vec2 * f64  = Vec2  (scale)
//! Vec2 · Vec2 = f64   (dot product)
//! Vec2 × Vec2 = f64   (2D cross product)
//! ```
use std::{
    fmt::{self, Display},
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign},
};

use super::{
    Point,
    axis::Axis,
    traits::{Along, FloatUnit, NumericUnit, Unit},
};

/// A 2D vector representing direction and magnitude.
///
/// Generic over unit type `T`. Common usage:
/// - `Vec2` - UI displacement
/// - `Vec2` - Normalized/dimensionless vector
///
/// This represents a displacement or direction, not an absolute position.
/// For positions, use [`Point`].
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Vec2;
///
/// let velocity = Vec2::<f64>::new(10.0, 5.0);
/// let normalized = Vec2::<f64>::new(0.6, 0.8);
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Vec2<T: Unit = f64> {
    /// X component.
    pub x: T,
    /// Y component.
    pub y: T,
}

// ============================================================================
// Constants (f64 only for backwards compatibility)
// ============================================================================

impl Vec2<f64> {
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
    pub const INFINITY: Self = Self::new(f64::INFINITY, f64::INFINITY);

    /// Vector with negative infinity components.
    pub const NEG_INFINITY: Self = Self::new(f64::NEG_INFINITY, f64::NEG_INFINITY);

    /// Vector with NaN components.
    pub const NAN: Self = Self::new(f64::NAN, f64::NAN);

    /// Checks if two vectors are approximately equal within epsilon tolerance.
    #[inline]
    #[must_use]
    pub fn approx_eq(self, other: Self) -> bool {
        (self.x - other.x).abs() < f64::EPSILON && (self.y - other.y).abs() < f64::EPSILON
    }

    /// Checks if the vector contains finite values (not NaN or infinity).
    #[inline]
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.is_finite()
    }

    /// Computes the Manhattan distance (L1 norm).
    #[inline]
    #[must_use]
    pub fn manhattan_length(self) -> f64 {
        self.x.abs() + self.y.abs()
    }

    /// Computes the Chebyshev distance (infinity norm).
    #[inline]
    #[must_use]
    pub fn chebyshev_length(self) -> f64 {
        self.x.abs().max(self.y.abs())
    }
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Vec2<T> {
    /// Creates a vector from x and y components.
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
// Array/Tuple Constructors (NumericUnit with Into<f64> + From<f64>)
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: FloatUnit,
{
    /// Creates a vector from a two-element array.
    #[inline]
    #[must_use]
    pub fn from_array(a: [f64; 2]) -> Self {
        Self::new(T::from_f64(a[0]), T::from_f64(a[1]))
    }

    /// Creates a vector from a tuple.
    #[inline]
    #[must_use]
    pub fn from_tuple(t: (f64, f64)) -> Self {
        Self::new(T::from_f64(t.0), T::from_f64(t.1))
    }
}

// ============================================================================
// Angle Constructors (f64 only)
// ============================================================================

impl Vec2<f64> {
    /// Creates a unit vector from an angle in radians.
    ///
    /// - `angle = 0` → `(1, 0)` (pointing right)
    #[inline]
    #[must_use]
    pub fn from_angle(angle: f64) -> Self {
        Self::new(angle.cos(), angle.sin())
    }
}

// ============================================================================
// Accessors & Conversion (generic)
// ============================================================================

impl<T: Unit> Vec2<T> {
    /// Returns a vector with a new x component.
    #[inline]
    #[must_use]
    pub const fn with_x(self, x: T) -> Self {
        Self::new(x, self.y)
    }

    /// Returns a vector with a new y component.
    #[inline]
    #[must_use]
    pub const fn with_y(self, y: T) -> Self {
        Self::new(self.x, y)
    }

    /// Swaps the x and y components.
    #[inline]
    #[must_use]
    pub fn swap(self) -> Self {
        Self::new(self.y, self.x)
    }

    /// Maps a function over both components to create a new vector.
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
    T: Into<f64>,
{
    /// Converts the vector to a two-element array.
    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f64; 2] {
        [self.x.into(), self.y.into()]
    }

    /// Converts the vector to a tuple.
    #[inline]
    #[must_use]
    pub fn to_tuple(self) -> (f64, f64) {
        (self.x.into(), self.y.into())
    }

    /// Converts the vector to a point (displacement becomes position).
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
    /// Requires `T: Into<U>` for the conversion to be valid.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Vec2;
    ///
    /// let logical = Vec2::<f64>::new(10.0, 20.0);
    /// let delta: Vec2 = logical.cast();
    /// assert_eq!(delta.x, 10.0);
    /// assert_eq!(delta.y, 20.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn cast<U: Unit>(self) -> Vec2<U>
    where
        T: Into<U>,
    {
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
    T: Into<f64> + FloatUnit,
{
    /// Returns the length (magnitude) of the vector.
    #[inline]
    #[must_use]
    pub fn length(self) -> f64 {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        x.hypot(y)
    }

    /// Returns the squared length of the vector.
    #[inline]
    #[must_use]
    pub fn length_squared(self) -> f64 {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        x * x + y * y
    }

    /// Returns a normalized (unit length) vector.
    #[inline]
    #[must_use]
    pub fn try_normalize(self) -> Option<Vec2<f64>> {
        let len = self.length();
        if len > f64::EPSILON {
            Some(Vec2::new(self.x.into() / len, self.y.into() / len))
        } else {
            None
        }
    }

    /// Returns a normalized (unit length) vector, or `Vec2::ZERO` if the
    /// length is near zero.
    #[inline]
    #[must_use]
    pub fn normalize(self) -> Vec2<f64> {
        self.try_normalize().unwrap_or(Vec2::ZERO)
    }

    /// Returns a normalized vector, or a fallback if length is near zero.
    #[inline]
    #[must_use]
    pub fn normalize_or(self, fallback: Vec2<f64>) -> Vec2<f64> {
        self.try_normalize().unwrap_or(fallback)
    }

    /// Checks if this vector is normalized (unit length).
    #[inline]
    #[must_use]
    pub fn is_normalized(self) -> bool {
        (self.length_squared() - 1.0).abs() < 1e-4
    }
}

// ============================================================================
// Vector Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64>,
{
    /// Dot product with another vector.
    ///
    /// Properties:
    /// - `a · b = |a| |b| cos(θ)`
    /// - `a · b = 0` when perpendicular
    #[inline]
    #[must_use]
    pub fn dot(self, other: Self) -> f64 {
        let x1: f64 = self.x.into();
        let y1: f64 = self.y.into();
        let x2: f64 = other.x.into();
        let y2: f64 = other.y.into();
        x1 * x2 + y1 * y2
    }

    /// 2D cross product (also called "perp dot product").
    ///
    /// Returns the z-component of the 3D cross product if vectors were in XY
    /// plane.
    #[inline]
    #[must_use]
    pub fn cross(self, other: Self) -> f64 {
        let x1: f64 = self.x.into();
        let y1: f64 = self.y.into();
        let x2: f64 = other.x.into();
        let y2: f64 = other.y.into();
        x1 * y2 - y1 * x2
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64> + FloatUnit,
{
    /// Returns a perpendicular vector (rotated 90° counter-clockwise).
    #[inline]
    #[must_use]
    pub fn perp(self) -> Self {
        Self::new(T::from_f64(-(self.y.into())), T::from_f64(self.x.into()))
    }

    /// Linear interpolation between two vectors.
    ///
    /// - `t = 0.0` → `self`
    /// - `t = 0.5` → midpoint
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        let x1: f64 = self.x.into();
        let y1: f64 = self.y.into();
        let x2: f64 = other.x.into();
        let y2: f64 = other.y.into();

        Self::new(
            T::from_f64(x1 + (x2 - x1) * t),
            T::from_f64(y1 + (y2 - y1) * t),
        )
    }

    /// Projects this vector onto another vector.
    #[inline]
    #[must_use]
    pub fn project(self, onto: Self) -> Self {
        let len_sq = onto.length_squared();
        if len_sq > f64::EPSILON {
            let scale = self.dot(onto) / len_sq;
            Self::new(
                T::from_f64(onto.x.into() * scale),
                T::from_f64(onto.y.into() * scale),
            )
        } else {
            Self::new(T::zero(), T::zero())
        }
    }

    /// Reflects this vector about a normal.
    #[inline]
    #[must_use]
    pub fn reflect(self, normal: Self) -> Self {
        let dot = self.dot(normal);
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        let nx: f64 = normal.x.into();
        let ny: f64 = normal.y.into();

        Self::new(
            T::from_f64(x - nx * (2.0 * dot)),
            T::from_f64(y - ny * (2.0 * dot)),
        )
    }
}

// ============================================================================
// Angle Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64>,
{
    /// Returns the angle from the positive X axis in radians.
    #[inline]
    #[must_use]
    pub fn angle(self) -> f64 {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        y.atan2(x)
    }

    /// Returns the angle from the positive X axis (type-safe version).
    ///
    /// Result is in range `(-π, π]`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::geometry::Vec2;
    /// use std::f64::consts::PI;
    ///
    /// let v = Vec2::new(0.0, 1.0);
    /// let angle = v.angle_radians();
    /// assert!((angle - PI / 2.0).abs() < 0.001);
    /// ```
    #[inline]
    #[must_use]
    pub fn angle_radians(self) -> f64 {
        self.angle()
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64> + FloatUnit,
{
    /// Returns the angle between this vector and another in radians.
    #[inline]
    #[must_use]
    pub fn angle_between(self, other: Self) -> f64 {
        let dot = self.dot(other);
        let mags = self.length() * other.length();
        if mags > f64::EPSILON {
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
    /// use flui_foundation::geometry::Vec2;
    /// use std::f64::consts::PI;
    ///
    /// let v1 = Vec2::new(1.0, 0.0);
    /// let v2 = Vec2::new(0.0, 1.0);
    /// let angle = v1.angle_between_radians(v2);
    /// assert!((angle - PI / 2.0).abs() < 0.001);
    /// ```
    #[inline]
    #[must_use]
    pub fn angle_between_radians(self, other: Self) -> f64 {
        self.angle_between(other)
    }
    /// Rotates the vector by an angle in radians (counter-clockwise).
    #[inline]
    #[must_use]
    pub fn rotate(self, angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();

        Self::new(
            T::from_f64(x * cos - y * sin),
            T::from_f64(x * sin + y * cos),
        )
    }
}

// ============================================================================
// Component-wise Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64> + FloatUnit,
{
    /// Returns a vector with the minimum components from two vectors.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        let x1: f64 = self.x.into();
        let y1: f64 = self.y.into();
        let x2: f64 = other.x.into();
        let y2: f64 = other.y.into();

        Self::new(T::from_f64(x1.min(x2)), T::from_f64(y1.min(y2)))
    }

    /// Returns a vector with the maximum components from two vectors.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        let x1: f64 = self.x.into();
        let y1: f64 = self.y.into();
        let x2: f64 = other.x.into();
        let y2: f64 = other.y.into();

        Self::new(T::from_f64(x1.max(x2)), T::from_f64(y1.max(y2)))
    }

    /// Clamps each component between corresponding min and max values.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        let min_x: f64 = min.x.into();
        let min_y: f64 = min.y.into();
        let max_x: f64 = max.x.into();
        let max_y: f64 = max.y.into();

        Self::new(
            T::from_f64(x.clamp(min_x, max_x)),
            T::from_f64(y.clamp(min_y, max_y)),
        )
    }

    /// Clamps the vector's length to a specified range.
    #[inline]
    #[must_use]
    pub fn clamp_length(self, min: f64, max: f64) -> Self {
        let len = self.length();
        if len < f64::EPSILON {
            Self::new(T::zero(), T::zero())
        } else if len < min {
            let scale = min / len;
            Self::new(
                T::from_f64(self.x.into() * scale),
                T::from_f64(self.y.into() * scale),
            )
        } else if len > max {
            let scale = max / len;
            Self::new(
                T::from_f64(self.x.into() * scale),
                T::from_f64(self.y.into() * scale),
            )
        } else {
            self
        }
    }

    /// Returns a vector with absolute values of each component.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        Self::new(T::from_f64(x.abs()), T::from_f64(y.abs()))
    }

    /// Returns a vector with the sign of each component (-1, 0, or 1).
    #[inline]
    #[must_use]
    pub fn signum(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        Self::new(T::from_f64(x.signum()), T::from_f64(y.signum()))
    }

    /// Returns the minimum component value.
    #[inline]
    #[must_use]
    pub fn min_element(self) -> f64 {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        x.min(y)
    }

    /// Returns the maximum component value.
    #[inline]
    #[must_use]
    pub fn max_element(self) -> f64 {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        x.max(y)
    }
}

// ============================================================================
// Rounding Operations
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64> + FloatUnit,
{
    /// Rounds each component to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        Self::new(T::from_f64(x.round()), T::from_f64(y.round()))
    }

    /// Rounds each component up to the nearest integer.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        Self::new(T::from_f64(x.ceil()), T::from_f64(y.ceil()))
    }

    /// Rounds each component down to the nearest integer.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        Self::new(T::from_f64(x.floor()), T::from_f64(y.floor()))
    }

    /// Truncates each component toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        Self::new(T::from_f64(x.trunc()), T::from_f64(y.trunc()))
    }

    /// Expands each component away from zero (ceil for positive, floor for
    /// negative).
    #[inline]
    #[must_use]
    pub fn expand(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();

        Self::new(
            T::from_f64(if x >= 0.0 { x.ceil() } else { x.floor() }),
            T::from_f64(if y >= 0.0 { y.ceil() } else { y.floor() }),
        )
    }

    /// Returns the fractional part of each component.
    #[inline]
    #[must_use]
    pub fn fract(self) -> Self {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        Self::new(T::from_f64(x.fract()), T::from_f64(y.fract()))
    }
}

// ============================================================================
// Validation
// ============================================================================

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64>,
{
    /// Checks if all components are finite (not infinity or NaN).
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        x.is_finite() && y.is_finite()
    }

    /// Checks if any component is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(self) -> bool {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        x.is_nan() || y.is_nan()
    }
}

impl<T: NumericUnit> Vec2<T>
where
    T: Into<f64> + FloatUnit,
{
    /// Checks if the vector is approximately zero (length near zero).
    #[inline]
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.length_squared() < f64::EPSILON * f64::EPSILON
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

impl<T: NumericUnit + Mul<f64, Output = T>> Mul<f64> for Vec2<T> {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl<T: NumericUnit + Mul<f64, Output = T>> Mul<Vec2<T>> for f64 {
    type Output = Vec2<T>;

    #[inline]
    fn mul(self, rhs: Vec2<T>) -> Vec2<T> {
        Vec2::new(rhs.x * self, rhs.y * self)
    }
}

impl<T: NumericUnit + Mul<f64, Output = T>> MulAssign<f64> for Vec2<T> {
    #[inline]
    fn mul_assign(&mut self, rhs: f64) {
        self.x = self.x * rhs;
        self.y = self.y * rhs;
    }
}

impl<T: NumericUnit + Div<f64, Output = T>> Div<f64> for Vec2<T> {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f64) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl<T: NumericUnit + Div<f64, Output = T>> DivAssign<f64> for Vec2<T> {
    #[inline]
    fn div_assign(&mut self, rhs: f64) {
        self.x = self.x / rhs;
        self.y = self.y / rhs;
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

impl<T: NumericUnit> From<(f64, f64)> for Vec2<T>
where
    T: FloatUnit,
{
    #[inline]
    fn from((x, y): (f64, f64)) -> Self {
        Self::new(T::from_f64(x), T::from_f64(y))
    }
}

impl<T: NumericUnit> From<[f64; 2]> for Vec2<T>
where
    T: FloatUnit,
{
    #[inline]
    fn from([x, y]: [f64; 2]) -> Self {
        Self::new(T::from_f64(x), T::from_f64(y))
    }
}

impl<T: NumericUnit> From<Vec2<T>> for (f64, f64)
where
    T: Into<f64>,
{
    #[inline]
    fn from(v: Vec2<T>) -> Self {
        (v.x.into(), v.y.into())
    }
}

impl<T: NumericUnit> From<Vec2<T>> for [f64; 2]
where
    T: Into<f64>,
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

impl<T: NumericUnit> Display for Vec2<T>
where
    T: Into<f64>,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let x: f64 = self.x.into();
        let y: f64 = self.y.into();
        write!(f, "({x}, {y})")
    }
}

// ============================================================================
// Default
// ============================================================================

impl<T: Unit> Default for Vec2<T> {
    #[inline]
    fn default() -> Self {
        Self::new(T::zero(), T::zero())
    }
}

// ============================================================================
// Convenience function (f64 only for backwards compatibility)
// ============================================================================

/// Creates a logical-pixel vector from `x` and `y`.
#[inline]
#[must_use]
pub const fn vec2(x: f64, y: f64) -> Vec2<f64> {
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
    T: super::traits::Half,
{
    #[inline]
    fn half(self) -> Self {
        Self {
            x: self.x.half(),
            y: self.y.half(),
        }
    }
}

// Negate is now replaced by std::ops::Neg (see Neg impl above)

// ============================================================================
// IsZero trait - Zero check
// ============================================================================

impl<T: Unit> super::traits::IsZero for Vec2<T>
where
    T: super::traits::IsZero,
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
    T: super::traits::Double,
{
    #[inline]
    fn double(self) -> Self {
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
    T: super::traits::ApproxEq,
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        self.x.approx_eq_eps(&other.x, epsilon) && self.y.approx_eq_eps(&other.y, epsilon)
    }
}

// ============================================================================
// Sign trait - Signum operations
// ============================================================================

impl<T: NumericUnit> super::traits::Sign for Vec2<T>
where
    T: super::traits::Sign,
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
    #[inline]
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Vec2::new(T::zero(), T::zero()), |acc, v| {
            Vec2::new(T::add(acc.x, v.x), T::add(acc.y, v.y))
        })
    }
}

impl<'a, T> std::iter::Sum<&'a Vec2<T>> for Vec2<T>
where
    T: NumericUnit,
{
    #[inline]
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Vec2::new(T::zero(), T::zero()), |acc, v| {
            Vec2::new(T::add(acc.x, v.x), T::add(acc.y, v.y))
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

// ============================================================================
// Typed Generic Tests
// ============================================================================
