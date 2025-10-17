//! Size types for representing 2D dimensions
//!
//! This module contains types for representing sizes,
//! similar to Flutter's Size system.

use egui::Vec2;

/// An immutable 2D size with width and height.
///
/// Similar to Flutter's `Size`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// Width in logical pixels
    pub width: f32,
    /// Height in logical pixels
    pub height: f32,
}

impl Size {
    /// A size with zero width and height.
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };

    /// A size with infinite width and height.
    pub const INFINITE: Size = Size {
        width: f32::INFINITY,
        height: f32::INFINITY,
    };

    /// Create a new size with the given width and height.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Create a square size with the given dimension.
    pub const fn square(dimension: f32) -> Self {
        Self {
            width: dimension,
            height: dimension,
        }
    }

    /// Create a size from width with height set to infinity.
    pub const fn from_width(width: f32) -> Self {
        Self {
            width,
            height: f32::INFINITY,
        }
    }

    /// Create a size from height with width set to infinity.
    pub const fn from_height(height: f32) -> Self {
        Self {
            width: f32::INFINITY,
            height,
        }
    }

    /// Create a size from a radius (diameter = radius * 2).
    pub fn from_radius(radius: f32) -> Self {
        let diameter = radius * 2.0;
        Self::square(diameter)
    }

    /// Whether this size has a width or height of zero.
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Whether this size has finite width and height.
    pub fn is_finite(&self) -> bool {
        self.width.is_finite() && self.height.is_finite()
    }

    /// Whether this size has infinite width or height.
    pub fn is_infinite(&self) -> bool {
        !self.is_finite()
    }

    /// The aspect ratio (width / height).
    ///
    /// Returns `None` if height is zero.
    pub fn aspect_ratio(&self) -> Option<f32> {
        if self.height == 0.0 {
            None
        } else {
            Some(self.width / self.height)
        }
    }

    /// The lesser of the width and height.
    pub fn shortest_side(&self) -> f32 {
        self.width.min(self.height)
    }

    /// The greater of the width and height.
    pub fn longest_side(&self) -> f32 {
        self.width.max(self.height)
    }

    /// A size with the width and height swapped.
    pub const fn flipped(&self) -> Size {
        Size {
            width: self.height,
            height: self.width,
        }
    }

    /// Linear interpolation between two sizes.
    pub fn lerp(a: Size, b: Size, t: f32) -> Size {
        Size {
            width: a.width + (b.width - a.width) * t,
            height: a.height + (b.height - a.height) * t,
        }
    }

    /// Returns a size with the dimensions clamped to be non-negative.
    pub fn clamp_non_negative(&self) -> Size {
        Size {
            width: self.width.max(0.0),
            height: self.height.max(0.0),
        }
    }

    /// Whether the given point is within the bounds of this size
    /// (assuming the top-left corner is at the origin).
    pub fn contains(&self, point: egui::Pos2) -> bool {
        point.x >= 0.0 && point.x <= self.width && point.y >= 0.0 && point.y <= self.height
    }

    /// The center point of this size (assuming top-left corner is at origin).
    pub fn center(&self) -> egui::Pos2 {
        egui::pos2(self.width / 2.0, self.height / 2.0)
    }

    /// The center as an offset from the origin.
    pub fn center_offset(&self) -> Vec2 {
        Vec2::new(self.width / 2.0, self.height / 2.0)
    }

    /// The top-left corner (always at origin when used as bounds).
    pub fn top_left(&self) -> egui::Pos2 {
        egui::pos2(0.0, 0.0)
    }

    /// The top-center point.
    pub fn top_center(&self) -> egui::Pos2 {
        egui::pos2(self.width / 2.0, 0.0)
    }

    /// The top-right corner.
    pub fn top_right(&self) -> egui::Pos2 {
        egui::pos2(self.width, 0.0)
    }

    /// The center-left point.
    pub fn center_left(&self) -> egui::Pos2 {
        egui::pos2(0.0, self.height / 2.0)
    }

    /// The center-right point.
    pub fn center_right(&self) -> egui::Pos2 {
        egui::pos2(self.width, self.height / 2.0)
    }

    /// The bottom-left corner.
    pub fn bottom_left(&self) -> egui::Pos2 {
        egui::pos2(0.0, self.height)
    }

    /// The bottom-center point.
    pub fn bottom_center(&self) -> egui::Pos2 {
        egui::pos2(self.width / 2.0, self.height)
    }

    /// The bottom-right corner.
    pub fn bottom_right(&self) -> egui::Pos2 {
        egui::pos2(self.width, self.height)
    }

    /// Convert to egui's Vec2.
    pub const fn to_vec2(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    /// Create from egui's Vec2.
    pub const fn from_vec2(vec: Vec2) -> Self {
        Self {
            width: vec.x,
            height: vec.y,
        }
    }

    /// Create a Rect from this size with the given position.
    pub fn at(&self, position: egui::Pos2) -> egui::Rect {
        egui::Rect::from_min_size(position, self.to_vec2())
    }

    /// Create a Rect from this size at the origin.
    pub fn to_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), self.to_vec2())
    }
}

