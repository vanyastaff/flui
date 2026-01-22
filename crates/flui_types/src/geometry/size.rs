//! Size type for 2D dimensions.
//!
//! API design inspired by kurbo, glam, and Flutter.

use std::fmt::{self, Debug, Display};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

use super::traits::{Along, Axis, Half, IsZero, NumericUnit, Unit};
use super::{Point, Vec2};

/// A 2D size with width and height.
///
/// Generic over unit type `T`. Common usage:
/// - `Size<Pixels>` - UI dimensions
/// - `Size<DevicePixels>` - Screen dimensions
/// - `Size<f32>` - Normalized/dimensionless size
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Size, px, Pixels};
///
/// let ui_size = Size::<Pixels>::new(px(800.0), px(600.0));
/// let normalized = Size::<f32>::new(1.0, 1.0);
/// assert_eq!(normalized.area(), 1.0);
/// ```
#[derive(Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Size<T: Unit> {
    /// Width dimension.
    pub width: T,
    /// Height dimension.
    pub height: T,
}

// ============================================================================
// Constants (f32 only for backwards compatibility)
// ============================================================================

impl Size<f32> {
    /// Zero size (0, 0).
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Infinite size.
    pub const INFINITY: Self = Self::new(f32::INFINITY, f32::INFINITY);

    /// NaN size.
    pub const NAN: Self = Self::new(f32::NAN, f32::NAN);
}

// ============================================================================
// Debug implementation
// ============================================================================

impl<T: Unit + Debug> Debug for Size<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Size")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

// ============================================================================
// Basic Constructors (generic)
// ============================================================================

impl<T: Unit> Size<T> {
    /// Creates a new size.
    #[inline]
    #[must_use]
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }

    /// Creates a square size (width == height).
    #[inline]
    #[must_use]
    pub fn splat(value: T) -> Self {
        Self {
            width: value,
            height: value,
        }
    }
}

// ============================================================================
// Size-specific operations (generic with NumericUnit)
// ============================================================================

