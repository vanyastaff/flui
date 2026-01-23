//! Point type for coordinates in 2D space.
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
//! Point - Point = Vec2  (displacement between positions)
//! Point + Vec2  = Point (translate position)
//! Point - Vec2  = Point (translate in opposite direction)
//! ```
use super::{px, Pixels};

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use super::traits::{NumericUnit, Unit};
use super::error::GeometryError;
use super::Vec2;

/// Absolute position in 2D space.
///
/// Generic over unit type `T`. Common usage:
/// - `Point<Pixels>` - UI coordinates
/// - `Point<DevicePixels>` - Screen pixels
/// - `Point<Pixels>` - Normalized/dimensionless coordinates
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Point, px, Pixels};
///
/// let ui_pos = Point::<Pixels>::new(px(100.0), px(200.0));
/// let normalized = Point::<f32>::new(0.5, 0.75);
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Point<T: Unit> {
    /// The x coordinate (horizontal position).
    pub x: T,
    /// The y coordinate (vertical position).
    pub y: T,
}

impl<T: Unit> Default for Point<T> {
    fn default() -> Self {
        Self {
            x: T::zero(),
            y: T::zero(),
        }
    }
}

// ============================================================================
// Constants (f32 only for backwards compatibility)
// ============================================================================

impl Point<Pixels> {
    /// The origin point (0, 0).
    pub const ORIGIN: Self = Self::new(px(0.0), px(0.0));

    /// Alias for [`ORIGIN`](Self::ORIGIN).
    pub const ZERO: Self = Self::ORIGIN;

    /// Point at positive infinity.
    pub const INFINITY: Self = Self::new(px(f32::INFINITY), px(f32::INFINITY));

    /// Point at negative infinity.
    pub const NEG_INFINITY: Self = Self::new(px(f32::NEG_INFINITY), px(f32::NEG_INFINITY));

    /// Point with NaN coordinates.
    pub const NAN: Self = Self::new(px(f32::NAN), px(f32::NAN));
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Point<T> {
    #[inline]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn splat(value: T) -> Self {
        Self { x: value, y: value }
    }

    /// Swaps x and y coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p = Point::new(10.0, 20.0);
    /// assert_eq!(p.swap(), Point::new(20.0, 10.0));
    #[must_use]
    pub fn swap(self) -> Self {
        Self { x: self.y, y: self.x }
    }
}

// ============================================================================
// Safe Constructors (NumericUnit with Into<f32> + From<f32>)
// ============================================================================

impl<T: NumericUnit> Point<T>
where
    T: Into<f32> + From<f32>
{
    /// Creates a point with validation (returns Result).
    pub fn try_new(x: T, y: T) -> Result<Self, GeometryError> {
        let point = Self { x, y };
        if !point.is_valid() {
            return Err(GeometryError::InvalidCoordinates {
                x: x.into(),
                y: y.into(),
            });
        }
        Ok(point)
    }

    /// Creates a point, clamping invalid values to valid range.
    pub fn new_clamped(x: T, y: T) -> Self {
        let clamp_f32 = |v: f32| {
            if v.is_nan() {
                0.0
            } else if v.is_infinite() {
                if v > 0.0 { f32::MAX } else { f32::MIN }
            } else {
                v
            }
        };

        Self {
            x: T::from(clamp_f32(x.into())),
            y: T::from(clamp_f32(y.into())),
        }
    }
}

// ============================================================================
// Validation Methods (NumericUnit with Into<f32>)
// ============================================================================

impl<T: NumericUnit> Point<T>
where
    T: Into<f32>
{
    /// Checks if coordinates are valid (finite, not NaN).
    pub fn is_valid(&self) -> bool {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        x_f32.is_finite() && y_f32.is_finite()
    }

    /// Returns true if both coordinates are finite.
    pub fn is_finite(&self) -> bool {
        self.is_valid()
    }

    /// Returns true if any coordinate is NaN.
    pub fn is_nan(&self) -> bool {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        x_f32.is_nan() || y_f32.is_nan()
    }
}

// ============================================================================
// Legacy Generic Methods (T: Clone + Debug + Default + PartialEq)
// ============================================================================

impl<T> Point<T>
where
    T: Unit + Clone + fmt::Debug + Default + PartialEq,
{
    /// Applies a transformation function to both coordinates.
    ///
    /// This enables converting between different unit types.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p: Point<Pixels> = Point::new(3.0, 4.0);
    /// let p_doubled: Point<Pixels> = p.map(|coord| coord * 2.0);
    /// assert_eq!(p_doubled, Point::new(6.0, 8.0));
    #[must_use]
    pub fn map<U>(&self, f: impl Fn(T) -> U) -> Point<U>
    where
        U: Unit,
    {
        Point {
            x: f(self.x),
            y: f(self.y),
        }
    }

    #[must_use]
    pub fn with_x(self, x: T) -> Self {
        Self::new(x, self.y)
    }

    #[must_use]
    pub fn with_y(self, y: T) -> Self {
        Self::new(self.x, y)
    }
}

// ============================================================================
// Accessors & Conversion (f32 specialization)
// ============================================================================

impl Point<Pixels> {
    #[must_use]
    pub const fn from_array(a: [f32; 2]) -> Self {
        Self::new(px(a[0]), px(a[1]))
    }