impl Default for Size {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<Vec2> for Size {
    fn from(vec: Vec2) -> Self {
        Self::from_vec2(vec)
    }
}

impl From<Size> for Vec2 {
    fn from(size: Size) -> Self {
        size.to_vec2()
    }
}

impl From<(f32, f32)> for Size {
    fn from((width, height): (f32, f32)) -> Self {
        Self::new(width, height)
    }
}

impl From<f32> for Size {
    fn from(dimension: f32) -> Self {
        Self::square(dimension)
    }
}

impl From<egui::Rect> for Size {
    fn from(rect: egui::Rect) -> Self {
        Self::from_vec2(rect.size())
    }
}

impl std::ops::Add for Size {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Size {
            width: self.width + rhs.width,
            height: self.height + rhs.height,
        }
    }
}

impl std::ops::Sub for Size {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Size {
            width: self.width - rhs.width,
            height: self.height - rhs.height,
        }
    }
}

impl std::ops::Mul<f32> for Size {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Size {
            width: self.width * rhs,
            height: self.height * rhs,
        }
    }
}

impl std::ops::Div<f32> for Size {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Size {
            width: self.width / rhs,
            height: self.height / rhs,
        }
    }
}

impl std::ops::Neg for Size {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Size {
            width: -self.width,
            height: -self.height,
        }
    }
}

/// Extension trait for Vec2 to add Size-like methods.
pub trait Vec2Ext {
    /// Convert Vec2 to Size.
    fn to_size(&self) -> Size;
}

