//! Rectangle types for bounding boxes and regions.
//!
//! This module provides type-safe rectangle types for representing bounding boxes,
//! clip regions, and layout bounds.

use crate::{Point, Size};
use std::fmt;

/// Rectangle defined by two corner points.
///
/// Represents an axis-aligned bounding box in 2D space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Minimum corner (typically top-left).
    pub min: Point,
    /// Maximum corner (typically bottom-right).
    pub max: Point,
}

impl Rect {
    /// Empty rectangle at origin.
    pub const ZERO: Rect = Rect {
        min: Point::ZERO,
        max: Point::ZERO,
    };

    /// Infinite rectangle.
    pub const EVERYTHING: Rect = Rect {
        min: Point {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
        },
        max: Point::INFINITY,
    };

    /// Create a rectangle from two corner points.
    pub fn from_min_max(min: impl Into<Point>, max: impl Into<Point>) -> Self {
        Self {
            min: min.into(),
            max: max.into(),
        }
    }

    /// Create a rectangle from minimum point and size.
    pub fn from_min_size(min: impl Into<Point>, size: impl Into<Size>) -> Self {
        let min = min.into();
        let size = size.into();
        Self {
            min,
            max: Point::new(min.x + size.width, min.y + size.height),
        }
    }

    /// Create a rectangle from center point and size.
    pub fn from_center_size(center: impl Into<Point>, size: impl Into<Size>) -> Self {
        let center = center.into();
        let size = size.into();
        let half_width = size.width * 0.5;
        let half_height = size.height * 0.5;
        Self {
            min: Point::new(center.x - half_width, center.y - half_height),
            max: Point::new(center.x + half_width, center.y + half_height),
        }
    }

