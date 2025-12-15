//! Rectangle type for bounding boxes and regions.
//!
//! API design inspired by kurbo, glam, and Flutter.

use std::fmt;

use super::{Point, Size, Vec2};

/// An axis-aligned rectangle.
///
/// Defined by minimum and maximum corner points. The rectangle is valid when
/// `min.x <= max.x` and `min.y <= max.y`.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Rect, Point, Size};
///
/// // Create from origin and size
/// let rect = Rect::from_origin_size(Point::ORIGIN, Size::new(100.0, 50.0));
///
/// // Query properties
/// assert_eq!(rect.width(), 100.0);
/// assert_eq!(rect.height(), 50.0);
/// assert_eq!(rect.area(), 5000.0);
///
/// // Hit testing
/// assert!(rect.contains(Point::new(50.0, 25.0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Rect {
    /// Minimum corner (top-left in screen coordinates).
    pub min: Point,
    /// Maximum corner (bottom-right in screen coordinates).
    pub max: Point,
}

// ============================================================================
// Constants
// ============================================================================

impl Rect {
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
// Constructors
// ============================================================================

impl Rect {
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

    /// Creates a rectangle from two points.
    ///
    /// Points are normalized so min ≤ max.
    #[inline]
    #[must_use]
    pub fn from_points(p0: Point, p1: Point) -> Self {
        Self {
            min: p0.min(p1),
            max: p0.max(p1),
        }
    }

