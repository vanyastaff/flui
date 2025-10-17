//! Edge insets for padding and margins
//!
//! This module contains types for representing padding and margins,
//! similar to Flutter's EdgeInsets system.

use crate::types::core::{Point, Size, Rect, Offset};
use egui::Margin;

/// An immutable set of offsets in each of the four cardinal directions.
///
/// Similar to Flutter's `EdgeInsets`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EdgeInsets {
    /// The offset from the left.
    pub left: f32,

    /// The offset from the top.
    pub top: f32,

    /// The offset from the right.
    pub right: f32,

    /// The offset from the bottom.
    pub bottom: f32,
}

impl EdgeInsets {
    /// Create edge insets with the given values.
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self { left, top, right, bottom }
    }

    /// Create edge insets with all sides set to the same value.
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    /// Create edge insets with zero offsets.
    pub const ZERO: Self = Self::all(0.0);

    /// Create edge insets with only the left side set.
    pub const fn only_left(value: f32) -> Self {
        Self {
            left: value,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the top side set.
    pub const fn only_top(value: f32) -> Self {
        Self {
            left: 0.0,
            top: value,
            right: 0.0,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the right side set.
    pub const fn only_right(value: f32) -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: value,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the bottom side set.
    pub const fn only_bottom(value: f32) -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: value,
        }
    }

    /// Create edge insets with symmetric horizontal and vertical values.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    /// Create edge insets with only horizontal values.
    pub const fn horizontal(value: f32) -> Self {
        Self::symmetric(value, 0.0)
    }

    /// Create edge insets with only vertical values.
    pub const fn vertical(value: f32) -> Self {
        Self::symmetric(0.0, value)
    }

    /// Get the total horizontal insets (left + right).
    pub fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }

    /// Get the total vertical insets (top + bottom).
    pub fn vertical_total(&self) -> f32 {
        self.top + self.bottom
    }

    /// Get the total insets as a size (width: left + right, height: top + bottom).
    pub fn total_size(&self) -> Size {
        Size::new(self.horizontal_total(), self.vertical_total())
    }

    /// Get the top-left offset.
    pub fn top_left(&self) -> Offset {
        Offset::new(self.left, self.top)
    }

    /// Get the bottom-right offset.
    pub fn bottom_right(&self) -> Offset {
        Offset::new(self.right, self.bottom)
    }

    /// Check if all insets are zero.
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    /// Check if all insets are non-negative.
    pub fn is_non_negative(&self) -> bool {
        self.left >= 0.0 && self.top >= 0.0 && self.right >= 0.0 && self.bottom >= 0.0
    }

    /// Clamp all insets to be non-negative.
    pub fn clamp_non_negative(&self) -> Self {
        Self {
            left: self.left.max(0.0),
            top: self.top.max(0.0),
            right: self.right.max(0.0),
            bottom: self.bottom.max(0.0),
        }
    }

    /// Inflate a rectangle by these insets.
    ///
    /// Increases the rectangle's size by adding the insets to all sides.
    pub fn inflate_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            rect.min - Offset::new(self.left, self.top),
            rect.max + Offset::new(self.right, self.bottom),
        )
    }

    /// Deflate a rectangle by these insets.
    ///
    /// Decreases the rectangle's size by subtracting the insets from all sides.
    pub fn deflate_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            rect.min + Offset::new(self.left, self.top),
            rect.max - Offset::new(self.right, self.bottom),
        )
    }

    /// Apply these insets to a size, shrinking it.
    pub fn shrink_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        Size::new(
            (size.width - self.horizontal_total()).max(0.0),
            (size.height - self.vertical_total()).max(0.0),
        )
    }

    /// Apply these insets to a size, expanding it.
    pub fn expand_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        Size::new(
            size.width + self.horizontal_total(),
            size.height + self.vertical_total(),
        )
    }

    /// Add two edge insets together.
    pub fn add(&self, other: &EdgeInsets) -> Self {
        Self {
            left: self.left + other.left,
            top: self.top + other.top,
            right: self.right + other.right,
            bottom: self.bottom + other.bottom,
        }
    }

    /// Subtract edge insets.
    pub fn subtract(&self, other: &EdgeInsets) -> Self {
        Self {
            left: self.left - other.left,
            top: self.top - other.top,
            right: self.right - other.right,
            bottom: self.bottom - other.bottom,
        }
    }

    /// Multiply edge insets by a scalar.
    pub fn multiply(&self, factor: f32) -> Self {
        Self {
            left: self.left * factor,
            top: self.top * factor,
            right: self.right * factor,
            bottom: self.bottom * factor,
        }
    }

    /// Divide edge insets by a scalar.
    pub fn divide(&self, divisor: f32) -> Self {
        Self {
            left: self.left / divisor,
            top: self.top / divisor,
            right: self.right / divisor,
            bottom: self.bottom / divisor,
        }
    }

    /// Flip the insets horizontally (swap left and right).
    pub fn flip_horizontal(&self) -> Self {
        Self {
            left: self.right,
            top: self.top,
            right: self.left,
            bottom: self.bottom,
        }
    }

    /// Flip the insets vertically (swap top and bottom).
    pub fn flip_vertical(&self) -> Self {
        Self {
            left: self.left,
            top: self.bottom,
            right: self.right,
            bottom: self.top,
        }
    }

    /// Convert to egui's Margin type.
    pub fn to_egui_margin(&self) -> Margin {
        Margin {
            left: self.left as i8,
            right: self.right as i8,
            top: self.top as i8,
            bottom: self.bottom as i8,
        }
    }

    /// Create from egui's Margin type.
    pub fn from_egui_margin(margin: Margin) -> Self {
        Self {
            left: margin.left as f32,
            top: margin.top as f32,
            right: margin.right as f32,
            bottom: margin.bottom as f32,
        }
    }
}

