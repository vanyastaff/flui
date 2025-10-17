//! Range types for value intervals and bounds.
//!
//! This module provides 1D and 2D range types for representing intervals of values.

use super::point::Point;
use super::rect::Rect;

/// 1D range with start and end values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range1D {
    pub start: f32,
    pub end: f32,
}

impl Range1D {
    /// Create a new 1D range.
    pub const fn new(start: f32, end: f32) -> Self {
        Self { start, end }
    }

    /// Create a range from 0 to value.
    pub const fn from_zero(end: f32) -> Self {
        Self { start: 0.0, end }
    }

    /// Get the length/size of the range.
    pub fn length(&self) -> f32 {
        (self.end - self.start).abs()
    }

    /// Get the center point of the range.
    pub fn center(&self) -> f32 {
        (self.start + self.end) * 0.5
    }

    /// Check if a value is within this range (inclusive).
    pub fn contains(&self, value: f32) -> bool {
        if self.start <= self.end {
            value >= self.start && value <= self.end
        } else {
            value >= self.end && value <= self.start
        }
    }

    /// Check if this range overlaps with another range.
    pub fn overlaps(&self, other: &Range1D) -> bool {
        let (min1, max1) = (self.start.min(self.end), self.start.max(self.end));
        let (min2, max2) = (other.start.min(other.end), other.start.max(other.end));
        min1 <= max2 && max1 >= min2
    }

    /// Clamp a value to this range.
    pub fn clamp(&self, value: f32) -> f32 {
        if self.start <= self.end {
            value.clamp(self.start, self.end)
        } else {
            value.clamp(self.end, self.start)
        }
    }

    /// Linear interpolation within the range.
    pub fn lerp(&self, t: f32) -> f32 {
        self.start + (self.end - self.start) * t
    }

    /// Get the normalized position of a value within the range (0.0 to 1.0).
    pub fn inverse_lerp(&self, value: f32) -> f32 {
        let len = self.end - self.start;
        if len.abs() < f32::EPSILON {
            0.0
        } else {
            (value - self.start) / len
        }
    }

    /// Map a value from this range to another range.
    pub fn map_to(&self, value: f32, target: &Range1D) -> f32 {
        let t = self.inverse_lerp(value);
        target.lerp(t)
    }

    /// Expand the range by a margin on both sides.
    pub fn expand(&self, margin: f32) -> Range1D {
        Range1D {
            start: self.start - margin,
            end: self.end + margin,
        }
    }

    /// Get the minimum value (accounting for inverted ranges).
    pub fn min(&self) -> f32 {
        self.start.min(self.end)
    }

    /// Get the maximum value (accounting for inverted ranges).
    pub fn max(&self) -> f32 {
        self.start.max(self.end)
    }

    /// Check if the range is empty (zero length).
    pub fn is_empty(&self) -> bool {
        (self.end - self.start).abs() < f32::EPSILON
    }

    /// Check if the range is inverted (end < start).
    pub fn is_inverted(&self) -> bool {
        self.end < self.start
    }

    /// Normalize the range (ensure start <= end).
    pub fn normalize(&self) -> Range1D {
        if self.is_inverted() {
            Range1D::new(self.end, self.start)
        } else {
            *self
        }
    }
}

impl Default for Range1D {
    fn default() -> Self {
        Self::new(0.0, 1.0)
    }
}

impl From<(f32, f32)> for Range1D {
    fn from((start, end): (f32, f32)) -> Self {
        Self::new(start, end)
    }
}

impl From<Range1D> for (f32, f32) {
    fn from(range: Range1D) -> Self {
        (range.start, range.end)
    }
}

impl std::fmt::Display for Range1D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:.2}..{:.2}]", self.start, self.end)
    }
}

/// 2D range with X and Y ranges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range2D {
    pub x: Range1D,
    pub y: Range1D,
}

impl Range2D {
    /// Create a new 2D range.
    pub const fn new(x: Range1D, y: Range1D) -> Self {
        Self { x, y }
    }

    /// Create a 2D range from separate x and y start/end values.
    pub const fn from_values(x_start: f32, x_end: f32, y_start: f32, y_end: f32) -> Self {
        Self {
            x: Range1D::new(x_start, x_end),
            y: Range1D::new(y_start, y_end),
        }
    }

    /// Create a 2D range from a rectangle.
    pub fn from_rect(rect: impl Into<Rect>) -> Self {
        let rect = rect.into();
        Self {
            x: Range1D::new(rect.min.x, rect.max.x),
            y: Range1D::new(rect.min.y, rect.max.y),
        }
    }

    /// Get the size of the range.
    pub fn size(&self) -> (f32, f32) {
        (self.x.length(), self.y.length())
    }

    /// Get the center point of the range.
    pub fn center(&self) -> Point {
        Point::new(self.x.center(), self.y.center())
    }

