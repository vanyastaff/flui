//! 2D offset (position/translation) type
//!
//! This module provides an immutable 2D offset type.
use std::{
    fmt::{self, Display},
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign},
};

use super::{
    Point, Size, Vec2,
    traits::{NumericUnit, Unit},
};

/// An immutable 2D offset in Cartesian coordinates.
///
/// This represents a translation or displacement in 2D space.
///
/// Generic over unit type `T`. Common usage:
/// - `Offset` - UI displacement
/// - `Offset` - Normalized/dimensionless offset
///
/// # Distinction from Vec2
///
/// `Offset` and `Vec2` are mathematically identical but semantically different:
/// - `Offset`: displacement with `dx`/`dy` naming
/// - `Vec2`: General vector with `x`/`y` naming
///
/// They are freely convertible.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Offset;
///
/// let offset = Offset::<f64>::new(10.0, 20.0);
/// assert_eq!(offset.dx, 10.0);
/// assert_eq!(offset.dy, 20.0);
///
/// let scaled = offset * 2.0;
/// assert_eq!(scaled.dx, 20.0);
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Offset<T: Unit = f64> {
    /// The horizontal component.
    pub dx: T,

    /// The vertical component.
    pub dy: T,
}

// ============================================================================
// Constants (f64 only for backwards compatibility)
// ============================================================================

impl Offset<f64> {
    /// An offset with zero displacement.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// An offset with infinite displacement.
    pub const INFINITE: Self = Self::new(f64::INFINITY, f64::INFINITY);
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Offset<T> {
    /// Creates a new offset (fast, no validation).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// assert_eq!(offset.dx, 10.0);
    /// assert_eq!(offset.dy, 20.0);
    #[inline]
    #[must_use]
    pub const fn new(dx: T, dy: T) -> Self {
        Self { dx, dy }
    }

    /// Returns a new offset with dx and dy swapped.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let swapped = offset.swap();
    /// assert_eq!(swapped.dx, 20.0);
    /// assert_eq!(swapped.dy, 10.0);
    #[inline]
    #[must_use]
    pub fn swap(self) -> Self {
        Self {
            dx: self.dy,
            dy: self.dx,
        }
    }

    /// Applies a transformation function to both components.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset: Offset = Offset::new(10.0, 20.0);
    /// let doubled: Offset = offset.map(|v| v * 2.0);
    /// assert_eq!(doubled.dx, 20.0);
    /// assert_eq!(doubled.dy, 40.0);
    #[inline]
    #[must_use]
    pub fn map<U: Unit>(self, f: impl Fn(T) -> U) -> Offset<U> {
        Offset {
            dx: f(self.dx),
            dy: f(self.dy),
        }
    }
}

// ============================================================================
// Conversions between Vec2 and Offset
// ============================================================================

impl<T: Unit> From<Vec2<T>> for Offset<T> {
    #[inline]
    fn from(v: Vec2<T>) -> Self {
        Offset { dx: v.x, dy: v.y }
    }
}

impl<T: Unit> From<Offset<T>> for Vec2<T> {
    #[inline]
    fn from(o: Offset<T>) -> Self {
        Vec2 { x: o.dx, y: o.dy }
    }
}

impl<T: Unit> Offset<T> {
    /// Convert to Vec2 with same coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::{Offset, Vec2};
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let vec: Vec2 = offset.to_vec2();
    /// assert_eq!(vec.x, 10.0);
    /// assert_eq!(vec.y, 20.0);
    #[inline]
    #[must_use]
    pub fn to_vec2(self) -> Vec2<T> {
        Vec2 {
            x: self.dx,
            y: self.dy,
        }
    }
}

// ============================================================================
// Type Conversions
// ============================================================================

impl<T: Unit> Offset<T> {
    /// Cast offset to different unit type.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let px_offset = Offset::<f64>::new(10.0, 20.0);
    /// let f32_offset: Offset = px_offset.cast();
    /// assert_eq!(f32_offset.dx, 10.0);
    #[inline]
    #[must_use]
    pub fn cast<U: Unit>(self) -> Offset<U>
    where
        T: Into<U>,
    {
        Offset {
            dx: self.dx.into(),
            dy: self.dy.into(),
        }
    }
}

// ============================================================================
// Legacy Float Methods (for backwards compatibility)
// ============================================================================

impl Offset<f64> {
    /// Create an offset from a direction (in radians) and distance.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset = Offset::from_direction(0.0, 10.0);
    /// assert!((offset.dx - 10.0).abs() < 0.001);
    /// assert!(offset.dy.abs() < 0.001);
    /// ```
    #[inline]
    pub fn from_direction(direction: f64, distance: f64) -> Self {
        Self::new(distance * direction.cos(), distance * direction.sin())
    }