    #[must_use]
    pub const fn from_tuple(t: (f32, f32)) -> Self {
        Self::new(px(t.0), px(t.1))
    }

    /// Converts to a vector with same coordinates.
    ///
    #[must_use]
    pub const fn to_vec2(self) -> Vec2<Pixels> {
        Vec2::new(self.x, self.y)
    }
}

// ============================================================================
// Geometric Operations (f32 only)
// ============================================================================

impl<T> Point<T>
where
    T: NumericUnit + Into<f32> + From<f32>,
{
    /// Euclidean distance to another point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p1 = Point::new(0.0, 0.0);
    /// let p2 = Point::new(3.0, 4.0);
    /// assert_eq!(p1.distance(p2), 5.0);
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        self.distance_squared(other).sqrt()
    }

    /// Squared euclidean distance to another point.
    ///
    /// This is faster than [`distance`](Self::distance) when you only need
    #[must_use]
    pub fn distance_squared(self, other: Self) -> f32 {
        let dx = T::sub(other.x, self.x);
        let dy = T::sub(other.y, self.y);
        let dx_f32: f32 = dx.into();
        let dy_f32: f32 = dy.into();
        dx_f32 * dx_f32 + dy_f32 * dy_f32
    }

    /// Midpoint between this point and another.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p1 = Point::new(0.0, 0.0);
    /// let p2 = Point::new(10.0, 20.0);
    /// assert_eq!(p1.midpoint(p2), Point::new(5.0, 10.0));
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        let sum_x = self.x + other.x;
        let sum_y = self.y + other.y;
        let sum_x_f32: f32 = sum_x.into();
        let sum_y_f32: f32 = sum_y.into();
        Self::new(
            T::from(sum_x_f32 / 2.0),
            T::from(sum_y_f32 / 2.0),
        )
    }
}

impl Point<Pixels> {
}

// ============================================================================
// Interpolation (generic with NumericUnit)
// ============================================================================

impl<T> Point<T>
where
    T: NumericUnit + Into<f32> + From<f32>,
{
    /// Linear interpolation between two points.
    ///
    /// - `t = 0.0` returns `self`
    /// - `t = 0.5` returns midpoint
    /// - `t = 1.0` returns `other`
    ///
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let x0: f32 = self.x.into();
        let y0: f32 = self.y.into();
        let x1: f32 = other.x.into();
        let y1: f32 = other.y.into();

        Self::new(
            T::from(x0 + (x1 - x0) * t),
            T::from(y0 + (y1 - y0) * t),
        )
    }
}

// ============================================================================
// Component-wise Operations (generic with PartialOrd)
// ============================================================================

impl<T> Point<T>
where
    T: Unit + PartialOrd + Clone + fmt::Debug + Default + PartialEq,
{
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self {
            x: if self.x <= other.x { self.x } else { other.x },
            y: if self.y <= other.y { self.y } else { other.y },
        }
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            x: if self.x >= other.x { self.x } else { other.x },
            y: if self.y >= other.y { self.y } else { other.y },
        }
    }

    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }
}

// ============================================================================
// f32-specific operations
// ============================================================================

impl Point<Pixels> {
    #[must_use]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs())
    }

    #[must_use]
    pub fn min_element(self) -> Pixels {
        self.x.min(self.y)
    }

    #[must_use]
    pub fn max_element(self) -> Pixels {
        self.x.max(self.y)
    }
}

// ============================================================================
// Rounding Operations (f32 only)
// ============================================================================

impl Point<Pixels> {
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil())
    }

    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    #[must_use]
    pub fn trunc(self) -> Self {
        Self::new(self.x.trunc(), self.y.trunc())
    }

    #[must_use]
    pub fn expand(self) -> Self {
        Self::new(
            if self.x.0 >= 0.0 {
                self.x.ceil()
            } else {
                self.x.floor()
            },
            if self.y.0 >= 0.0 {
                self.y.ceil()
            } else {
                self.y.floor()
            },
        )
    }

    #[must_use]
    pub fn fract(self) -> Self {
        Self::new(self.x.fract(), self.y.fract())
    }
}

// ============================================================================
// Validation (f32 only)
// ============================================================================

impl Point<Pixels> {
}

// ============================================================================
// Operators: Point - Point = Vec2 (generic)
// ============================================================================

impl<T> Sub for Point<T>
where
    T: NumericUnit,
{
    type Output = Vec2<T>;

    /// Returns the displacement vector from `rhs` to `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, Vec2, px, Pixels};
    ///
    /// let p1 = Point::<f32>::new(10.0, 20.0);
    /// let p2 = Point::<f32>::new(3.0, 5.0);
    /// let v: Vec2<Pixels> = p1 - p2;
    /// assert_eq!(v, Vec2::new(7.0, 15.0));
    ///
    /// // Works with Pixels too
    /// let p1 = Point::new(px(100.0), px(200.0));
    /// let p2 = Point::new(px(30.0), px(50.0));
    /// let v: Vec2<Pixels> = p1 - p2;
    /// assert_eq!(v.x.get(), 70.0);
    #[inline]
    fn sub(self, rhs: Self) -> Vec2<T> {
        Vec2::new(
            T::sub(self.x, rhs.x),
            T::sub(self.y, rhs.y),
        )
    }
}

