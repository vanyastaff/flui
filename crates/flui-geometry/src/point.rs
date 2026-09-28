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
use std::{
    fmt,
    iter::Sum,
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign},
};

use super::{
    Vec2,
    error::GeometryError,
    traits::{FloatUnit, NumericUnit, Unit},
};

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
/// use flui_geometry::{Point, px, Pixels};
///
/// let ui_pos = Point::<Pixels>::new(100.0, 200.0);
/// let normalized = Point::<Pixels>::new(0.5, 0.75);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point<T: Unit = f64> {
    /// The x coordinate (horizontal position).
    pub x: T,
    /// The y coordinate (vertical position).
    pub y: T,
}

impl<T: Unit> Default for Point<T> {
    #[inline]
    fn default() -> Self {
        Self {
            x: T::zero(),
            y: T::zero(),
        }
    }
}

// ============================================================================
// Constants (f64 only for backwards compatibility)
// ============================================================================

impl Point<f64> {
    /// The origin point (0, 0).
    pub const ORIGIN: Self = Self::new(0.0, 0.0);

    /// Alias for [`ORIGIN`](Self::ORIGIN).
    pub const ZERO: Self = Self::ORIGIN;

    /// Point at positive infinity.
    pub const INFINITY: Self = Self::new(f64::INFINITY, f64::INFINITY);

    /// Point at negative infinity.
    pub const NEG_INFINITY: Self = Self::new(f64::NEG_INFINITY, f64::NEG_INFINITY);

    /// Point with NaN coordinates.
    pub const NAN: Self = Self::new(f64::NAN, f64::NAN);
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Point<T> {
    /// Creates a point from x and y coordinates.
    #[inline]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    /// Creates a point with both coordinates set to the same value.
    #[inline]
    pub fn splat(value: T) -> Self {
        Self { x: value, y: value }
    }

    /// Swaps x and y coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Point, px};
    ///
    /// let p = Point::new(10.0, 20.0);
    /// assert_eq!(p.swap(), Point::new(20.0, 10.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn swap(self) -> Self {
        Self {
            x: self.y,
            y: self.x,
        }
    }
}

// ============================================================================
// Safe Constructors (NumericUnit with Into<f64> + From<f64>)
// ============================================================================

impl<T: NumericUnit> Point<T>
where
    T: Into<f64> + FloatUnit,
{
    /// Creates a point with validation, returning an error for invalid
    /// coordinates.
    #[inline]
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
    #[inline]
    pub fn new_clamped(x: T, y: T) -> Self {
        let clamp_f32 = |v: f64| {
            if v.is_nan() {
                0.0
            } else if v.is_infinite() {
                if v > 0.0 { f64::MAX } else { f64::MIN }
            } else {
                v
            }
        };

        Self {
            x: T::from_f64(clamp_f32(x.into())),
            y: T::from_f64(clamp_f32(y.into())),
        }
    }
}

// ============================================================================
// Validation Methods (NumericUnit with Into<f64>)
// ============================================================================

impl<T: NumericUnit> Point<T>
where
    T: Into<f64>,
{
    /// Checks if coordinates are valid (finite, not NaN).
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let x_f32: f64 = self.x.into();
        let y_f32: f64 = self.y.into();
        x_f32.is_finite() && y_f32.is_finite()
    }

    /// Checks if both coordinates are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.is_valid()
    }

    /// Checks if any coordinate is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(&self) -> bool {
        let x_f32: f64 = self.x.into();
        let y_f32: f64 = self.y.into();
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
    /// use flui_geometry::{Pixels, Point, px};
    ///
    /// let p: Point<Pixels> = Point::new(3.0, 4.0);
    /// let p_doubled: Point<Pixels> = p.map(|coord| coord * 2.0);
    /// assert_eq!(p_doubled, Point::new(6.0, 8.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn map<U>(self, f: impl Fn(T) -> U) -> Point<U>
    where
        U: Unit,
    {
        Point {
            x: f(self.x),
            y: f(self.y),
        }
    }

    /// Returns a point with a new x coordinate.
    #[inline]
    #[must_use]
    pub fn with_x(self, x: T) -> Self {
        Self::new(x, self.y)
    }

    /// Returns a point with a new y coordinate.
    #[inline]
    #[must_use]
    pub fn with_y(self, y: T) -> Self {
        Self::new(self.x, y)
    }
}