    /// Create an offset representing the displacement from one point to
    /// another.
    ///
    /// Returns `to - from` as an offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::{Offset, Point};
    ///
    /// let from = Point::new(10.0, 20.0);
    /// let to = Point::new(30.0, 50.0);
    /// let offset = Offset::from_points(from, to);
    /// assert_eq!(offset.dx, 20.0);
    /// assert_eq!(offset.dy, 30.0);
    /// ```
    #[inline]
    pub fn from_points(from: Point<f64>, to: Point<f64>) -> Self {
        Self::new(to.x - from.x, to.y - from.y)
    }

    /// Check if this offset is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// assert!(Offset::ZERO.is_zero());
    /// assert!(!Offset::new(1.0, 0.0).is_zero());
    /// ```
    #[inline]
    pub fn is_zero(self) -> bool {
        self.dx == 0.0 && self.dy == 0.0
    }

    /// Get the magnitude (distance) of this offset from the origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset = Offset::new(3.0, 4.0);
    /// assert_eq!(offset.distance(), 5.0); // 3-4-5 triangle
    #[inline]
    #[must_use]
    pub fn distance(self) -> f64 {
        self.dx.hypot(self.dy)
    }

    /// Get the squared magnitude (avoids sqrt for performance).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset = Offset::new(3.0, 4.0);
    /// assert_eq!(offset.distance_squared(), 25.0);
    #[inline]
    #[must_use]
    pub const fn distance_squared(&self) -> f64 {
        self.dx * self.dx + self.dy * self.dy
    }

    /// Get the direction of this offset in radians.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let right = Offset::new(1.0, 0.0);
    /// assert!((right.direction() - 0.0).abs() < 0.001);
    /// ```
    #[inline]
    pub fn direction(self) -> f64 {
        self.dy.atan2(self.dx)
    }

    /// Check if this offset is finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// assert!(Offset::ZERO.is_finite());
    /// assert!(!Offset::INFINITE.is_finite());
    /// ```
    #[inline]
    pub fn is_finite(self) -> bool {
        self.dx.is_finite() && self.dy.is_finite()
    }

    /// Check if this offset is infinite.
    #[inline]
    pub fn is_infinite(self) -> bool {
        !self.is_finite()
    }

    /// Scale the offset by a factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let scaled = offset.scale(2.0);
    /// assert_eq!(scaled, Offset::new(20.0, 40.0));
    /// ```
    #[inline]
    pub fn scale(self, factor: f64) -> Self {
        Self::new(self.dx * factor, self.dy * factor)
    }

    /// Translate an offset by another offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let a = Offset::new(10.0, 20.0);
    /// let b = Offset::new(5.0, 10.0);
    /// let c = a.translate(b);
    /// assert_eq!(c, Offset::new(15.0, 30.0));
    /// ```
    #[inline]
    pub fn translate(self, other: impl Into<Offset<f64>>) -> Self {
        let other = other.into();
        Self::new(self.dx + other.dx, self.dy + other.dy)
    }