// ============================================================================
// Operators: Point ± Vec2 = Point (generic)
// ============================================================================

impl<T> Add<Vec2<T>> for Point<T>
where
    T: NumericUnit,
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: Vec2<T>) -> Self {
        Self::new(
            T::add(self.x, rhs.x),
            T::add(self.y, rhs.y),
        )
    }
}

impl<T> AddAssign<Vec2<T>> for Point<T>
where
    T: NumericUnit,
{
    #[inline]
    fn add_assign(&mut self, rhs: Vec2<T>) {
        self.x = T::add(self.x, rhs.x);
        self.y = T::add(self.y, rhs.y);
    }
}

impl<T> Sub<Vec2<T>> for Point<T>
where
    T: NumericUnit,
{
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Vec2<T>) -> Self {
        Self::new(
            T::sub(self.x, rhs.x),
            T::sub(self.y, rhs.y),
        )
    }
}

impl<T> SubAssign<Vec2<T>> for Point<T>
where
    T: NumericUnit,
{
    #[inline]
    fn sub_assign(&mut self, rhs: Vec2<T>) {
        self.x = T::sub(self.x, rhs.x);
        self.y = T::sub(self.y, rhs.y);
    }
}

// ============================================================================
// Operators: Scalar multiplication/division (generic)
// ============================================================================

impl<T, Rhs> Mul<Rhs> for Point<T>
where
    T: Unit + Mul<Rhs, Output = T> + Clone + fmt::Debug + Default + PartialEq,
    Rhs: Clone,
{
    type Output = Point<T>;

    #[inline]
    fn mul(self, rhs: Rhs) -> Point<T> {
        Point {
            x: self.x * rhs.clone(),
            y: self.y * rhs,
        }
    }
}

// Reverse multiplication: f32 * Point<Pixels>
impl Mul<Point<Pixels>> for f32 {
    type Output = Point<Pixels>;