// ============================================================================
// Accessors & Conversion (f64 specialization)
// ============================================================================

impl Point<f64> {
    /// Creates a point from a two-element array.
    #[inline]
    #[must_use]
    pub const fn from_array(a: [f64; 2]) -> Self {
        Self::new(a[0], a[1])
    }

    /// Creates a point from a tuple.
    #[inline]
    #[must_use]
    pub const fn from_tuple(t: (f64, f64)) -> Self {
        Self::new(t.0, t.1)
    }

    /// Converts to a vector with same coordinates.
    #[inline]
    #[must_use]
    pub const fn to_vec2(self) -> Vec2<f64> {
        Vec2::new(self.x, self.y)
    }
}

// ============================================================================
// Geometric Operations (f64 only)
// ============================================================================

impl<T> Point<T>
where
    T: NumericUnit + Into<f64> + FloatUnit,
{
    /// Euclidean distance to another point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Point, px};
    ///
    /// let p1 = Point::new(0.0, 0.0);
    /// let p2 = Point::new(3.0, 4.0);
    /// assert_eq!(p1.distance(p2), 5.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        let dx: f64 = T::sub(other.x, self.x).into();
        let dy: f64 = T::sub(other.y, self.y).into();
        dx.hypot(dy)
    }

    /// Squared euclidean distance to another point.
    ///
    /// This is faster than [`distance`](Self::distance) when you only need
    #[inline]
    #[must_use]
    pub fn distance_squared(self, other: Self) -> f64 {
        let dx = T::sub(other.x, self.x);
        let dy = T::sub(other.y, self.y);
        let dx_f32: f64 = dx.into();
        let dy_f32: f64 = dy.into();
        dx_f32 * dx_f32 + dy_f32 * dy_f32
    }

    /// Midpoint between this point and another.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Point, px};
    ///
    /// let p1 = Point::new(0.0, 0.0);
    /// let p2 = Point::new(10.0, 20.0);
    /// assert_eq!(p1.midpoint(p2), Point::new(5.0, 10.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        let sum_x = self.x + other.x;
        let sum_y = self.y + other.y;
        let sum_x_f32: f64 = sum_x.into();
        let sum_y_f32: f64 = sum_y.into();
        Self::new(T::from_f64(sum_x_f32 / 2.0), T::from_f64(sum_y_f32 / 2.0))
    }
}

impl Point<f64> {}

// ============================================================================
// Interpolation (generic with NumericUnit)
// ============================================================================

impl<T> Point<T>
where
    T: NumericUnit + Into<f64> + FloatUnit,
{
    /// Linear interpolation between two points.
    ///
    /// - `t = 0.0` returns `self`
    /// - `t = 0.5` returns midpoint
    /// - `t = 1.0` returns `other`
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        let x0: f64 = self.x.into();
        let y0: f64 = self.y.into();
        let x1: f64 = other.x.into();
        let y1: f64 = other.y.into();

        Self::new(
            T::from_f64(x0 + (x1 - x0) * t),
            T::from_f64(y0 + (y1 - y0) * t),
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
    /// Returns a point with the minimum of each coordinate.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self {
            x: if self.x <= other.x { self.x } else { other.x },
            y: if self.y <= other.y { self.y } else { other.y },
        }
    }

    /// Returns a point with the maximum of each coordinate.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            x: if self.x >= other.x { self.x } else { other.x },
            y: if self.y >= other.y { self.y } else { other.y },
        }
    }

    /// Clamps each coordinate between min and max values.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }
}

// ============================================================================
// f64-specific operations
// ============================================================================