impl<T: NumericUnit> Size<T>
where
    T: Into<f32> + From<f32>,
{
    /// Creates a square size with the given side length.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Size;
    ///
    /// let s = Size::<f32>::square(10.0);
    /// assert_eq!(s.width, 10.0);
    /// assert_eq!(s.height, 10.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn square(side: T) -> Self {
        Self {
            width: side,
            height: side,
        }
    }

    /// Returns true if width or height is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Size;
    ///
    /// let s1 = Size::<f32>::new(0.0, 10.0);
    /// assert!(s1.is_empty());
    ///
    /// let s2 = Size::<f32>::new(10.0, 10.0);
    /// assert!(!s2.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool
    where
        T: IsZero,
    {
        self.width.is_zero() || self.height.is_zero()
    }

    /// Returns the area (width × height).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Size;
    ///
    /// let s = Size::<f32>::new(10.0, 20.0);
    /// assert_eq!(s.area(), 200.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        let w: f32 = self.width.into();
        let h: f32 = self.height.into();
        w * h
    }

    /// Returns the aspect ratio (width / height).
    ///
    /// Returns 0.0 if height is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Size;
    ///
    /// let s = Size::<f32>::new(16.0, 9.0);
    /// assert!((s.aspect_ratio() - 1.777).abs() < 0.01);
    /// ```
    #[inline]
    #[must_use]
    pub fn aspect_ratio(&self) -> f32 {
        let w: f32 = self.width.into();
        let h: f32 = self.height.into();
        if h != 0.0 {
            w / h
        } else {
            0.0
        }
    }

    /// Returns the center point (half width, half height).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Size, Point};
    ///
    /// let s = Size::<f32>::new(100.0, 200.0);
    /// let c = s.center();
    /// assert_eq!(c, Point::<f32>::new(50.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn center(&self) -> Point<T>
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
    /// use flui_types::geometry::{Size, Point};
    ///
    /// let s = Size::<f32>::new(10.0, 20.0);
    /// assert!(s.contains(Point::<f32>::new(5.0, 10.0)));
    /// assert!(!s.contains(Point::<f32>::new(15.0, 10.0)));
    /// ```
    #[inline]
    #[must_use]
    pub fn contains(&self, point: Point<T>) -> bool {
        let w: f32 = self.width.into();
        let h: f32 = self.height.into();
        let x: f32 = point.x.into();
        let y: f32 = point.y.into();

        x >= 0.0 && x <= w && y >= 0.0 && y <= h
    }

    /// Component-wise minimum.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Size;
    ///
    /// let s1 = Size::<f32>::new(100.0, 50.0);
    /// let s2 = Size::<f32>::new(80.0, 60.0);
    /// let result = s1.min(s2);
    /// assert_eq!(result, Size::<f32>::new(80.0, 50.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        let w1: f32 = self.width.into();
        let h1: f32 = self.height.into();
        let w2: f32 = other.width.into();
        let h2: f32 = other.height.into();

        Self {
            width: T::from(w1.min(w2)),
            height: T::from(h1.min(h2)),
        }
    }

    /// Component-wise maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Size;
    ///
    /// let s1 = Size::<f32>::new(100.0, 50.0);
    /// let s2 = Size::<f32>::new(80.0, 60.0);
    /// let result = s1.max(s2);
    /// assert_eq!(result, Size::<f32>::new(100.0, 60.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        let w1: f32 = self.width.into();
        let h1: f32 = self.height.into();
        let w2: f32 = other.width.into();
        let h2: f32 = other.height.into();

        Self {
            width: T::from(w1.max(w2)),
            height: T::from(h1.max(h2)),
        }
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
    /// use flui_types::geometry::{Size, px};
    ///
    /// let size_px = Size::new(px(100.0), px(200.0));
    /// let size_f32: Size<f32> = size_px.cast();
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
    T: Into<f32>,
{
    /// Converts to a size with f32 components.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Size, px};
    ///
    /// let size = Size::new(px(100.0), px(200.0));
    /// let f32_size = size.to_f32();
    /// assert_eq!(f32_size.width, 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Size<f32> {
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
    /// use flui_types::geometry::Size;
    ///
    /// let s = Size::<f32>::new(100.0, 200.0);
    /// assert_eq!(s.to_array(), [100.0, 200.0]);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f32; 2] {
        [self.width.into(), self.height.into()]
    }

    /// Converts to a Vec2 with the same components.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Size, Vec2};
    ///
    /// let s = Size::<f32>::new(100.0, 200.0);
    /// let v = s.to_vec2();
    /// assert_eq!(v, Vec2::<f32>::new(100.0, 200.0));
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
    /// Returns a new size with the width replaced.
    #[inline]
    #[must_use]
    pub fn with_width(self, width: T) -> Self {
        Self {
            width,
            height: self.height,
        }
    }

    /// Returns a new size with the height replaced.
    #[inline]
    #[must_use]
    pub fn with_height(self, height: T) -> Self {
        Self {
            width: self.width,
            height,
        }
    }

    /// Returns size with width and height swapped.
    #[inline]
    #[must_use]
    pub fn transpose(self) -> Self {
        Self::new(self.height, self.width)
    }
}

// ============================================================================
// f32-specific operations
// ============================================================================

impl Size<f32> {
    /// Returns `true` if the area is zero or negative.
    #[inline]
    #[must_use]
    pub fn is_zero_area(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Returns `true` if both dimensions are zero (or very close).
    #[inline]
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.width.abs() < f32::EPSILON && self.height.abs() < f32::EPSILON
    }

    /// Returns the smaller dimension.
    #[inline]
    #[must_use]
    pub fn min_side(self) -> f32 {
        self.width.min(self.height)
    }

    /// Returns the larger dimension.
    #[inline]
    #[must_use]
    pub fn max_side(self) -> f32 {
        self.width.max(self.height)
    }

    /// Clamp size between min and max.
    #[inline]
    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }

    /// Rounds dimensions to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.width.round(), self.height.round())
    }

    /// Rounds dimensions up.
    #[inline]
    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.width.ceil(), self.height.ceil())
    }

    /// Rounds dimensions down.
    #[inline]
    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.width.floor(), self.height.floor())
    }

    /// Rounds dimensions toward zero.
    #[inline]
    #[must_use]
    pub fn trunc(self) -> Self {
        Self::new(self.width.trunc(), self.height.trunc())
    }

    /// Rounds dimensions away from zero.
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

    /// Returns `true` if both dimensions are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.width.is_finite() && self.height.is_finite()
    }

    /// Returns `true` if either dimension is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(self) -> bool {
        self.width.is_nan() || self.height.is_nan()
    }

    /// Returns `true` if both dimensions are positive.
    #[inline]
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    /// Linear interpolation between two sizes.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.width + (other.width - self.width) * t,
            self.height + (other.height - self.height) * t,
        )
    }

    /// Scales to fit within bounds while maintaining aspect ratio.
    ///
    /// Returns the largest size that fits completely within `bounds`.
    /// Equivalent to `BoxFit::contain`.
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
    /// Equivalent to `BoxFit::cover`.
    #[must_use]
    pub fn fill_bounds(self, bounds: Self) -> Self {
        if self.is_zero_area() || bounds.is_zero_area() {
            return Self::ZERO;
        }
        let scale = (bounds.width / self.width).max(bounds.height / self.height);
        Self::new(self.width * scale, self.height * scale)
    }

    /// Adjusts height to match a specific aspect ratio.
    #[inline]
    #[must_use]
    pub fn with_aspect_ratio(self, ratio: f32) -> Self {
        if ratio <= 0.0 {
            self
        } else {
            Self::new(self.width, self.width / ratio)
        }
    }

    /// Maps the size components through a function.
    #[inline]
    #[must_use]
    pub fn map(&self, f: impl Fn(f32) -> f32) -> Size<f32> {
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

impl<T: NumericUnit> Mul<f32> for Size<T> {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            width: self.width.mul(rhs),
            height: self.height.mul(rhs),
        }
    }
}

