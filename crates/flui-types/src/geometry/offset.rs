//! 2D offset (position/translation) type
//!
//! This module provides an immutable 2D offset type, similar to Flutter's Offset.
use super::{px, PixelDelta, Pixels};

use std::fmt::{self, Display};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use super::traits::{NumericUnit, Unit};
use super::{Point, Size, Vec2};

/// An immutable 2D offset in Cartesian coordinates.
///
/// This represents a translation or displacement in 2D space.
/// Similar to Flutter's `Offset`.
///
/// Generic over unit type `T`. Common usage:
/// - `Offset<Pixels>` - UI displacement
/// - `Offset<Pixels>` - Normalized/dimensionless offset
///
/// # Distinction from Vec2
///
/// `Offset` and `Vec2` are mathematically identical but semantically different:
/// - `Offset`: Flutter-style displacement with `dx`/`dy` naming
/// - `Vec2`: General vector with `x`/`y` naming
///
/// They are freely convertible.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Offset, px, Pixels};
///
/// let offset = Offset::<Pixels>::new(px(10.0), px(20.0));
/// assert_eq!(offset.dx.get(), 10.0);
/// assert_eq!(offset.dy.get(), 20.0);
///
/// let scaled = offset * 2.0;
/// assert_eq!(scaled.dx.get(), 20.0);
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Offset<T: Unit> {
    /// The horizontal component.
    pub dx: T,

    /// The vertical component.
    pub dy: T,
}

// ============================================================================
// Constants (f32 only for backwards compatibility)
// ============================================================================

impl Offset<Pixels> {
    /// An offset with zero displacement.
    pub const ZERO: Self = Self::new(Pixels::ZERO, Pixels::ZERO);

    /// An offset with infinite displacement.
    pub const INFINITE: Self = Self::new(Pixels(f32::INFINITY), Pixels(f32::INFINITY));
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
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// assert_eq!(offset.dx.get(), 10.0);
    /// assert_eq!(offset.dy.get(), 20.0);
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
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// let swapped = offset.swap();
    /// assert_eq!(swapped.dx.get(), 20.0);
    /// assert_eq!(swapped.dy.get(), 10.0);
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
    /// use flui_types::geometry::{Offset, px, Pixels};
    ///
    /// let offset: Offset<Pixels> = Offset::new(px(10.0), px(20.0));
    /// let doubled: Offset<Pixels> = offset.map(|v| v * 2.0);
    /// assert_eq!(doubled.dx.get(), 20.0);
    /// assert_eq!(doubled.dy.get(), 40.0);
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
    /// use flui_types::geometry::{Offset, Vec2, px, Pixels};
    ///
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// let vec: Vec2<Pixels> = offset.to_vec2();
    /// assert_eq!(vec.x.get(), 10.0);
    /// assert_eq!(vec.y.get(), 20.0);
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
    /// use flui_types::geometry::{Offset, Pixels, px};
    ///
    /// let px_offset = Offset::<Pixels>::new(px(10.0), px(20.0));
    /// let f32_offset: Offset<Pixels> = px_offset.cast();
    /// assert_eq!(f32_offset.dx.get(), 10.0);
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

impl<T: NumericUnit> Offset<T>
where
    T: Into<f32>,
{
    /// Convert to f32 offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// let f32_offset = offset.to_f32();
    /// assert_eq!(f32_offset.dx.get(), 10.0);
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Offset<Pixels> {
        Offset {
            dx: Pixels(self.dx.into()),
            dy: Pixels(self.dy.into()),
        }
    }
}

// ============================================================================
// Legacy Float Methods (for backwards compatibility)
// ============================================================================

impl Offset<Pixels> {
    /// Create an offset from a direction (in radians) and distance.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::from_direction(0.0, 10.0);
    /// assert!((offset.dx.get() - 10.0).abs() < 0.001);
    /// assert!(offset.dy.get().abs() < 0.001);
    /// ```
    #[inline]
    pub fn from_direction(direction: f32, distance: f32) -> Self {
        Self::new(
            Pixels(distance * direction.cos()),
            Pixels(distance * direction.sin()),
        )
    }

    /// Create an offset representing the displacement from one point to another.
    ///
    /// Returns `to - from` as an offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, Point, px};
    ///
    /// let from = Point::new(px(10.0), px(20.0));
    /// let to = Point::new(px(30.0), px(50.0));
    /// let offset = Offset::from_points(from, to);
    /// assert_eq!(offset.dx.get(), 20.0);
    /// assert_eq!(offset.dy.get(), 30.0);
    /// ```
    #[inline]
    pub fn from_points(from: Point<Pixels>, to: Point<Pixels>) -> Self {
        Self::new(to.x - from.x, to.y - from.y)
    }

