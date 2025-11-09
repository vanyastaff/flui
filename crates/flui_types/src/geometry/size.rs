//! Size type for 2D dimensions

use std::fmt;
use std::ops::{Add, Div, Mul, Sub};

/// Epsilon for safe float comparisons (Rust 1.91.0 strict arithmetic)
const EPSILON: f32 = 1e-6;

/// A 2D size with width and height
///
/// Similar to Flutter's Size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// Width in logical pixels
    pub width: f32,
    /// Height in logical pixels
    pub height: f32,
}

impl Size {
    /// Zero size constant.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Infinite size constant.
    pub const INFINITY: Self = Self::new(f32::INFINITY, f32::INFINITY);

    /// Create a new size
    #[inline]
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Create a size with both dimensions set to zero
    #[inline]
    #[must_use]
    pub const fn zero() -> Self {
        Self::ZERO
    }

    /// Create a size with infinite dimensions
    #[inline]
    #[must_use]
    pub const fn infinite() -> Self {
        Self::INFINITY
    }

    /// Create a square size (width == height).
    #[inline]
    #[must_use]
    pub const fn square(size: f32) -> Self {
        Self::new(size, size)
    }

    /// Check if this size is zero
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.width.abs() < EPSILON && self.height.abs() < EPSILON
    }

    /// Check if this size has finite dimensions
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.width.is_finite() && self.height.is_finite()
    }

    /// Check if this size is empty (width or height is zero)
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Get the aspect ratio (width / height)
    #[inline]
    #[must_use]
    pub fn aspect_ratio(&self) -> f32 {
        if self.height.abs() < EPSILON {
            0.0
        } else {
            self.width / self.height
        }
    }

    /// Calculate the shortest side
    #[inline]
    #[must_use]
    pub fn shortest_side(&self) -> f32 {
        self.width.min(self.height)
    }

    /// Calculate the longest side
    #[inline]
    #[must_use]
    pub fn longest_side(&self) -> f32 {
        self.width.max(self.height)
    }

    /// Get the area (width * height)
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        self.width * self.height
    }

    /// Scale this size by a factor.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self::new(self.width * factor, self.height * factor)
    }

    /// Returns a size with width and height swapped.
    #[inline]
    #[must_use]
    pub fn flipped(&self) -> Self {
        Self::new(self.height, self.width)
    }

    /// Linearly interpolate between two sizes.
    #[inline]
    #[must_use]
    pub fn lerp(a: impl Into<Size>, b: impl Into<Size>, t: f32) -> Self {
        let a = a.into();
        let b = b.into();
        Self::new(
            a.width + (b.width - a.width) * t,
            a.height + (b.height - a.height) * t,
        )
    }

    // ===== Helper methods for rendering & layout =====

    /// Fit this size within bounds while maintaining aspect ratio.
    ///
    /// Returns the largest size that fits completely within `bounds`.
    #[must_use]
    pub fn fit_within(&self, bounds: Size) -> Size {
        if self.width.abs() < EPSILON || self.height.abs() < EPSILON {
            return Size::ZERO;
        }

        let scale = (bounds.width / self.width).min(bounds.height / self.height);
        Size::new(self.width * scale, self.height * scale)
    }

    /// Fill bounds while maintaining aspect ratio.
    ///
    /// Returns the smallest size that completely covers `bounds`.
    #[must_use]
    pub fn fill_bounds(&self, bounds: Size) -> Size {
        if self.width.abs() < EPSILON || self.height.abs() < EPSILON {
            return Size::ZERO;
        }

        let scale = (bounds.width / self.width).max(bounds.height / self.height);
        Size::new(self.width * scale, self.height * scale)
    }

    /// Expand to cover the given size (no aspect ratio constraint).
    #[inline]
    #[must_use]
    pub const fn expand_to(&self, other: Size) -> Size {
        Size::new(
            if self.width > other.width {
                self.width
            } else {
                other.width
            },
            if self.height > other.height {
                self.height
            } else {
                other.height
            },
        )
    }

    /// Shrink to fit within the given size (no aspect ratio constraint).
    #[inline]
    #[must_use]
    pub const fn shrink_to(&self, other: Size) -> Size {
        Size::new(
            if self.width < other.width {
                self.width
            } else {
                other.width
            },
            if self.height < other.height {
                self.height
            } else {
                other.height
            },
        )
    }

    /// Round components to nearest integer.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Size {
        Size::new(self.width.round(), self.height.round())
    }

    /// Floor components.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Size {
        Size::new(self.width.floor(), self.height.floor())
    }

    /// Ceil components.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Size {
        Size::new(self.width.ceil(), self.height.ceil())
    }

    /// Clamp width and height between min and max sizes.
    #[inline]
    #[must_use]
    pub fn clamp(&self, min: Size, max: Size) -> Size {
        Size::new(
            self.width.clamp(min.width, max.width),
            self.height.clamp(min.height, max.height),
        )
    }

    /// Adjust height to maintain a specific aspect ratio.
    ///
    /// Returns a new size with the same width but height adjusted to match the aspect ratio.
    /// Useful for maintaining aspect ratios when resizing images or videos.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Size;
    ///
    /// // 16:9 aspect ratio
    /// let size = Size::new(1920.0, 0.0);
    /// let adjusted = size.with_aspect_ratio(16.0 / 9.0);
    ///
    /// assert_eq!(adjusted.width, 1920.0);
    /// assert_eq!(adjusted.height, 1080.0); // 1920 / (16/9) = 1080
    /// assert!((adjusted.aspect_ratio() - 16.0 / 9.0).abs() < 0.01);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_aspect_ratio(&self, aspect_ratio: f32) -> Size {
        if aspect_ratio <= 0.0 {
            return *self;
        }
        Size::new(self.width, self.width / aspect_ratio)
    }

    /// Scale this size to fit within bounds while maintaining aspect ratio.
    ///
    /// Returns a size that fits completely inside `bounds` with the same aspect ratio.
    /// This is equivalent to `BoxFit::Contain` behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Size;
    ///
    /// // 1920x1080 image scaled to fit in 800x600
    /// let image = Size::new(1920.0, 1080.0);
    /// let bounds = Size::new(800.0, 600.0);
    /// let fitted = image.scaled_to_fit(bounds);
    ///
    /// // Should be 800x450 (maintains 16:9 aspect ratio, fits width)
    /// assert!((fitted.width - 800.0).abs() < 0.1);
    /// assert!((fitted.height - 450.0).abs() < 0.1);
    /// assert!(fitted.width <= bounds.width);
    /// assert!(fitted.height <= bounds.height);
    /// ```
    #[must_use]
    pub fn scaled_to_fit(&self, bounds: impl Into<Size>) -> Size {
        let bounds = bounds.into();

        if self.is_empty() || bounds.is_empty() {
            return Size::ZERO;
        }

        let self_aspect = self.aspect_ratio();
        let bounds_aspect = bounds.aspect_ratio();

        if self_aspect > bounds_aspect {
            // Width constrained
            Size::new(bounds.width, bounds.width / self_aspect)
        } else {
            // Height constrained
            Size::new(bounds.height * self_aspect, bounds.height)
        }
    }
}

