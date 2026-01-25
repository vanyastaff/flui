//! Rectangle type for bounding boxes and regions.
//!
//! API design inspired by kurbo, glam, and Flutter.
//!
//! # Type Safety
//!
//! `Rect<T>` is generic over unit type `T`, preventing accidental mixing
//! of coordinate systems.

use std::fmt;

use super::traits::{NumericUnit, Unit};
use super::units::Pixels;
use super::{Offset, Point, Size, Vec2};

/// An axis-aligned rectangle.
///
/// Generic over unit type `T`. Common usage:
/// - `Rect<f32>` - Raw coordinates (GPU-ready)
/// - `Rect<Pixels>` - Logical pixel coordinates
///
/// Defined by minimum and maximum corner points. The rectangle is valid when
/// `min.x <= max.x` and `min.y <= max.y`.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Rect, Point, Size, point, size};
///
/// // Create from origin and size
/// let rect = Rect::from_origin_size(point(0.0, 0.0), size(100.0, 50.0));
///
/// // Query properties
/// assert_eq!(rect.width(), 100.0);
/// assert_eq!(rect.height(), 50.0);
/// assert_eq!(rect.area(), 5000.0);
///
/// // Hit testing
/// assert!(rect.contains(point(50.0, 25.0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Rect<T: Unit> {
    /// Minimum corner (top-left in screen coordinates).
    pub min: Point<T>,
    /// Maximum corner (bottom-right in screen coordinates).
    pub max: Point<T>,
}

impl<T: Unit> Default for Rect<T> {
    fn default() -> Self {
        Self {
            min: Point::new(T::zero(), T::zero()),
            max: Point::new(T::zero(), T::zero()),
        }
    }
}

// ============================================================================
// Constants
// ============================================================================

impl<T: NumericUnit> Rect<T> {
    /// Creates an empty rectangle at the origin.
    #[inline]
    #[must_use]
    pub fn zero() -> Self {
        Self {
            min: Point::new(T::zero(), T::zero()),
            max: Point::new(T::zero(), T::zero()),
        }
    }
}

impl Rect<Pixels> {
    /// Empty rectangle at origin.
    pub const ZERO: Self = Self {
        min: Point::ZERO,
        max: Point::ZERO,
    };

    /// Infinite rectangle containing everything.
    pub const EVERYTHING: Self = Self {
        min: Point::NEG_INFINITY,
        max: Point::INFINITY,
    };
}

// ============================================================================
// Generic Constructors
// ============================================================================

impl<T: Unit> Rect<T> {
    /// Creates a rectangle from min and max points.
    #[inline]
    #[must_use]
    pub const fn from_min_max(min: Point<T>, max: Point<T>) -> Self {
        Self { min, max }
    }

    /// Applies a function to all coordinates of the rectangle.
    #[inline]
    #[must_use]
    pub fn map<U: Unit>(&self, f: impl Fn(T) -> U + Copy) -> Rect<U>
    where
        T: Clone + fmt::Debug + Default + PartialEq,
    {
        Rect {
            min: self.min.map(f),
            max: self.max.map(f),
        }
    }
}