    /// Check if a point is within this range.
    pub fn contains(&self, point: impl Into<Point>) -> bool {
        let point = point.into();
        self.x.contains(point.x) && self.y.contains(point.y)
    }

    /// Check if this range overlaps with another range.
    pub fn overlaps(&self, other: &Range2D) -> bool {
        self.x.overlaps(&other.x) && self.y.overlaps(&other.y)
    }

    /// Clamp a point to this range.
    pub fn clamp(&self, point: impl Into<Point>) -> Point {
        let point = point.into();
        Point::new(self.x.clamp(point.x), self.y.clamp(point.y))
    }

    /// Linear interpolation within the range.
    pub fn lerp(&self, tx: f32, ty: f32) -> Point {
        Point::new(self.x.lerp(tx), self.y.lerp(ty))
    }

    /// Get the normalized position of a point within the range.
    pub fn inverse_lerp(&self, point: impl Into<Point>) -> (f32, f32) {
        let point = point.into();
        (
            self.x.inverse_lerp(point.x),
            self.y.inverse_lerp(point.y),
        )
    }

    /// Expand the range by margins.
    pub fn expand(&self, x_margin: f32, y_margin: f32) -> Range2D {
        Range2D {
            x: self.x.expand(x_margin),
            y: self.y.expand(y_margin),
        }
    }

    /// Expand the range uniformly.
    pub fn expand_uniform(&self, margin: f32) -> Range2D {
        self.expand(margin, margin)
    }

    /// Convert to a Rect.
    pub fn to_rect(&self) -> Rect {
        Rect::from_min_max(
            Point::new(self.x.start, self.y.start),
            Point::new(self.x.end, self.y.end),
        )
    }

    /// Check if the range is empty (zero area).
    pub fn is_empty(&self) -> bool {
        self.x.is_empty() || self.y.is_empty()
    }

    /// Normalize both axes (ensure start <= end).
    pub fn normalize(&self) -> Range2D {
        Range2D {
            x: self.x.normalize(),
            y: self.y.normalize(),
        }
    }
}

impl Default for Range2D {
    fn default() -> Self {
        Self {
            x: Range1D::default(),
            y: Range1D::default(),
        }
    }
}

impl From<Rect> for Range2D {
    fn from(rect: Rect) -> Self {
        Range2D::from_rect(rect)
    }
}

impl From<Range2D> for Rect {
    fn from(range: Range2D) -> Self {
        range.to_rect()
    }
}