impl Default for Size {
    fn default() -> Self {
        Self::zero()
    }
}

impl From<(f32, f32)> for Size {
    fn from((width, height): (f32, f32)) -> Self {
        Self::new(width, height)
    }
}

impl From<[f32; 2]> for Size {
    fn from([width, height]: [f32; 2]) -> Self {
        Self::new(width, height)
    }
}

impl From<Size> for (f32, f32) {
    fn from(size: Size) -> Self {
        (size.width, size.height)
    }
}

impl From<Size> for [f32; 2] {
    fn from(size: Size) -> Self {
        [size.width, size.height]
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Size({}x{})", self.width, self.height)
    }
}

// Math operators
impl Add for Size {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.width + rhs.width, self.height + rhs.height)
    }
}

impl Sub for Size {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.width - rhs.width, self.height - rhs.height)
    }
}

impl Mul<f32> for Size {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.width * rhs, self.height * rhs)
    }
}

impl Mul<Size> for f32 {
    type Output = Size;

    #[inline]
    fn mul(self, rhs: Size) -> Self::Output {
        Size::new(rhs.width * self, rhs.height * self)
    }
}

impl Div<f32> for Size {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.width / rhs, self.height / rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_zero() {
        let size = Size::zero();
        assert_eq!(size.width, 0.0);
        assert_eq!(size.height, 0.0);
        assert!(size.is_zero());
    }

