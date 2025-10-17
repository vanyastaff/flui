//! Margin types for external spacing
//!
//! This module contains types for representing margins (external spacing),
//! separate from padding (internal spacing).

use crate::types::core::{Point, Size, Rect, Offset};
use egui::Margin as EguiMargin;

/// Represents external spacing around an element.
///
/// Similar to CSS margin. This is type-safe wrapper to distinguish
/// from padding (internal spacing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margin {
    /// Space on the left side
    pub left: f32,
    /// Space on the right side
    pub right: f32,
    /// Space on the top side
    pub top: f32,
    /// Space on the bottom side
    pub bottom: f32,
}

impl Margin {
    /// No margin (all zeros).
    pub const ZERO: Margin = Margin {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };

    /// Create a new margin with specific values for each side.
    pub const fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// Create a margin with the same value for all sides.
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }

    /// Create a margin with separate horizontal and vertical values.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    /// Create a margin with only horizontal spacing.
    pub const fn horizontal(value: f32) -> Self {
        Self {
            left: value,
            right: value,
            top: 0.0,
            bottom: 0.0,
        }
    }

    /// Create a margin with only vertical spacing.
    pub const fn vertical(value: f32) -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: value,
            bottom: value,
        }
    }

    /// Create a margin with specific values for each side.
    pub const fn only(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self::new(left, right, top, bottom)
    }

    /// Total horizontal margin (left + right).
    pub const fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }

    /// Total vertical margin (top + bottom).
    pub const fn vertical_total(&self) -> f32 {
        self.top + self.bottom
    }

    /// Total margin as a size (horizontal, vertical).
    pub fn total_size(&self) -> Size {
        Size::new(self.horizontal_total(), self.vertical_total())
    }

    /// Shrink a rect by this margin (inward).
    pub fn shrink_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            rect.min + Offset::new(self.left, self.top),
            rect.max - Offset::new(self.right, self.bottom),
        )
    }

    /// Expand a rect by this margin (outward).
    pub fn expand_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            rect.min - Offset::new(self.left, self.top),
            rect.max + Offset::new(self.right, self.bottom),
        )
    }

    /// Reduce a size by this margin.
    pub fn shrink_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        size - self.total_size()
    }

    /// Increase a size by this margin.
    pub fn expand_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        size + self.total_size()
    }

    /// Convert to egui's Margin type (with clamping to i8 range).
    pub fn to_egui_margin(&self) -> EguiMargin {
        EguiMargin {
            left: self.left.clamp(i8::MIN as f32, i8::MAX as f32) as i8,
            right: self.right.clamp(i8::MIN as f32, i8::MAX as f32) as i8,
            top: self.top.clamp(i8::MIN as f32, i8::MAX as f32) as i8,
            bottom: self.bottom.clamp(i8::MIN as f32, i8::MAX as f32) as i8,
        }
    }

    /// Create from egui's Margin type.
    pub fn from_egui_margin(margin: EguiMargin) -> Self {
        Self {
            left: margin.left as f32,
            right: margin.right as f32,
            top: margin.top as f32,
            bottom: margin.bottom as f32,
        }
    }

    /// Create a flipped margin (swap left/right and top/bottom).
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
}

impl Default for Margin {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<f32> for Margin {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl From<(f32, f32)> for Margin {
    fn from((horizontal, vertical): (f32, f32)) -> Self {
        Self::symmetric(horizontal, vertical)
    }
}

impl From<EguiMargin> for Margin {
    fn from(margin: EguiMargin) -> Self {
        Self::from_egui_margin(margin)
    }
}

impl std::ops::Add for Margin {
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

impl std::ops::Sub for Margin {
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

impl std::ops::Mul<f32> for Margin {
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

impl std::ops::Div<f32> for Margin {
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
    fn test_margin_creation() {
        let margin = Margin::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(margin.left, 10.0);
        assert_eq!(margin.right, 20.0);
        assert_eq!(margin.top, 30.0);
        assert_eq!(margin.bottom, 40.0);

        let all = Margin::all(10.0);
        assert_eq!(all.left, 10.0);
        assert_eq!(all.right, 10.0);
        assert_eq!(all.top, 10.0);
        assert_eq!(all.bottom, 10.0);

        let sym = Margin::symmetric(10.0, 20.0);
        assert_eq!(sym.left, 10.0);
        assert_eq!(sym.right, 10.0);
        assert_eq!(sym.top, 20.0);
        assert_eq!(sym.bottom, 20.0);
    }

    #[test]
    fn test_margin_totals() {
        let margin = Margin::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(margin.horizontal_total(), 30.0);
        assert_eq!(margin.vertical_total(), 70.0);
        assert_eq!(margin.total_size(), Size::new(30.0, 70.0));
    }

    #[test]
    fn test_margin_rect_operations() {
        let margin = Margin::all(10.0);
        let rect = Rect::from_min_max(Point::new(0.0, 0.0), Point::new(100.0, 100.0));

        let shrunk = margin.shrink_rect(rect);
        assert_eq!(shrunk.min, Point::new(10.0, 10.0));
        assert_eq!(shrunk.max, Point::new(90.0, 90.0));

        let expanded = margin.expand_rect(rect);
        assert_eq!(expanded.min, Point::new(-10.0, -10.0));
        assert_eq!(expanded.max, Point::new(110.0, 110.0));
    }

    #[test]
    fn test_margin_size_operations() {
        let margin = Margin::all(10.0);
        let size = Size::new(100.0, 100.0);

        let shrunk = margin.shrink_size(size);
        assert_eq!(shrunk, Size::new(80.0, 80.0));

        let expanded = margin.expand_size(size);
        assert_eq!(expanded, Size::new(120.0, 120.0));
    }

    #[test]
    fn test_margin_arithmetic() {
        let a = Margin::all(10.0);
        let b = Margin::all(5.0);

        let sum = a + b;
        assert_eq!(sum, Margin::all(15.0));

        let diff = a - b;
        assert_eq!(diff, Margin::all(5.0));

        let scaled = a * 2.0;
        assert_eq!(scaled, Margin::all(20.0));

        let divided = a / 2.0;
        assert_eq!(divided, Margin::all(5.0));
    }

    #[test]
    fn test_margin_conversions() {
        let from_f32: Margin = 10.0.into();
        assert_eq!(from_f32, Margin::all(10.0));

        let from_tuple: Margin = (10.0, 20.0).into();
        assert_eq!(from_tuple, Margin::symmetric(10.0, 20.0));
    }

    #[test]
    fn test_margin_flipped() {
        let margin = Margin::new(10.0, 20.0, 30.0, 40.0);
        let flipped = margin.flipped();
        assert_eq!(flipped.left, 20.0);
        assert_eq!(flipped.right, 10.0);
        assert_eq!(flipped.top, 40.0);
        assert_eq!(flipped.bottom, 30.0);
    }

    #[test]
    fn test_margin_clamp() {
        let margin = Margin::new(-10.0, 20.0, -5.0, 30.0);
        let clamped = margin.clamp_non_negative();
        assert_eq!(clamped.left, 0.0);
        assert_eq!(clamped.right, 20.0);
        assert_eq!(clamped.top, 0.0);
        assert_eq!(clamped.bottom, 30.0);
    }
}