    #[inline]
    fn mul(self, rhs: Point<Pixels>) -> Point<Pixels> {
        Point {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}

impl<T, Rhs> Div<Rhs> for Point<T>
where
    T: Unit + Div<Rhs, Output = T> + Clone + fmt::Debug + Default + PartialEq,
    Rhs: Clone,
{
    type Output = Point<T>;

    #[inline]
    fn div(self, rhs: Rhs) -> Point<T> {
        Point {
            x: self.x / rhs.clone(),
            y: self.y / rhs,
        }
    }
}

impl<T, Rhs> MulAssign<Rhs> for Point<T>
where
    T: Unit + MulAssign<Rhs> + Clone + fmt::Debug + Default + PartialEq,
    Rhs: Clone,
{
    #[inline]
    fn mul_assign(&mut self, rhs: Rhs) {
        self.x *= rhs.clone();
        self.y *= rhs;
    }
}

impl<T, Rhs> DivAssign<Rhs> for Point<T>
where
    T: Unit + DivAssign<Rhs> + Clone + fmt::Debug + Default + PartialEq,
    Rhs: Clone,
{
    #[inline]
    fn div_assign(&mut self, rhs: Rhs) {
        self.x /= rhs.clone();
        self.y /= rhs;
    }
}

impl<T> Neg for Point<T>
where
    T: Unit + Neg<Output = T> + Clone + fmt::Debug + Default + PartialEq,
{
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

// ============================================================================
// Checked Arithmetic (NumericUnit with validation)
// ============================================================================

impl<T: NumericUnit> Point<T>
where
    T: Into<f32> + From<f32>
{
    /// Checked addition with a vector (returns None on invalid result).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, point};
    ///
    /// let p = Point::<f32>::new(1.0, 2.0);
    /// // Note: This would need Vec2<Pixels> to work fully generically
    /// let result = p.checked_add_vec(3.0, 4.0);
    /// assert!(result.is_some());
    /// assert_eq!(result.unwrap(), Point::new(4.0, 6.0));
    /// ```
    pub fn checked_add_vec(self, dx: T, dy: T) -> Option<Self> {
        let result = Self {
            x: self.x.add(dx),
            y: self.y.add(dy),
        };

        if result.is_valid() {
            Some(result)
        } else {
            None
        }
    }

    /// Saturating addition with a vector (clamps to valid range).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, point};
    ///
    /// let p = Point::<f32>::new(1.0, 2.0);
    /// let result = p.saturating_add_vec(f32::NAN, 4.0);
    /// // NaN gets clamped to 0
    /// assert_eq!(result.x, 0.0);
    /// assert_eq!(result.y, 6.0);
    /// ```
    pub fn saturating_add_vec(self, dx: T, dy: T) -> Self {
        Self::new_clamped(
            self.x.add(dx),
            self.y.add(dy),
        )
    }

    /// Checked scalar multiplication (returns None on invalid result).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, point};
    ///
    /// let p = Point::<f32>::new(1.0, 2.0);
    /// let result = p.checked_mul(2.0);
    /// assert!(result.is_some());
    /// assert_eq!(result.unwrap(), Point::new(2.0, 4.0));
    ///
    /// let infinity = p.checked_mul(f32::INFINITY);
    /// assert!(infinity.is_none());
    /// ```
    pub fn checked_mul(self, scalar: f32) -> Option<Self> {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        let result = Self {
            x: T::from(x_f32 * scalar),
            y: T::from(y_f32 * scalar),
        };

        if result.is_valid() {
            Some(result)
        } else {
            None
        }
    }

    /// Saturating scalar multiplication (clamps to valid range).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, point};
    ///
    /// let p = Point::<f32>::new(1.0, 2.0);
    /// let result = p.saturating_mul(f32::INFINITY);
    /// assert_eq!(result.x, f32::MAX);
    /// assert_eq!(result.y, f32::MAX);
    /// ```
    pub fn saturating_mul(self, scalar: f32) -> Self {
        let x_f32: f32 = self.x.into();
        let y_f32: f32 = self.y.into();
        Self::new_clamped(
            T::from(x_f32 * scalar),
            T::from(y_f32 * scalar),
        )
    }
}

// ============================================================================
// Type Conversion Methods (generic)
// ============================================================================

impl<T: Unit> Point<T> {
    /// Converts point to different unit type.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, Pixels, px};
    ///
    /// let p = Point::<Pixels>::new(px(100.0), px(200.0));
    /// let p_f32: Point<Pixels> = p.cast();
    /// assert_eq!(p_f32.x, 100.0);
    /// assert_eq!(p_f32.y, 200.0);
    #[must_use]
    pub fn cast<U>(self) -> Point<U>
    where
        U: Unit,
        T: Into<U>
    {
        Point {
            x: self.x.into(),
            y: self.y.into(),
        }
    }
}

// ============================================================================
// GPU Conversion Methods (NumericUnit → f32)
// ============================================================================

impl<T: NumericUnit> Point<T>
where
    T: Into<f32>
{
    /// Converts to `Point<Pixels>` (shorthand for GPU usage).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, Pixels, px};
    ///
    /// let p = Point::<Pixels>::new(px(100.0), px(200.0));
    /// let p_f32 = p.to_f32();
    /// assert_eq!(p_f32, Point::new(px(100.0), px(200.0)));
    /// ```
    #[must_use]
    pub fn to_f32(self) -> Point<Pixels> {
        Point {
            x: Pixels(self.x.into()),
            y: Pixels(self.y.into()),
        }
    }

    /// Converts to raw array [x, y] for GPU buffers.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, Pixels, px};
    ///
    /// let p = Point::<Pixels>::new(px(100.0), px(200.0));
    /// let arr = p.to_array();
    /// assert_eq!(arr, [100.0, 200.0]);
    #[must_use]
    pub fn to_array(self) -> [f32; 2] {
        [self.x.into(), self.y.into()]
    }

    /// Converts to tuple (x, y).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, Pixels, px};
    ///
    /// let p = Point::<Pixels>::new(px(100.0), px(200.0));
    /// let tuple = p.to_tuple();
    /// assert_eq!(tuple, (100.0, 200.0));
    #[must_use]
    pub fn to_tuple(self) -> (f32, f32) {
        (self.x.into(), self.y.into())
    }
}

// ============================================================================
// From Trait Implementations
// ============================================================================

// Note: We cannot implement From<Point<T>> for Point<Pixels> generically
// because it conflicts with the reflexive impl From<T> for T when T=f32.
// Instead, users should use .cast(), .to_f32(), or .into() on specific types.

/// Converts from `Point<T>` to `(f32, f32)` for any T that converts to f32.
impl<T: Unit> From<Point<T>> for (f32, f32)
where
    T: Into<f32>
{
    #[inline]
    fn from(p: Point<T>) -> (f32, f32) {
        (p.x.into(), p.y.into())
    }
}

/// Converts from `Point<T>` to `[f32; 2]` for any T that converts to f32.
impl<T: Unit> From<Point<T>> for [f32; 2]
where
    T: Into<f32>
{
    #[inline]
    fn from(p: Point<T>) -> [f32; 2] {
        [p.x.into(), p.y.into()]
    }
}

// ============================================================================
// Conversions (f32 only - specialized)
// ============================================================================

impl From<(Pixels, Pixels)> for Point<Pixels> {
    #[inline]
    fn from((x, y): (Pixels, Pixels)) -> Self {
        Self::new(x, y)
    }
}

impl From<[Pixels; 2]> for Point<Pixels> {
    #[inline]
    fn from([x, y]: [Pixels; 2]) -> Self {
        Self::new(x, y)
    }
}

impl From<Vec2<Pixels>> for Point<Pixels> {
    #[inline]
    fn from(v: Vec2<Pixels>) -> Self {
        Self::new(v.x, v.y)
    }
}

// ============================================================================
// Display (generic)
// ============================================================================

impl<T> fmt::Display for Point<T>
where
    T: Unit + fmt::Display + Clone + fmt::Debug + Default + PartialEq,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

// ============================================================================
// Convenience function (f32 only)
// ============================================================================

/// Shorthand for `Point::new(x, y)`.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::point;
///
/// let p = point(10.0, 20.0);
#[must_use]
pub const fn point(x: f32, y: f32) -> Point<Pixels> {
    Point::new(px(x), px(y))
}

// ============================================================================
// Along trait - Axis-based access (generic)
// ============================================================================

impl<T> super::traits::Along for Point<T>
where
    T: Unit + Clone + fmt::Debug + Default + PartialEq,
{
    type Unit = T;

    #[inline]
    fn along(&self, axis: super::traits::Axis) -> Self::Unit {
        match axis {
            super::traits::Axis::Horizontal => self.x,
            super::traits::Axis::Vertical => self.y,
        }
    }

    #[inline]
    fn apply_along(
        &self,
        axis: super::traits::Axis,
        f: impl FnOnce(Self::Unit) -> Self::Unit,
    ) -> Self {
        match axis {
            super::traits::Axis::Horizontal => Self::new(f(self.x), self.y),
            super::traits::Axis::Vertical => Self::new(self.x, f(self.y)),
        }
    }
}

// ============================================================================
// Half trait - Compute half value (generic)
// ============================================================================

impl<T: Unit> super::traits::Half for Point<T>
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
// IsZero trait - Zero check (generic)
// ============================================================================

impl<T: Unit> super::traits::IsZero for Point<T>
where
    T: super::traits::IsZero
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.x.is_zero() && self.y.is_zero()
    }
}

// ============================================================================
// Double trait - Compute double value (generic)
// ============================================================================

impl<T: Unit> super::traits::Double for Point<T>
where
    T: super::traits::Double
{
    #[inline]
    fn double(&self) -> Self {
        Self { x: self.x.double(), y: self.y.double() }
    }
}

// ============================================================================
// Sum trait - Iterator support (generic)
// ============================================================================

impl<T> Sum for Point<T>
where
    T: NumericUnit,
{
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Point::default(), |acc, p| Point::new(
            T::add(acc.x, p.x),
            T::add(acc.y, p.y),
        ))
    }
}

