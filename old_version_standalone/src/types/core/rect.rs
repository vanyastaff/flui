//! Rectangle types for bounding boxes and regions.
//!
//! This module provides type-safe rectangle types for representing bounding boxes,
//! clip regions, and layout bounds.

use super::offset::Offset;
use super::point::Point;
use super::size::Size;
use egui::Rect as EguiRect;

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

    /// Get the width of the rectangle.
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Get the height of the rectangle.
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Get the size of the rectangle.
    pub fn size(&self) -> Size {
        Size::new(self.width(), self.height())
    }

    /// Get the center point of the rectangle.
    pub fn center(&self) -> Point {
        Point::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    /// Get the area of the rectangle.
    pub fn area(&self) -> f32 {
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
    pub fn contains(&self, point: impl Into<Point>) -> bool {
        let point = point.into();
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Check if this rectangle intersects another.
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

    /// Translate the rectangle by an offset.
    pub fn translate(&self, offset: impl Into<Offset>) -> Rect {
        let offset = offset.into();
        Rect {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    /// Check if the rectangle is empty (zero or negative size).
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// Check if the rectangle has finite coordinates.
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }

    /// Get the four corners of the rectangle.
    pub fn corners(&self) -> RectCorners {
        RectCorners {
            top_left: self.min,
            top_right: Point::new(self.max.x, self.min.y),
            bottom_left: Point::new(self.min.x, self.max.y),
            bottom_right: self.max,
        }
    }

    /// Get the left edge X coordinate.
    pub fn left(&self) -> f32 {
        self.min.x
    }

    /// Get the right edge X coordinate.
    pub fn right(&self) -> f32 {
        self.max.x
    }

    /// Get the top edge Y coordinate.
    pub fn top(&self) -> f32 {
        self.min.y
    }

    /// Get the bottom edge Y coordinate.
    pub fn bottom(&self) -> f32 {
        self.max.y
    }
}

impl Default for Rect {
    fn default() -> Self {
        Self::ZERO
    }
}

// Conversions
impl From<EguiRect> for Rect {
    fn from(rect: EguiRect) -> Self {
        Self {
            min: rect.min.into(),
            max: rect.max.into(),
        }
    }
}

impl From<Rect> for EguiRect {
    fn from(rect: Rect) -> Self {
        EguiRect {
            min: rect.min.into(),
            max: rect.max.into(),
        }
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

/// The four corners of a rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectCorners {
    pub top_left: Point,
    pub top_right: Point,
    pub bottom_left: Point,
    pub bottom_right: Point,
}

impl RectCorners {
    /// Create corners from individual points.
    pub const fn new(
        top_left: Point,
        top_right: Point,
        bottom_left: Point,
        bottom_right: Point,
    ) -> Self {
        Self {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        }
    }

    /// Get all corners as an array [TL, TR, BR, BL].
    pub fn as_array(&self) -> [Point; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }

    /// Get the bounding rectangle of these corners.
    pub fn bounding_rect(&self) -> Rect {
        let mut min_x = self.top_left.x;
        let mut max_x = self.top_left.x;
        let mut min_y = self.top_left.y;
        let mut max_y = self.top_left.y;

        for point in &[self.top_right, self.bottom_left, self.bottom_right] {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }

        Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
    }
}

impl From<Rect> for RectCorners {
    fn from(rect: Rect) -> Self {
        rect.corners()
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
    fn test_rect_translate() {
        let rect = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
        let translated = rect.translate(Offset::new(5.0, 3.0));

        assert_eq!(translated.min, Point::new(5.0, 3.0));
        assert_eq!(translated.max, Point::new(15.0, 13.0));
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
    fn test_rect_corners() {
        let rect = Rect::from_xywh(10.0, 20.0, 30.0, 40.0);
        let corners = rect.corners();

        assert_eq!(corners.top_left, Point::new(10.0, 20.0));
        assert_eq!(corners.top_right, Point::new(40.0, 20.0));
        assert_eq!(corners.bottom_left, Point::new(10.0, 60.0));
        assert_eq!(corners.bottom_right, Point::new(40.0, 60.0));
    }

    #[test]
    fn test_rect_corners_bounding_rect() {
        let rect = Rect::from_xywh(10.0, 20.0, 30.0, 40.0);
        let corners = rect.corners();
        let bounding = corners.bounding_rect();

        assert_eq!(bounding.min, rect.min);
        assert_eq!(bounding.max, rect.max);
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
    fn test_rect_egui_conversions() {
        use egui::{Pos2, Rect as EguiRect};

        let egui_rect = EguiRect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(10.0, 20.0));
        let rect: Rect = egui_rect.into();

        assert_eq!(rect.width(), 10.0);
        assert_eq!(rect.height(), 20.0);

        let back: EguiRect = rect.into();
        assert_eq!(back, egui_rect);
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