// ============================================================================
// Constructors (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: PartialOrd + Clone + fmt::Debug + Default + PartialEq,
{
    /// Creates a rectangle from two points, normalizing coordinates.
    ///
    /// Points are normalized so min ≤ max.
    #[inline]
    #[must_use]
    pub fn from_points(p0: Point<T>, p1: Point<T>) -> Self {
        Self {
            min: p0.min(p1),
            max: p0.max(p1),
        }
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: std::ops::Add<Output = T>,
{
    /// Creates a rectangle from origin and size.
    #[inline]
    #[must_use]
    pub fn from_origin_size(origin: Point<T>, size: Size<T>) -> Self {
        Self {
            min: origin,
            max: Point::new(origin.x + size.width, origin.y + size.height),
        }
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: std::ops::Add<Output = T> + std::ops::Sub<Output = T> + std::ops::Div<f32, Output = T>,
{
    /// Creates a rectangle centered at a point with given size.
    #[inline]
    #[must_use]
    pub fn from_center_size(center: Point<T>, size: Size<T>) -> Self {
        let half = Vec2::new(size.width / 2.0, size.height / 2.0);
        Self {
            min: center - half,
            max: center + half,
        }
    }
}

// ============================================================================
// Pixels-specific Constructors
// ============================================================================

impl Rect<Pixels> {
    /// Creates a rectangle from raw coordinates.
    ///
    /// Note: Does not normalize — if `x0 > x1` or `y0 > y1`, the rect is inverted.
    #[inline]
    #[must_use]
    pub const fn new(x0: Pixels, y0: Pixels, x1: Pixels, y1: Pixels) -> Self {
        Self {
            min: Point::new(x0, y0),
            max: Point::new(x1, y1),
        }
    }

    /// Creates a rectangle from x, y, width, height.
    #[inline]
    #[must_use]
    pub fn from_xywh(x: Pixels, y: Pixels, width: Pixels, height: Pixels) -> Self {
        Self::new(x, y, x + width, y + height)
    }

    /// Creates a rectangle from left, top, right, bottom (Flutter-style).
    #[inline]
    #[must_use]
    pub const fn from_ltrb(left: Pixels, top: Pixels, right: Pixels, bottom: Pixels) -> Self {
        Self::new(left, top, right, bottom)
    }

    /// Creates a rectangle from left, top, width, height (Flutter-style).
    #[inline]
    #[must_use]
    pub fn from_ltwh(left: Pixels, top: Pixels, width: Pixels, height: Pixels) -> Self {
        Self::new(left, top, left + width, top + height)
    }

    /// Creates a rectangle from center point with width and height.
    #[inline]
    #[must_use]
    pub fn from_center(center: Offset<Pixels>, width: Pixels, height: Pixels) -> Self {
        let half_width = width / 2.0;
        let half_height = height / 2.0;
        Self::new(
            center.dx - half_width,
            center.dy - half_height,
            center.dx + half_width,
            center.dy + half_height,
        )
    }
}

// ============================================================================
// Accessors (Generic)
// ============================================================================

impl<T: Unit> Rect<T> {
    /// Returns the x-coordinate of the left edge.
    #[inline]
    #[must_use]
    pub fn left(&self) -> T {
        self.min.x
    }

    /// Returns the y-coordinate of the top edge.
    #[inline]
    #[must_use]
    pub fn top(&self) -> T {
        self.min.y
    }

    /// Returns the x-coordinate of the right edge.
    #[inline]
    #[must_use]
    pub fn right(&self) -> T {
        self.max.x
    }

    /// Returns the y-coordinate of the bottom edge.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> T {
        self.max.y
    }

    /// Returns the origin point (top-left corner).
    #[inline]
    #[must_use]
    pub fn origin(&self) -> Point<T> {
        self.min
    }

    /// Returns the top-left corner point.
    #[inline]
    #[must_use]
    pub fn top_left(&self) -> Point<T> {
        self.min
    }

    /// Returns the top-right corner point.
    #[inline]
    #[must_use]
    pub fn top_right(&self) -> Point<T> {
        Point::new(self.max.x, self.min.y)
    }

    /// Returns the bottom-left corner point.
    #[inline]
    #[must_use]
    pub fn bottom_left(&self) -> Point<T> {
        Point::new(self.min.x, self.max.y)
    }

    /// Returns the bottom-right corner point.
    #[inline]
    #[must_use]
    pub fn bottom_right(&self) -> Point<T> {
        self.max
    }
}

// ============================================================================
// Accessors (NumericUnit)
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: std::ops::Sub<Output = T>,
{
    /// Width of the rectangle.
    #[inline]
    #[must_use]
    pub fn width(&self) -> T {
        self.max.x - self.min.x
    }

    /// Height of the rectangle.
    #[inline]
    #[must_use]
    pub fn height(&self) -> T {
        self.max.y - self.min.y
    }

    /// Size of the rectangle.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size<T> {
        Size::new(self.width(), self.height())
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: Into<f32> + std::ops::Sub<Output = T>,
{
    /// Area of the rectangle.
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        let w: f32 = self.width().into();
        let h: f32 = self.height().into();
        w * h
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: std::ops::Add<Output = T> + std::ops::Div<f32, Output = T>,
{
    /// Center point of the rectangle.
    #[inline]
    #[must_use]
    pub fn center(&self) -> Point<T> {
        Point::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: PartialOrd,
{
    /// Returns the minimum x-coordinate (handles inverted rectangles).
    #[inline]
    #[must_use]
    pub fn min_x(&self) -> T {
        if self.min.x < self.max.x {
            self.min.x
        } else {
            self.max.x
        }
    }

    /// Returns the maximum x-coordinate (handles inverted rectangles).
    #[inline]
    #[must_use]
    pub fn max_x(&self) -> T {
        if self.min.x > self.max.x {
            self.min.x
        } else {
            self.max.x
        }
    }

    /// Returns the minimum y-coordinate (handles inverted rectangles).
    #[inline]
    #[must_use]
    pub fn min_y(&self) -> T {
        if self.min.y < self.max.y {
            self.min.y
        } else {
            self.max.y
        }
    }

    /// Returns the maximum y-coordinate (handles inverted rectangles).
    #[inline]
    #[must_use]
    pub fn max_y(&self) -> T {
        if self.min.y > self.max.y {
            self.min.y
        } else {
            self.max.y
        }
    }
}

// ============================================================================
// Validation
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: Into<f32> + std::ops::Sub<Output = T>,
{
    /// Checks if the rectangle has zero or negative area.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let w: f32 = self.width().into();
        let h: f32 = self.height().into();
        w <= 0.0 || h <= 0.0
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: Into<f32>,
{
    /// Checks if all coordinates are finite (not infinity or NaN).
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }

    /// Checks if any coordinate is NaN.
    #[inline]
    #[must_use]
    pub fn is_nan(&self) -> bool {
        self.min.is_nan() || self.max.is_nan()
    }
}

// ============================================================================
// Hit Testing & Containment
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: PartialOrd,
{
    /// Checks if the given point is inside the rectangle (inclusive).
    #[inline]
    #[must_use]
    pub fn contains(&self, point: Point<T>) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Checks if this rectangle completely contains another rectangle.
    #[inline]
    #[must_use]
    pub fn contains_rect(&self, other: &Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && self.max.x >= other.max.x
            && self.max.y >= other.max.y
    }

    /// Checks if this rectangle overlaps with another rectangle.
    #[inline]
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.max.x > other.min.x
            && self.min.x < other.max.x
            && self.max.y > other.min.y
            && self.min.y < other.max.y
    }

    /// Checks if this rectangle intersects another (alias for overlaps).
    ///
    /// This is an alias for [`overlaps`](Self::overlaps).
    #[inline]
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.overlaps(other)
    }
}

// ============================================================================
// Pixels-specific Containment (Offset)
// ============================================================================

impl Rect<Pixels> {
    /// Checks if the given offset is inside the rectangle (inclusive).
    #[inline]
    #[must_use]
    pub fn contains_offset(&self, offset: Offset<Pixels>) -> bool {
        offset.dx >= self.min.x
            && offset.dx <= self.max.x
            && offset.dy >= self.min.y
            && offset.dy <= self.max.y
    }
}

// ============================================================================
// Set Operations
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: PartialOrd + std::ops::Sub<Output = T> + Clone + fmt::Debug + Default + PartialEq,
{
    /// Returns the intersection of two rectangles, or `None` if they don't overlap.
    #[must_use]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let min_x = if self.min.x > other.min.x {
            self.min.x
        } else {
            other.min.x
        };
        let min_y = if self.min.y > other.min.y {
            self.min.y
        } else {
            other.min.y
        };
        let max_x = if self.max.x < other.max.x {
            self.max.x
        } else {
            other.max.x
        };
        let max_y = if self.max.y < other.max.y {
            self.max.y
        } else {
            other.max.y
        };

        if min_x <= max_x && min_y <= max_y {
            Some(Self {
                min: Point::new(min_x, min_y),
                max: Point::new(max_x, max_y),
            })
        } else {
            None
        }
    }

    /// Returns the smallest rectangle containing both rectangles.
    #[inline]
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// Returns the smallest rectangle containing this rectangle and a point.
    #[inline]
    #[must_use]
    pub fn union_pt(&self, pt: Point<T>) -> Self {
        Self {
            min: self.min.min(pt),
            max: self.max.max(pt),
        }
    }
}

// ============================================================================
// Transformations
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: std::ops::Add<Output = T> + std::ops::Sub<Output = T>,
{
    /// Returns a new rectangle with a different origin.
    #[inline]
    #[must_use]
    pub fn with_origin(&self, origin: Point<T>) -> Self {
        Self::from_origin_size(origin, self.size())
    }

    /// Returns a new rectangle with a different size.
    #[inline]
    #[must_use]
    pub fn with_size(&self, size: Size<T>) -> Self {
        Self::from_origin_size(self.min, size)
    }

    /// Expands the rectangle by the given amount on each side.
    #[inline]
    #[must_use]
    pub fn inflate(&self, width: T, height: T) -> Self {
        Self {
            min: Point::new(self.min.x - width, self.min.y - height),
            max: Point::new(self.max.x + width, self.max.y + height),
        }
    }

    /// Contracts the rectangle by the given amount on each side.
    #[inline]
    #[must_use]
    pub fn inset(&self, amount: T) -> Self {
        self.inflate(T::zero() - amount, T::zero() - amount)
    }

    /// Expands the rectangle by the given amount on all sides.
    ///
    /// This is equivalent to `inflate(amount, amount)`.
    #[inline]
    #[must_use]
    pub fn expand(&self, amount: T) -> Self {
        self.inflate(amount, amount)
    }

    /// Contracts the rectangle by different amounts on each side.
    #[inline]
    #[must_use]
    pub fn inset_by(&self, left: T, top: T, right: T, bottom: T) -> Self {
        Self {
            min: Point::new(self.min.x + left, self.min.y + top),
            max: Point::new(self.max.x - right, self.max.y - bottom),
        }
    }

    /// Translates the rectangle by a vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2<T>) -> Self {
        Self {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    /// Returns a rectangle expanded to include another rectangle.
    #[inline]
    #[must_use]
    pub fn expand_to_include(&self, other: &Self) -> Self
    where
        T: PartialOrd + Clone + fmt::Debug + Default + PartialEq,
    {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: Into<f32> + From<f32>,
{
    /// Scales the rectangle from origin.
    #[inline]
    #[must_use]
    pub fn scale_from_origin(&self, factor: f32) -> Self {
        Self {
            min: Point::new(
                T::from(self.min.x.into() * factor),
                T::from(self.min.y.into() * factor),
            ),
            max: Point::new(
                T::from(self.max.x.into() * factor),
                T::from(self.max.y.into() * factor),
            ),
        }
    }

    /// Scales the rectangle from its center.
    #[inline]
    #[must_use]
    pub fn scale_from_center(&self, factor: f32) -> Self
    where
        T: std::ops::Add<Output = T>
            + std::ops::Sub<Output = T>
            + std::ops::Div<f32, Output = T>
            + std::ops::Mul<f32, Output = T>,
    {
        Self::from_center_size(self.center(), self.size() * factor)
    }
}

impl<T: NumericUnit> Rect<T>
where
    T: PartialOrd + Clone + fmt::Debug + Default + PartialEq,
{
    /// Returns a normalized rectangle (min ≤ max).
    #[inline]
    #[must_use]
    pub fn abs(&self) -> Self {
        Self::from_points(self.min, self.max)
    }
}

// ============================================================================
// Pixels-specific Transformations (Offset)
// ============================================================================

impl Rect<Pixels> {
    /// Translates the rectangle by an offset.
    #[inline]
    #[must_use]
    pub fn translate_offset(&self, offset: Offset<Pixels>) -> Self {
        self.translate(Vec2::new(offset.dx, offset.dy))
    }
}

// ============================================================================
// Rounding Operations (Pixels only)
// ============================================================================

impl Rect<Pixels> {
    /// Rounds all coordinates to the nearest integer.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Self {
        Self {
            min: self.min.round(),
            max: self.max.round(),
        }
    }

    /// Rounds all coordinates up.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Self {
        Self {
            min: self.min.ceil(),
            max: self.max.ceil(),
        }
    }

    /// Rounds all coordinates down.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Self {
        Self {
            min: self.min.floor(),
            max: self.max.floor(),
        }
    }

    /// Expands to integer bounds (floors min, ceils max).
    ///
    /// This ensures the resulting rectangle fully contains the original.
    #[inline]
    #[must_use]
    pub fn expand_to_int(&self) -> Self {
        Self {
            min: self.min.floor(),
            max: self.max.ceil(),
        }
    }

    /// Contracts to integer bounds (ceils min, floors max).
    ///
    /// This ensures the resulting rectangle is fully contained within the original.
    #[inline]
    #[must_use]
    pub fn contract_to_int(&self) -> Self {
        Self {
            min: self.min.ceil(),
            max: self.max.floor(),
        }
    }
}

// ============================================================================
// Interpolation
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: Into<f32> + From<f32>,
{
    /// Linear interpolation between two rectangles.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            min: self.min.lerp(other.min, t),
            max: self.max.lerp(other.max, t),
        }
    }
}

// ============================================================================
// Type-safe scale conversions with ScaleFactor
// ============================================================================

impl Rect<Pixels> {
    /// Type-safe scale conversion to DevicePixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Rect, Point, Size, ScaleFactor, Pixels, DevicePixels, px};
    ///
    /// let logical = Rect::new(Point::new(px(10.0), px(20.0)), Size::new(px(100.0), px(200.0)));
    /// let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    /// let device = logical.scale_with(scale);
    /// assert_eq!(device.origin().x.get(), 20);
    /// assert_eq!(device.size().width.get(), 200);
    /// ```
    #[must_use]
    pub fn scale_with(
        self,
        scale: super::units::ScaleFactor<Pixels, super::units::DevicePixels>,
    ) -> Rect<super::units::DevicePixels> {
        Rect {
            min: self.min.scale_with(scale),
            max: self.max.scale_with(scale),
        }
    }
}

impl Rect<super::units::DevicePixels> {
    /// Converts to logical pixels using a type-safe scale factor.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Rect, Point, Size, ScaleFactor, Pixels, DevicePixels, device_px, px};
    ///
    /// let device = Rect::new(Point::new(device_px(20.0), device_px(40.0)), Size::new(device_px(200.0), device_px(400.0)));
    /// let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    /// let logical = device.unscale(scale);
    /// assert_eq!(logical.origin().x, px(10.0));
    /// assert_eq!(logical.size().width, px(100.0));
    /// ```
    #[must_use]
    pub fn unscale(
        self,
        scale: super::units::ScaleFactor<Pixels, super::units::DevicePixels>,
    ) -> Rect<Pixels> {
        Rect {
            min: self.min.unscale(scale),
            max: self.max.unscale(scale),
        }
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: Into<f32>,
{
    /// Converts to `Rect<Pixels>` with f32 values.
    #[inline]
    #[must_use]
    pub fn to_f32(&self) -> Rect<Pixels> {
        Rect {
            min: self.min.to_f32(),
            max: self.max.to_f32(),
        }
    }
}

impl<T: Unit> Rect<T>
where
    T: Into<f32>,
{
    /// Converts to array `[x0, y0, x1, y1]` for GPU usage.
    #[inline]
    #[must_use]
    pub fn to_array(&self) -> [f32; 4] {
        [
            self.min.x.into(),
            self.min.y.into(),
            self.max.x.into(),
            self.max.y.into(),
        ]
    }
}

// ============================================================================
// Display
// ============================================================================

impl<T: Unit + fmt::Display> Rect<T>
where
    T: std::ops::Sub<Output = T>,
{
    /// Format for display.
    fn format_display(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Rect({}, {} - {}×{})",
            self.min.x,
            self.min.y,
            self.max.x - self.min.x,
            self.max.y - self.min.y,
        )
    }
}

impl fmt::Display for Rect<Pixels> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format_display(f)
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Rect::from_xywh(x, y, w, h)`.
#[inline]
#[must_use]
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect<Pixels> {
    use super::units::px;
    Rect::from_xywh(px(x), px(y), px(w), px(h))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{point, px, size, Pixels};

    #[test]
    fn test_construction() {
        let r = Rect::new(px(10.0), px(20.0), px(110.0), px(70.0));
        assert_eq!(r.left(), px(10.0));
        assert_eq!(r.top(), px(20.0));
        assert_eq!(r.right(), px(110.0));
        assert_eq!(r.bottom(), px(70.0));

        let r2 = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        assert_eq!(r, r2);

        let r3 = Rect::from_ltrb(px(10.0), px(20.0), px(110.0), px(70.0));
        assert_eq!(r, r3);
    }

    #[test]
    fn test_generic_construction() {
        let r = Rect::<Pixels>::from_origin_size(
            Point::new(px(10.0), px(20.0)),
            Size::new(px(100.0), px(50.0)),
        );
        assert_eq!(r.left(), px(10.0));
        assert_eq!(r.width(), px(100.0));
    }

    #[test]
    fn test_from_points() {
        // Normalized automatically
        let r = Rect::from_points(point(px(100.0), px(100.0)), point(px(0.0), px(0.0)));
        assert_eq!(r.min, point(px(0.0), px(0.0)));
        assert_eq!(r.max, point(px(100.0), px(100.0)));
    }

    #[test]
    fn test_from_center_size() {
        let r = Rect::from_center_size(point(px(50.0), px(50.0)), size(px(20.0), px(10.0)));
        assert_eq!(r.center(), point(px(50.0), px(50.0)));
        assert_eq!(r.width(), px(20.0));
        assert_eq!(r.height(), px(10.0));
    }

    #[test]
    fn test_accessors() {
        let r = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));

        assert_eq!(r.width(), px(100.0));
        assert_eq!(r.height(), px(50.0));
        assert_eq!(r.size(), size(px(100.0), px(50.0)));
        assert_eq!(r.area(), px(5000.0));
        assert_eq!(r.origin(), point(px(10.0), px(20.0)));
        assert_eq!(r.center(), point(px(60.0), px(45.0)));

        assert_eq!(r.top_left(), point(px(10.0), px(20.0)));
        assert_eq!(r.top_right(), point(px(110.0), px(20.0)));
        assert_eq!(r.bottom_left(), point(px(10.0), px(70.0)));
        assert_eq!(r.bottom_right(), point(px(110.0), px(70.0)));
    }

    #[test]
    fn test_validation() {
        assert!(!Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0)).is_empty());
        assert!(Rect::ZERO.is_empty());
        assert!(Rect::from_xywh(px(0.0), px(0.0), px(0.0), px(50.0)).is_empty());
        assert!(Rect::from_xywh(0.0, 0.0, -10.0, 50.0).is_empty());

        assert!(Rect::ZERO.is_finite());
        assert!(!Rect::EVERYTHING.is_finite());
    }

    #[test]
    fn test_contains() {
        let r = Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(100.0));

        assert!(r.contains(point(px(50.0), px(50.0))));
        assert!(r.contains(point(px(10.0), px(10.0)))); // on edge
        assert!(r.contains(point(px(110.0), px(110.0)))); // on edge
        assert!(!r.contains(point(px(5.0), px(50.0))));
        assert!(!r.contains(point(px(115.0), px(50.0))));
    }

    #[test]
    fn test_contains_rect() {
        let outer = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let inner = Rect::from_xywh(px(25.0), px(25.0), px(50.0), px(50.0));
        let outside = Rect::from_xywh(px(200.0), px(200.0), px(50.0), px(50.0));

        assert!(outer.contains_rect(&inner));
        assert!(!inner.contains_rect(&outer));
        assert!(!outer.contains_rect(&outside));
    }

    #[test]
    fn test_overlaps() {
        let r1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let r2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));
        let r3 = Rect::from_xywh(px(200.0), px(200.0), px(50.0), px(50.0));

        assert!(r1.overlaps(&r2));
        assert!(r2.overlaps(&r1));
        assert!(!r1.overlaps(&r3));
    }

    #[test]
    fn test_intersect() {
        let r1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let r2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));

        let intersection = r1.intersect(&r2).unwrap();
        assert_eq!(
            intersection,
            Rect::from_xywh(px(50.0), px(50.0), px(50.0), px(50.0))
        );

        let r3 = Rect::from_xywh(px(200.0), px(200.0), px(50.0), px(50.0));
        assert!(r1.intersect(&r3).is_none());
    }

    #[test]
    fn test_union() {
        let r1 = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let r2 = Rect::from_xywh(px(25.0), px(25.0), px(50.0), px(50.0));

        let union = r1.union(&r2);
        assert_eq!(union, Rect::from_xywh(px(0.0), px(0.0), px(75.0), px(75.0)));
    }

    #[test]
    fn test_union_pt() {
        let r = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));
        let expanded = r.union_pt(point(px(100.0), px(100.0)));
        assert_eq!(expanded.max, point(px(100.0), px(100.0)));
    }

    #[test]
    fn test_transformations() {
        let r = Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(50.0));

        let inflated = r.inflate(5.0, 5.0);
        assert_eq!(
            inflated,
            Rect::from_xywh(px(5.0), px(5.0), px(110.0), px(60.0))
        );

        let inset = r.inset(5.0);
        assert_eq!(
            inset,
            Rect::from_xywh(px(15.0), px(15.0), px(90.0), px(40.0))
        );

        let translated = r.translate(Vec2::new(10.0, 20.0));
        assert_eq!(
            translated,
            Rect::from_xywh(px(20.0), px(30.0), px(100.0), px(50.0))
        );

        let scaled = r.scale_from_origin(2.0);
        assert_eq!(
            scaled,
            Rect::from_xywh(px(20.0), px(20.0), px(200.0), px(100.0))
        );
    }

    #[test]
    fn test_with_origin_size() {
        let r = Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(50.0));

        let moved = r.with_origin(point(px(20.0), px(20.0)));
        assert_eq!(moved.origin(), point(px(20.0), px(20.0)));
        assert_eq!(moved.size(), r.size());

        let resized = r.with_size(size(px(200.0), px(100.0)));
        assert_eq!(resized.origin(), r.origin());
        assert_eq!(resized.size(), size(px(200.0), px(100.0)));
    }

    #[test]
    fn test_rounding() {
        let r = Rect::new(px(10.3), px(20.7), px(110.5), px(71.3));

        let rounded = r.round();
        assert_eq!(rounded.min, point(px(10.0), px(21.0)));
        assert_eq!(rounded.max, point(px(111.0), px(71.0)));

        let expanded = r.expand_to_int();
        assert_eq!(expanded.min, point(px(10.0), px(20.0)));
        assert_eq!(expanded.max, point(px(111.0), px(72.0)));

        let contracted = r.contract_to_int();
        assert_eq!(contracted.min, point(px(11.0), px(21.0)));
        assert_eq!(contracted.max, point(px(110.0), px(71.0)));
    }

    #[test]
    fn test_lerp() {
        let r1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let r2 = Rect::from_xywh(px(100.0), px(100.0), px(200.0), px(200.0));

        let mid = r1.lerp(r2, 0.5);
        assert_eq!(
            mid,
            Rect::from_xywh(px(50.0), px(50.0), px(150.0), px(150.0))
        );
    }

    #[test]
    fn test_constants() {
        assert!(Rect::ZERO.is_empty());
        assert!(Rect::EVERYTHING.contains(point(1e10, -1e10)));
    }

    #[test]
    fn test_display() {
        let r = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let s = format!("{}", r);
        assert!(s.contains("10"));
        assert!(s.contains("20"));
        assert!(s.contains("100"));
        assert!(s.contains("50"));
    }

    #[test]
    fn test_convenience_fn() {
        assert_eq!(
            rect(10.0, 20.0, 100.0, 50.0),
            Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0))
        );
    }

    #[test]
    fn test_to_f32() {
        let r = Rect::<Pixels>::from_origin_size(
            Point::new(px(10.0), px(20.0)),
            Size::new(px(100.0), px(50.0)),
        );
        let f = r.to_f32();
        assert_eq!(f.min, point(px(10.0), px(20.0)));
        assert_eq!(f.max, point(px(110.0), px(70.0)));
    }

    #[test]
    fn test_to_array() {
        let r = rect(10.0, 20.0, 100.0, 50.0);
        let arr = r.to_array();
        assert_eq!(arr, [10.0, 20.0, 110.0, 70.0]);
    }

    #[test]
    fn test_map() {
        // rect(x=10, y=20, w=100, h=50) creates min=(10,20), max=(110,70)
        let r = rect(10.0, 20.0, 100.0, 50.0);
        let doubled = r.map(|x| x * 2.0);
        assert_eq!(doubled.min, point(px(20.0), px(40.0))); // (10*2, 20*2)
        assert_eq!(doubled.max, point(px(220.0), px(140.0))); // (110*2, 70*2)
    }

    #[test]
    fn test_default() {
        let r: Rect<f32> = Rect::default();
        assert_eq!(r.min, Point::ORIGIN);
        assert_eq!(r.max, Point::ORIGIN);
    }
}