impl From<f32> for EdgeInsets {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl From<(f32, f32)> for EdgeInsets {
    fn from((horizontal, vertical): (f32, f32)) -> Self {
        Self::symmetric(horizontal, vertical)
    }
}

impl From<(f32, f32, f32, f32)> for EdgeInsets {
    fn from((left, top, right, bottom): (f32, f32, f32, f32)) -> Self {
        Self::new(left, top, right, bottom)
    }
}

impl From<Margin> for EdgeInsets {
    fn from(margin: Margin) -> Self {
        Self::from_egui_margin(margin)
    }
}

impl std::ops::Add for EdgeInsets {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            left: self.left + rhs.left,
            top: self.top + rhs.top,
            right: self.right + rhs.right,
            bottom: self.bottom + rhs.bottom,
        }
    }
}

impl std::ops::Sub for EdgeInsets {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self.subtract(&rhs)
    }
}

impl std::ops::Mul<f32> for EdgeInsets {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.multiply(rhs)
    }
}

impl std::ops::Div<f32> for EdgeInsets {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        self.divide(rhs)
    }
}

impl std::ops::Neg for EdgeInsets {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            left: -self.left,
            top: -self.top,
            right: -self.right,
            bottom: -self.bottom,
        }
    }
}

/// Extension trait for working with edge insets.
pub trait EdgeInsetsExt {
    /// Apply edge insets to shrink this value.
    fn shrink_by(&self, insets: EdgeInsets) -> Self;

    /// Apply edge insets to expand this value.
    fn expand_by(&self, insets: EdgeInsets) -> Self;
}

impl EdgeInsetsExt for Rect {
    fn shrink_by(&self, insets: EdgeInsets) -> Self {
        insets.deflate_rect(*self)
    }

    fn expand_by(&self, insets: EdgeInsets) -> Self {
        insets.inflate_rect(*self)
    }
}

impl EdgeInsetsExt for Size {
    fn shrink_by(&self, insets: EdgeInsets) -> Self {
        insets.shrink_size(*self)
    }