    /// Check if this offset is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// assert!(Offset::ZERO.is_zero());
    /// assert!(!Offset::new(px(1.0), px(0.0)).is_zero());
    /// ```
    #[inline]
    pub fn is_zero(self) -> bool {
        self.dx == Pixels::ZERO && self.dy == Pixels::ZERO
    }

    /// Get the magnitude (distance) of this offset from the origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::new(px(3.0), px(4.0));
    /// assert_eq!(offset.distance().get(), 5.0); // 3-4-5 triangle
    #[inline]
    #[must_use]
    pub fn distance(self) -> Pixels {
        Pixels(self.dx.0.hypot(self.dy.0))
    }

    /// Get the squared magnitude (avoids sqrt for performance).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::new(px(3.0), px(4.0));
    /// assert_eq!(offset.distance_squared().get(), 25.0);
    #[inline]
    #[must_use]
    pub const fn distance_squared(&self) -> Pixels {
        Pixels(self.dx.0 * self.dx.0 + self.dy.0 * self.dy.0)
    }

    /// Get the direction of this offset in radians.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let right = Offset::new(px(1.0), px(0.0));
    /// assert!((right.direction() - 0.0).abs() < 0.001);
    /// ```
    #[inline]
    pub fn direction(self) -> f32 {
        self.dy.atan2(self.dx)
    }

    /// Check if this offset is finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Offset;
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
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// let scaled = offset.scale(2.0);
    /// assert_eq!(scaled, Offset::new(px(20.0), px(40.0)));
    /// ```
    #[inline]
    pub fn scale(self, factor: f32) -> Self {
        Self::new(self.dx * factor, self.dy * factor)
    }

    /// Translate an offset by another offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let a = Offset::new(px(10.0), px(20.0));
    /// let b = Offset::new(px(5.0), px(10.0));
    /// let c = a.translate(b);
    /// assert_eq!(c, Offset::new(px(15.0), px(30.0)));
    /// ```
    #[inline]
    pub fn translate(self, other: impl Into<Offset<Pixels>>) -> Self {
        let other = other.into();
        Self::new(self.dx + other.dx, self.dy + other.dy)
    }