    /// Create a rectangle from x, y, width, height.
    pub fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            min: Point::new(x, y),
            max: Point::new(x + width, y + height),
        }
    }

    /// Create a rectangle from left, top, width, height.
    ///
    /// This is an alias for `from_xywh` with more explicit parameter names.
    #[inline]
    pub fn from_ltwh(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self::from_xywh(left, top, width, height)
    }

    /// Create a rectangle from left, top, right, bottom coordinates.
    ///
    /// This is a common pattern in graphics APIs where you specify the edges directly.
    /// Similar to Flutter's `Rect.fromLTRB`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Rect;
    ///
    /// let rect = Rect::from_ltrb(10.0, 20.0, 110.0, 120.0);
    /// assert_eq!(rect.left(), 10.0);
    /// assert_eq!(rect.top(), 20.0);
    /// assert_eq!(rect.right(), 110.0);
    /// assert_eq!(rect.bottom(), 120.0);
    /// assert_eq!(rect.width(), 100.0);
    /// assert_eq!(rect.height(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_ltrb(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            min: Point::new(left, top),
            max: Point::new(right, bottom),
        }
    }

    /// Get the width of the rectangle.
    #[inline]
    #[must_use]
    pub const fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Get the height of the rectangle.
    #[inline]
    #[must_use]
    pub const fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Get the size of the rectangle.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size {
        Size::new(self.width(), self.height())
    }

    /// Get the center point of the rectangle.
    #[inline]
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    /// Get the area of the rectangle.
    #[inline]
    #[must_use]
    pub const fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// Get the aspect ratio (width / height).
    pub fn aspect_ratio(&self) -> Option<f32> {
        let h = self.height();
        if h > 0.0 {
            Some(self.width() / h)
        } else {
            None
        }
    }

    /// Check if the rectangle contains a point.
    #[inline]
    #[must_use]
    pub fn contains(&self, point: impl Into<Point>) -> bool {
        let point = point.into();
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Check if this rectangle intersects another.
    #[inline]
    #[must_use]
    pub fn intersects(&self, other: &Rect) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    /// Get the union of two rectangles (smallest rect containing both).
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            min: Point::min(self.min, other.min),
            max: Point::max(self.max, other.max),
        }
    }

    /// Get the intersection of two rectangles.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let min = Point::max(self.min, other.min);
        let max = Point::min(self.max, other.max);

        if min.x <= max.x && min.y <= max.y {
            Some(Rect { min, max })
        } else {
            None
        }
    }

    /// Expand the rectangle by a margin on all sides.
    pub fn expand(&self, margin: f32) -> Rect {
        Rect {
            min: Point::new(self.min.x - margin, self.min.y - margin),
            max: Point::new(self.max.x + margin, self.max.y + margin),
        }
    }

    /// Shrink the rectangle by a margin on all sides.
    pub fn shrink(&self, margin: f32) -> Rect {
        self.expand(-margin)
    }

    /// Check if the rectangle is empty (zero or negative size).
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// Check if the rectangle has finite coordinates.
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }

    /// Get the left edge X coordinate.
    #[inline]
    #[must_use]
    pub const fn left(&self) -> f32 {
        self.min.x
    }

    /// Get the right edge X coordinate.
    #[inline]
    #[must_use]
    pub const fn right(&self) -> f32 {
        self.max.x
    }

    /// Get the top edge Y coordinate.
    #[inline]
    #[must_use]
    pub const fn top(&self) -> f32 {
        self.min.y
    }

    /// Get the bottom edge Y coordinate.
    #[inline]
    #[must_use]
    pub const fn bottom(&self) -> f32 {
        self.max.y
    }

    // ===== Helper methods for rendering =====

    /// Get all four corners as points (clockwise from top-left).
    ///
    /// Order: [top_left, top_right, bottom_right, bottom_left]
    #[inline]
    #[must_use]
    pub const fn corners(&self) -> [Point; 4] {
        [
            self.min,                           // top_left
            Point::new(self.max.x, self.min.y), // top_right
            self.max,                           // bottom_right
            Point::new(self.min.x, self.max.y), // bottom_left
        ]
    }

    /// Get the top-left corner.
    #[inline]
    #[must_use]
    pub const fn top_left(&self) -> Point {
        self.min
    }

    /// Get the top-right corner.
    #[inline]
    #[must_use]
    pub const fn top_right(&self) -> Point {
        Point::new(self.max.x, self.min.y)
    }

    /// Get the bottom_left corner.
    #[inline]
    #[must_use]
    pub const fn bottom_left(&self) -> Point {
        Point::new(self.min.x, self.max.y)
    }

    /// Get the bottom-right corner.
    #[inline]
    #[must_use]
    pub const fn bottom_right(&self) -> Point {
        self.max
    }

    /// Translate rect by an offset.
    #[inline]
    #[must_use]
    pub const fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            min: Point::new(self.min.x + dx, self.min.y + dy),
            max: Point::new(self.max.x + dx, self.max.y + dy),
        }
    }

    /// Scale rect from origin (0, 0).
    #[inline]
    #[must_use]
    pub const fn scale(&self, scale_x: f32, scale_y: f32) -> Rect {
        Rect {
            min: Point::new(self.min.x * scale_x, self.min.y * scale_y),
            max: Point::new(self.max.x * scale_x, self.max.y * scale_y),
        }
    }

    /// Round all coordinates to nearest integer.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Rect {
        Rect {
            min: self.min.round(),
            max: self.max.round(),
        }
    }

    /// Floor all coordinates.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Rect {
        Rect {
            min: self.min.floor(),
            max: self.max.floor(),
        }
    }

    /// Ceil all coordinates.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Rect {
        Rect {
            min: self.min.ceil(),
            max: self.max.ceil(),
        }
    }

    /// Round outward (floor min, ceil max) - useful for pixel-perfect rendering.
    #[inline]
    #[must_use]
    pub fn round_out(&self) -> Rect {
        Rect {
            min: self.min.floor(),
            max: self.max.ceil(),
        }
    }

    /// Round inward (ceil min, floor max) - useful for clipping.
    #[inline]
    #[must_use]
    pub fn round_in(&self) -> Rect {
        Rect {
            min: self.min.ceil(),
            max: self.max.floor(),
        }
    }

    /// Clamp a point to be inside this rectangle.
    #[inline]
    #[must_use]
    pub fn clamp_point(&self, point: Point) -> Point {
        point.clamp(self.min, self.max)
    }

    /// Check if rect fully contains another rect.
    #[inline]
    #[must_use]
    pub fn contains_rect(&self, other: &Rect) -> bool {
        self.min.x <= other.min.x
            && self.max.x >= other.max.x
            && self.min.y <= other.min.y
            && self.max.y >= other.max.y
    }

    /// Linear interpolation between two rects.
    #[must_use]
    pub fn lerp(a: &Rect, b: &Rect, t: f32) -> Rect {
        Rect {
            min: Point::lerp(a.min, b.min, t),
            max: Point::lerp(a.max, b.max, t),
        }
    }

    /// Create a rectangle centered on a specific point.
    ///
    /// Useful for positioning dialogs, tooltips, or popups around a point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Rect, Size, Point};
    ///
    /// let size = Size::new(100.0, 50.0);
    /// let center = Point::new(200.0, 150.0);
    /// let rect = Rect::center_on(center, size);
    ///
    /// assert_eq!(rect.center(), center);
    /// assert_eq!(rect.size(), size);
    /// ```
    #[inline]
    #[must_use]
    pub fn center_on(center: impl Into<Point>, size: impl Into<Size>) -> Self {
        let center = center.into();
        let size = size.into();
        let half_width = size.width * 0.5;
        let half_height = size.height * 0.5;

        Rect {
            min: Point::new(center.x - half_width, center.y - half_height),
            max: Point::new(center.x + half_width, center.y + half_height),
        }
    }

    /// Create a bounding rectangle from a collection of points.
    ///
    /// Returns the smallest rectangle that contains all the given points.
    /// Returns None if the slice is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Rect, Point};
    ///
    /// let points = vec![
    ///     Point::new(10.0, 20.0),
    ///     Point::new(50.0, 30.0),
    ///     Point::new(30.0, 60.0),
    /// ];
    ///
    /// let bounds = Rect::from_points(&points).unwrap();
    /// assert_eq!(bounds.min, Point::new(10.0, 20.0));
    /// assert_eq!(bounds.max, Point::new(50.0, 60.0));
    /// ```
    #[must_use]
    pub fn from_points(points: &[Point]) -> Option<Self> {
        if points.is_empty() {
            return None;
        }

        let mut min_x = points[0].x;
        let mut min_y = points[0].y;
        let mut max_x = points[0].x;
        let mut max_y = points[0].y;

        for point in &points[1..] {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }

        Some(Rect {
            min: Point::new(min_x, min_y),
            max: Point::new(max_x, max_y),
        })
    }

    /// Shrink this rectangle by the given edge insets.
    ///
    /// This is the opposite of `inflate()` - it applies insets to shrink the rectangle.
    /// Commonly used for applying padding or margins.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Rect, Point};
    /// use flui_types::layout::EdgeInsets;
    ///
    /// let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
    /// let insets = EdgeInsets::all(10.0);
    /// let inset_rect = rect.inset_by(&insets);
    ///
    /// assert_eq!(inset_rect, Rect::from_xywh(10.0, 10.0, 80.0, 80.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn inset_by(&self, insets: &crate::layout::EdgeInsets) -> Self {
        Rect {
            min: Point::new(self.min.x + insets.left, self.min.y + insets.top),
            max: Point::new(self.max.x - insets.right, self.max.y - insets.bottom),
        }
    }

    /// Expand this rectangle to include the given point.
    ///
    /// If the point is already inside, returns the same rectangle.
    /// Otherwise, expands the minimum bounds to include the point.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Rect, Point};
    ///
    /// let rect = Rect::from_xywh(10.0, 10.0, 20.0, 20.0);
    ///
    /// // Point inside - no change
    /// let expanded = rect.expanded_to_include(Point::new(15.0, 15.0));
    /// assert_eq!(expanded, rect);
    ///
    /// // Point outside - expands
    /// let expanded = rect.expanded_to_include(Point::new(50.0, 5.0));
    /// assert_eq!(expanded.min, Point::new(10.0, 5.0));
    /// assert_eq!(expanded.max, Point::new(50.0, 30.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn expanded_to_include(&self, point: impl Into<Point>) -> Self {
        let point = point.into();
        Rect {
            min: Point::new(self.min.x.min(point.x), self.min.y.min(point.y)),
            max: Point::new(self.max.x.max(point.x), self.max.y.max(point.y)),
        }
    }

    /// Check if this rectangle intersects with any rectangle in a batch.
    ///
    /// Returns true if there's at least one intersection.
    /// Faster than checking each rectangle individually in a loop.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Rect, Point};
    ///
    /// let rect = Rect::from_min_max(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
    /// let rects = vec![
    ///     Rect::from_min_max(Point::new(20.0, 20.0), Point::new(30.0, 30.0)), // No intersection
    ///     Rect::from_min_max(Point::new(5.0, 5.0), Point::new(15.0, 15.0)),   // Intersection!
    /// ];
    ///
    /// assert!(rect.intersects_any(&rects));
    /// ```
    #[inline]
    #[must_use]
    pub fn intersects_any(&self, rects: &[Rect]) -> bool {
        rects.iter().any(|r| self.intersects(r))
    }

    /// Batch intersection test - get all rectangles that intersect with this one.
    ///
    /// Returns indices of rectangles that intersect.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Rect, Point};
    ///
    /// let rect = Rect::from_min_max(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
    /// let rects = vec![
    ///     Rect::from_min_max(Point::new(20.0, 20.0), Point::new(30.0, 30.0)),
    ///     Rect::from_min_max(Point::new(5.0, 5.0), Point::new(15.0, 15.0)),
    ///     Rect::from_min_max(Point::new(8.0, 8.0), Point::new(12.0, 12.0)),
    /// ];
    ///
    /// let intersecting = rect.intersecting_indices(&rects);
    /// assert_eq!(intersecting, vec![1, 2]);
    /// ```
    #[must_use]
    pub fn intersecting_indices(&self, rects: &[Rect]) -> Vec<usize> {
        rects
            .iter()
            .enumerate()
            .filter_map(|(i, r)| if self.intersects(r) { Some(i) } else { None })
            .collect()
    }
}

