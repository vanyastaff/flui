//! Size type for 2D dimensions.
//!
//! API design inspired by kurbo, glam, and Flutter.
use std::{
    fmt::{self, Display},
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign},
};

use super::{
    Point, Vec2,
    traits::{Along, Axis, FloatUnit, Half, IsZero, NumericUnit, Unit},
};

/// A 2D size with width and height.
///
/// Generic over unit type `T`. Common usage:
/// - `Size` - UI dimensions
/// - `DeviceSize` - Screen dimensions
///
/// Display format: `{width}×{height}` (e.g. `800px×600px`).
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Size;
///
/// let ui_size = Size::<f64>::new(800.0, 600.0);
/// assert_eq!(ui_size.area(), 480_000.0);
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Size<T: Unit = f64> {
    /// Width dimension.
    pub width: T,
    /// Height dimension.
    pub height: T,
}

// ============================================================================
// Constants (f64 only for backwards compatibility)
// ============================================================================

impl Size<f64> {
    /// Zero size (0, 0).
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Infinite size.
    pub const INFINITY: Self = Self::new(f64::INFINITY, f64::INFINITY);

    /// NaN size.
    pub const NAN: Self = Self::new(f64::NAN, f64::NAN);
}

// ============================================================================
// Debug implementation
// ============================================================================

// ============================================================================
// Basic Constructors (generic)
// ============================================================================

impl<T: Unit> Size<T> {
    /// Creates a size with the given width and height.
    #[inline]
    #[must_use]
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }

    /// Creates a size with the same value for width and height.
    #[inline]
    #[must_use]
    pub const fn splat(value: T) -> Self {
        Self {
            width: value,
            height: value,
        }
    }
}

// ============================================================================
// Size-specific operations (generic with NumericUnit)
// ============================================================================

impl<T: Unit> Size<T> {
    /// Creates a square size with the given side length.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let s = Size::<f64>::square(10.0);
    /// assert_eq!(s.width, 10.0);
    /// assert_eq!(s.height, 10.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn square(side: T) -> Self {
        Self {
            width: side,
            height: side,
        }
    }
}

impl<T: NumericUnit> Size<T> {
    /// Component-wise minimum.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let s1 = Size::<f64>::new(100.0, 50.0);
    /// let s2 = Size::<f64>::new(80.0, 60.0);
    /// let result = s1.min(s2);
    /// assert_eq!(result, Size::<f64>::new(80.0, 50.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self {
            width: NumericUnit::min(self.width, other.width),
            height: NumericUnit::min(self.height, other.height),
        }
    }

    /// Component-wise maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let s1 = Size::<f64>::new(100.0, 50.0);
    /// let s2 = Size::<f64>::new(80.0, 60.0);
    /// let result = s1.max(s2);
    /// assert_eq!(result, Size::<f64>::new(100.0, 60.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            width: NumericUnit::max(self.width, other.width),
            height: NumericUnit::max(self.height, other.height),
        }
    }
}

impl<T: NumericUnit> Size<T>
where
    T: Into<f64> + FloatUnit,
{
    /// Returns true if width or height is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let s1 = Size::<f64>::new(0.0, 10.0);
    /// assert!(s1.is_empty());
    ///
    /// let s2 = Size::<f64>::new(10.0, 10.0);
    /// assert!(!s2.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_empty(self) -> bool
    where
        T: IsZero,
    {
        self.width.is_zero() || self.height.is_zero()
    }

    /// Returns the area (width * height).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let s = Size::<f64>::new(10.0, 20.0);
    /// assert_eq!(s.area(), 200.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn area(self) -> f64 {
        let w: f64 = self.width.into();
        let h: f64 = self.height.into();
        w * h
    }

    /// Returns the aspect ratio (width / height).
    ///
    /// Returns 0.0 if height is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let s = Size::<f64>::new(16.0, 9.0);
    /// assert!((s.aspect_ratio() - 1.777).abs() < 0.01);
    /// ```
    #[inline]
    #[must_use]
    pub fn aspect_ratio(self) -> f64 {
        let w: f64 = self.width.into();
        let h: f64 = self.height.into();
        if h == 0.0 { 0.0 } else { w / h }
    }

    /// Returns the center point (half width, half height).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::{Point, Size};
    ///
    /// let s = Size::<f64>::new(100.0, 200.0);
    /// let c = s.center();
    /// assert_eq!(c, Point::<f64>::new(50.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn center(self) -> Point<T>
    where
        T: Half,
    {
        Point {
            x: self.width.half(),
            y: self.height.half(),
        }
    }

    /// Checks if this size contains a point.
    ///
    /// The point is considered inside if:
    /// - 0 <= x <= width
    /// - 0 <= y <= height
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::{Point, Size};
    ///
    /// let s = Size::<f64>::new(10.0, 20.0);
    /// assert!(s.contains(Point::<f64>::new(5.0, 10.0)));
    /// assert!(!s.contains(Point::<f64>::new(15.0, 10.0)));
    /// ```
    #[inline]
    #[must_use]
    pub fn contains(self, point: Point<T>) -> bool {
        let w: f64 = self.width.into();
        let h: f64 = self.height.into();
        let x: f64 = point.x.into();
        let y: f64 = point.y.into();

        x >= 0.0 && x <= w && y >= 0.0 && y <= h
    }
}