impl<'a, T> Sum<&'a Point<T>> for Point<T>
where
    T: NumericUnit,
{
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Point::default(), |acc, p| Point::new(
            T::add(acc.x, p.x),
            T::add(acc.y, p.y),
        ))
    }
}

// ============================================================================
// ApproxEq trait - Approximate equality (generic)
// ============================================================================

impl<T: Unit> super::traits::ApproxEq for Point<T>
where
    T: super::traits::ApproxEq
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        self.x.approx_eq_eps(&other.x, epsilon) && self.y.approx_eq_eps(&other.y, epsilon)
    }
}

// ============================================================================
// Sign trait - Sign operations (generic)
// ============================================================================

impl<T: Unit> super::traits::Sign for Point<T>
where
    T: super::traits::Sign + Clone + std::fmt::Debug + Default + PartialEq
{
    #[inline]
    fn is_positive(&self) -> bool {
        self.x.is_positive() && self.y.is_positive()
    }

    #[inline]
    fn is_negative(&self) -> bool {
        self.x.is_negative() && self.y.is_negative()
    }

    #[inline]
    fn signum(self) -> Self {
        Self {
            x: super::traits::Sign::signum(self.x),
            y: super::traits::Sign::signum(self.y),
        }
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Point<super::units::Pixels> {
    /// Scales the point by a given factor, producing a `Point<ScaledPixels>`.
    ///
    /// This is typically used to convert logical pixel coordinates to scaled
    /// pixels for high-DPI displays.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, px};
    ///
    /// let p = Point::new(px(100.0), px(200.0));
    /// let scaled = p.scale(2.0);  // 2x Retina display
    #[must_use]
    pub fn scale(&self, factor: f32) -> Point<super::units::ScaledPixels> {
        Point {
            x: self.x.scale(factor),
            y: self.y.scale(factor),
        }
    }

    /// Calculates the Euclidean distance from the origin (0, 0) to this point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, px};
    ///
    /// let p = Point::new(px(3.0), px(4.0));
    /// assert_eq!(p.magnitude(), 5.0);
    #[must_use]
    pub fn magnitude(&self) -> f32 {
        (self.x.get().powi(2) + self.y.get().powi(2)).sqrt()
    }
}

// ============================================================================
// Specialized implementations for ScaledPixels
// ============================================================================

impl Point<super::units::ScaledPixels> {
    /// Converts to device pixels by rounding both coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, scaled_px};
    ///
    /// let p = Point::new(scaled_px(199.7), scaled_px(299.3));
    /// let device = p.to_device_pixels();
    #[must_use]
    pub fn to_device_pixels(&self) -> Point<super::units::DevicePixels> {
        Point {
            x: self.x.to_device_pixels(),
            y: self.y.to_device_pixels(),
        }
    }
}

// ============================================================================
// Type-safe scale conversions with ScaleFactor
// ============================================================================

impl Point<Pixels> {
    /// Type-safe scale conversion to DevicePixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, ScaleFactor, Pixels, DevicePixels, px, device_px};
    ///
    /// let logical = Point::new(px(100.0), px(200.0));
    /// let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    /// let device = logical.scale_with(scale);
    /// assert_eq!(device.x.get(), 200);
    /// assert_eq!(device.y.get(), 400);
    /// ```
    #[must_use]
    pub fn scale_with(self, scale: super::units::ScaleFactor<Pixels, super::units::DevicePixels>) -> Point<super::units::DevicePixels> {
        use super::units::device_px;
        Point {
            x: device_px((self.x.get() * scale.get()).round() as i32),
            y: device_px((self.y.get() * scale.get()).round() as i32),
        }
    }
}

impl Point<super::units::DevicePixels> {
    /// Converts to logical pixels using a type-safe scale factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Point, ScaleFactor, Pixels, DevicePixels, device_px, px};
    ///
    /// let device = Point::new(device_px(200.0), device_px(400.0));
    /// let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    /// let logical = device.unscale(scale);
    /// assert_eq!(logical.x, px(100.0));
    /// assert_eq!(logical.y, px(200.0));
    /// ```
    #[must_use]
    pub fn unscale(self, scale: super::units::ScaleFactor<Pixels, super::units::DevicePixels>) -> Point<Pixels> {
        let inverse = scale.inverse();
        Point {
            x: px(self.x.get() as f32 * inverse.get()),
            y: px(self.y.get() as f32 * inverse.get()),
        }
    }
}

// ============================================================================
// Generic relative positioning (requires Sub)
// ============================================================================

impl<T> Point<T>
where
    T: Unit + Sub<T, Output = T> + Clone + fmt::Debug + Default + PartialEq,
{
    /// Returns the position of this point relative to the given origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Point;
    ///
    /// let p = Point::new(100.0, 150.0);
    /// let origin = Point::new(20.0, 30.0);
    /// let relative = p.relative_to(&origin);
    /// assert_eq!(relative, Point::new(80.0, 120.0));
    #[must_use]
    pub fn relative_to(&self, origin: &Point<T>) -> Point<T> {
        Point {
            x: self.x - origin.x,
            y: self.y - origin.y,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construction() {
        let p = Point::new(10.0, 20.0);
        assert_eq!(p.x, 10.0);
        assert_eq!(p.y, 20.0);

        assert_eq!(Point::splat(5.0), Point::new(5.0, 5.0));
        assert_eq!(Point::from_array([1.0, 2.0]), Point::new(1.0, 2.0));
        assert_eq!(Point::from_tuple((3.0, 4.0)), Point::new(3.0, 4.0));
    }

    #[test]
    fn test_constants() {
        assert_eq!(Point::ORIGIN, Point::new(0.0, 0.0));
        assert_eq!(Point::ZERO, Point::ORIGIN);
        assert!(Point::INFINITY.x.is_infinite());
        assert!(Point::NAN.is_nan());
    }

    #[test]
    fn test_accessors() {
        let p = Point::new(10.0, 20.0);
        assert_eq!(p.to_array(), [10.0, 20.0]);
        assert_eq!(p.to_tuple(), (10.0, 20.0));
        assert_eq!(p.with_x(5.0), Point::new(5.0, 20.0));
        assert_eq!(p.with_y(5.0), Point::new(10.0, 5.0));
    }

    #[test]
    fn test_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);
        assert_eq!(p1.distance(p2), 5.0);
        assert_eq!(p1.distance_squared(p2), 25.0);
    }

    #[test]
    fn test_midpoint() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(10.0, 20.0);
        assert_eq!(p1.midpoint(p2), Point::new(5.0, 10.0));
    }

    #[test]
    fn test_lerp() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(10.0, 20.0);

        assert_eq!(p1.lerp(p2, 0.0), p1);
        assert_eq!(p1.lerp(p2, 0.5), Point::new(5.0, 10.0));
        assert_eq!(p1.lerp(p2, 1.0), p2);
    }

    #[test]
    fn test_min_max_clamp() {
        let p1 = Point::new(5.0, 15.0);
        let p2 = Point::new(10.0, 8.0);

        assert_eq!(p1.min(p2), Point::new(5.0, 8.0));
        assert_eq!(p1.max(p2), Point::new(10.0, 15.0));

        let p = Point::new(15.0, -5.0);
        let min = Point::ZERO;
        let max = Point::splat(10.0);
        assert_eq!(p.clamp(min, max), Point::new(10.0, 0.0));
    }

    #[test]
    fn test_rounding() {
        let p = Point::new(10.6, -3.3);
        assert_eq!(p.round(), Point::new(11.0, -3.0));
        assert_eq!(p.ceil(), Point::new(11.0, -3.0));
        assert_eq!(p.floor(), Point::new(10.0, -4.0));
        assert_eq!(p.trunc(), Point::new(10.0, -3.0));
        assert_eq!(p.expand(), Point::new(11.0, -4.0));
    }

    #[test]
    fn test_validation() {
        assert!(Point::new(1.0, 2.0).is_finite());
        assert!(!Point::INFINITY.is_finite());
        assert!(!Point::NAN.is_finite());
        assert!(Point::NAN.is_nan());
        assert!(!Point::ZERO.is_nan());
    }

    #[test]
    fn test_point_minus_point() {
        let p1 = Point::new(10.0, 20.0);
        let p2 = Point::new(3.0, 5.0);
        let v: Vec2<Pixels> = p1 - p2;
        assert_eq!(v, Vec2::new(7.0, 15.0));
    }

    #[test]
    fn test_point_vec_ops() {
        let p = Point::new(10.0, 20.0);
        let v = Vec2::new(5.0, 10.0);

        assert_eq!(p + v, Point::new(15.0, 30.0));
        assert_eq!(p - v, Point::new(5.0, 10.0));

        let mut p2 = p;
        p2 += v;
        assert_eq!(p2, Point::new(15.0, 30.0));

        let mut p3 = p;
        p3 -= v;
        assert_eq!(p3, Point::new(5.0, 10.0));
    }

    #[test]
    fn test_scalar_ops() {
        let p = Point::new(10.0, 20.0);

        assert_eq!(p * 2.0, Point::new(20.0, 40.0));
        assert_eq!(2.0 * p, Point::new(20.0, 40.0));
        assert_eq!(p / 2.0, Point::new(5.0, 10.0));
        assert_eq!(-p, Point::new(-10.0, -20.0));
    }

    #[test]
    fn test_conversions() {
        let p = Point::new(10.0, 20.0);

        let from_tuple: Point<Pixels> = (10.0, 20.0).into();
        let from_array: Point<Pixels> = [10.0, 20.0].into();
        assert_eq!(from_tuple, p);
        assert_eq!(from_array, p);

        let to_tuple: (f32, f32) = p.into();
        let to_array: [f32; 2] = p.into();
        assert_eq!(to_tuple, (10.0, 20.0));
        assert_eq!(to_array, [10.0, 20.0]);

        let v = Vec2::new(5.0, 10.0);
        let p_from_v: Point<Pixels> = v.into();
        assert_eq!(p_from_v, Point::new(5.0, 10.0));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Point::new(10.5, 20.5)), "(10.5, 20.5)");
    }

    #[test]
    fn test_convenience_fn() {
        assert_eq!(point(1.0, 2.0), Point::new(1.0, 2.0));
    }
}