impl Default for Rect {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Rect[{:.1}, {:.1}; {:.1}×{:.1}]",
            self.min.x,
            self.min.y,
            self.width(),
            self.height()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_from_min_max() {
        let rect = Rect::from_min_max(Point::new(0.0, 0.0), Point::new(10.0, 20.0));
        assert_eq!(rect.width(), 10.0);
        assert_eq!(rect.height(), 20.0);
    }

    #[test]
    fn test_rect_from_min_size() {
        let rect = Rect::from_min_size(Point::new(5.0, 5.0), Size::new(10.0, 20.0));
        assert_eq!(rect.min, Point::new(5.0, 5.0));
        assert_eq!(rect.max, Point::new(15.0, 25.0));
    }

    #[test]
    fn test_rect_from_center_size() {
        let rect = Rect::from_center_size(Point::new(10.0, 10.0), Size::new(20.0, 10.0));
        assert_eq!(rect.min, Point::new(0.0, 5.0));
        assert_eq!(rect.max, Point::new(20.0, 15.0));
        assert_eq!(rect.center(), Point::new(10.0, 10.0));
    }

    #[test]
    fn test_rect_from_xywh() {
        let rect = Rect::from_xywh(5.0, 10.0, 20.0, 30.0);
        assert_eq!(rect.left(), 5.0);
        assert_eq!(rect.top(), 10.0);
        assert_eq!(rect.width(), 20.0);
        assert_eq!(rect.height(), 30.0);
    }

