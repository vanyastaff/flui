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
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Rect<T: Unit = f32> {
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
// Constants (f32 only)
// ============================================================================

impl Rect<f32> {
    /// Empty rectangle at origin.
    pub const ZERO: Self = Self {
        min: Point::ORIGIN,
        max: Point::ORIGIN,
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

    /// Maps the rectangle through a function.
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
    /// Creates a rectangle from two points.
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
// f32-specific Constructors
// ============================================================================

impl Rect<f32> {
    /// Creates a rectangle from raw coordinates.
    ///
    /// Note: Does not normalize — if `x0 > x1` or `y0 > y1`, the rect is inverted.
    #[inline]
    #[must_use]
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            min: Point::new(x0, y0),
            max: Point::new(x1, y1),
        }
    }

    /// Creates a rectangle from x, y, width, height.
    #[inline]
    #[must_use]
    pub fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self::new(x, y, x + width, y + height)
    }

    /// Creates a rectangle from left, top, right, bottom (Flutter-style).
    #[inline]
    #[must_use]
    pub const fn from_ltrb(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::new(left, top, right, bottom)
    }

    /// Creates a rectangle from left, top, width, height (Flutter-style).
    #[inline]
    #[must_use]
    pub fn from_ltwh(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self::new(left, top, left + width, top + height)
    }

    /// Creates a rectangle from center point with width and height.
    #[inline]
    #[must_use]
    pub fn from_center(center: Offset<f32>, width: f32, height: f32) -> Self {
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
    /// Left edge (min x).
    #[inline]
    #[must_use]
    pub fn left(&self) -> T {
        self.min.x
    }

    /// Top edge (min y).
    #[inline]
    #[must_use]
    pub fn top(&self) -> T {
        self.min.y
    }

    /// Right edge (max x).
    #[inline]
    #[must_use]
    pub fn right(&self) -> T {
        self.max.x
    }

    /// Bottom edge (max y).
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> T {
        self.max.y
    }

    /// Origin point (top-left corner).
    #[inline]
    #[must_use]
    pub fn origin(&self) -> Point<T> {
        self.min
    }

    /// Top-left corner.
    #[inline]
    #[must_use]
    pub fn top_left(&self) -> Point<T> {
        self.min
    }

    /// Top-right corner.
    #[inline]
    #[must_use]
    pub fn top_right(&self) -> Point<T> {
        Point::new(self.max.x, self.min.y)
    }

    /// Bottom-left corner.
    #[inline]
    #[must_use]
    pub fn bottom_left(&self) -> Point<T> {
        Point::new(self.min.x, self.max.y)
    }

    /// Bottom-right corner.
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
    /// Minimum x coordinate.
    #[inline]
    #[must_use]
    pub fn min_x(&self) -> T {
        if self.min.x < self.max.x {
            self.min.x
        } else {
            self.max.x
        }
    }

    /// Maximum x coordinate.
    #[inline]
    #[must_use]
    pub fn max_x(&self) -> T {
        if self.min.x > self.max.x {
            self.min.x
        } else {
            self.max.x
        }
    }

    /// Minimum y coordinate.
    #[inline]
    #[must_use]
    pub fn min_y(&self) -> T {
        if self.min.y < self.max.y {
            self.min.y
        } else {
            self.max.y
        }
    }

    /// Maximum y coordinate.
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
    /// Returns `true` if the rectangle has zero or negative area.
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
    /// Returns `true` if all coordinates are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }

    /// Returns `true` if any coordinate is NaN.
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
    /// Returns `true` if the point is inside the rectangle.
    #[inline]
    #[must_use]
    pub fn contains(&self, point: Point<T>) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Returns `true` if this rectangle completely contains another.
    #[inline]
    #[must_use]
    pub fn contains_rect(&self, other: &Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && self.max.x >= other.max.x
            && self.max.y >= other.max.y
    }

    /// Returns `true` if this rectangle overlaps with another.
    #[inline]
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.max.x > other.min.x
            && self.min.x < other.max.x
            && self.max.y > other.min.y
            && self.min.y < other.max.y
    }

    /// Returns whether this rectangle intersects another.
    ///
    /// This is an alias for [`overlaps`](Self::overlaps).
    #[inline]
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.overlaps(other)
    }
}

// ============================================================================
// f32-specific Containment (Offset)
// ============================================================================