    /// Linear interpolation between two offsets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let a = Offset::new(0.0, 0.0);
    /// let b = Offset::new(10.0, 10.0);
    /// let mid = a.lerp(b, 0.5);
    /// assert_eq!(mid, Offset::new(5.0, 5.0));
    /// ```
    #[inline]
    pub fn lerp(self, other: impl Into<Offset<f64>>, t: f64) -> Offset<f64> {
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
    /// use flui_foundation::geometry::{Offset, Point};
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let point = offset.to_point();
    /// assert_eq!(point, Point::new(10.0, 20.0));
    #[inline]
    #[must_use]
    pub const fn to_point(self) -> Point<f64> {
        Point::new(self.dx, self.dy)
    }

    /// Convert to a Size (treating offset as width/height).
    ///
    /// Negative components are clamped to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::{Offset, Size};
    ///
    /// let offset = Offset::new(10.0, 20.0);
    /// let size = offset.to_size();
    /// assert_eq!(size, Size::new(10.0, 20.0));
    #[inline]
    #[must_use]
    pub fn to_size(self) -> Size<f64> {
        Size::new(self.dx.max(0.0), self.dy.max(0.0))
    }

    /// Normalize this offset to a unit vector.
    ///
    /// Returns zero for a near-zero length or non-finite components. Finite
    /// directions are retained even when their magnitude exceeds `f64::MAX`.
    #[inline]
    #[must_use]
    pub fn normalize(self) -> Offset<f64> {
        Vec2::from(self).normalize().into()
    }

    /// Compute the dot product of this offset and another.
    #[inline]
    #[must_use]
    pub const fn dot(self, other: Offset<f64>) -> f64 {
        self.dx * other.dx + self.dy * other.dy
    }

    /// Compute the 2D cross product (determinant) of this offset and another.
    ///
    /// Positive when `other` is counter-clockwise from `self`, negative when
    /// clockwise, and zero when the offsets are parallel.
    #[inline]
    #[must_use]
    pub const fn cross(self, other: Offset<f64>) -> f64 {
        self.dx * other.dy - self.dy * other.dx
    }

    /// Rotate this offset around the origin by `angle` radians.
    #[inline]
    #[must_use]
    pub fn rotate(self, angle: f64) -> Offset<f64> {
        let (sin, cos) = angle.sin_cos();
        Offset::new(self.dx * cos - self.dy * sin, self.dx * sin + self.dy * cos)
    }

    /// Round each component to the nearest whole pixel.
    #[inline]
    #[must_use]
    pub fn round(self) -> Offset<f64> {
        Offset::new(self.dx.round(), self.dy.round())
    }

    /// Round each component down to the nearest whole pixel.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Offset<f64> {
        Offset::new(self.dx.floor(), self.dy.floor())
    }

    /// Round each component up to the nearest whole pixel.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Offset<f64> {
        Offset::new(self.dx.ceil(), self.dy.ceil())
    }

    /// Clamp each component to the range defined by `min` and `max`.
    ///
    /// Components are clamped independently; `min` and `max` are not treated
    /// as a magnitude bound (see [`clamp_magnitude`](Self::clamp_magnitude)).
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Offset<f64>, max: Offset<f64>) -> Offset<f64> {
        Offset::new(self.dx.clamp(min.dx, max.dx), self.dy.clamp(min.dy, max.dy))
    }

    /// Take the absolute value of each component.
    #[inline]
    #[must_use]
    pub const fn abs(self) -> Offset<f64> {
        Offset::new(
            if self.dx >= 0.0 { self.dx } else { -self.dx },
            if self.dy >= 0.0 { self.dy } else { -self.dy },
        )
    }

    /// Clamp the magnitude (length) of this offset to a maximum value.
    ///
    /// If the magnitude exceeds `max`, returns a scaled version with magnitude
    /// `max`. Direction is preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    ///
    /// let offset = Offset::new(30.0, 40.0); // magnitude = 50
    /// let clamped = offset.clamp_magnitude(25.0);
    ///
    /// assert!((clamped.distance() - 25.0).abs() < 0.1);
    /// // Direction preserved: still pointing in same direction
    /// assert!((clamped.direction() - offset.direction()).abs() < 0.01);
    #[inline]
    #[must_use]
    pub fn clamp_magnitude(self, max: f64) -> Offset<f64> {
        let magnitude = self.distance();
        let max_px = max;
        if magnitude > max_px && magnitude > f64::EPSILON {
            let scale = max / magnitude;
            Offset::new(self.dx * scale, self.dy * scale)
        } else {
            self
        }
    }

    /// Move towards another offset by a specific distance.
    ///
    /// If the distance to target is less than `max_distance`, returns the
    /// target. Otherwise, moves `max_distance` units towards the target.
    ///
    /// Useful for smooth following behavior and lerping with fixed step size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
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
    #[inline]
    #[must_use]
    pub fn move_towards(self, target: impl Into<Offset<f64>>, max_distance: f64) -> Offset<f64> {
        let target = target.into();
        let delta = target - self;
        let distance = delta.distance();

        if distance <= max_distance || distance < f64::EPSILON {
            target
        } else {
            let direction = delta.normalize();
            self + direction * max_distance
        }
    }

    /// Calculate the angle between this offset and another, in radians.
    ///
    /// Returns the absolute angle difference in range [0, π].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Offset;
    /// use std::f64::consts::PI;
    ///
    /// let right = Offset::new(1.0, 0.0);
    /// let up = Offset::new(0.0, 1.0);
    ///
    /// let angle = right.angle_to(up);
    /// assert!((angle - PI / 2.0).abs() < 0.01);
    #[inline]
    #[must_use]
    pub fn angle_to(self, other: impl Into<Offset<f64>>) -> f64 {
        let other = other.into();
        let dot = self.dot(other);
        let det = self.cross(other);
        det.atan2(dot).abs()
    }

    /// Convert this offset to a delta offset.
    #[inline]
    #[must_use]
    pub const fn to_delta(self) -> Offset<f64> {
        Offset::new(self.dx, self.dy)
    }
}

