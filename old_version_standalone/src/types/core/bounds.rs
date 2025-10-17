//! Bounding box types for collision detection and spatial optimization.
//!
//! This module provides center-based bounding boxes for physics and game systems.

use super::point::Point;
use super::rect::Rect;
use super::size::Size;
use super::vector::Vector2;

/// Center-based bounding box (center + half-extents).
///
/// Unlike [`Rect`] which uses min/max corners, [`Bounds`] uses center and extents,
/// which is often more convenient for physics and collision detection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Center point of the bounding box.
    pub center: Point,
    /// Half-size in each direction (radius from center).
    pub extents: Vector2,
}

impl Bounds {
    /// Create bounds from center and extents (half-size).
    pub const fn new(center: Point, extents: Vector2) -> Self {
        Self { center, extents }
    }

    /// Create bounds from center and full size.
    pub fn from_center_size(center: impl Into<Point>, size: impl Into<Size>) -> Self {
        let center = center.into();
        let size = size.into();
        Self {
            center,
            extents: Vector2::new(size.width * 0.5, size.height * 0.5),
        }
    }

    /// Create bounds from min and max points.
    pub fn from_min_max(min: impl Into<Point>, max: impl Into<Point>) -> Self {
        let min = min.into();
        let max = max.into();
        let center = Point::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5);
        let extents = Vector2::new((max.x - min.x) * 0.5, (max.y - min.y) * 0.5);
        Self { center, extents }
    }

    /// Get the minimum corner point.
    pub fn min(&self) -> Point {
        Point::new(
            self.center.x - self.extents.x,
            self.center.y - self.extents.y,
        )
    }

    /// Get the maximum corner point.
    pub fn max(&self) -> Point {
        Point::new(
            self.center.x + self.extents.x,
            self.center.y + self.extents.y,
        )
    }

    /// Get the full size of the bounding box.
    pub fn size(&self) -> Size {
        Size::new(self.extents.x * 2.0, self.extents.y * 2.0)
    }

    /// Get the width of the bounding box.
    pub fn width(&self) -> f32 {
        self.extents.x * 2.0
    }

    /// Get the height of the bounding box.
    pub fn height(&self) -> f32 {
        self.extents.y * 2.0
    }

    /// Check if this bounds contains a point.
    pub fn contains(&self, point: impl Into<Point>) -> bool {
        let point = point.into();
        let dx = (point.x - self.center.x).abs();
        let dy = (point.y - self.center.y).abs();
        dx <= self.extents.x && dy <= self.extents.y
    }

    /// Check if this bounds intersects another bounds.
    pub fn intersects(&self, other: &Bounds) -> bool {
        let dx = (self.center.x - other.center.x).abs();
        let dy = (self.center.y - other.center.y).abs();
        dx <= (self.extents.x + other.extents.x) && dy <= (self.extents.y + other.extents.y)
    }

    /// Check if this bounds completely contains another bounds.
    pub fn contains_bounds(&self, other: &Bounds) -> bool {
        // Check if all corners of other are inside self
        let other_min = other.min();
        let other_max = other.max();
        let self_min = self.min();
        let self_max = self.max();

        other_min.x >= self_min.x
            && other_min.y >= self_min.y
            && other_max.x <= self_max.x
            && other_max.y <= self_max.y
    }

    /// Expand the bounds by a margin in all directions.
    pub fn expand(&self, margin: f32) -> Bounds {
        Bounds {
            center: self.center,
            extents: Vector2::new(
                self.extents.x + margin,
                self.extents.y + margin,
            ),
        }
    }

    /// Shrink the bounds by a margin in all directions.
    pub fn shrink(&self, margin: f32) -> Bounds {
        self.expand(-margin)
    }

    /// Translate the bounds by an offset.
    pub fn translate(&self, offset: impl Into<Vector2>) -> Bounds {
        let offset = offset.into();
        Bounds {
            center: Point::new(
                self.center.x + offset.x,
                self.center.y + offset.y,
            ),
            extents: self.extents,
        }
    }

    /// Get the closest point on the bounds to a given point.
    pub fn closest_point(&self, point: impl Into<Point>) -> Point {
        let point = point.into();
        let min = self.min();
        let max = self.max();
        Point::new(
            point.x.clamp(min.x, max.x),
            point.y.clamp(min.y, max.y),
        )
    }

    /// Get the distance from the bounds to a point (0 if inside).
    pub fn distance_to_point(&self, point: impl Into<Point>) -> f32 {
        let point = point.into();
        let closest = self.closest_point(point);
        point.distance_to(closest)
    }

    /// Merge with another bounds to create the smallest bounds containing both.
    pub fn merge(&self, other: &Bounds) -> Bounds {
        let min = Point::min(self.min(), other.min());
        let max = Point::max(self.max(), other.max());
        Bounds::from_min_max(min, max)
    }

    /// Get the intersection of two bounds.
    pub fn intersection(&self, other: &Bounds) -> Option<Bounds> {
        if !self.intersects(other) {
            return None;
        }

        let min = Point::max(self.min(), other.min());
        let max = Point::min(self.max(), other.max());

        if min.x <= max.x && min.y <= max.y {
            Some(Bounds::from_min_max(min, max))
        } else {
            None
        }
    }

    /// Convert to a Rect.
    pub fn to_rect(&self) -> Rect {
        Rect::from_min_max(self.min(), self.max())
    }

    /// Check if the bounds is valid (positive extents).
    pub fn is_valid(&self) -> bool {
        self.extents.x >= 0.0 && self.extents.y >= 0.0
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            center: Point::ZERO,
            extents: Vector2::ZERO,
        }
    }
}