    fn expand_by(&self, insets: EdgeInsets) -> Self {
        insets.expand_size(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_insets_creation() {
        let all = EdgeInsets::all(10.0);
        assert_eq!(all.left, 10.0);
        assert_eq!(all.top, 10.0);
        assert_eq!(all.right, 10.0);
        assert_eq!(all.bottom, 10.0);

        let symmetric = EdgeInsets::symmetric(5.0, 10.0);
        assert_eq!(symmetric.left, 5.0);
        assert_eq!(symmetric.right, 5.0);
        assert_eq!(symmetric.top, 10.0);
        assert_eq!(symmetric.bottom, 10.0);

        let custom = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(custom.left, 1.0);
        assert_eq!(custom.top, 2.0);
        assert_eq!(custom.right, 3.0);
        assert_eq!(custom.bottom, 4.0);

        let zero = EdgeInsets::ZERO;
        assert!(zero.is_zero());
    }

    #[test]
    fn test_edge_insets_only() {
        let left = EdgeInsets::only_left(5.0);
        assert_eq!(left.left, 5.0);
        assert_eq!(left.top, 0.0);
        assert_eq!(left.right, 0.0);
        assert_eq!(left.bottom, 0.0);

        let top = EdgeInsets::only_top(5.0);
        assert_eq!(top.top, 5.0);
        assert_eq!(top.left, 0.0);

        let right = EdgeInsets::only_right(5.0);
        assert_eq!(right.right, 5.0);

        let bottom = EdgeInsets::only_bottom(5.0);
        assert_eq!(bottom.bottom, 5.0);
    }

    #[test]
    fn test_edge_insets_totals() {
        let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(insets.horizontal_total(), 4.0);
        assert_eq!(insets.vertical_total(), 6.0);
        assert_eq!(insets.total_size(), Size::new(4.0, 6.0));
    }

    #[test]
    fn test_edge_insets_rect_operations() {
        let insets = EdgeInsets::all(10.0);
        let rect = Rect::from_min_size(Point::new(0.0, 0.0), Size::new(100.0, 100.0));

        // Deflate (shrink)
        let deflated = insets.deflate_rect(rect);
        assert_eq!(deflated.min, Point::new(10.0, 10.0));
        assert_eq!(deflated.max, Point::new(90.0, 90.0));
        assert_eq!(deflated.size(), Size::new(80.0, 80.0));

        // Inflate (expand)
        let inflated = insets.inflate_rect(rect);
        assert_eq!(inflated.min, Point::new(-10.0, -10.0));
        assert_eq!(inflated.max, Point::new(110.0, 110.0));
        assert_eq!(inflated.size(), Size::new(120.0, 120.0));
    }

    #[test]
    fn test_edge_insets_size_operations() {
        let insets = EdgeInsets::symmetric(5.0, 10.0);
        let size = Size::new(100.0, 100.0);

        let shrunk = insets.shrink_size(size);
        assert_eq!(shrunk, Size::new(90.0, 80.0));

        let expanded = insets.expand_size(size);
        assert_eq!(expanded, Size::new(110.0, 120.0));
    }

    #[test]
    fn test_edge_insets_arithmetic() {
        let a = EdgeInsets::all(10.0);
        let b = EdgeInsets::all(5.0);

        let sum = a + b;
        assert_eq!(sum, EdgeInsets::all(15.0));

        let diff = a - b;
        assert_eq!(diff, EdgeInsets::all(5.0));

        let product = a * 2.0;
        assert_eq!(product, EdgeInsets::all(20.0));

        let quotient = a / 2.0;
        assert_eq!(quotient, EdgeInsets::all(5.0));

        let negated = -a;
        assert_eq!(negated, EdgeInsets::all(-10.0));
    }

    #[test]
    fn test_edge_insets_flip() {
        let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);

        let h_flipped = insets.flip_horizontal();
        assert_eq!(h_flipped.left, 3.0);
        assert_eq!(h_flipped.right, 1.0);
        assert_eq!(h_flipped.top, 2.0);
        assert_eq!(h_flipped.bottom, 4.0);

        let v_flipped = insets.flip_vertical();
        assert_eq!(v_flipped.left, 1.0);
        assert_eq!(v_flipped.right, 3.0);
        assert_eq!(v_flipped.top, 4.0);
        assert_eq!(v_flipped.bottom, 2.0);
    }

    #[test]
    fn test_edge_insets_validation() {
        let positive = EdgeInsets::all(5.0);
        assert!(positive.is_non_negative());

        let negative = EdgeInsets::new(-5.0, 10.0, -3.0, 2.0);
        assert!(!negative.is_non_negative());

        let clamped = negative.clamp_non_negative();
        assert!(clamped.is_non_negative());
        assert_eq!(clamped.left, 0.0);
        assert_eq!(clamped.top, 10.0);
        assert_eq!(clamped.right, 0.0);
        assert_eq!(clamped.bottom, 2.0);
    }

    #[test]
    fn test_edge_insets_conversions() {
        let from_f32: EdgeInsets = 10.0.into();
        assert_eq!(from_f32, EdgeInsets::all(10.0));

        let from_tuple: EdgeInsets = (5.0, 10.0).into();
        assert_eq!(from_tuple, EdgeInsets::symmetric(5.0, 10.0));

        let from_tuple4: EdgeInsets = (1.0, 2.0, 3.0, 4.0).into();
        assert_eq!(from_tuple4, EdgeInsets::new(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn test_edge_insets_extension_trait() {
        let insets = EdgeInsets::all(10.0);
        let rect = Rect::from_min_size(Point::new(0.0, 0.0), Size::new(100.0, 100.0));

        let shrunk = rect.shrink_by(insets);
        assert_eq!(shrunk.size(), Size::new(80.0, 80.0));

        let expanded = rect.expand_by(insets);
        assert_eq!(expanded.size(), Size::new(120.0, 120.0));

        let size = Size::new(100.0, 100.0);
        let shrunk_size = size.shrink_by(insets);
        assert_eq!(shrunk_size, Size::new(80.0, 80.0));

        let expanded_size = size.expand_by(insets);
        assert_eq!(expanded_size, Size::new(120.0, 120.0));
    }
}