#[cfg(test)]
mod typed_tests {
    use super::*;
    use crate::geometry::{Pixels, px};

    #[test]
    fn test_point_new() {
        let p = Point::<Pixels>::new(px(10.0), px(20.0));
        assert_eq!(p.x.0, 10.0);
        assert_eq!(p.y.0, 20.0);
    }

    #[test]
    fn test_point_f32() {
        let p = Point::<f32>::new(0.5, 0.75);
        assert_eq!(p.x, 0.5);
        assert_eq!(p.y, 0.75);
    }

    #[test]
    fn test_point_validation() {
        let valid = Point::<f32>::new(1.0, 2.0);
        assert!(valid.is_valid());
        assert!(!valid.is_nan());

        let invalid = Point::<f32>::new(f32::NAN, 2.0);
        assert!(!invalid.is_valid());
        assert!(invalid.is_nan());
    }

    #[test]
    fn test_point_try_new() {
        let result = Point::<f32>::try_new(1.0, 2.0);
        assert!(result.is_ok());

        let result = Point::<f32>::try_new(f32::NAN, 2.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_point_clamped() {
        let p = Point::<f32>::new_clamped(f32::NAN, 2.0);
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 2.0);

        let p = Point::<f32>::new_clamped(f32::INFINITY, -f32::INFINITY);
        assert_eq!(p.x, f32::MAX);
        assert_eq!(p.y, f32::MIN);
    }

    #[test]
    fn test_point_cast() {
        let p = Point::<Pixels>::new(px(100.0), px(200.0));
        let p_f32: Point<Pixels> = p.cast();
        assert_eq!(p_f32.x, 100.0);
        assert_eq!(p_f32.y, 200.0);
    }

    #[test]
    fn test_point_to_f32() {
        let p = Point::<Pixels>::new(px(100.0), px(200.0));
        let p_f32 = p.to_f32();
        assert_eq!(p_f32.x, 100.0);
    }

    #[test]
    fn test_point_to_array() {
        let p = Point::<Pixels>::new(px(100.0), px(200.0));
        let arr = p.to_array();
        assert_eq!(arr, [100.0, 200.0]);
    }

    #[test]
    fn test_point_from_into() {
        let p = Point::<Pixels>::new(px(100.0), px(200.0));

        // Test tuple conversion
        let tuple: (f32, f32) = p.into();
        assert_eq!(tuple, (100.0, 200.0));

        // Test array conversion
        let arr: [f32; 2] = p.into();
        assert_eq!(arr, [100.0, 200.0]);
    }
}

#[cfg(test)]
mod arithmetic_tests {
    use super::*;
    use crate::geometry::{Pixels, px, vec2};