    #[test]
    fn test_size_finite() {
        let size = Size::new(100.0, 50.0);
        assert!(size.is_finite());

        let infinite = Size::infinite();
        assert!(!infinite.is_finite());
    }

    #[test]
    fn test_size_aspect_ratio() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(size.aspect_ratio(), 2.0);
    }

    #[test]
    fn test_size_shortest_longest() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(size.shortest_side(), 50.0);
        assert_eq!(size.longest_side(), 100.0);
    }

    #[test]
    fn test_size_area() {
        let size = Size::new(10.0, 20.0);
        assert_eq!(size.area(), 200.0);
    }

    #[test]
    fn test_size_is_empty() {
        assert!(!Size::new(10.0, 20.0).is_empty());
        assert!(Size::new(0.0, 20.0).is_empty());
        assert!(Size::new(10.0, 0.0).is_empty());
        assert!(Size::new(-5.0, 20.0).is_empty());
    }

    #[test]
    fn test_size_conversions() {
        let size = Size::new(10.0, 20.0);

        let tuple: (f32, f32) = size.into();
        assert_eq!(tuple, (10.0, 20.0));

        let array: [f32; 2] = size.into();
        assert_eq!(array, [10.0, 20.0]);

        let from_tuple: Size = (15.0, 25.0).into();
        assert_eq!(from_tuple, Size::new(15.0, 25.0));
    }

    #[test]
    fn test_size_constants() {
        assert_eq!(Size::ZERO, Size::new(0.0, 0.0));
        assert!(Size::INFINITY.width.is_infinite());
        assert!(Size::INFINITY.height.is_infinite());
    }

    #[test]
    fn test_size_square() {
        let square = Size::square(10.0);
        assert_eq!(square.width, 10.0);
        assert_eq!(square.height, 10.0);
    }

    #[test]
    fn test_size_scale() {
        let size = Size::new(10.0, 20.0);
        let scaled = size.scale(2.0);
        assert_eq!(scaled, Size::new(20.0, 40.0));
    }

    #[test]
    fn test_size_flipped() {
        let size = Size::new(10.0, 20.0);
        let flipped = size.flipped();
        assert_eq!(flipped, Size::new(20.0, 10.0));
    }

    #[test]
    fn test_size_lerp() {
        let a = Size::new(0.0, 0.0);
        let b = Size::new(100.0, 200.0);

        assert_eq!(Size::lerp(a, b, 0.0), a);
        assert_eq!(Size::lerp(a, b, 1.0), b);
        assert_eq!(Size::lerp(a, b, 0.5), Size::new(50.0, 100.0));
    }

    #[test]
    fn test_size_math_operators() {
        let s1 = Size::new(10.0, 20.0);
        let s2 = Size::new(5.0, 8.0);

        // Addition
        assert_eq!(s1 + s2, Size::new(15.0, 28.0));

        // Subtraction
        assert_eq!(s1 - s2, Size::new(5.0, 12.0));

        // Multiplication by scalar
        assert_eq!(s1 * 2.0, Size::new(20.0, 40.0));
        assert_eq!(2.0 * s1, Size::new(20.0, 40.0));

        // Division by scalar
        assert_eq!(s1 / 2.0, Size::new(5.0, 10.0));
    }
}