// ============================================================================
// Conversions from tuples/arrays (f64 only for backwards compat)
// ============================================================================

impl From<(f64, f64)> for Offset<f64> {
    #[inline]
    fn from((dx, dy): (f64, f64)) -> Self {
        Offset::new(dx, dy)
    }
}

impl From<[f64; 2]> for Offset<f64> {
    #[inline]
    fn from([dx, dy]: [f64; 2]) -> Self {
        Offset::new(dx, dy)
    }
}

impl From<Point<f64>> for Offset<f64> {
    #[inline]
    fn from(point: Point<f64>) -> Self {
        Offset::new(point.x, point.y)
    }
}

impl From<Offset<f64>> for Point<f64> {
    #[inline]
    fn from(offset: Offset<f64>) -> Self {
        offset.to_point()
    }
}

// ============================================================================
// Arithmetic Operators (generic over NumericUnit)
// ============================================================================

impl<T: NumericUnit> Add for Offset<T> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            dx: self.dx.add(rhs.dx),
            dy: self.dy.add(rhs.dy),
        }
    }
}

impl<T: NumericUnit> AddAssign for Offset<T> {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.dx = self.dx.add(rhs.dx);
        self.dy = self.dy.add(rhs.dy);
    }
}

impl<T: NumericUnit> Sub for Offset<T> {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            dx: self.dx.sub(rhs.dx),
            dy: self.dy.sub(rhs.dy),
        }
    }
}

impl<T: NumericUnit> SubAssign for Offset<T> {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.dx = self.dx.sub(rhs.dx);
        self.dy = self.dy.sub(rhs.dy);
    }
}

impl<T: NumericUnit + Mul<f64, Output = T>> Mul<f64> for Offset<T> {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f64) -> Self::Output {
        Self {
            dx: self.dx * rhs,
            dy: self.dy * rhs,
        }
    }
}

impl<T: NumericUnit + Mul<f64, Output = T>> Mul<Offset<T>> for f64 {
    type Output = Offset<T>;

    #[inline]
    fn mul(self, rhs: Offset<T>) -> Self::Output {
        rhs * self
    }
}

impl<T: NumericUnit + Mul<f64, Output = T>> MulAssign<f64> for Offset<T> {
    #[inline]
    fn mul_assign(&mut self, rhs: f64) {
        self.dx = self.dx * rhs;
        self.dy = self.dy * rhs;
    }
}

impl<T: NumericUnit + Div<f64, Output = T>> Div<f64> for Offset<T> {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f64) -> Self::Output {
        Self {
            dx: self.dx / rhs,
            dy: self.dy / rhs,
        }
    }
}

impl<T: NumericUnit + Div<f64, Output = T>> DivAssign<f64> for Offset<T> {
    #[inline]
    fn div_assign(&mut self, rhs: f64) {
        self.dx = self.dx / rhs;
        self.dy = self.dy / rhs;
    }
}

