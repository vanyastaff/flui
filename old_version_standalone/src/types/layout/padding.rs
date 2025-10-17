//! Padding types for internal spacing
//!
//! This module contains types for representing padding (internal spacing),
//! separate from margin (external spacing).

use crate::types::core::{Point, Size, Rect, Offset};

/// Represents internal spacing within an element.
///
/// Similar to CSS padding. This is type-safe wrapper to distinguish
/// from margin (external spacing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    /// Space on the left side
    pub left: f32,
    /// Space on the right side
    pub right: f32,
    /// Space on the top side
    pub top: f32,
    /// Space on the bottom side
    pub bottom: f32,
}

impl Padding {
    /// No padding (all zeros).
    pub const ZERO: Padding = Padding {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };

    /// Small padding (4px all sides).
    pub const SMALL: Padding = Padding::all(4.0);

    /// Medium padding (8px all sides).
    pub const MEDIUM: Padding = Padding::all(8.0);

    /// Large padding (16px all sides).
    pub const LARGE: Padding = Padding::all(16.0);

    /// Extra large padding (24px all sides).
    pub const EXTRA_LARGE: Padding = Padding::all(24.0);

    /// Create a new padding with specific values for each side.
    pub const fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// Create a padding with the same value for all sides.
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }

    /// Create a padding with separate horizontal and vertical values.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    /// Create a padding with only horizontal spacing.
    pub const fn horizontal(value: f32) -> Self {
        Self {
            left: value,
            right: value,
            top: 0.0,
            bottom: 0.0,
        }
    }

    /// Create a padding with only vertical spacing.
    pub const fn vertical(value: f32) -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: value,
            bottom: value,
        }
    }

    /// Create a padding with specific values for each side.
    pub const fn only(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self::new(left, right, top, bottom)
    }

    /// Total horizontal padding (left + right).
    pub const fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }

    /// Total vertical padding (top + bottom).
    pub const fn vertical_total(&self) -> f32 {
        self.top + self.bottom
    }

    /// Total padding as a size (horizontal, vertical).
    pub fn total_size(&self) -> Size {
        Size::new(self.horizontal_total(), self.vertical_total())
    }

    /// Shrink a rect by this padding (inward - content area).
    pub fn shrink_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            rect.min + Offset::new(self.left, self.top),
            rect.max - Offset::new(self.right, self.bottom),
        )
    }

    /// Expand a rect by this padding (outward - total area).
    pub fn expand_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            rect.min - Offset::new(self.left, self.top),
            rect.max + Offset::new(self.right, self.bottom),
        )
    }

    /// Reduce a size by this padding (get content size).
    pub fn shrink_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        size - self.total_size()
    }

    /// Increase a size by this padding (get total size).
    pub fn expand_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        size + self.total_size()
    }

    /// Create a flipped padding (swap left/right and top/bottom).
    pub const fn flipped(&self) -> Self {
        Self {
            left: self.right,
            right: self.left,
            top: self.bottom,
            bottom: self.top,
        }
    }

    /// Ensure all values are non-negative.
    pub fn clamp_non_negative(&self) -> Self {
        Self {
            left: self.left.max(0.0),
            right: self.right.max(0.0),
            top: self.top.max(0.0),
            bottom: self.bottom.max(0.0),
        }
    }

    /// Linear interpolation between two paddings.
    pub fn lerp(a: Padding, b: Padding, t: f32) -> Padding {
        Padding {
            left: a.left + (b.left - a.left) * t,
            right: a.right + (b.right - a.right) * t,
            top: a.top + (b.top - a.top) * t,
            bottom: a.bottom + (b.bottom - a.bottom) * t,
        }
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<f32> for Padding {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl From<(f32, f32)> for Padding {
    fn from((horizontal, vertical): (f32, f32)) -> Self {
        Self::symmetric(horizontal, vertical)
    }
}

impl std::ops::Add for Padding {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            left: self.left + rhs.left,
            right: self.right + rhs.right,
            top: self.top + rhs.top,
            bottom: self.bottom + rhs.bottom,
        }
    }
}

impl std::ops::Sub for Padding {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            left: self.left - rhs.left,
            right: self.right - rhs.right,
            top: self.top - rhs.top,
            bottom: self.bottom - rhs.bottom,
        }
    }
}

impl std::ops::Mul<f32> for Padding {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            left: self.left * rhs,
            right: self.right * rhs,
            top: self.top * rhs,
            bottom: self.bottom * rhs,
        }
    }
}