    #[test]
    fn test_point_add_vec2() {
        let p = Point::<f32>::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        let result = p + v;
        assert_eq!(result.x, 15.0);
        assert_eq!(result.y, 30.0);
    }

    #[test]
    fn test_point_add_assign_vec2() {
        let mut p = Point::<f32>::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        p += v;
        assert_eq!(p.x, 15.0);
        assert_eq!(p.y, 30.0);
    }

    #[test]
    fn test_point_sub_point() {
        let p1 = Point::<f32>::new(20.0, 30.0);
        let p2 = Point::<f32>::new(10.0, 15.0);

        let v = p1 - p2;
        assert_eq!(v.x, 10.0);
        assert_eq!(v.y, 15.0);
    }

    #[test]
    fn test_point_sub_vec2() {
        let p = Point::<f32>::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        let result = p - v;
        assert_eq!(result.x, 5.0);
        assert_eq!(result.y, 10.0);
    }

    #[test]
    fn test_point_sub_assign_vec2() {
        let mut p = Point::<f32>::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        p -= v;
        assert_eq!(p.x, 5.0);
        assert_eq!(p.y, 10.0);
    }

    #[test]
    fn test_point_scalar_mul() {
        let p = Point::<f32>::new(10.0, 20.0);

        let p2 = p * 2.0;
        assert_eq!(p2.x, 20.0);
        assert_eq!(p2.y, 40.0);
    }