impl Point<f64> {
    /// Returns a point with absolute values of both coordinates.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs())
    }

    /// Returns the minimum coordinate value.
    #[inline]
    #[must_use]
    pub fn min_element(self) -> f64 {
        self.x.min(self.y)
    }

    /// Returns the maximum coordinate value.
    #[inline]
    #[must_use]
    pub fn max_element(self) -> f64 {
        self.x.max(self.y)
    }
}

// ============================================================================
// Rounding Operations (f64 only)
// ============================================================================

impl Point<f64> {
    /// Rounds both coordinates to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    /// Rounds both coordinates up.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil())
    }

    /// Rounds both coordinates down.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    /// Truncates both coordinates toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self::new(self.x.trunc(), self.y.trunc())
    }

    /// Expands both coordinates away from zero.
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

    /// Returns the fractional part of each coordinate.
    #[inline]
    #[must_use]
    pub fn fract(self) -> Self {
        Self::new(self.x.fract(), self.y.fract())
    }
}

// ============================================================================
// Validation (f64 only)
// ============================================================================

impl Point<f64> {}

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
    /// use flui_geometry::{Pixels, Point, Vec2, px};
    ///
    /// let p1 = Point::<Pixels>::new(10.0, 20.0);
    /// let p2 = Point::<Pixels>::new(3.0, 5.0);
    /// let v: Vec2<Pixels> = p1 - p2;
    /// assert_eq!(v, Vec2::new(7.0, 15.0));
    ///
    /// // Works with Pixels too
    /// let p1 = Point::new(100.0, 200.0);
    /// let p2 = Point::new(30.0, 50.0);
    /// let v: Vec2<Pixels> = p1 - p2;
    /// assert_eq!(v.x.get(), 70.0);
    /// ```
    #[inline]
    fn sub(self, rhs: Self) -> Vec2<T> {
        Vec2::new(T::sub(self.x, rhs.x), T::sub(self.y, rhs.y))
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
        Self::new(T::add(self.x, rhs.x), T::add(self.y, rhs.y))
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
        Self::new(T::sub(self.x, rhs.x), T::sub(self.y, rhs.y))
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

// Reverse multiplication: f64 * Point<Pixels>
impl Mul<Point<f64>> for f64 {
    type Output = Point<f64>;

    #[inline]
    fn mul(self, rhs: Point<f64>) -> Point<f64> {
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
    T: Into<f64> + FloatUnit,
{
    /// Checked addition with a vector, returns None if result is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Pixels, Point, px};
    ///
    /// let p = Point::<Pixels>::new(1.0, 2.0);
    /// let result = p.checked_add_vec(3.0, 4.0);
    /// assert!(result.is_some());
    /// assert_eq!(result.unwrap(), Point::new(4.0, 6.0));
    /// ```
    #[inline]
    #[must_use]
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

    /// Saturating addition with a vector, clamps invalid values to valid range.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Pixels, Point, px};
    ///
    /// let p = Point::<Pixels>::new(1.0, 2.0);
    /// let result = p.saturating_add_vec((f64::NAN), 4.0);
    /// // NaN gets clamped to 0
    /// assert_eq!(result.x, 0.0);
    /// assert_eq!(result.y, 6.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn saturating_add_vec(self, dx: T, dy: T) -> Self {
        Self::new_clamped(self.x.add(dx), self.y.add(dy))
    }

    /// Checked scalar multiplication, returns None if result is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Pixels, Point, px};
    ///
    /// let p = Point::<Pixels>::new(1.0, 2.0);
    /// let result = p.checked_mul(2.0);
    /// assert!(result.is_some());
    /// assert_eq!(result.unwrap(), Point::new(2.0, 4.0));
    ///
    /// let infinity = p.checked_mul(f64::INFINITY);
    /// assert!(infinity.is_none());
    /// ```
    #[inline]
    #[must_use]
    pub fn checked_mul(self, scalar: f64) -> Option<Self> {
        let x_f32: f64 = self.x.into();
        let y_f32: f64 = self.y.into();
        let result = Self {
            x: T::from_f64(x_f32 * scalar),
            y: T::from_f64(y_f32 * scalar),
        };

        if result.is_valid() {
            Some(result)
        } else {
            None
        }
    }

    /// Saturating scalar multiplication, clamps invalid values to valid range.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Pixels, Point, px};
    ///
    /// let p = Point::<Pixels>::new(1.0, 2.0);
    /// let result = p.saturating_mul(f64::INFINITY);
    /// assert_eq!(result.x, (f64::MAX));
    /// assert_eq!(result.y, (f64::MAX));
    /// ```
    #[inline]
    #[must_use]
    pub fn saturating_mul(self, scalar: f64) -> Self {
        let x_f32: f64 = self.x.into();
        let y_f32: f64 = self.y.into();
        Self::new_clamped(T::from_f64(x_f32 * scalar), T::from_f64(y_f32 * scalar))
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
    /// use flui_geometry::{Point, Pixels, px};
    ///
    /// let p = Point::<Pixels>::new(100.0, 200.0);
    /// let p_f32: Point<Pixels> = p.cast();
    /// assert_eq!(p_f32.x.get(), 100.0);
    /// assert_eq!(p_f32.y.get(), 200.0);
    #[inline]
    #[must_use]
    pub fn cast<U>(self) -> Point<U>
    where
        U: Unit,
        T: Into<U>,
    {
        Point {
            x: self.x.into(),
            y: self.y.into(),
        }
    }
}

// ============================================================================
// GPU Conversion Methods (NumericUnit → f64)
// ============================================================================

impl<T: NumericUnit> Point<T>
where
    T: Into<f64>,
{
    /// Converts to `Point<Pixels>` (shorthand for GPU usage).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Pixels, Point, px};
    ///
    /// let p = Point::<Pixels>::new(100.0, 200.0);
    /// let p_f32 = p.to_f32();
    /// assert_eq!(p_f32, Point::new(100.0, 200.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Point<f64> {
        Point {
            x: self.x.into(),
            y: self.y.into(),
        }
    }

    /// Converts to raw array [x, y] for GPU buffers.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Point, Pixels, px};
    ///
    /// let p = Point::<Pixels>::new(100.0, 200.0);
    /// let arr = p.to_array();
    /// assert_eq!(arr, [100.0, 200.0]);
    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f64; 2] {
        [self.x.into(), self.y.into()]
    }

    /// Converts to tuple (x, y).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Point, Pixels, px};
    ///
    /// let p = Point::<Pixels>::new(100.0, 200.0);
    /// let tuple = p.to_tuple();
    /// assert_eq!(tuple, (100.0, 200.0));
    #[inline]
    #[must_use]
    pub fn to_tuple(self) -> (f64, f64) {
        (self.x.into(), self.y.into())
    }
}

// ============================================================================
// From Trait Implementations
// ============================================================================

// Note: We cannot implement From<Point<T>> for Point<Pixels> generically
// because it conflicts with the reflexive impl From<T> for T when T=f64.
// Instead, users should use .cast(), .to_f32(), or .into() on specific types.

/// Converts from `Point<T>` to `(f64, f64)` for any T that converts to f64.
impl<T: Unit> From<Point<T>> for (f64, f64)
where
    T: Into<f64>,
{
    #[inline]
    fn from(p: Point<T>) -> (f64, f64) {
        (p.x.into(), p.y.into())
    }
}

/// Converts from `Point<T>` to `[f64; 2]` for any T that converts to f64.
impl<T: Unit> From<Point<T>> for [f64; 2]
where
    T: Into<f64>,
{
    #[inline]
    fn from(p: Point<T>) -> [f64; 2] {
        [p.x.into(), p.y.into()]
    }
}

// ============================================================================
// Conversions (f64 only - specialized)
// ============================================================================

impl From<(f64, f64)> for Point<f64> {
    #[inline]
    fn from((x, y): (f64, f64)) -> Self {
        Self::new(x, y)
    }
}

impl From<[f64; 2]> for Point<f64> {
    #[inline]
    fn from([x, y]: [f64; 2]) -> Self {
        Self::new(x, y)
    }
}

impl From<Vec2<f64>> for Point<f64> {
    #[inline]
    fn from(v: Vec2<f64>) -> Self {
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
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

// ============================================================================
// Convenience function (f64 only)
// ============================================================================

/// Shorthand for `Point::new(x, y)`.
///
/// # Examples
///
/// ```
/// use flui_geometry::point;
///
/// let p = point(10.0, 20.0);
#[inline]
#[must_use]
pub const fn point(x: f64, y: f64) -> Point<f64> {
    Point::new(x, y)
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
// IsZero trait - Zero check (generic)
// ============================================================================

impl<T: Unit> super::traits::IsZero for Point<T>
where
    T: super::traits::IsZero,
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
// Sum trait - Iterator support (generic)
// ============================================================================

impl<T> Sum for Point<T>
where
    T: NumericUnit,
{
    #[inline]
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Point::default(), |acc, p| {
            Point::new(T::add(acc.x, p.x), T::add(acc.y, p.y))
        })
    }
}

impl<'a, T> Sum<&'a Point<T>> for Point<T>
where
    T: NumericUnit,
{
    #[inline]
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Point::default(), |acc, p| {
            Point::new(T::add(acc.x, p.x), T::add(acc.y, p.y))
        })
    }
}