impl std::fmt::Display for Range2D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[x:{}, y:{}]", self.x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range1d_creation() {
        let range = Range1D::new(0.0, 10.0);
        assert_eq!(range.start, 0.0);
        assert_eq!(range.end, 10.0);
        assert_eq!(range.length(), 10.0);
    }

    #[test]
    fn test_range1d_from_zero() {
        let range = Range1D::from_zero(100.0);
        assert_eq!(range.start, 0.0);
        assert_eq!(range.end, 100.0);
    }

    #[test]
    fn test_range1d_center() {
        let range = Range1D::new(0.0, 10.0);
        assert_eq!(range.center(), 5.0);
    }

    #[test]
    fn test_range1d_contains() {
        let range = Range1D::new(0.0, 10.0);
        assert!(range.contains(5.0));
        assert!(range.contains(0.0));
        assert!(range.contains(10.0));
        assert!(!range.contains(-1.0));
        assert!(!range.contains(11.0));
    }

    #[test]
    fn test_range1d_overlaps() {
        let r1 = Range1D::new(0.0, 10.0);
        let r2 = Range1D::new(5.0, 15.0);
        let r3 = Range1D::new(20.0, 30.0);

        assert!(r1.overlaps(&r2));
        assert!(!r1.overlaps(&r3));
    }

    #[test]
    fn test_range1d_clamp() {
        let range = Range1D::new(0.0, 10.0);
        assert_eq!(range.clamp(-5.0), 0.0);
        assert_eq!(range.clamp(5.0), 5.0);
        assert_eq!(range.clamp(15.0), 10.0);
    }

    #[test]
    fn test_range1d_lerp() {
        let range = Range1D::new(0.0, 10.0);
        assert_eq!(range.lerp(0.0), 0.0);
        assert_eq!(range.lerp(0.5), 5.0);
        assert_eq!(range.lerp(1.0), 10.0);
    }

    #[test]
    fn test_range1d_inverse_lerp() {
        let range = Range1D::new(0.0, 10.0);
        assert_eq!(range.inverse_lerp(0.0), 0.0);
        assert_eq!(range.inverse_lerp(5.0), 0.5);
        assert_eq!(range.inverse_lerp(10.0), 1.0);
    }

    #[test]
    fn test_range1d_map_to() {
        let from = Range1D::new(0.0, 10.0);
        let to = Range1D::new(0.0, 100.0);

        assert_eq!(from.map_to(5.0, &to), 50.0);
        assert_eq!(from.map_to(2.5, &to), 25.0);
    }

    #[test]
    fn test_range1d_expand() {
        let range = Range1D::new(5.0, 10.0);
        let expanded = range.expand(2.0);
        assert_eq!(expanded.start, 3.0);
        assert_eq!(expanded.end, 12.0);
    }

    #[test]
    fn test_range1d_min_max() {
        let range = Range1D::new(5.0, 10.0);
        assert_eq!(range.min(), 5.0);
        assert_eq!(range.max(), 10.0);

        let inverted = Range1D::new(10.0, 5.0);
        assert_eq!(inverted.min(), 5.0);
        assert_eq!(inverted.max(), 10.0);
    }

    #[test]
    fn test_range1d_inverted() {
        let normal = Range1D::new(0.0, 10.0);
        assert!(!normal.is_inverted());

        let inverted = Range1D::new(10.0, 0.0);
        assert!(inverted.is_inverted());
    }

    #[test]
    fn test_range1d_normalize() {
        let inverted = Range1D::new(10.0, 0.0);
        let normalized = inverted.normalize();
        assert_eq!(normalized.start, 0.0);
        assert_eq!(normalized.end, 10.0);
    }

    #[test]
    fn test_range2d_creation() {
        let range = Range2D::new(
            Range1D::new(0.0, 10.0),
            Range1D::new(0.0, 20.0),
        );
        assert_eq!(range.x.start, 0.0);
        assert_eq!(range.y.end, 20.0);
    }

    #[test]
    fn test_range2d_from_values() {
        let range = Range2D::from_values(0.0, 10.0, 5.0, 15.0);
        assert_eq!(range.x, Range1D::new(0.0, 10.0));
        assert_eq!(range.y, Range1D::new(5.0, 15.0));
    }

    #[test]
    fn test_range2d_from_rect() {
        let rect = Rect::from_xywh(10.0, 20.0, 30.0, 40.0);
        let range = Range2D::from_rect(rect);
        assert_eq!(range.x.start, 10.0);
        assert_eq!(range.x.end, 40.0);
        assert_eq!(range.y.start, 20.0);
        assert_eq!(range.y.end, 60.0);
    }

    #[test]
    fn test_range2d_size() {
        let range = Range2D::from_values(0.0, 10.0, 0.0, 20.0);
        assert_eq!(range.size(), (10.0, 20.0));
    }

    #[test]
    fn test_range2d_center() {
        let range = Range2D::from_values(0.0, 10.0, 0.0, 20.0);
        assert_eq!(range.center(), Point::new(5.0, 10.0));
    }

    #[test]
    fn test_range2d_contains() {
        let range = Range2D::from_values(0.0, 10.0, 0.0, 10.0);
        assert!(range.contains(Point::new(5.0, 5.0)));
        assert!(range.contains(Point::new(0.0, 0.0)));
        assert!(!range.contains(Point::new(-1.0, 5.0)));
        assert!(!range.contains(Point::new(5.0, 11.0)));
    }

    #[test]
    fn test_range2d_overlaps() {
        let r1 = Range2D::from_values(0.0, 10.0, 0.0, 10.0);
        let r2 = Range2D::from_values(5.0, 15.0, 5.0, 15.0);
        let r3 = Range2D::from_values(20.0, 30.0, 20.0, 30.0);

        assert!(r1.overlaps(&r2));
        assert!(!r1.overlaps(&r3));
    }

    #[test]
    fn test_range2d_clamp() {
        let range = Range2D::from_values(0.0, 10.0, 0.0, 10.0);
        let clamped = range.clamp(Point::new(-5.0, 15.0));
        assert_eq!(clamped, Point::new(0.0, 10.0));
    }

    #[test]
    fn test_range2d_lerp() {
        let range = Range2D::from_values(0.0, 10.0, 0.0, 20.0);
        let point = range.lerp(0.5, 0.5);
        assert_eq!(point, Point::new(5.0, 10.0));
    }

    #[test]
    fn test_range2d_rect_conversion() {
        let rect = Rect::from_xywh(10.0, 20.0, 30.0, 40.0);
        let range: Range2D = rect.into();
        let back: Rect = range.into();

        assert_eq!(back.min, rect.min);
        assert_eq!(back.max, rect.max);
    }

    #[test]
    fn test_range1d_conversions() {
        let range: Range1D = (0.0, 10.0).into();
        assert_eq!(range.start, 0.0);
        assert_eq!(range.end, 10.0);

        let tuple: (f32, f32) = range.into();
        assert_eq!(tuple, (0.0, 10.0));
    }

    #[test]
    fn test_range1d_display() {
        let range = Range1D::new(0.0, 10.5);
        assert_eq!(format!("{}", range), "[0.00..10.50]");
    }

    #[test]
    fn test_range2d_display() {
        let range = Range2D::from_values(0.0, 10.0, 5.0, 15.0);
        let display = format!("{}", range);
        assert!(display.contains("x:"));
        assert!(display.contains("y:"));
    }
}