    #[test]
    fn test_point_scalar_mul_reverse() {
        let p = Point::<f32>::new(10.0, 20.0);

        let p2 = 2.0 * p;
        assert_eq!(p2.x, 20.0);
        assert_eq!(p2.y, 40.0);
    }

    #[test]
    fn test_point_scalar_div() {
        let p = Point::<f32>::new(10.0, 20.0);

        let p2 = p / 2.0;
        assert_eq!(p2.x, 5.0);
        assert_eq!(p2.y, 10.0);
    }

    #[test]
    fn test_point_negation() {
        let p = Point::<f32>::new(10.0, -20.0);

        let neg_p = -p;
        assert_eq!(neg_p.x, -10.0);
        assert_eq!(neg_p.y, 20.0);
    }

    #[test]
    fn test_point_checked_add_vec() {
        let p = Point::<f32>::new(1.0, 2.0);

        let result = p.checked_add_vec(3.0, 4.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().x, 4.0);
        assert_eq!(result.unwrap().y, 6.0);

        // Test with invalid values
        let invalid = p.checked_add_vec(f32::NAN, 4.0);
        assert!(invalid.is_none());
    }

    #[test]
    fn test_point_saturating_add_vec() {
        let p = Point::<f32>::new(1.0, 2.0);

        let result = p.saturating_add_vec(3.0, 4.0);
        assert_eq!(result.x, 4.0);
        assert_eq!(result.y, 6.0);

        // Test with NaN - should clamp to 0
        let saturated = p.saturating_add_vec(f32::NAN, 4.0);
        assert_eq!(saturated.x, 0.0);
        assert_eq!(saturated.y, 6.0);

        // Test with infinity - should clamp to MAX
        let inf_result = p.saturating_add_vec(f32::INFINITY, 4.0);
        assert_eq!(inf_result.x, f32::MAX);
        assert_eq!(inf_result.y, 6.0);
    }

    #[test]
    fn test_point_checked_mul() {
        let p = Point::<f32>::new(1.0, 2.0);

        let result = p.checked_mul(2.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().x, 2.0);
        assert_eq!(result.unwrap().y, 4.0);

        // Test with infinity - should return None
        let infinity = p.checked_mul(f32::INFINITY);
        assert!(infinity.is_none());
    }

    #[test]
    fn test_point_saturating_mul() {
        let p = Point::<f32>::new(1.0, 2.0);

        let result = p.saturating_mul(2.0);
        assert_eq!(result.x, 2.0);
        assert_eq!(result.y, 4.0);

        // Test with infinity - should clamp to MAX
        let saturated = p.saturating_mul(f32::INFINITY);
        assert_eq!(saturated.x, f32::MAX);
        assert_eq!(saturated.y, f32::MAX);
    }

    #[test]
    fn test_typed_point_scalar_ops() {
        let p = Point::<Pixels>::new(px(10.0), px(20.0));

        // Scalar multiplication
        let p2 = p * 2.0;
        assert_eq!(p2.x.0, 20.0);
        assert_eq!(p2.y.0, 40.0);

        // Scalar division
        let p3 = p / 2.0;
        assert_eq!(p3.x.0, 5.0);
        assert_eq!(p3.y.0, 10.0);
    }

    #[test]
    fn test_typed_point_checked_operations() {
        let p = Point::<Pixels>::new(px(10.0), px(20.0));

        // Checked addition
        let result = p.checked_add_vec(px(5.0), px(10.0));
        assert!(result.is_some());
        assert_eq!(result.unwrap().x.0, 15.0);
        assert_eq!(result.unwrap().y.0, 30.0);

        // Checked multiplication
        let result = p.checked_mul(2.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().x.0, 20.0);
        assert_eq!(result.unwrap().y.0, 40.0);
    }

    #[test]
    fn test_point_utility_traits() {
        use crate::geometry::{Axis, Along, Half, IsZero};

        // Test Along trait
        let p = Point::<Pixels>::new(px(10.0), px(20.0));
        assert_eq!(p.along(Axis::Horizontal).0, 10.0);
        assert_eq!(p.along(Axis::Vertical).0, 20.0);

        let modified = p.apply_along(Axis::Horizontal, |x| px(x.0 * 2.0));
        assert_eq!(modified.x.0, 20.0);
        assert_eq!(modified.y.0, 20.0);

        // Test Half trait
        let half_p = p.half();
        assert_eq!(half_p.x.0, 5.0);
        assert_eq!(half_p.y.0, 10.0);

        // Test negation (using std::ops::Neg)
        let neg_p = -p;
        assert_eq!(neg_p.x.0, -10.0);
        assert_eq!(neg_p.y.0, -20.0);

        // Test IsZero trait
        let zero = Point::<Pixels>::new(px(0.0), px(0.0));
        assert!(zero.is_zero());
        assert!(!p.is_zero());
    }
}