// ============================================================================
// ApproxEq trait - Approximate equality (generic)
// ============================================================================

impl<T: Unit> super::traits::ApproxEq for Point<T>
where
    T: super::traits::ApproxEq,
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        self.x.approx_eq_eps(&other.x, epsilon) && self.y.approx_eq_eps(&other.y, epsilon)
    }
}

// ============================================================================
// Sign trait - Sign operations (generic)
// ============================================================================

impl<T: Unit> super::traits::Sign for Point<T>
where
    T: super::traits::Sign + Clone + std::fmt::Debug + Default + PartialEq,
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

impl Point<f64> {
    /// Scales the point by a given factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Point, px};
    ///
    /// let p = Point::new(100.0, 200.0);
    /// let scaled = p.scale(2.0);  // 2x Retina display
    #[inline]
    #[must_use]
    pub fn scale(self, factor: f64) -> Point<f64> {
        Point {
            x: self.x * factor,
            y: self.y * factor,
        }
    }

    /// Calculates the Euclidean distance from the origin (0, 0) to this point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_geometry::{Point, px};
    ///
    /// let p = Point::new(3.0, 4.0);
    /// assert_eq!(p.magnitude(), 5.0);
    #[inline]
    #[must_use]
    pub fn magnitude(self) -> f64 {
        self.x.hypot(self.y)
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
    /// use flui_geometry::{Point, px};
    ///
    /// let p = Point::new(100.0, 150.0);
    /// let origin = Point::new(20.0, 30.0);
    /// let relative = p.relative_to(&origin);
    /// assert_eq!(relative, Point::new(80.0, 120.0));
    /// ```
    #[inline]
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
        let v: Vec2<f64> = p1 - p2;
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

        let from_tuple: Point<f64> = (10.0, 20.0).into();
        let from_array: Point<f64> = [10.0, 20.0].into();
        assert_eq!(from_tuple, p);
        assert_eq!(from_array, p);

        let to_tuple: (f64, f64) = p.into();
        let to_array: [f64; 2] = p.into();
        assert_eq!(to_tuple, (10.0, 20.0));
        assert_eq!(to_array, [10.0, 20.0]);

        let v = Vec2::new(5.0, 10.0);
        let p_from_v: Point<f64> = v.into();
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

    #[test]
    fn test_point_new() {
        let p = Point::<f64>::new(10.0, 20.0);
        assert_eq!(p.x, 10.0);
        assert_eq!(p.y, 20.0);
    }

    #[test]
    fn test_point_pixels() {
        let p = Point::<f64>::new(0.5, 0.75);
        assert_eq!(p.x, 0.5);
        assert_eq!(p.y, 0.75);
    }

    #[test]
    fn test_point_validation() {
        let valid = Point::<f64>::new(1.0, 2.0);
        assert!(valid.is_valid());
        assert!(!valid.is_nan());

        let invalid = Point::<f64>::new(f64::NAN, 2.0);
        assert!(!invalid.is_valid());
        assert!(invalid.is_nan());
    }

    #[test]
    fn test_point_try_new() {
        let result = Point::<f64>::try_new(1.0, 2.0);
        assert!(result.is_ok());

        let result = Point::<f64>::try_new(f64::NAN, 2.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_point_clamped() {
        let p = Point::<f64>::new_clamped(f64::NAN, 2.0);
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 2.0);

        let p = Point::<f64>::new_clamped(f64::INFINITY, -f64::INFINITY);
        assert_eq!(p.x, f64::MAX);
        assert_eq!(p.y, f64::MIN);
    }

    #[test]
    fn test_point_cast() {
        let p = Point::<f64>::new(100.0, 200.0);
        let p_f32: Point<f64> = p.cast();
        assert_eq!(p_f32.x, 100.0);
        assert_eq!(p_f32.y, 200.0);
    }

    #[test]
    fn test_point_to_f32() {
        let p = Point::<f64>::new(100.0, 200.0);
        let p_f32 = p.to_f32();
        assert_eq!(p_f32.x, 100.0);
    }

    #[test]
    fn test_point_to_array() {
        let p = Point::<f64>::new(100.0, 200.0);
        let arr = p.to_array();
        assert_eq!(arr, [100.0, 200.0]);
    }

    #[test]
    fn test_point_from_into() {
        let p = Point::<f64>::new(100.0, 200.0);

        // Test tuple conversion
        let tuple: (f64, f64) = p.into();
        assert_eq!(tuple, (100.0, 200.0));

        // Test array conversion
        let arr: [f64; 2] = p.into();
        assert_eq!(arr, [100.0, 200.0]);
    }
}

#[cfg(test)]
mod arithmetic_tests {
    use super::*;
    use crate::vec2;

    #[test]
    fn test_point_add_vec2() {
        let p = Point::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        let result = p + v;
        assert_eq!(result.x, 15.0);
        assert_eq!(result.y, 30.0);
    }

    #[test]
    fn test_point_add_assign_vec2() {
        let mut p = Point::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        p += v;
        assert_eq!(p.x, 15.0);
        assert_eq!(p.y, 30.0);
    }

    #[test]
    fn test_point_sub_point() {
        let p1 = Point::new(20.0, 30.0);
        let p2 = Point::new(10.0, 15.0);

        let v = p1 - p2;
        assert_eq!(v.x, 10.0);
        assert_eq!(v.y, 15.0);
    }

    #[test]
    fn test_point_sub_vec2() {
        let p = Point::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        let result = p - v;
        assert_eq!(result.x, 5.0);
        assert_eq!(result.y, 10.0);
    }

    #[test]
    fn test_point_sub_assign_vec2() {
        let mut p = Point::new(10.0, 20.0);
        let v = vec2(5.0, 10.0);

        p -= v;
        assert_eq!(p.x, 5.0);
        assert_eq!(p.y, 10.0);
    }

    #[test]
    fn test_point_scalar_mul() {
        let p = Point::new(10.0, 20.0);

        let p2 = p * 2.0;
        assert_eq!(p2.x, 20.0);
        assert_eq!(p2.y, 40.0);
    }

    #[test]
    fn test_point_scalar_mul_reverse() {
        let p = Point::new(10.0, 20.0);

        let p2 = 2.0 * p;
        assert_eq!(p2.x, 20.0);
        assert_eq!(p2.y, 40.0);
    }

    #[test]
    fn test_point_scalar_div() {
        let p = Point::new(10.0, 20.0);

        let p2 = p / 2.0;
        assert_eq!(p2.x, 5.0);
        assert_eq!(p2.y, 10.0);
    }

    #[test]
    fn test_point_negation() {
        let p = Point::new(10.0, -20.0);

        let neg_p = -p;
        assert_eq!(neg_p.x, -10.0);
        assert_eq!(neg_p.y, 20.0);
    }

    #[test]
    fn test_point_checked_add_vec() {
        let p = Point::new(1.0, 2.0);

        let result = p.checked_add_vec(3.0, 4.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().x, 4.0);
        assert_eq!(result.unwrap().y, 6.0);

        // Test with invalid values
        let invalid = p.checked_add_vec(f64::NAN, 4.0);
        assert!(invalid.is_none());
    }

    #[test]
    fn test_point_saturating_add_vec() {
        let p = Point::new(1.0, 2.0);

        let result = p.saturating_add_vec(3.0, 4.0);
        assert_eq!(result.x, 4.0);
        assert_eq!(result.y, 6.0);

        // Test with NaN - should clamp to 0
        let saturated = p.saturating_add_vec(f64::NAN, 4.0);
        assert_eq!(saturated.x, 0.0);
        assert_eq!(saturated.y, 6.0);

        // Test with infinity - should clamp to MAX
        let inf_result = p.saturating_add_vec(f64::INFINITY, 4.0);
        assert_eq!(inf_result.x, (f64::MAX));
        assert_eq!(inf_result.y, 6.0);
    }

    #[test]
    fn test_point_checked_mul() {
        let p = Point::new(1.0, 2.0);

        let result = p.checked_mul(2.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().x, 2.0);
        assert_eq!(result.unwrap().y, 4.0);

        // Test with infinity - should return None
        let infinity = p.checked_mul(f64::INFINITY);
        assert!(infinity.is_none());
    }

    #[test]
    fn test_point_saturating_mul() {
        let p = Point::new(1.0, 2.0);

        let result = p.saturating_mul(2.0);
        assert_eq!(result.x, 2.0);
        assert_eq!(result.y, 4.0);

        // Test with infinity - should clamp to MAX
        let saturated = p.saturating_mul(f64::INFINITY);
        assert_eq!(saturated.x, (f64::MAX));
        assert_eq!(saturated.y, (f64::MAX));
    }

    #[test]
    fn test_typed_point_scalar_ops() {
        let p = Point::<f64>::new(10.0, 20.0);

        // Scalar multiplication
        let p2 = p * 2.0;
        assert_eq!(p2.x, 20.0);
        assert_eq!(p2.y, 40.0);

        // Scalar division
        let p3 = p / 2.0;
        assert_eq!(p3.x, 5.0);
        assert_eq!(p3.y, 10.0);
    }

    #[test]
    fn test_typed_point_checked_operations() {
        let p = Point::<f64>::new(10.0, 20.0);

        // Checked addition
        let result = p.checked_add_vec(5.0, 10.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().x, 15.0);
        assert_eq!(result.unwrap().y, 30.0);

        // Checked multiplication
        let result = p.checked_mul(2.0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().x, 20.0);
        assert_eq!(result.unwrap().y, 40.0);
    }

    #[test]
    fn test_point_utility_traits() {
        use crate::{Along, Axis, Half, IsZero};

        // Test Along trait
        let p = Point::<f64>::new(10.0, 20.0);
        assert_eq!(p.along(Axis::Horizontal), 10.0);
        assert_eq!(p.along(Axis::Vertical), 20.0);

        let modified = p.apply_along(Axis::Horizontal, |x| x * 2.0);
        assert_eq!(modified.x, 20.0);
        assert_eq!(modified.y, 20.0);

        // Test Half trait
        let half_p = p.half();
        assert_eq!(half_p.x, 5.0);
        assert_eq!(half_p.y, 10.0);

        // Test negation (using std::ops::Neg)
        let neg_p = -p;
        assert_eq!(neg_p.x, -10.0);
        assert_eq!(neg_p.y, -20.0);

        // Test IsZero trait
        let zero = Point::<f64>::new(0.0, 0.0);
        assert!(zero.is_zero());
        assert!(!p.is_zero());
    }
}