impl Rect<f32> {
    /// Returns `true` if the offset is inside the rectangle.
    #[inline]
    #[must_use]
    pub fn contains_offset(&self, offset: Offset<f32>) -> bool {
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
// f32-specific Transformations (Offset)
// ============================================================================

impl Rect<f32> {
    /// Translates the rectangle by an offset.
    #[inline]
    #[must_use]
    pub fn translate_offset(&self, offset: Offset<f32>) -> Self {
        self.translate(Vec2::new(offset.dx, offset.dy))
    }
}

// ============================================================================
// Rounding Operations (f32 only)
// ============================================================================

impl Rect<f32> {
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
// Conversions
// ============================================================================

impl<T: NumericUnit> Rect<T>
where
    T: Into<f32>,
{
    /// Converts to `Rect<f32>`.
    #[inline]
    #[must_use]
    pub fn to_f32(&self) -> Rect<f32> {
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

impl fmt::Display for Rect<f32> {
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
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect<f32> {
    Rect::from_xywh(x, y, w, h)
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
        let r = Rect::new(10.0, 20.0, 110.0, 70.0);
        assert_eq!(r.left(), 10.0);
        assert_eq!(r.top(), 20.0);
        assert_eq!(r.right(), 110.0);
        assert_eq!(r.bottom(), 70.0);

        let r2 = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r, r2);

        let r3 = Rect::from_ltrb(10.0, 20.0, 110.0, 70.0);
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
        let r = Rect::from_points(point(100.0, 100.0), point(0.0, 0.0));
        assert_eq!(r.min, point(0.0, 0.0));
        assert_eq!(r.max, point(100.0, 100.0));
    }

    #[test]
    fn test_from_center_size() {
        let r = Rect::from_center_size(point(50.0, 50.0), size(20.0, 10.0));
        assert_eq!(r.center(), point(50.0, 50.0));
        assert_eq!(r.width(), 20.0);
        assert_eq!(r.height(), 10.0);
    }

    #[test]
    fn test_accessors() {
        let r = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);

        assert_eq!(r.width(), 100.0);
        assert_eq!(r.height(), 50.0);
        assert_eq!(r.size(), size(100.0, 50.0));
        assert_eq!(r.area(), 5000.0);
        assert_eq!(r.origin(), point(10.0, 20.0));
        assert_eq!(r.center(), point(60.0, 45.0));

        assert_eq!(r.top_left(), point(10.0, 20.0));
        assert_eq!(r.top_right(), point(110.0, 20.0));
        assert_eq!(r.bottom_left(), point(10.0, 70.0));
        assert_eq!(r.bottom_right(), point(110.0, 70.0));
    }

    #[test]
    fn test_validation() {
        assert!(!Rect::from_xywh(0.0, 0.0, 100.0, 50.0).is_empty());
        assert!(Rect::ZERO.is_empty());
        assert!(Rect::from_xywh(0.0, 0.0, 0.0, 50.0).is_empty());
        assert!(Rect::from_xywh(0.0, 0.0, -10.0, 50.0).is_empty());

        assert!(Rect::ZERO.is_finite());
        assert!(!Rect::EVERYTHING.is_finite());
    }

    #[test]
    fn test_contains() {
        let r = Rect::from_xywh(10.0, 10.0, 100.0, 100.0);

        assert!(r.contains(point(50.0, 50.0)));
        assert!(r.contains(point(10.0, 10.0))); // on edge
        assert!(r.contains(point(110.0, 110.0))); // on edge
        assert!(!r.contains(point(5.0, 50.0)));
        assert!(!r.contains(point(115.0, 50.0)));
    }

    #[test]
    fn test_contains_rect() {
        let outer = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let inner = Rect::from_xywh(25.0, 25.0, 50.0, 50.0);
        let outside = Rect::from_xywh(200.0, 200.0, 50.0, 50.0);

        assert!(outer.contains_rect(&inner));
        assert!(!inner.contains_rect(&outer));
        assert!(!outer.contains_rect(&outside));
    }

    #[test]
    fn test_overlaps() {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);
        let r3 = Rect::from_xywh(200.0, 200.0, 50.0, 50.0);

        assert!(r1.overlaps(&r2));
        assert!(r2.overlaps(&r1));
        assert!(!r1.overlaps(&r3));
    }

    #[test]
    fn test_intersect() {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);

        let intersection = r1.intersect(&r2).unwrap();
        assert_eq!(intersection, Rect::from_xywh(50.0, 50.0, 50.0, 50.0));

        let r3 = Rect::from_xywh(200.0, 200.0, 50.0, 50.0);
        assert!(r1.intersect(&r3).is_none());
    }

    #[test]
    fn test_union() {
        let r1 = Rect::from_xywh(0.0, 0.0, 50.0, 50.0);
        let r2 = Rect::from_xywh(25.0, 25.0, 50.0, 50.0);

        let union = r1.union(&r2);
        assert_eq!(union, Rect::from_xywh(0.0, 0.0, 75.0, 75.0));
    }

    #[test]
    fn test_union_pt() {
        let r = Rect::from_xywh(10.0, 10.0, 50.0, 50.0);
        let expanded = r.union_pt(point(100.0, 100.0));
        assert_eq!(expanded.max, point(100.0, 100.0));
    }

    #[test]
    fn test_transformations() {
        let r = Rect::from_xywh(10.0, 10.0, 100.0, 50.0);

        let inflated = r.inflate(5.0, 5.0);
        assert_eq!(inflated, Rect::from_xywh(5.0, 5.0, 110.0, 60.0));

        let inset = r.inset(5.0);
        assert_eq!(inset, Rect::from_xywh(15.0, 15.0, 90.0, 40.0));

        let translated = r.translate(Vec2::new(10.0, 20.0));
        assert_eq!(translated, Rect::from_xywh(20.0, 30.0, 100.0, 50.0));

        let scaled = r.scale_from_origin(2.0);
        assert_eq!(scaled, Rect::from_xywh(20.0, 20.0, 200.0, 100.0));
    }

    #[test]
    fn test_with_origin_size() {
        let r = Rect::from_xywh(10.0, 10.0, 100.0, 50.0);

        let moved = r.with_origin(point(20.0, 20.0));
        assert_eq!(moved.origin(), point(20.0, 20.0));
        assert_eq!(moved.size(), r.size());

        let resized = r.with_size(size(200.0, 100.0));
        assert_eq!(resized.origin(), r.origin());
        assert_eq!(resized.size(), size(200.0, 100.0));
    }

    #[test]
    fn test_rounding() {
        let r = Rect::new(10.3, 20.7, 110.5, 71.3);

        let rounded = r.round();
        assert_eq!(rounded.min, point(10.0, 21.0));
        assert_eq!(rounded.max, point(111.0, 71.0));

        let expanded = r.expand_to_int();
        assert_eq!(expanded.min, point(10.0, 20.0));
        assert_eq!(expanded.max, point(111.0, 72.0));

        let contracted = r.contract_to_int();
        assert_eq!(contracted.min, point(11.0, 21.0));
        assert_eq!(contracted.max, point(110.0, 71.0));
    }

    #[test]
    fn test_lerp() {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(100.0, 100.0, 200.0, 200.0);

        let mid = r1.lerp(r2, 0.5);
        assert_eq!(mid, Rect::from_xywh(50.0, 50.0, 150.0, 150.0));
    }

    #[test]
    fn test_constants() {
        assert!(Rect::ZERO.is_empty());
        assert!(Rect::EVERYTHING.contains(point(1e10, -1e10)));
    }

    #[test]
    fn test_display() {
        let r = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
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
            Rect::from_xywh(10.0, 20.0, 100.0, 50.0)
        );
    }

    #[test]
    fn test_to_f32() {
        let r = Rect::<Pixels>::from_origin_size(
            Point::new(px(10.0), px(20.0)),
            Size::new(px(100.0), px(50.0)),
        );
        let f = r.to_f32();
        assert_eq!(f.min, point(10.0, 20.0));
        assert_eq!(f.max, point(110.0, 70.0));
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
        assert_eq!(doubled.min, point(20.0, 40.0));   // (10*2, 20*2)
        assert_eq!(doubled.max, point(220.0, 140.0)); // (110*2, 70*2)
    }

    #[test]
    fn test_default() {
        let r: Rect<f32> = Rect::default();
        assert_eq!(r.min, Point::ORIGIN);
        assert_eq!(r.max, Point::ORIGIN);
    }
}