impl<T: NumericUnit> Mul<Size<T>> for f32 {
    type Output = Size<T>;

    #[inline]
    fn mul(self, rhs: Size<T>) -> Self::Output {
        rhs * self
    }
}

impl<T: NumericUnit> MulAssign<f32> for Size<T> {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.width = self.width.mul(rhs);
        self.height = self.height.mul(rhs);
    }
}

impl<T: NumericUnit> Div<f32> for Size<T> {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self {
            width: self.width.div(rhs),
            height: self.height.div(rhs),
        }
    }
}

impl<T: NumericUnit> DivAssign<f32> for Size<T> {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.width = self.width.div(rhs);
        self.height = self.height.div(rhs);
    }
}

// ============================================================================
// Conversions for f32 (backwards compatibility)
// ============================================================================

impl From<(f32, f32)> for Size<f32> {
    #[inline]
    fn from((width, height): (f32, f32)) -> Self {
        Self::new(width, height)
    }
}

impl From<[f32; 2]> for Size<f32> {
    #[inline]
    fn from([width, height]: [f32; 2]) -> Self {
        Self::new(width, height)
    }
}

impl From<Size<f32>> for (f32, f32) {
    #[inline]
    fn from(s: Size<f32>) -> Self {
        (s.width, s.height)
    }
}

impl From<Size<f32>> for [f32; 2] {
    #[inline]
    fn from(s: Size<f32>) -> Self {
        [s.width, s.height]
    }
}

// ============================================================================
// Display (generic)
// ============================================================================

impl<T: Unit + Display> Display for Size<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

// ============================================================================
// Default (generic)
// ============================================================================

impl<T: Unit> Default for Size<T> {
    fn default() -> Self {
        Self {
            width: T::zero(),
            height: T::zero(),
        }
    }
}

// ============================================================================
// Convenience function (f32 only)
// ============================================================================