// ============================================================================
// Conversions (generic)
// ============================================================================

impl<T: Unit> Size<T> {
    /// Casts this size to a different unit type.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let size_px = Size::new(100.0, 200.0);
    /// let size_f32: Size = size_px.cast();
    /// assert_eq!(size_f32.width, 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn cast<U: Unit>(self) -> Size<U>
    where
        T: Into<U>,
    {
        Size {
            width: self.width.into(),
            height: self.height.into(),
        }
    }
}

impl<T: NumericUnit> Size<T>
where
    T: Into<f64>,
{
    /// Converts to a size with f64 components.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let size = Size::new(100.0, 200.0);
    /// let f32_size = size.to_f32();
    /// assert_eq!(f32_size.width, 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Size<f64> {
        Size {
            width: self.width.into(),
            height: self.height.into(),
        }
    }

    /// Converts to an array [width, height].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let s = Size::<f64>::new(100.0, 200.0);
    /// assert_eq!(s.to_array(), [100.0, 200.0]);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f64; 2] {
        [self.width.into(), self.height.into()]
    }

    /// Converts to a Vec2 with the same components.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::{Size, Vec2};
    ///
    /// let s = Size::<f64>::new(100.0, 200.0);
    /// let v = s.to_vec2();
    /// assert_eq!(v, Vec2::<f64>::new(100.0, 200.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn to_vec2(self) -> Vec2<T> {
        Vec2 {
            x: self.width,
            y: self.height,
        }
    }
}

// From Point<T> for Size<T>
impl<T: Unit> From<Point<T>> for Size<T> {
    #[inline]
    fn from(p: Point<T>) -> Self {
        Size {
            width: p.x,
            height: p.y,
        }
    }
}

// From Vec2<T> for Size<T>
impl<T: Unit> From<Vec2<T>> for Size<T> {
    #[inline]
    fn from(v: Vec2<T>) -> Self {
        Size {
            width: v.x,
            height: v.y,
        }
    }
}

// ============================================================================
// Additional accessors (generic)
// ============================================================================

impl<T: Unit> Size<T> {
    /// Returns a size with a new width value.
    #[inline]
    #[must_use]
    pub fn with_width(self, width: T) -> Self {
        Self {
            width,
            height: self.height,
        }
    }

    /// Returns a size with a new height value.
    #[inline]
    #[must_use]
    pub fn with_height(self, height: T) -> Self {
        Self {
            width: self.width,
            height,
        }
    }

    /// Transposes the size (swaps width and height).
    #[inline]
    #[must_use]
    pub fn transpose(self) -> Self {
        Self::new(self.height, self.width)
    }

    /// Swaps width and height (alias for transpose).
    #[inline]
    #[must_use]
    pub fn swap(self) -> Self {
        self.transpose()
    }
}

// ============================================================================
// f64-specific operations
// ============================================================================

impl Size<f64> {
    /// Checks if width or height is zero or negative.
    #[inline]
    #[must_use]
    pub fn is_zero_area(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Returns the smaller of width or height.
    #[inline]
    #[must_use]
    pub fn min_side(self) -> f64 {
        self.width.min(self.height)
    }

    /// Returns the larger of width or height.
    #[inline]
    #[must_use]
    pub fn max_side(self) -> f64 {
        self.width.max(self.height)
    }

    /// Clamps both dimensions between min and max values.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }

    /// Rounds both dimensions to nearest integer.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.width.round(), self.height.round())
    }