    #[test]
    fn test_rect_dimensions() {
        let rect = Rect::from_xywh(0.0, 0.0, 10.0, 20.0);
        assert_eq!(rect.width(), 10.0);
        assert_eq!(rect.height(), 20.0);
        assert_eq!(rect.size(), Size::new(10.0, 20.0));
        assert_eq!(rect.area(), 200.0);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);

        assert!(rect.contains(Point::new(5.0, 5.0)));
        assert!(rect.contains(Point::new(0.0, 0.0)));
        assert!(rect.contains(Point::new(10.0, 10.0)));
        assert!(!rect.contains(Point::new(-1.0, 5.0)));
        assert!(!rect.contains(Point::new(11.0, 5.0)));
    }

    #[test]
    fn test_rect_intersects() {
        let r1 = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
        let r2 = Rect::from_xywh(5.0, 5.0, 10.0, 10.0);
        let r3 = Rect::from_xywh(20.0, 20.0, 10.0, 10.0);

        assert!(r1.intersects(&r2));
        assert!(!r1.intersects(&r3));
    }

    #[test]
    fn test_rect_union() {
        let r1 = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
        let r2 = Rect::from_xywh(5.0, 5.0, 10.0, 10.0);

        let union = r1.union(&r2);
        assert_eq!(union.min, Point::new(0.0, 0.0));
        assert_eq!(union.max, Point::new(15.0, 15.0));
    }

    #[test]
    fn test_rect_intersection() {
        let r1 = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
        let r2 = Rect::from_xywh(5.0, 5.0, 10.0, 10.0);

        let intersection = r1.intersection(&r2).unwrap();
        assert_eq!(intersection.min, Point::new(5.0, 5.0));
        assert_eq!(intersection.max, Point::new(10.0, 10.0));

        let r3 = Rect::from_xywh(20.0, 20.0, 10.0, 10.0);
        assert!(r1.intersection(&r3).is_none());
    }

    #[test]
    fn test_rect_expand_shrink() {
        let rect = Rect::from_xywh(10.0, 10.0, 10.0, 10.0);

        let expanded = rect.expand(5.0);
        assert_eq!(expanded.min, Point::new(5.0, 5.0));
        assert_eq!(expanded.max, Point::new(25.0, 25.0));

        let shrunk = rect.shrink(2.0);
        assert_eq!(shrunk.min, Point::new(12.0, 12.0));
        assert_eq!(shrunk.max, Point::new(18.0, 18.0));
    }

    #[test]
    fn test_rect_is_empty() {
        let rect1 = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
        assert!(!rect1.is_empty());

        let rect2 = Rect::from_xywh(0.0, 0.0, 0.0, 10.0);
        assert!(rect2.is_empty());

        let rect3 = Rect::from_xywh(0.0, 0.0, -5.0, 10.0);
        assert!(rect3.is_empty());
    }

    #[test]
    fn test_rect_aspect_ratio() {
        let rect = Rect::from_xywh(0.0, 0.0, 16.0, 9.0);
        assert_eq!(rect.aspect_ratio(), Some(16.0 / 9.0));

        let zero_height = Rect::from_xywh(0.0, 0.0, 10.0, 0.0);
        assert_eq!(zero_height.aspect_ratio(), None);
    }

    #[test]
    fn test_rect_edges() {
        let rect = Rect::from_xywh(5.0, 10.0, 20.0, 30.0);
        assert_eq!(rect.left(), 5.0);
        assert_eq!(rect.right(), 25.0);
        assert_eq!(rect.top(), 10.0);
        assert_eq!(rect.bottom(), 40.0);
    }

    #[test]
    fn test_rect_display() {
        let rect = Rect::from_xywh(10.0, 20.0, 30.0, 40.0);
        let display = format!("{}", rect);
        assert!(display.contains("10.0"));
        assert!(display.contains("20.0"));
        assert!(display.contains("30.0"));
        assert!(display.contains("40.0"));
    }
}