    /// Linear interpolation between two offsets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let a = Offset::new(px(0.0), px(0.0));
    /// let b = Offset::new(px(10.0), px(10.0));
    /// let mid = a.lerp(b, 0.5);
    /// assert_eq!(mid, Offset::new(px(5.0), px(5.0)));
    /// ```
    #[inline]
    pub fn lerp(self, other: impl Into<Offset<Pixels>>, t: f32) -> Offset<Pixels> {
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
    /// use flui_types::geometry::{Offset, Point, px};
    ///
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// let point = offset.to_point();
    /// assert_eq!(point, Point::new(px(10.0), px(20.0)));
    #[inline]
    #[must_use]
    pub const fn to_point(self) -> Point<Pixels> {
        Point::new(self.dx, self.dy)
    }

    /// Convert to a Size (treating offset as width/height).
    ///
    /// Negative components are clamped to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, Size, px};
    ///
    /// let offset = Offset::new(px(10.0), px(20.0));
    /// let size = offset.to_size();
    /// assert_eq!(size, Size::new(px(10.0), px(20.0)));
    #[inline]
    #[must_use]
    pub fn to_size(self) -> Size<Pixels> {
        Size::new(self.dx.max(Pixels::ZERO), self.dy.max(Pixels::ZERO))
    }

    /// Normalize this offset to a unit vector.
    ///
    #[inline]
    #[must_use]
    pub fn normalize(self) -> Offset<Pixels> {
        let dist = self.distance();
        if dist > Pixels(f32::EPSILON) {
            let dist_f32 = dist.0;
            Offset::new(Pixels(self.dx.0 / dist_f32), Pixels(self.dy.0 / dist_f32))
        } else {
            Offset::ZERO
        }
    }

    #[inline]
    #[must_use]
    pub const fn dot(self, other: Offset<Pixels>) -> Pixels {
        Pixels(self.dx.0 * other.dx.0 + self.dy.0 * other.dy.0)
    }

    #[inline]
    #[must_use]
    pub const fn cross(self, other: Offset<Pixels>) -> Pixels {
        Pixels(self.dx.0 * other.dy.0 - self.dy.0 * other.dx.0)
    }

    #[inline]
    #[must_use]
    pub fn rotate(self, angle: f32) -> Offset<Pixels> {
        let (sin, cos) = angle.sin_cos();
        Offset::new(self.dx * cos - self.dy * sin, self.dx * sin + self.dy * cos)
    }

    #[inline]
    #[must_use]
    pub fn rotate_radians(self, angle: crate::geometry::Radians) -> Offset<Pixels> {
        self.rotate(angle.0)
    }

    #[inline]
    #[must_use]
    pub fn round(self) -> Offset<Pixels> {
        Offset::new(self.dx.round(), self.dy.round())
    }

    #[inline]
    #[must_use]
    pub fn floor(self) -> Offset<Pixels> {
        Offset::new(self.dx.floor(), self.dy.floor())
    }

    #[inline]
    #[must_use]
    pub fn ceil(self) -> Offset<Pixels> {
        Offset::new(self.dx.ceil(), self.dy.ceil())
    }

    #[inline]
    #[must_use]
    pub fn clamp(self, min: Offset<Pixels>, max: Offset<Pixels>) -> Offset<Pixels> {
        Offset::new(self.dx.clamp(min.dx, max.dx), self.dy.clamp(min.dy, max.dy))
    }

    #[inline]
    #[must_use]
    pub const fn abs(self) -> Offset<Pixels> {
        Offset::new(
            if self.dx.0 >= 0.0 {
                self.dx
            } else {
                Pixels(-self.dx.0)
            },
            if self.dy.0 >= 0.0 {
                self.dy
            } else {
                Pixels(-self.dy.0)
            },
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
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let offset = Offset::new(px(30.0), px(40.0)); // magnitude = 50
    /// let clamped = offset.clamp_magnitude(25.0);
    ///
    /// assert!((clamped.distance().get() - 25.0).abs() < 0.1);
    /// // Direction preserved: still pointing in same direction
    /// assert!((clamped.direction() - offset.direction()).abs() < 0.01);
    #[inline]
    #[must_use]
    pub fn clamp_magnitude(self, max: f32) -> Offset<Pixels> {
        let magnitude = self.distance();
        let max_px = Pixels(max);
        if magnitude > max_px && magnitude > Pixels(f32::EPSILON) {
            let scale = max / magnitude.0;
            Offset::new(self.dx * scale, self.dy * scale)
        } else {
            self
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
    /// use flui_types::geometry::{Offset, px};
    ///
    /// let start = Offset::new(px(0.0), px(0.0));
    /// let target = Offset::new(px(10.0), px(0.0));
    ///
    /// // Move 3 units towards target
    /// let moved = start.move_towards(target, 3.0);
    /// assert_eq!(moved, Offset::new(px(3.0), px(0.0)));
    ///
    /// // Moving beyond target distance returns target
    /// let at_target = start.move_towards(target, 20.0);
    /// assert_eq!(at_target, target);
    #[inline]
    #[must_use]
    pub fn move_towards(
        self,
        target: impl Into<Offset<Pixels>>,
        max_distance: f32,
    ) -> Offset<Pixels> {
        let target = target.into();
        let delta = target - self;
        let distance = delta.distance();

        if distance <= Pixels(max_distance) || distance < Pixels(f32::EPSILON) {
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
    /// use flui_types::geometry::{Offset, px};
    /// use std::f32::consts::PI;
    ///
    /// let right = Offset::new(px(1.0), px(0.0));
    /// let up = Offset::new(px(0.0), px(1.0));
    ///
    /// let angle = right.angle_to(up);
    /// assert!((angle - PI / 2.0).abs() < 0.01);
    #[inline]
    #[must_use]
    pub fn angle_to(self, other: impl Into<Offset<Pixels>>) -> f32 {
        let other = other.into();
        let dot = self.dot(other);
        let det = self.cross(other);
        det.atan2(dot).abs()
    }

    #[inline]
    #[must_use]
    pub fn angle_to_radians(self, other: impl Into<Offset<Pixels>>) -> crate::geometry::Radians {
        crate::geometry::radians(self.angle_to(other))
    }

    /// Convert this offset to a delta offset.
    #[inline]
    #[must_use]
    pub const fn to_delta(self) -> Offset<PixelDelta> {
        Offset::new(PixelDelta(self.dx.0), PixelDelta(self.dy.0))
    }
}

// ============================================================================
// PixelDelta-specific methods
// ============================================================================

impl Offset<PixelDelta> {
    /// Get the magnitude (distance) of this offset from the origin.
    #[inline]
    #[must_use]
    pub fn distance(self) -> PixelDelta {
        PixelDelta(self.dx.0.hypot(self.dy.0))
    }

    /// Convert this delta offset to a regular pixel offset.
    #[inline]
    #[must_use]
    pub const fn to_pixels(self) -> Offset<Pixels> {
        Offset::new(Pixels(self.dx.0), Pixels(self.dy.0))
    }
}

// ============================================================================
// Conversions from tuples/arrays (f32 only for backwards compat)
// ============================================================================

impl From<(Pixels, Pixels)> for Offset<Pixels> {
    #[inline]
    fn from((dx, dy): (Pixels, Pixels)) -> Self {
        Offset::new(dx, dy)
    }
}

impl From<[Pixels; 2]> for Offset<Pixels> {
    #[inline]
    fn from([dx, dy]: [Pixels; 2]) -> Self {
        Offset::new(dx, dy)
    }
}

impl From<Point<Pixels>> for Offset<Pixels> {
    #[inline]
    fn from(point: Point<Pixels>) -> Self {
        Offset::new(point.x, point.y)
    }
}

impl From<Offset<Pixels>> for Point<Pixels> {
    #[inline]
    fn from(offset: Offset<Pixels>) -> Self {
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

impl<T: NumericUnit + Mul<f32, Output = T>> Mul<f32> for Offset<T> {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            dx: self.dx * rhs,
            dy: self.dy * rhs,
        }
    }
}

impl<T: NumericUnit + Mul<f32, Output = T>> Mul<Offset<T>> for f32 {
    type Output = Offset<T>;

    #[inline]
    fn mul(self, rhs: Offset<T>) -> Self::Output {
        rhs * self
    }
}

impl<T: NumericUnit + Mul<f32, Output = T>> MulAssign<f32> for Offset<T> {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.dx = self.dx * rhs;
        self.dy = self.dy * rhs;
    }
}

impl<T: NumericUnit + Div<f32, Output = T>> Div<f32> for Offset<T> {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self {
            dx: self.dx / rhs,
            dy: self.dy / rhs,
        }
    }
}

impl<T: NumericUnit + Div<f32, Output = T>> DivAssign<f32> for Offset<T> {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
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
    T: Into<f32>,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dx: f32 = self.dx.into();
        let dy: f32 = self.dy.into();
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
    fn along(&self, axis: super::traits::Axis) -> Self::Unit {
        match axis {
            super::traits::Axis::Horizontal => self.dx,
            super::traits::Axis::Vertical => self.dy,
        }
    }

    #[inline]
    fn apply_along(
        &self,
        axis: super::traits::Axis,
        f: impl FnOnce(Self::Unit) -> Self::Unit,
    ) -> Self {
        match axis {
            super::traits::Axis::Horizontal => Self::new(f(self.dx), self.dy),
            super::traits::Axis::Vertical => Self::new(self.dx, f(self.dy)),
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
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
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
        if dx_sign == dy_sign {
            dx_sign
        } else {
            0
        }
    }
}

// ============================================================================
// Type-safe scale conversions with ScaleFactor
// ============================================================================

impl Offset<Pixels> {
    /// Type-safe scale conversion to DevicePixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, ScaleFactor, Pixels, DevicePixels, px, device_px};
    ///
    /// let logical = Offset::new(px(10.0), px(20.0));
    /// let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    /// let device = logical.scale_with(scale);
    /// assert_eq!(device.dx.get(), 20);
    /// assert_eq!(device.dy.get(), 40);
    /// ```
    #[inline]
    #[must_use]
    pub fn scale_with(
        self,
        scale: super::units::ScaleFactor<Pixels, super::units::DevicePixels>,
    ) -> Offset<super::units::DevicePixels> {
        use super::units::device_px;
        Offset {
            dx: device_px((self.dx.get() * scale.get()).round() as i32),
            dy: device_px((self.dy.get() * scale.get()).round() as i32),
        }
    }
}

impl Offset<super::units::DevicePixels> {
    /// Unscales this offset to logical pixels using a type-safe scale factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Offset, ScaleFactor, Pixels, DevicePixels, device_px, px};
    ///
    /// let device = Offset::new(device_px(20), device_px(40));
    /// let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    /// let logical = device.unscale(scale);
    /// assert_eq!(logical.dx, px(10.0));
    /// assert_eq!(logical.dy, px(20.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn unscale(
        self,
        scale: super::units::ScaleFactor<Pixels, super::units::DevicePixels>,
    ) -> Offset<Pixels> {
        let inverse = scale.inverse();
        Offset {
            dx: px(self.dx.get() as f32 * inverse.get()),
            dy: px(self.dy.get() as f32 * inverse.get()),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::px;

    #[test]
    fn test_offset_creation() {
        let offset = Offset::new(px(10.0), px(20.0));
        assert_eq!(offset.dx, px(10.0));
        assert_eq!(offset.dy, px(20.0));

        assert_eq!(Offset::ZERO.dx, px(0.0));
        assert_eq!(Offset::ZERO.dy, px(0.0));
        assert!(Offset::ZERO.is_zero());
    }

    #[test]
    fn test_offset_distance() {
        let offset = Offset::new(px(3.0), px(4.0));
        assert_eq!(offset.distance(), px(5.0)); // 3-4-5 triangle
        assert_eq!(offset.distance_squared(), px(25.0));
    }

    #[test]
    fn test_offset_direction() {
        let right = Offset::new(px(1.0), px(0.0));
        assert!((right.direction() - 0.0).abs() < 0.001);

        let up = Offset::new(px(0.0), px(1.0));
        assert!((up.direction() - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    }

    #[test]
    fn test_offset_from_direction() {
        let offset = Offset::from_direction(0.0, 10.0); // Right direction
        assert!((offset.dx.get() - 10.0).abs() < 0.001);
        assert!(offset.dy.get().abs() < 0.001);
    }

    #[test]
    fn test_offset_arithmetic() {
        let a = Offset::new(px(10.0), px(20.0));
        let b = Offset::new(px(5.0), px(10.0));

        let sum = a + b;
        assert_eq!(sum.dx, px(15.0));
        assert_eq!(sum.dy, px(30.0));

        let diff = a - b;
        assert_eq!(diff.dx, px(5.0));
        assert_eq!(diff.dy, px(10.0));

        let scaled = a * 2.0;
        assert_eq!(scaled.dx, px(20.0));
        assert_eq!(scaled.dy, px(40.0));

        let divided = a / 2.0;
        assert_eq!(divided.dx, px(5.0));
        assert_eq!(divided.dy, px(10.0));

        let negated = -a;
        assert_eq!(negated.dx, px(-10.0));
        assert_eq!(negated.dy, px(-20.0));
    }

    #[test]
    fn test_offset_lerp() {
        let a = Offset::new(px(0.0), px(0.0));
        let b = Offset::new(px(10.0), px(10.0));

        let mid = a.lerp(b, 0.5);
        assert_eq!(mid.dx, px(5.0));
        assert_eq!(mid.dy, px(5.0));

        let start = a.lerp(b, 0.0);
        assert_eq!(start, a);

        let end = a.lerp(b, 1.0);
        assert_eq!(end, b);
    }

    #[test]
    fn test_offset_conversions() {
        let offset = Offset::new(px(10.0), px(20.0));

        let from_tuple: Offset<Pixels> = (px(10.0), px(20.0)).into();
        assert_eq!(from_tuple, offset);

        let from_array: Offset<Pixels> = [px(10.0), px(20.0)].into();
        assert_eq!(from_array, offset);

        let point = offset.to_point();
        assert_eq!(point.x, px(10.0));
        assert_eq!(point.y, px(20.0));

        let from_point: Offset<Pixels> = point.into();
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
        let offset = Offset::new(px(10.0), px(20.0));
        let scaled = offset.scale(3.0);
        assert_eq!(scaled, Offset::new(px(30.0), px(60.0)));
    }

    #[test]
    fn test_offset_translate() {
        let a = Offset::new(px(10.0), px(20.0));
        let b = Offset::new(px(5.0), px(3.0));
        let translated = a.translate(b);
        assert_eq!(translated, Offset::new(px(15.0), px(23.0)));
    }

    #[test]
    fn test_offset_to_size() {
        let offset = Offset::new(px(10.0), px(20.0));
        let size = offset.to_size();
        assert_eq!(size.width, px(10.0));
        assert_eq!(size.height, px(20.0));

        // Negative components should be clamped
        let negative = Offset::new(px(-5.0), px(10.0));
        let size2 = negative.to_size();
        assert_eq!(size2.width, px(0.0));
        assert_eq!(size2.height, px(10.0));
    }
}

// ============================================================================
// Typed Generic Tests
// ============================================================================

#[cfg(test)]
mod typed_tests {
    use super::*;
    use crate::geometry::{px, Pixels};

    #[test]
    fn test_offset_new() {
        let o = Offset::<Pixels>::new(px(10.0), px(20.0));
        assert_eq!(o.dx.get(), 10.0);
        assert_eq!(o.dy.get(), 20.0);
    }

    #[test]
    fn test_offset_vec2_conversion() {
        let o = Offset::<Pixels>::new(px(5.0), px(10.0));
        let v: Vec2<Pixels> = o.into();
        assert_eq!(v.x, px(5.0));
        assert_eq!(v.y, px(10.0));

        let o2: Offset<Pixels> = v.into();
        assert_eq!(o2.dx, px(5.0));
        assert_eq!(o2.dy, px(10.0));
    }

    #[test]
    fn test_offset_arithmetic() {
        let o1 = Offset::<Pixels>::new(px(10.0), px(20.0));
        let o2 = Offset::<Pixels>::new(px(5.0), px(10.0));

        let o3 = o1 + o2;
        assert_eq!(o3.dx.get(), 15.0);
        assert_eq!(o3.dy.get(), 30.0);

        let o4 = o1 * 2.0;
        assert_eq!(o4.dx.get(), 20.0);
        assert_eq!(o4.dy.get(), 40.0);
    }

    #[test]
    fn test_offset_cast() {
        let px_offset = Offset::<Pixels>::new(px(10.0), px(20.0));
        let f32_offset: Offset<Pixels> = px_offset.cast();
        assert_eq!(f32_offset.dx, px(10.0));
        assert_eq!(f32_offset.dy, px(20.0));
    }

    #[test]
    fn test_offset_to_f32() {
        let px_offset = Offset::<Pixels>::new(px(10.0), px(20.0));
        let f32_offset = px_offset.to_f32();
        assert_eq!(f32_offset.dx, px(10.0));
        assert_eq!(f32_offset.dy, px(20.0));
    }

    #[test]
    fn test_offset_to_vec2() {
        let offset = Offset::<Pixels>::new(px(10.0), px(20.0));
        let vec = offset.to_vec2();
        assert_eq!(vec.x.get(), 10.0);
        assert_eq!(vec.y.get(), 20.0);
    }

    #[test]
    fn test_offset_default() {
        let o = Offset::<Pixels>::default();
        assert_eq!(o.dx, px(0.0));
        assert_eq!(o.dy, px(0.0));
    }

    #[test]
    fn test_offset_assign_ops() {
        let mut o = Offset::<Pixels>::new(px(10.0), px(20.0));

        o += Offset::<Pixels>::new(px(5.0), px(10.0));
        assert_eq!(o.dx.get(), 15.0);

        o *= 2.0;
        assert_eq!(o.dx.get(), 30.0);

        o /= 2.0;
        assert_eq!(o.dx.get(), 15.0);

        o -= Offset::<Pixels>::new(px(5.0), px(10.0));
        assert_eq!(o.dx.get(), 10.0);
        assert_eq!(o.dy.get(), 20.0); // 30.0 - 10.0 = 20.0
    }

    #[test]
    fn test_offset_commutative_mul() {
        let o = Offset::<Pixels>::new(px(10.0), px(20.0));
        let left = 2.0 * o;
        let right = o * 2.0;
        assert_eq!(left.dx.get(), right.dx.get());
        assert_eq!(left.dy.get(), right.dy.get());
    }

    #[test]
    fn test_offset_utility_traits() {
        use crate::geometry::{Along, Axis, Half};

        // Test Along trait
        let o = Offset::<Pixels>::new(px(10.0), px(20.0));
        assert_eq!(o.along(Axis::Horizontal).0, 10.0);
        assert_eq!(o.along(Axis::Vertical).0, 20.0);

        let modified = o.apply_along(Axis::Horizontal, |dx| px(dx.0 * 2.0));
        assert_eq!(modified.dx.0, 20.0);
        assert_eq!(modified.dy.0, 20.0);

        // Test Half trait
        let half_o = o.half();
        assert_eq!(half_o.dx.0, 5.0);
        assert_eq!(half_o.dy.0, 10.0);

        // Test negation (using std::ops::Neg)
        let neg_o = -o;
        assert_eq!(neg_o.dx.0, -10.0);
        assert_eq!(neg_o.dy.0, -20.0);

        // Test IsZero trait
        let zero = Offset::<Pixels>::new(px(0.0), px(0.0));
        assert!(zero.is_zero());
        assert!(!o.is_zero());
    }
}