    /// Rounds both dimensions up.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.width.ceil(), self.height.ceil())
    }

    /// Rounds both dimensions down.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.width.floor(), self.height.floor())
    }

    /// Truncates both dimensions toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self::new(self.width.trunc(), self.height.trunc())
    }

    /// Expands both dimensions away from zero.
    #[inline]
    #[must_use]
    pub fn expand(self) -> Self {
        Self::new(
            if self.width >= 0.0 {
                self.width.ceil()
            } else {
                self.width.floor()
            },
            if self.height >= 0.0 {
                self.height.ceil()
            } else {
                self.height.floor()
            },
        )
    }

    /// Checks if both dimensions are finite (not infinity or NaN).
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.width.is_finite() && self.height.is_finite()
    }

    /// Checks if either dimension is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(self) -> bool {
        self.width.is_nan() || self.height.is_nan()
    }

    /// Checks if both dimensions are positive.
    #[inline]
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    /// Linearly interpolates between two sizes.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        Self::new(
            self.width + (other.width - self.width) * t,
            self.height + (other.height - self.height) * t,
        )
    }

    /// Scales to fit within bounds while maintaining aspect ratio.
    ///
    /// Returns the largest size that fits completely within `bounds`.
    #[inline]
    #[must_use]
    pub fn fit_within(self, bounds: Self) -> Self {
        if self.is_zero_area() || bounds.is_zero_area() {
            return Self::ZERO;
        }
        let scale = (bounds.width / self.width).min(bounds.height / self.height);
        Self::new(self.width * scale, self.height * scale)
    }

    /// Scales to fill bounds while maintaining aspect ratio.
    ///
    /// Returns the smallest size that completely covers `bounds`.
    #[inline]
    #[must_use]
    pub fn fill_bounds(self, bounds: Self) -> Self {
        if self.is_zero_area() || bounds.is_zero_area() {
            return Self::ZERO;
        }
        let scale = (bounds.width / self.width).max(bounds.height / self.height);
        Self::new(self.width * scale, self.height * scale)
    }

    /// Adjusts height to match the given aspect ratio (width / height).
    #[inline]
    #[must_use]
    pub fn with_aspect_ratio(self, ratio: f64) -> Self {
        if ratio <= 0.0 {
            self
        } else {
            Self::new(self.width, self.width / ratio)
        }
    }

    /// Computes the perimeter (2 * (width + height)).
    #[inline]
    #[must_use]
    pub fn perimeter(self) -> f64 {
        (self.width + self.height) * 2.0
    }

    /// Computes the diagonal length (Pythagorean theorem).
    #[inline]
    #[must_use]
    pub fn diagonal(self) -> f64 {
        self.width.hypot(self.height)
    }

    /// Returns a size scaled uniformly to the given maximum dimension.
    ///
    /// Scales uniformly to fit within the given maximum dimension.
    #[inline]
    #[must_use]
    pub fn scale_to_max(self, max: f64) -> Self {
        let w = self.width;
        let h = self.height;
        if w <= 0.0 || h <= 0.0 || max <= 0.0 {
            return Self::ZERO;
        }
        let scale = (max / w).min(max / h);
        Self::new(w * scale, h * scale)
    }

    /// Checks if the size is valid (finite and non-negative).
    #[inline]
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.is_finite() && self.width >= 0.0 && self.height >= 0.0
    }

    /// Returns a size with absolute values of both dimensions.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self::new(self.width.abs(), self.height.abs())
    }

    /// Returns the sign of each dimension.
    #[inline]
    #[must_use]
    pub fn signum(self) -> Self {
        Self::new(self.width.signum(), self.height.signum())
    }
}

// ============================================================================
// Generic map function
// ============================================================================

impl<T: Unit> Size<T> {
    /// Maps a function over both dimensions.
    #[inline]
    #[must_use]
    pub fn map<U>(self, f: impl Fn(T) -> U) -> Size<U>
    where
        U: Unit,
    {
        Size {
            width: f(self.width),
            height: f(self.height),
        }
    }
}

// ============================================================================
// Arithmetic operators (generic with NumericUnit)
// ============================================================================

impl<T: NumericUnit> Add for Size<T> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            width: self.width.add(rhs.width),
            height: self.height.add(rhs.height),
        }
    }
}

impl<T: NumericUnit> AddAssign for Size<T> {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.width = self.width.add(rhs.width);
        self.height = self.height.add(rhs.height);
    }
}