impl Vec2Ext for Vec2 {
    fn to_size(&self) -> Size {
        Size::from_vec2(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_creation() {
        let size = Size::new(100.0, 200.0);
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 200.0);

        let square = Size::square(50.0);
        assert_eq!(square.width, 50.0);
        assert_eq!(square.height, 50.0);

        assert_eq!(Size::ZERO, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_size_from_methods() {
        let from_width = Size::from_width(100.0);
        assert_eq!(from_width.width, 100.0);
        assert!(from_width.height.is_infinite());

        let from_height = Size::from_height(200.0);
        assert_eq!(from_height.height, 200.0);
        assert!(from_height.width.is_infinite());

        let from_radius = Size::from_radius(50.0);
        assert_eq!(from_radius.width, 100.0);
        assert_eq!(from_radius.height, 100.0);
    }

    #[test]
    fn test_size_properties() {
        let size = Size::new(100.0, 200.0);
        assert!(!size.is_empty());
        assert!(size.is_finite());
        assert!(!size.is_infinite());

        let empty = Size::new(0.0, 100.0);
        assert!(empty.is_empty());

        let infinite = Size::INFINITE;
        assert!(infinite.is_infinite());
        assert!(!infinite.is_finite());
    }

    #[test]
    fn test_aspect_ratio() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(size.aspect_ratio(), Some(2.0));

        let square = Size::square(100.0);
        assert_eq!(square.aspect_ratio(), Some(1.0));

        let zero_height = Size::new(100.0, 0.0);
        assert_eq!(zero_height.aspect_ratio(), None);
    }

    #[test]
    fn test_sides() {
        let size = Size::new(100.0, 200.0);
        assert_eq!(size.shortest_side(), 100.0);
        assert_eq!(size.longest_side(), 200.0);
    }

    #[test]
    fn test_flipped() {
        let size = Size::new(100.0, 200.0);
        let flipped = size.flipped();
        assert_eq!(flipped.width, 200.0);
        assert_eq!(flipped.height, 100.0);
    }

    #[test]
    fn test_lerp() {
        let a = Size::new(0.0, 0.0);
        let b = Size::new(100.0, 200.0);
        let mid = Size::lerp(a, b, 0.5);
        assert_eq!(mid.width, 50.0);
        assert_eq!(mid.height, 100.0);
    }

    #[test]
    fn test_clamp_non_negative() {
        let size = Size::new(-10.0, 20.0);
        let clamped = size.clamp_non_negative();
        assert_eq!(clamped.width, 0.0);
        assert_eq!(clamped.height, 20.0);
    }

    #[test]
    fn test_contains() {
        let size = Size::new(100.0, 100.0);
        assert!(size.contains(egui::pos2(50.0, 50.0)));
        assert!(size.contains(egui::pos2(0.0, 0.0)));
        assert!(size.contains(egui::pos2(100.0, 100.0)));
        assert!(!size.contains(egui::pos2(150.0, 50.0)));
        assert!(!size.contains(egui::pos2(-10.0, 50.0)));
    }

    #[test]
    fn test_center() {
        let size = Size::new(100.0, 200.0);
        let center = size.center();
        assert_eq!(center, egui::pos2(50.0, 100.0));
    }

    #[test]
    fn test_corner_methods() {
        let size = Size::new(100.0, 200.0);
        assert_eq!(size.top_left(), egui::pos2(0.0, 0.0));
        assert_eq!(size.top_center(), egui::pos2(50.0, 0.0));
        assert_eq!(size.top_right(), egui::pos2(100.0, 0.0));
        assert_eq!(size.center_left(), egui::pos2(0.0, 100.0));
        assert_eq!(size.center_right(), egui::pos2(100.0, 100.0));
        assert_eq!(size.bottom_left(), egui::pos2(0.0, 200.0));
        assert_eq!(size.bottom_center(), egui::pos2(50.0, 200.0));
        assert_eq!(size.bottom_right(), egui::pos2(100.0, 200.0));
    }

    #[test]
    fn test_conversions() {
        let size = Size::new(100.0, 200.0);
        let vec = size.to_vec2();
        assert_eq!(vec.x, 100.0);
        assert_eq!(vec.y, 200.0);

        let back = Size::from_vec2(vec);
        assert_eq!(back, size);

        let from_tuple: Size = (50.0, 100.0).into();
        assert_eq!(from_tuple.width, 50.0);
        assert_eq!(from_tuple.height, 100.0);

        let from_f32: Size = 75.0.into();
        assert_eq!(from_f32, Size::square(75.0));
    }

    #[test]
    fn test_rect_conversion() {
        let size = Size::new(100.0, 200.0);
        let rect = size.to_rect();
        assert_eq!(rect.min, egui::pos2(0.0, 0.0));
        assert_eq!(rect.max, egui::pos2(100.0, 200.0));

        let rect_at = size.at(egui::pos2(10.0, 20.0));
        assert_eq!(rect_at.min, egui::pos2(10.0, 20.0));
        assert_eq!(rect_at.max, egui::pos2(110.0, 220.0));

        let from_rect: Size = rect.into();
        assert_eq!(from_rect, size);
    }

    #[test]
    fn test_arithmetic() {
        let a = Size::new(100.0, 200.0);
        let b = Size::new(50.0, 100.0);

        let sum = a + b;
        assert_eq!(sum.width, 150.0);
        assert_eq!(sum.height, 300.0);

        let diff = a - b;
        assert_eq!(diff.width, 50.0);
        assert_eq!(diff.height, 100.0);

        let scaled = a * 2.0;
        assert_eq!(scaled.width, 200.0);
        assert_eq!(scaled.height, 400.0);

        let divided = a / 2.0;
        assert_eq!(divided.width, 50.0);
        assert_eq!(divided.height, 100.0);

        let negated = -a;
        assert_eq!(negated.width, -100.0);
        assert_eq!(negated.height, -200.0);
    }

    #[test]
    fn test_vec2_ext() {
        let vec = Vec2::new(100.0, 200.0);
        let size = vec.to_size();
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 200.0);
    }
}