/// Shorthand for `Size::new(width, height)`.
#[inline]
#[must_use]
pub const fn size(width: f32, height: f32) -> Size<f32> {
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
    T: super::traits::Half
{
    #[inline]
    fn half(&self) -> Self {
        Self { width: self.width.half(), height: self.height.half() }
    }
}

// ============================================================================
// IsZero trait - Zero check
// ============================================================================

impl<T: Unit> super::traits::IsZero for Size<T>
where
    T: super::traits::IsZero
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.width.is_zero() && self.height.is_zero()
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Size<super::units::Pixels> {
    /// Scales the size by a given factor, producing a Size<ScaledPixels>.
    ///
    /// This is typically used to convert logical pixel sizes to scaled
    /// pixels for high-DPI displays.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Size, px};
    ///
    /// let size = Size::new(px(100.0), px(200.0));
    /// let scaled = size.scale(2.0);  // 2x Retina display
    /// ```
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Size<super::units::ScaledPixels> {
        Size {
            width: self.width.scale(factor),
            height: self.height.scale(factor),
        }
    }
}

// ============================================================================
// Specialized implementations for ScaledPixels
// ============================================================================

impl Size<super::units::ScaledPixels> {
    /// Converts to device pixels by rounding both dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Size, scaled_px};
    ///
    /// let size = Size::new(scaled_px(199.7), scaled_px(299.3));
    /// let device = size.to_device_pixels();
    /// ```
    #[inline]
    #[must_use]
    pub fn to_device_pixels(&self) -> Size<super::units::DevicePixels> {
        Size {
            width: self.width.to_device_pixels(),
            height: self.height.to_device_pixels(),
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

        let from_tuple: Size = (100.0, 50.0).into();
        let from_array: Size = [100.0, 50.0].into();
        assert_eq!(from_tuple, s);
        assert_eq!(from_array, s);

        let to_tuple: (f32, f32) = s.into();
        let to_array: [f32; 2] = s.into();
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
    use crate::geometry::{px, Pixels};

    #[test]
    fn test_size_new() {
        let s = Size::<Pixels>::new(px(100.0), px(200.0));
        assert_eq!(s.width.get(), 100.0);
        assert_eq!(s.height.get(), 200.0);
    }

    #[test]
    fn test_size_square() {
        let s = Size::<f32>::square(10.0);
        assert_eq!(s.width, 10.0);
        assert_eq!(s.height, 10.0);
    }

    #[test]
    fn test_size_area_aspect() {
        let s = Size::<f32>::new(10.0, 20.0);
        assert_eq!(s.area(), 200.0);
        assert_eq!(s.aspect_ratio(), 0.5);
    }

    #[test]
    fn test_size_is_empty() {
        let s1 = Size::<f32>::new(0.0, 10.0);
        assert!(s1.is_empty());

        let s2 = Size::<f32>::new(10.0, 10.0);
        assert!(!s2.is_empty());
    }

    #[test]
    fn test_size_contains() {
        let s = Size::<f32>::new(10.0, 20.0);
        let p1 = Point::<f32>::new(5.0, 10.0);
        assert!(s.contains(p1));

        let p2 = Point::<f32>::new(15.0, 10.0);
        assert!(!s.contains(p2));
    }

    #[test]
    fn test_size_arithmetic() {
        let s1 = Size::<Pixels>::new(px(10.0), px(20.0));
        let s2 = Size::<Pixels>::new(px(5.0), px(10.0));

        let s3 = s1 + s2;
        assert_eq!(s3.width.get(), 15.0);

        let s4 = s1 * 2.0;
        assert_eq!(s4.width.get(), 20.0);
    }

    #[test]
    fn test_size_center() {
        let s = Size::<f32>::new(100.0, 200.0);
        let c = s.center();
        assert_eq!(c.x, 50.0);
        assert_eq!(c.y, 100.0);
    }

    #[test]
    fn test_size_conversions() {
        let s = Size::<Pixels>::new(px(100.0), px(200.0));
        let arr = s.to_array();
        assert_eq!(arr, [100.0, 200.0]);

        let v = s.to_vec2();
        assert_eq!(v.x.get(), 100.0);
        assert_eq!(v.y.get(), 200.0);
    }

    #[test]
    fn test_from_point_vec2() {
        let p = Point::<f32>::new(10.0, 20.0);
        let s: Size<f32> = p.into();
        assert_eq!(s.width, 10.0);
        assert_eq!(s.height, 20.0);

        let v = Vec2::<f32>::new(30.0, 40.0);
        let s2: Size<f32> = v.into();
        assert_eq!(s2.width, 30.0);
        assert_eq!(s2.height, 40.0);
    }

    #[test]
    fn test_size_min_max_generic() {
        let s1 = Size::<Pixels>::new(px(100.0), px(50.0));
        let s2 = Size::<Pixels>::new(px(80.0), px(60.0));

        let min = s1.min(s2);
        assert_eq!(min.width.get(), 80.0);
        assert_eq!(min.height.get(), 50.0);

        let max = s1.max(s2);
        assert_eq!(max.width.get(), 100.0);
        assert_eq!(max.height.get(), 60.0);
    }

    #[test]
    fn test_size_utility_traits() {
        use crate::geometry::{Axis, Along, Half, IsZero};

        // Test Along trait
        let s = Size::<Pixels>::new(px(100.0), px(200.0));
        assert_eq!(s.along(Axis::Horizontal).0, 100.0);
        assert_eq!(s.along(Axis::Vertical).0, 200.0);

        let modified = s.apply_along(Axis::Horizontal, |w| px(w.0 * 2.0));
        assert_eq!(modified.width.0, 200.0);
        assert_eq!(modified.height.0, 200.0);

        // Test Half trait
        let half_s = s.half();
        assert_eq!(half_s.width.0, 50.0);
        assert_eq!(half_s.height.0, 100.0);

        // Test IsZero trait
        let zero = Size::<Pixels>::new(px(0.0), px(0.0));
        assert!(zero.is_zero());
        assert!(!s.is_zero());
    }
}