impl std::ops::Div<f32> for Padding {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            left: self.left / rhs,
            right: self.right / rhs,
            top: self.top / rhs,
            bottom: self.bottom / rhs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding_creation() {
        let padding = Padding::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(padding.left, 10.0);
        assert_eq!(padding.right, 20.0);
        assert_eq!(padding.top, 30.0);
        assert_eq!(padding.bottom, 40.0);

        let all = Padding::all(10.0);
        assert_eq!(all.left, 10.0);
        assert_eq!(all.right, 10.0);
        assert_eq!(all.top, 10.0);
        assert_eq!(all.bottom, 10.0);

        let sym = Padding::symmetric(10.0, 20.0);
        assert_eq!(sym.left, 10.0);
        assert_eq!(sym.right, 10.0);
        assert_eq!(sym.top, 20.0);
        assert_eq!(sym.bottom, 20.0);
    }

    #[test]
    fn test_padding_constants() {
        assert_eq!(Padding::SMALL, Padding::all(4.0));
        assert_eq!(Padding::MEDIUM, Padding::all(8.0));
        assert_eq!(Padding::LARGE, Padding::all(16.0));
        assert_eq!(Padding::EXTRA_LARGE, Padding::all(24.0));
    }

    #[test]
    fn test_padding_totals() {
        let padding = Padding::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(padding.horizontal_total(), 30.0);
        assert_eq!(padding.vertical_total(), 70.0);
        assert_eq!(padding.total_size(), Size::new(30.0, 70.0));
    }

    #[test]
    fn test_padding_rect_operations() {
        let padding = Padding::all(10.0);
        let rect = Rect::from_min_max(Point::new(0.0, 0.0), Point::new(100.0, 100.0));

        let shrunk = padding.shrink_rect(rect);
        assert_eq!(shrunk.min, Point::new(10.0, 10.0));
        assert_eq!(shrunk.max, Point::new(90.0, 90.0));

        let expanded = padding.expand_rect(rect);
        assert_eq!(expanded.min, Point::new(-10.0, -10.0));
        assert_eq!(expanded.max, Point::new(110.0, 110.0));
    }

    #[test]
    fn test_padding_size_operations() {
        let padding = Padding::all(10.0);
        let size = Size::new(100.0, 100.0);

        let shrunk = padding.shrink_size(size);
        assert_eq!(shrunk, Size::new(80.0, 80.0));

        let expanded = padding.expand_size(size);
        assert_eq!(expanded, Size::new(120.0, 120.0));
    }

    #[test]
    fn test_padding_arithmetic() {
        let a = Padding::all(10.0);
        let b = Padding::all(5.0);

        let sum = a + b;
        assert_eq!(sum, Padding::all(15.0));

        let diff = a - b;
        assert_eq!(diff, Padding::all(5.0));

        let scaled = a * 2.0;
        assert_eq!(scaled, Padding::all(20.0));

        let divided = a / 2.0;
        assert_eq!(divided, Padding::all(5.0));
    }

    #[test]
    fn test_padding_conversions() {
        let from_f32: Padding = 10.0.into();
        assert_eq!(from_f32, Padding::all(10.0));

        let from_tuple: Padding = (10.0, 20.0).into();
        assert_eq!(from_tuple, Padding::symmetric(10.0, 20.0));
    }

    #[test]
    fn test_padding_lerp() {
        let a = Padding::all(0.0);
        let b = Padding::all(100.0);
        let mid = Padding::lerp(a, b, 0.5);
        assert_eq!(mid, Padding::all(50.0));
    }

    #[test]
    fn test_padding_flipped() {
        let padding = Padding::new(10.0, 20.0, 30.0, 40.0);
        let flipped = padding.flipped();
        assert_eq!(flipped.left, 20.0);
        assert_eq!(flipped.right, 10.0);
        assert_eq!(flipped.top, 40.0);
        assert_eq!(flipped.bottom, 30.0);
    }

    #[test]
    fn test_padding_clamp() {
        let padding = Padding::new(-10.0, 20.0, -5.0, 30.0);
        let clamped = padding.clamp_non_negative();
        assert_eq!(clamped.left, 0.0);
        assert_eq!(clamped.right, 20.0);
        assert_eq!(clamped.top, 0.0);
        assert_eq!(clamped.bottom, 30.0);
    }
}