    /// Creates a rectangle from origin and size.
    #[inline]
    #[must_use]
    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            min: origin,
            max: Point::new(origin.x + size.width, origin.y + size.height),
        }
    }

    /// Creates a rectangle centered at a point with given size.
    #[inline]
    #[must_use]
    pub fn from_center_size(center: Point, size: Size) -> Self {
        let half = Vec2::new(size.width * 0.5, size.height * 0.5);
        Self {
            min: center - half,
            max: center + half,
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

    /// Creates a rectangle from min and max points.
    #[inline]
    #[must_use]
    pub const fn from_min_max(min: Point, max: Point) -> Self {
        Self { min, max }
    }
}

// ============================================================================
// Accessors
// ============================================================================

impl Rect {
    /// Left edge (min x).
    #[inline]
    #[must_use]
    pub const fn left(&self) -> f32 {
        self.min.x
    }

    /// Top edge (min y).
    #[inline]
    #[must_use]
    pub const fn top(&self) -> f32 {
        self.min.y
    }

    /// Right edge (max x).
    #[inline]
    #[must_use]
    pub const fn right(&self) -> f32 {
        self.max.x
    }

    /// Bottom edge (max y).
    #[inline]
    #[must_use]
    pub const fn bottom(&self) -> f32 {
        self.max.y
    }

    /// Minimum x coordinate.
    #[inline]
    #[must_use]
    pub fn min_x(&self) -> f32 {
        self.min.x.min(self.max.x)
    }

    /// Maximum x coordinate.
    #[inline]
    #[must_use]
    pub fn max_x(&self) -> f32 {
        self.min.x.max(self.max.x)
    }

    /// Minimum y coordinate.
    #[inline]
    #[must_use]
    pub fn min_y(&self) -> f32 {
        self.min.y.min(self.max.y)
    }

    /// Maximum y coordinate.
    #[inline]
    #[must_use]
    pub fn max_y(&self) -> f32 {
        self.min.y.max(self.max.y)
    }

    /// Width of the rectangle.
    #[inline]
    #[must_use]
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Height of the rectangle.
    #[inline]
    #[must_use]
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Size of the rectangle.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size {
        Size::new(self.width(), self.height())
    }

    /// Area of the rectangle.
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// Origin point (top-left corner).
    #[inline]
    #[must_use]
    pub const fn origin(&self) -> Point {
        self.min
    }

    /// Center point of the rectangle.
    #[inline]
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    /// Top-left corner.
    #[inline]
    #[must_use]
    pub const fn top_left(&self) -> Point {
        self.min
    }

    /// Top-right corner.
    #[inline]
    #[must_use]
    pub const fn top_right(&self) -> Point {
        Point::new(self.max.x, self.min.y)
    }

    /// Bottom-left corner.
    #[inline]
    #[must_use]
    pub const fn bottom_left(&self) -> Point {
        Point::new(self.min.x, self.max.y)
    }

    /// Bottom-right corner.
    #[inline]
    #[must_use]
    pub const fn bottom_right(&self) -> Point {
        self.max
    }
}

// ============================================================================
// Validation
// ============================================================================

impl Rect {
    /// Returns `true` if the rectangle has zero or negative area.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

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

impl Rect {
    /// Returns `true` if the point is inside the rectangle.
    #[inline]
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Returns `true` if this rectangle completely contains another.
    #[inline]
    #[must_use]
    pub fn contains_rect(&self, other: Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && self.max.x >= other.max.x
            && self.max.y >= other.max.y
    }

    /// Returns `true` if the offset is inside the rectangle.
    #[inline]
    #[must_use]
    pub fn contains_offset(&self, offset: crate::Offset) -> bool {
        offset.dx >= self.min.x
            && offset.dx <= self.max.x
            && offset.dy >= self.min.y
            && offset.dy <= self.max.y
    }

    /// Returns `true` if this rectangle overlaps with another.
    #[inline]
    #[must_use]
    pub fn overlaps(&self, other: Self) -> bool {
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
    pub fn intersects(&self, other: Self) -> bool {
        self.overlaps(other)
    }
}

// ============================================================================
// Set Operations
// ============================================================================

impl Rect {
    /// Returns the intersection of two rectangles, or `None` if they don't overlap.
    #[must_use]
    pub fn intersect(&self, other: Self) -> Option<Self> {
        let min_x = self.min.x.max(other.min.x);
        let min_y = self.min.y.max(other.min.y);
        let max_x = self.max.x.min(other.max.x);
        let max_y = self.max.y.min(other.max.y);

        if min_x <= max_x && min_y <= max_y {
            Some(Self::new(min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    /// Returns the smallest rectangle containing both rectangles.
    #[inline]
    #[must_use]
    pub fn union(&self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// Returns the smallest rectangle containing this rectangle and a point.
    #[inline]
    #[must_use]
    pub fn union_pt(&self, pt: Point) -> Self {
        Self {
            min: self.min.min(pt),
            max: self.max.max(pt),
        }
    }
}

// ============================================================================
// Transformations
// ============================================================================

impl Rect {
    /// Returns a new rectangle with a different origin.
    #[inline]
    #[must_use]
    pub fn with_origin(&self, origin: Point) -> Self {
        Self::from_origin_size(origin, self.size())
    }

    /// Returns a new rectangle with a different size.
    #[inline]
    #[must_use]
    pub fn with_size(&self, size: Size) -> Self {
        Self::from_origin_size(self.min, size)
    }

    /// Expands the rectangle by the given amount on each side.
    #[inline]
    #[must_use]
    pub fn inflate(&self, width: f32, height: f32) -> Self {
        Self::new(
            self.min.x - width,
            self.min.y - height,
            self.max.x + width,
            self.max.y + height,
        )
    }

    /// Contracts the rectangle by the given amount on each side.
    #[inline]
    #[must_use]
    pub fn inset(&self, amount: f32) -> Self {
        self.inflate(-amount, -amount)
    }

    /// Expands the rectangle by the given amount on all sides.
    ///
    /// This is equivalent to `inflate(amount, amount)`.
    #[inline]
    #[must_use]
    pub fn expand(&self, amount: f32) -> Self {
        self.inflate(amount, amount)
    }

    /// Contracts the rectangle by different amounts on each side.
    #[inline]
    #[must_use]
    pub fn inset_by(&self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::new(
            self.min.x + left,
            self.min.y + top,
            self.max.x - right,
            self.max.y - bottom,
        )
    }

    /// Translates the rectangle by a vector.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2) -> Self {
        Self {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    /// Translates the rectangle by an offset.
    #[inline]
    #[must_use]
    pub fn translate_offset(&self, offset: crate::Offset) -> Self {
        self.translate(Vec2::new(offset.dx, offset.dy))
    }

    /// Returns a rectangle expanded to include another rectangle.
    #[inline]
    #[must_use]
    pub fn expand_to_include(&self, other: &Self) -> Self {
        Self::new(
            self.min.x.min(other.min.x),
            self.min.y.min(other.min.y),
            self.max.x.max(other.max.x),
            self.max.y.max(other.max.y),
        )
    }

    /// Creates a rectangle from center point with width and height.
    #[inline]
    #[must_use]
    pub fn from_center(center: crate::Offset, width: f32, height: f32) -> Self {
        let half_width = width / 2.0;
        let half_height = height / 2.0;
        Self::new(
            center.dx - half_width,
            center.dy - half_height,
            center.dx + half_width,
            center.dy + half_height,
        )
    }

    /// Creates a rectangle from left, top, width, and height (Flutter-style).
    #[inline]
    #[must_use]
    pub fn from_ltwh(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self::new(left, top, left + width, top + height)
    }

    /// Scales the rectangle from origin.
    #[inline]
    #[must_use]
    pub fn scale_from_origin(&self, factor: f32) -> Self {
        Self::new(
            self.min.x * factor,
            self.min.y * factor,
            self.max.x * factor,
            self.max.y * factor,
        )
    }

    /// Scales the rectangle from its center.
    #[inline]
    #[must_use]
    pub fn scale_from_center(&self, factor: f32) -> Self {
        Self::from_center_size(self.center(), self.size() * factor)
    }

    /// Returns a normalized rectangle (min ≤ max).
    #[inline]
    #[must_use]
    pub fn abs(&self) -> Self {
        Self::from_points(self.min, self.max)
    }
}

// ============================================================================
// Rounding Operations
// ============================================================================

impl Rect {
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

impl Rect {
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
// Display
// ============================================================================

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Rect({}, {} - {}×{})",
            self.min.x,
            self.min.y,
            self.width(),
            self.height()
        )
    }
}

// ============================================================================
// Convenience function
// ============================================================================

/// Shorthand for `Rect::from_xywh(x, y, w, h)`.
#[inline]
#[must_use]
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_xywh(x, y, w, h)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_from_points() {
        // Normalized automatically
        let r = Rect::from_points(Point::new(100.0, 100.0), Point::new(0.0, 0.0));
        assert_eq!(r.min, Point::new(0.0, 0.0));
        assert_eq!(r.max, Point::new(100.0, 100.0));
    }

    #[test]
    fn test_from_center_size() {
        let r = Rect::from_center_size(Point::new(50.0, 50.0), Size::new(20.0, 10.0));
        assert_eq!(r.center(), Point::new(50.0, 50.0));
        assert_eq!(r.width(), 20.0);
        assert_eq!(r.height(), 10.0);
    }

    #[test]
    fn test_accessors() {
        let r = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);

        assert_eq!(r.width(), 100.0);
        assert_eq!(r.height(), 50.0);
        assert_eq!(r.size(), Size::new(100.0, 50.0));
        assert_eq!(r.area(), 5000.0);
        assert_eq!(r.origin(), Point::new(10.0, 20.0));
        assert_eq!(r.center(), Point::new(60.0, 45.0));

        assert_eq!(r.top_left(), Point::new(10.0, 20.0));
        assert_eq!(r.top_right(), Point::new(110.0, 20.0));
        assert_eq!(r.bottom_left(), Point::new(10.0, 70.0));
        assert_eq!(r.bottom_right(), Point::new(110.0, 70.0));
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

        assert!(r.contains(Point::new(50.0, 50.0)));
        assert!(r.contains(Point::new(10.0, 10.0))); // on edge
        assert!(r.contains(Point::new(110.0, 110.0))); // on edge
        assert!(!r.contains(Point::new(5.0, 50.0)));
        assert!(!r.contains(Point::new(115.0, 50.0)));
    }

    #[test]
    fn test_contains_rect() {
        let outer = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let inner = Rect::from_xywh(25.0, 25.0, 50.0, 50.0);
        let outside = Rect::from_xywh(200.0, 200.0, 50.0, 50.0);

        assert!(outer.contains_rect(inner));
        assert!(!inner.contains_rect(outer));
        assert!(!outer.contains_rect(outside));
    }

    #[test]
    fn test_overlaps() {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);
        let r3 = Rect::from_xywh(200.0, 200.0, 50.0, 50.0);

        assert!(r1.overlaps(r2));
        assert!(r2.overlaps(r1));
        assert!(!r1.overlaps(r3));
    }

    #[test]
    fn test_intersect() {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);

        let intersection = r1.intersect(r2).unwrap();
        assert_eq!(intersection, Rect::from_xywh(50.0, 50.0, 50.0, 50.0));

        let r3 = Rect::from_xywh(200.0, 200.0, 50.0, 50.0);
        assert!(r1.intersect(r3).is_none());
    }

    #[test]
    fn test_union() {
        let r1 = Rect::from_xywh(0.0, 0.0, 50.0, 50.0);
        let r2 = Rect::from_xywh(25.0, 25.0, 50.0, 50.0);

        let union = r1.union(r2);
        assert_eq!(union, Rect::from_xywh(0.0, 0.0, 75.0, 75.0));
    }

    #[test]
    fn test_union_pt() {
        let r = Rect::from_xywh(10.0, 10.0, 50.0, 50.0);
        let expanded = r.union_pt(Point::new(100.0, 100.0));
        assert_eq!(expanded.max, Point::new(100.0, 100.0));
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

        let moved = r.with_origin(Point::new(20.0, 20.0));
        assert_eq!(moved.origin(), Point::new(20.0, 20.0));
        assert_eq!(moved.size(), r.size());

        let resized = r.with_size(Size::new(200.0, 100.0));
        assert_eq!(resized.origin(), r.origin());
        assert_eq!(resized.size(), Size::new(200.0, 100.0));
    }

    #[test]
    fn test_rounding() {
        let r = Rect::new(10.3, 20.7, 110.5, 71.3);

        let rounded = r.round();
        assert_eq!(rounded.min, Point::new(10.0, 21.0));
        assert_eq!(rounded.max, Point::new(111.0, 71.0));

        let expanded = r.expand_to_int();
        assert_eq!(expanded.min, Point::new(10.0, 20.0));
        assert_eq!(expanded.max, Point::new(111.0, 72.0));

        let contracted = r.contract_to_int();
        assert_eq!(contracted.min, Point::new(11.0, 21.0));
        assert_eq!(contracted.max, Point::new(110.0, 71.0));
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
        assert!(Rect::EVERYTHING.contains(Point::new(1e10, -1e10)));
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
}