impl<T: NumericUnit> Sub for Size<T> {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            width: self.width.sub(rhs.width),
            height: self.height.sub(rhs.height),
        }
    }
}

impl<T: NumericUnit> SubAssign for Size<T> {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.width = self.width.sub(rhs.width);
        self.height = self.height.sub(rhs.height);
    }
}

// Generic Mul/Div for any Rhs that T supports
impl<T, Rhs> Mul<Rhs> for Size<T>
where
    T: Unit + Mul<Rhs, Output = T>,
    Rhs: Copy,
{
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Rhs) -> Self::Output {
        Self {
            width: self.width * rhs,
            height: self.height * rhs,
        }
    }
}

impl<T, Rhs> Div<Rhs> for Size<T>
where
    T: Unit + Div<Rhs, Output = T>,
    Rhs: Copy,
{
    type Output = Self;

    #[inline]
    fn div(self, rhs: Rhs) -> Self::Output {
        Self {
            width: self.width / rhs,
            height: self.height / rhs,
        }
    }
}

impl<T, Rhs> MulAssign<Rhs> for Size<T>
where
    T: Unit + MulAssign<Rhs>,
    Rhs: Copy,
{
    #[inline]
    fn mul_assign(&mut self, rhs: Rhs) {
        self.width *= rhs;
        self.height *= rhs;
    }
}

impl<T, Rhs> DivAssign<Rhs> for Size<T>
where
    T: Unit + DivAssign<Rhs>,
    Rhs: Copy,
{
    #[inline]
    fn div_assign(&mut self, rhs: Rhs) {
        self.width /= rhs;
        self.height /= rhs;
    }
}

// Reverse multiplication for f64 * Size
impl<T: NumericUnit> Mul<Size<T>> for f64
where
    T: Mul<f64, Output = T>,
{
    type Output = Size<T>;

    #[inline]
    fn mul(self, rhs: Size<T>) -> Self::Output {
        rhs * self
    }
}

// ============================================================================
// Conversions for f64 (backwards compatibility)
// ============================================================================

impl From<(f64, f64)> for Size<f64> {
    #[inline]
    fn from((width, height): (f64, f64)) -> Self {
        Self::new(width, height)
    }
}

impl From<[f64; 2]> for Size<f64> {
    #[inline]
    fn from([width, height]: [f64; 2]) -> Self {
        Self::new(width, height)
    }
}

impl From<Size<f64>> for (f64, f64) {
    #[inline]
    fn from(s: Size<f64>) -> Self {
        (s.width, s.height)
    }
}

impl From<Size<f64>> for [f64; 2] {
    #[inline]
    fn from(s: Size<f64>) -> Self {
        [s.width, s.height]
    }
}

// ============================================================================
// Display (generic)
// ============================================================================

impl<T: Unit + Display> Display for Size<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

// ============================================================================
// Default (generic)
// ============================================================================

impl<T: Unit> Default for Size<T> {
    #[inline]
    fn default() -> Self {
        Self {
            width: T::zero(),
            height: T::zero(),
        }
    }
}

// ============================================================================
// Convenience function (f64 only)
// ============================================================================

/// Convenience function to create a Pixels size from width and height floats.
#[inline]
#[must_use]
pub const fn size(width: f64, height: f64) -> Size<f64> {
    Size::new(width, height)
}

// ============================================================================
// Along trait - Axis-based access (generic)
// ============================================================================

impl<T: Unit> Along for Size<T> {
    type Unit = T;

    #[inline]
    fn along(&self, axis: Axis) -> Self::Unit {
        match axis {
            Axis::Horizontal => self.width,
            Axis::Vertical => self.height,
        }
    }

    #[inline]
    fn apply_along(&self, axis: Axis, f: impl FnOnce(Self::Unit) -> Self::Unit) -> Self {
        match axis {
            Axis::Horizontal => Self::new(f(self.width), self.height),
            Axis::Vertical => Self::new(self.width, f(self.height)),
        }
    }
}

// ============================================================================
// Half trait - Compute half value
// ============================================================================

impl<T: Unit> super::traits::Half for Size<T>
where
    T: super::traits::Half,
{
    #[inline]
    fn half(self) -> Self {
        Self {
            width: self.width.half(),
            height: self.height.half(),
        }
    }
}

// ============================================================================
// IsZero trait - Zero check
// ============================================================================

impl<T: Unit> super::traits::IsZero for Size<T>
where
    T: super::traits::IsZero,
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.width.is_zero() && self.height.is_zero()
    }
}