impl<T: NumericUnit> Neg for Offset<T>
where
    T: std::ops::Neg<Output = T>,
{
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self::new(-self.dx, -self.dy)
    }
}

// ============================================================================
// Debug & Display
// ============================================================================

impl<T: NumericUnit> Display for Offset<T>
where
    T: Into<f64>,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dx: f64 = self.dx.into();
        let dy: f64 = self.dy.into();
        write!(f, "Offset({dx}, {dy})")
    }
}

// ============================================================================
// Default
// ============================================================================

impl<T: Unit> Default for Offset<T> {
    #[inline]
    fn default() -> Self {
        Self::new(T::zero(), T::zero())
    }
}

// ============================================================================
// Along trait - Axis-based access
// ============================================================================

impl<T: Unit> super::traits::Along for Offset<T> {
    type Unit = T;

    #[inline]
    fn along(&self, axis: super::axis::Axis) -> Self::Unit {
        match axis {
            super::axis::Axis::Horizontal => self.dx,
            super::axis::Axis::Vertical => self.dy,
        }
    }

    #[inline]
    fn apply_along(
        &self,
        axis: super::axis::Axis,
        f: impl FnOnce(Self::Unit) -> Self::Unit,
    ) -> Self {
        match axis {
            super::axis::Axis::Horizontal => Self::new(f(self.dx), self.dy),
            super::axis::Axis::Vertical => Self::new(self.dx, f(self.dy)),
        }
    }
}

// ============================================================================
// Half trait - Compute half value
// ============================================================================

impl<T: Unit> super::traits::Half for Offset<T>
where
    T: super::traits::Half,
{
    #[inline]
    fn half(self) -> Self {
        Self {
            dx: self.dx.half(),
            dy: self.dy.half(),
        }
    }
}

// Negate is now replaced by std::ops::Neg (see Neg impl above)

// ============================================================================
// IsZero trait - Zero check
// ============================================================================

impl<T: Unit> super::traits::IsZero for Offset<T>
where
    T: super::traits::IsZero,
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.dx.is_zero() && self.dy.is_zero()
    }
}

// ============================================================================
// Double trait - Compute double value
// ============================================================================

impl<T: Unit> super::traits::Double for Offset<T>
where
    T: super::traits::Double,
{
    #[inline]
    fn double(self) -> Self {
        Self {
            dx: self.dx.double(),
            dy: self.dy.double(),
        }
    }
}

// ============================================================================
// ApproxEq trait - Approximate equality for floating-point
// ============================================================================

impl<T: Unit> super::traits::ApproxEq for Offset<T>
where
    T: super::traits::ApproxEq,
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        self.dx.approx_eq_eps(&other.dx, epsilon) && self.dy.approx_eq_eps(&other.dy, epsilon)
    }
}

// ============================================================================
// Sign trait - Sign operations
// ============================================================================

impl<T: NumericUnit> super::traits::Sign for Offset<T>
where
    T: super::traits::Sign,
{
    #[inline]
    fn is_positive(&self) -> bool {
        self.dx.is_positive() && self.dy.is_positive()
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.dx.is_negative() && self.dy.is_negative()
    }

    #[inline]
    fn signum(self) -> Self {
        Self {
            dx: self.dx.signum(),
            dy: self.dy.signum(),
        }
    }

    #[inline]
    fn abs_sign(&self) -> i32 {
        // Return sign of both components (0 if mixed)
        let dx_sign = self.dx.abs_sign();
        let dy_sign = self.dy.abs_sign();
        if dx_sign == dy_sign { dx_sign } else { 0 }
    }
}

// ============================================================================
// Sum trait - Iterator support
// ============================================================================

impl<T> std::iter::Sum for Offset<T>
where
    T: NumericUnit,
{
    #[inline]
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Offset::new(T::zero(), T::zero()), |acc, o| {
            Offset::new(T::add(acc.dx, o.dx), T::add(acc.dy, o.dy))
        })
    }
}

// ============================================================================
// Tests (backwards compatibility)
// ============================================================================

// ============================================================================
// Typed Generic Tests
// ============================================================================