impl From<Rect> for Bounds {
    fn from(rect: Rect) -> Self {
        Bounds::from_min_max(rect.min, rect.max)
    }
}

impl From<Bounds> for Rect {
    fn from(bounds: Bounds) -> Self {
        bounds.to_rect()
    }
}

impl std::fmt::Display for Bounds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bounds[center: {}, extents: {:.1}×{:.1}]",
            self.center, self.extents.x, self.extents.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounds_creation() {
        let bounds = Bounds::new(
            Point::new(10.0, 20.0),
            Vector2::new(5.0, 3.0),
        );
        assert_eq!(bounds.center, Point::new(10.0, 20.0));
        assert_eq!(bounds.extents, Vector2::new(5.0, 3.0));
    }

    #[test]
    fn test_bounds_from_center_size() {
        let bounds = Bounds::from_center_size(
            Point::new(10.0, 20.0),
            Size::new(20.0, 10.0),
        );
        assert_eq!(bounds.center, Point::new(10.0, 20.0));
        assert_eq!(bounds.extents, Vector2::new(10.0, 5.0));
    }

    #[test]
    fn test_bounds_from_min_max() {
        let bounds = Bounds::from_min_max(
            Point::new(5.0, 10.0),
            Point::new(15.0, 20.0),
        );
        assert_eq!(bounds.center, Point::new(10.0, 15.0));
        assert_eq!(bounds.extents, Vector2::new(5.0, 5.0));
    }

    #[test]
    fn test_bounds_min_max() {
        let bounds = Bounds::new(
            Point::new(10.0, 20.0),
            Vector2::new(5.0, 3.0),
        );
        assert_eq!(bounds.min(), Point::new(5.0, 17.0));
        assert_eq!(bounds.max(), Point::new(15.0, 23.0));
    }

    #[test]
    fn test_bounds_size() {
        let bounds = Bounds::new(
            Point::new(0.0, 0.0),
            Vector2::new(10.0, 5.0),
        );
        assert_eq!(bounds.size(), Size::new(20.0, 10.0));
        assert_eq!(bounds.width(), 20.0);
        assert_eq!(bounds.height(), 10.0);
    }

    #[test]
    fn test_bounds_contains() {
        let bounds = Bounds::from_center_size(
            Point::new(10.0, 10.0),
            Size::new(20.0, 20.0),
        );

        assert!(bounds.contains(Point::new(10.0, 10.0))); // center
        assert!(bounds.contains(Point::new(0.0, 0.0))); // corner
        assert!(bounds.contains(Point::new(20.0, 20.0))); // corner
        assert!(!bounds.contains(Point::new(-1.0, 10.0))); // outside
        assert!(!bounds.contains(Point::new(21.0, 10.0))); // outside
    }

    #[test]
    fn test_bounds_intersects() {
        let b1 = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(10.0, 10.0));
        let b2 = Bounds::from_center_size(Point::new(12.0, 12.0), Size::new(10.0, 10.0));
        let b3 = Bounds::from_center_size(Point::new(30.0, 30.0), Size::new(10.0, 10.0));

        assert!(b1.intersects(&b2)); // overlapping
        assert!(!b1.intersects(&b3)); // separate
    }

    #[test]
    fn test_bounds_contains_bounds() {
        let outer = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(20.0, 20.0));
        let inner = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(10.0, 10.0));
        let overlapping = Bounds::from_center_size(Point::new(16.0, 16.0), Size::new(10.0, 10.0));

        assert!(outer.contains_bounds(&inner));
        assert!(!outer.contains_bounds(&overlapping));
    }

    #[test]
    fn test_bounds_expand_shrink() {
        let bounds = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(10.0, 10.0));

        let expanded = bounds.expand(5.0);
        assert_eq!(expanded.extents, Vector2::new(10.0, 10.0));
        assert_eq!(expanded.size(), Size::new(20.0, 20.0));

        let shrunk = bounds.shrink(2.0);
        assert_eq!(shrunk.extents, Vector2::new(3.0, 3.0));
    }

    #[test]
    fn test_bounds_translate() {
        let bounds = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(10.0, 10.0));
        let translated = bounds.translate(Vector2::new(5.0, -3.0));

        assert_eq!(translated.center, Point::new(15.0, 7.0));
        assert_eq!(translated.extents, bounds.extents);
    }

    #[test]
    fn test_bounds_closest_point() {
        let bounds = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(10.0, 10.0));

        // Point inside -> same point
        let inside = Point::new(10.0, 10.0);
        assert_eq!(bounds.closest_point(inside), inside);

        // Point outside -> clamped to edge
        let outside = Point::new(20.0, 25.0);
        let closest = bounds.closest_point(outside);
        assert_eq!(closest, Point::new(15.0, 15.0));
    }

    #[test]
    fn test_bounds_distance_to_point() {
        let bounds = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(10.0, 10.0));

        // Point inside -> distance 0
        assert_eq!(bounds.distance_to_point(Point::new(10.0, 10.0)), 0.0);

        // Point outside -> distance to closest edge
        let outside = Point::new(20.0, 10.0);
        assert_eq!(bounds.distance_to_point(outside), 5.0);
    }

    #[test]
    fn test_bounds_merge() {
        let b1 = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        let b2 = Bounds::from_min_max(Point::new(5.0, 5.0), Point::new(15.0, 15.0));

        let merged = b1.merge(&b2);
        assert_eq!(merged.min(), Point::new(0.0, 0.0));
        assert_eq!(merged.max(), Point::new(15.0, 15.0));
    }

    #[test]
    fn test_bounds_intersection() {
        let b1 = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        let b2 = Bounds::from_min_max(Point::new(5.0, 5.0), Point::new(15.0, 15.0));
        let b3 = Bounds::from_min_max(Point::new(20.0, 20.0), Point::new(30.0, 30.0));

        let intersection = b1.intersection(&b2).unwrap();
        assert_eq!(intersection.min(), Point::new(5.0, 5.0));
        assert_eq!(intersection.max(), Point::new(10.0, 10.0));

        assert!(b1.intersection(&b3).is_none());
    }

    #[test]
    fn test_bounds_rect_conversion() {
        let bounds = Bounds::from_center_size(Point::new(10.0, 10.0), Size::new(20.0, 20.0));
        let rect = bounds.to_rect();

        assert_eq!(rect.min, Point::new(0.0, 0.0));
        assert_eq!(rect.max, Point::new(20.0, 20.0));

        let back: Bounds = rect.into();
        assert_eq!(back.center, bounds.center);
        assert_eq!(back.extents, bounds.extents);
    }

    #[test]
    fn test_bounds_is_valid() {
        assert!(Bounds::from_center_size(Point::ZERO, Size::new(10.0, 10.0)).is_valid());
        assert!(!Bounds::new(Point::ZERO, Vector2::new(-5.0, 10.0)).is_valid());
    }

    #[test]
    fn test_bounds_display() {
        let bounds = Bounds::from_center_size(Point::new(10.0, 20.0), Size::new(8.0, 6.0));
        let display = format!("{}", bounds);
        assert!(display.contains("10"));
        assert!(display.contains("20"));
    }
}