// ============================================================================
// Double trait - Double the value
// ============================================================================

impl<T: Unit> super::traits::Double for Size<T>
where
    T: super::traits::Double,
{
    #[inline]
    fn double(self) -> Self {
        Self {
            width: self.width.double(),
            height: self.height.double(),
        }
    }
}

// ============================================================================
// ApproxEq trait - Approximate equality
// ============================================================================

impl<T: Unit> super::traits::ApproxEq for Size<T>
where
    T: super::traits::ApproxEq,
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        self.width.approx_eq_eps(&other.width, epsilon)
            && self.height.approx_eq_eps(&other.height, epsilon)
    }
}

// ============================================================================
// Sum trait - Iterator summing
// ============================================================================

impl<T> std::iter::Sum for Size<T>
where
    T: NumericUnit,
{
    #[inline]
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Size::new(T::zero(), T::zero()), |acc, s| {
            Size::new(T::add(acc.width, s.width), T::add(acc.height, s.height))
        })
    }
}

impl<'a, T> std::iter::Sum<&'a Size<T>> for Size<T>
where
    T: NumericUnit,
{
    #[inline]
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Size::new(T::zero(), T::zero()), |acc, s| {
            Size::new(T::add(acc.width, s.width), T::add(acc.height, s.height))
        })
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Size<f64> {
    /// Scales the size by a given factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Size;
    ///
    /// let size = Size::new(100.0, 200.0);
    /// let scaled = size.scale(2.0); // 2x Retina display
    /// ```
    #[inline]
    #[must_use]
    pub fn scale(self, factor: f64) -> Size<f64> {
        Size {
            width: self.width * factor,
            height: self.height * factor,
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
        let s = Size::new(800.0, 600.0);
        assert_eq!(s.width, 800.0);
        assert_eq!(s.height, 600.0);

        assert_eq!(Size::splat(10.0), Size::new(10.0, 10.0));
    }

    #[test]
    fn test_constants() {
        assert_eq!(Size::ZERO, Size::new(0.0, 0.0));
        assert!(Size::INFINITY.width.is_infinite());
        assert!(Size::NAN.is_nan());
    }

    #[test]
    fn test_dimensions() {
        let s = Size::new(100.0, 50.0);
        assert_eq!(s.area(), 5000.0);
        assert_eq!(s.min_side(), 50.0);
        assert_eq!(s.max_side(), 100.0);
        assert_eq!(s.aspect_ratio(), 2.0);
    }

    #[test]
    fn test_zero_checks() {
        assert!(Size::ZERO.is_zero());
        assert!(Size::ZERO.is_zero_area());
        assert!(Size::new(0.0, 100.0).is_zero_area());
        assert!(Size::new(-5.0, 100.0).is_zero_area());
        assert!(!Size::new(10.0, 20.0).is_zero_area());
    }

    #[test]
    fn test_min_max_clamp() {
        let s1 = Size::new(100.0, 50.0);
        let s2 = Size::new(80.0, 60.0);

        assert_eq!(s1.min(s2), Size::new(80.0, 50.0));
        assert_eq!(s1.max(s2), Size::new(100.0, 60.0));

        let s = Size::new(150.0, 30.0);
        let clamped = s.clamp(Size::splat(50.0), Size::splat(100.0));
        assert_eq!(clamped, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_transpose() {
        let s = Size::new(100.0, 50.0);
        assert_eq!(s.transpose(), Size::new(50.0, 100.0));
    }

    #[test]
    fn test_rounding() {
        let s = Size::new(10.6, 20.3);
        assert_eq!(s.round(), Size::new(11.0, 20.0));
        assert_eq!(s.ceil(), Size::new(11.0, 21.0));
        assert_eq!(s.floor(), Size::new(10.0, 20.0));
    }

    #[test]
    fn test_validation() {
        assert!(Size::new(10.0, 20.0).is_finite());
        assert!(!Size::INFINITY.is_finite());
        assert!(Size::NAN.is_nan());
        assert!(Size::new(10.0, 20.0).is_positive());
        assert!(!Size::new(-5.0, 20.0).is_positive());
    }

    #[test]
    fn test_lerp() {
        let a = Size::ZERO;
        let b = Size::new(100.0, 200.0);

        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 0.5), Size::new(50.0, 100.0));
        assert_eq!(a.lerp(b, 1.0), b);
    }

    #[test]
    fn test_fit_fill() {
        let image = Size::new(1920.0, 1080.0); // 16:9
        let bounds = Size::new(800.0, 600.0); // 4:3

        let fitted = image.fit_within(bounds);
        assert!(fitted.width <= bounds.width + 0.01);
        assert!(fitted.height <= bounds.height + 0.01);

        let filled = image.fill_bounds(bounds);
        assert!(filled.width >= bounds.width - 0.01);
        assert!(filled.height >= bounds.height - 0.01);
    }

    #[test]
    fn test_aspect_ratio_set() {
        let s = Size::new(1920.0, 0.0);
        let adjusted = s.with_aspect_ratio(16.0 / 9.0);
        assert_eq!(adjusted.width, 1920.0);
        assert!((adjusted.height - 1080.0).abs() < 0.1);
    }

    #[test]
    fn test_operators() {
        let s1 = Size::new(100.0, 50.0);
        let s2 = Size::new(30.0, 20.0);

        assert_eq!(s1 + s2, Size::new(130.0, 70.0));
        assert_eq!(s1 - s2, Size::new(70.0, 30.0));
        assert_eq!(s1 * 2.0, Size::new(200.0, 100.0));
        assert_eq!(2.0 * s1, Size::new(200.0, 100.0));
        assert_eq!(s1 / 2.0, Size::new(50.0, 25.0));
    }

    #[test]
    fn test_assign_operators() {
        let mut s = Size::new(100.0, 50.0);

        s += Size::new(10.0, 5.0);
        assert_eq!(s, Size::new(110.0, 55.0));

        s -= Size::new(10.0, 5.0);
        assert_eq!(s, Size::new(100.0, 50.0));

        s *= 2.0;
        assert_eq!(s, Size::new(200.0, 100.0));

        s /= 2.0;
        assert_eq!(s, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_conversions() {
        let s = Size::new(100.0, 50.0);

        let from_tuple: Size<f64> = (100.0, 50.0).into();
        let from_array: Size<f64> = [100.0, 50.0].into();
        assert_eq!(from_tuple, s);
        assert_eq!(from_array, s);

        let to_tuple: (f64, f64) = s.into();
        let to_array: [f64; 2] = s.into();
        assert_eq!(to_tuple, (100.0, 50.0));
        assert_eq!(to_array, [100.0, 50.0]);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Size::new(800.0, 600.0)), "800×600");
    }

    #[test]
    fn test_convenience_fn() {
        assert_eq!(size(100.0, 50.0), Size::new(100.0, 50.0));
    }
}

// ============================================================================
// Typed tests (generic with unit types)
// ============================================================================

#[cfg(test)]
mod typed_tests {
    use super::*;

    #[test]
    fn test_size_new() {
        let s = Size::<f64>::new(100.0, 200.0);
        assert_eq!(s.width, 100.0);
        assert_eq!(s.height, 200.0);
    }

    #[test]
    fn test_size_square() {
        let s = Size::square(10.0);
        assert_eq!(s.width, 10.0);
        assert_eq!(s.height, 10.0);
    }

    #[test]
    fn test_size_area_aspect() {
        let s = Size::new(10.0, 20.0);
        assert_eq!(s.area(), 200.0);
        assert_eq!(s.aspect_ratio(), 0.5);
    }

    #[test]
    fn test_size_is_empty() {
        let s1 = Size::new(0.0, 10.0);
        assert!(s1.is_empty());

        let s2 = Size::new(10.0, 10.0);
        assert!(!s2.is_empty());
    }

    #[test]
    fn test_size_contains() {
        let s = Size::new(10.0, 20.0);
        let p1 = Point::new(5.0, 10.0);
        assert!(s.contains(p1));

        let p2 = Point::new(15.0, 10.0);
        assert!(!s.contains(p2));
    }

    #[test]
    fn test_size_arithmetic() {
        let s1 = Size::<f64>::new(10.0, 20.0);
        let s2 = Size::<f64>::new(5.0, 10.0);

        let s3 = s1 + s2;
        assert_eq!(s3.width, 15.0);

        let s4 = s1 * 2.0;
        assert_eq!(s4.width, 20.0);
    }

    #[test]
    fn test_size_center() {
        let s = Size::new(100.0, 200.0);
        let c = s.center();
        assert_eq!(c.x, 50.0);
        assert_eq!(c.y, 100.0);
    }

    #[test]
    fn test_size_conversions() {
        let s = Size::<f64>::new(100.0, 200.0);
        let arr = s.to_array();
        assert_eq!(arr, [100.0, 200.0]);

        let v = s.to_vec2();
        assert_eq!(v.x, 100.0);
        assert_eq!(v.y, 200.0);
    }

    #[test]
    fn test_from_point_vec2() {
        let p = Point::new(10.0, 20.0);
        let s: Size<f64> = p.into();
        assert_eq!(s.width, 10.0);
        assert_eq!(s.height, 20.0);

        let v = Vec2::new(30.0, 40.0);
        let s2: Size<f64> = v.into();
        assert_eq!(s2.width, 30.0);
        assert_eq!(s2.height, 40.0);
    }

    #[test]
    fn test_size_min_max_generic() {
        let s1 = Size::<f64>::new(100.0, 50.0);
        let s2 = Size::<f64>::new(80.0, 60.0);

        let min = s1.min(s2);
        assert_eq!(min.width, 80.0);
        assert_eq!(min.height, 50.0);

        let max = s1.max(s2);
        assert_eq!(max.width, 100.0);
        assert_eq!(max.height, 60.0);
    }

    #[test]
    fn test_size_utility_traits() {
        use crate::geometry::{Along, ApproxEq, Axis, Double, Half};

        // Test Along trait
        let s = Size::<f64>::new(100.0, 200.0);
        assert_eq!(s.along(Axis::Horizontal), 100.0);
        assert_eq!(s.along(Axis::Vertical), 200.0);

        let modified = s.apply_along(Axis::Horizontal, |w| w * 2.0);
        assert_eq!(modified.width, 200.0);
        assert_eq!(modified.height, 200.0);

        // Test Half trait
        let half_s = s.half();
        assert_eq!(half_s.width, 50.0);
        assert_eq!(half_s.height, 100.0);

        // Test IsZero trait
        let zero = Size::<f64>::new(0.0, 0.0);
        assert!(zero.is_zero());
        assert!(!s.is_zero());

        // Test Double trait
        let doubled = s.double();
        assert_eq!(doubled.width, 200.0);
        assert_eq!(doubled.height, 400.0);

        // Test ApproxEq trait
        let s2 = Size::<f64>::new(100.0 + 1e-8, 200.0 - 1e-8);
        assert!(s.approx_eq_eps(&s2, 1e-6));
    }

    #[test]
    fn test_size_abs_signum() {
        // Test abs and signum methods with Pixels
        let s_px = Size::new(-10.0, 20.0);
        let abs_s = s_px.abs();
        assert_eq!(abs_s.width, 10.0);
        assert_eq!(abs_s.height, 20.0);

        let signum_s = s_px.signum();
        assert_eq!(signum_s.width, -1.0);
        assert_eq!(signum_s.height, 1.0);
    }

    #[test]
    fn test_size_swap() {
        let s = Size::new(100.0, 50.0);
        let swapped = s.swap();
        assert_eq!(swapped.width, 50.0);
        assert_eq!(swapped.height, 100.0);
    }

    #[test]
    fn test_size_perimeter_diagonal() {
        let s = Size::new(3.0, 4.0);
        assert_eq!(s.perimeter(), 14.0);
        assert_eq!(s.diagonal(), 5.0); // 3-4-5 triangle
    }

    #[test]
    fn test_size_scale_to_max() {
        let s = Size::new(200.0, 100.0);
        let scaled = s.scale_to_max(50.0);
        assert_eq!(scaled.width, 50.0);
        assert_eq!(scaled.height, 25.0);
    }

    #[test]
    fn test_size_is_valid() {
        assert!(Size::new(10.0, 20.0).is_valid());
        assert!(!Size::new(-10.0, 20.0).is_valid());
        assert!(!Size::<f64>::INFINITY.is_valid());
        assert!(!Size::<f64>::NAN.is_valid());
    }

    #[test]
    fn test_size_sum_iterator() {
        let sizes = vec![
            Size::new(10.0, 20.0),
            Size::new(30.0, 40.0),
            Size::new(50.0, 60.0),
        ];
        let total: Size<f64> = sizes.iter().sum();
        assert_eq!(total.width, 90.0);
        assert_eq!(total.height, 120.0);

        let total_owned: Size<f64> = sizes.into_iter().sum();
        assert_eq!(total_owned.width, 90.0);
        assert_eq!(total_owned.height, 120.0);
    }
}
