//! Border types for drawing borders around widgets
//!
//! This module contains types for representing borders,
//! similar to Flutter's Border and BorderSide system.

use crate::types::core::{Color, Size};
use egui::Stroke;

/// The style of line to draw for a border.
///
/// Similar to Flutter's `BorderStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    /// No border.
    None,

    /// A solid line.
    #[default]
    Solid,
}

/// A side of a border of a box.
///
/// Similar to Flutter's `BorderSide`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSide {
    /// The color of this side of the border.
    pub color: Color,

    /// The width of this side of the border, in logical pixels.
    pub width: f32,

    /// The style of this side of the border.
    pub style: BorderStyle,
}

impl BorderSide {
    /// Create a border side with the given properties.
    pub fn new(color: impl Into<Color>, width: f32, style: BorderStyle) -> Self {
        Self {
            color: color.into(),
            width,
            style
        }
    }

    /// Create a solid border side with the given color and width.
    pub fn solid(color: impl Into<Color>, width: f32) -> Self {
        Self::new(color, width, BorderStyle::Solid)
    }

    /// Create a border side with no border.
    pub const NONE: Self = Self {
        color: Color::TRANSPARENT,
        width: 0.0,
        style: BorderStyle::None,
    };

    /// Check if this border side has no width or is set to none.
    pub fn is_none(&self) -> bool {
        self.width == 0.0 || self.style == BorderStyle::None
    }

    /// Scale the width of this border side.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            color: self.color,
            width: self.width * factor,
            style: self.style,
        }
    }

    /// Create a copy with a different color.
    pub fn with_color(&self, color: impl Into<Color>) -> Self {
        Self {
            color: color.into(),
            width: self.width,
            style: self.style,
        }
    }

    /// Create a copy with a different width.
    pub fn with_width(&self, width: f32) -> Self {
        Self {
            color: self.color,
            width,
            style: self.style,
        }
    }

    /// Create a copy with a different style.
    pub fn with_style(&self, style: BorderStyle) -> Self {
        Self {
            color: self.color,
            width: self.width,
            style,
        }
    }

    /// Convert to egui's Stroke type.
    pub fn to_egui_stroke(&self) -> Stroke {
        if self.is_none() {
            Stroke::NONE
        } else {
            Stroke::new(self.width, self.color.to_egui())
        }
    }

    /// Create from egui's Stroke type.
    pub fn from_egui_stroke(stroke: Stroke) -> Self {
        if stroke.width == 0.0 {
            Self::NONE
        } else {
            Self::solid(stroke.color, stroke.width)
        }
    }
}

impl Default for BorderSide {
    fn default() -> Self {
        Self::solid(Color::BLACK, 1.0)
    }
}

impl From<Color> for BorderSide {
    fn from(color: Color) -> Self {
        Self::solid(color, 1.0)
    }
}

impl From<Stroke> for BorderSide {
    fn from(stroke: Stroke) -> Self {
        Self::from_egui_stroke(stroke)
    }
}

impl std::ops::Mul<f32> for BorderSide {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

/// A border of a box, comprised of four sides.
///
/// Similar to Flutter's `Border`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    /// The top side of this border.
    pub top: BorderSide,

    /// The right side of this border.
    pub right: BorderSide,

    /// The bottom side of this border.
    pub bottom: BorderSide,

    /// The left side of this border.
    pub left: BorderSide,
}

impl Border {
    /// Create a border with the given sides.
    pub const fn new(top: BorderSide, right: BorderSide, bottom: BorderSide, left: BorderSide) -> Self {
        Self { top, right, bottom, left }
    }

    /// Create a border with all sides the same.
    pub const fn all(side: BorderSide) -> Self {
        Self {
            top: side,
            right: side,
            bottom: side,
            left: side,
        }
    }

    /// Create a uniform border with the given color and width.
    pub fn uniform(color: impl Into<Color>, width: f32) -> Self {
        Self::all(BorderSide::solid(color, width))
    }

    /// A border with no sides.
    pub const NONE: Self = Self::all(BorderSide::NONE);

    /// Create a border with symmetric vertical and horizontal sides.
    pub const fn symmetric(vertical: BorderSide, horizontal: BorderSide) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Create a border with only the top side.
    pub const fn only_top(side: BorderSide) -> Self {
        Self {
            top: side,
            right: BorderSide::NONE,
            bottom: BorderSide::NONE,
            left: BorderSide::NONE,
        }
    }

    /// Create a border with only the right side.
    pub const fn only_right(side: BorderSide) -> Self {
        Self {
            top: BorderSide::NONE,
            right: side,
            bottom: BorderSide::NONE,
            left: BorderSide::NONE,
        }
    }

    /// Create a border with only the bottom side.
    pub const fn only_bottom(side: BorderSide) -> Self {
        Self {
            top: BorderSide::NONE,
            right: BorderSide::NONE,
            bottom: side,
            left: BorderSide::NONE,
        }
    }

    /// Create a border with only the left side.
    pub const fn only_left(side: BorderSide) -> Self {
        Self {
            top: BorderSide::NONE,
            right: BorderSide::NONE,
            bottom: BorderSide::NONE,
            left: side,
        }
    }

    /// Check if all sides are none.
    pub fn is_none(&self) -> bool {
        self.top.is_none() && self.right.is_none() && self.bottom.is_none() && self.left.is_none()
    }

    /// Check if all sides are uniform (same properties).
    pub fn is_uniform(&self) -> bool {
        self.top == self.right && self.top == self.bottom && self.top == self.left
    }

    /// Get the uniform side if all sides are the same.
    pub fn uniform_side(&self) -> Option<BorderSide> {
        if self.is_uniform() {
            Some(self.top)
        } else {
            None
        }
    }

    /// Scale all border widths by a factor.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            top: self.top * factor,
            right: self.right * factor,
            bottom: self.bottom * factor,
            left: self.left * factor,
        }
    }

    /// Get the total width of horizontal borders (top + bottom).
    pub fn horizontal_width(&self) -> f32 {
        self.top.width + self.bottom.width
    }

    /// Get the total width of vertical borders (left + right).
    pub fn vertical_width(&self) -> f32 {
        self.left.width + self.right.width
    }

    /// Get the dimensions consumed by this border (width: left + right, height: top + bottom).
    pub fn dimensions(&self) -> Size {
        Size::new(self.vertical_width(), self.horizontal_width())
    }

    /// Merge this border with another, using the maximum width for each side.
    pub fn merge_max(&self, other: &Border) -> Self {
        Self {
            top: if self.top.width >= other.top.width { self.top } else { other.top },
            right: if self.right.width >= other.right.width { self.right } else { other.right },
            bottom: if self.bottom.width >= other.bottom.width { self.bottom } else { other.bottom },
            left: if self.left.width >= other.left.width { self.left } else { other.left },
        }
    }

    /// Merge this border with another, using the minimum width for each side.
    pub fn merge_min(&self, other: &Border) -> Self {
        Self {
            top: if self.top.width <= other.top.width { self.top } else { other.top },
            right: if self.right.width <= other.right.width { self.right } else { other.right },
            bottom: if self.bottom.width <= other.bottom.width { self.bottom } else { other.bottom },
            left: if self.left.width <= other.left.width { self.left } else { other.left },
        }
    }
}

impl Default for Border {
    fn default() -> Self {
        Self::NONE
    }
}

impl From<BorderSide> for Border {
    fn from(side: BorderSide) -> Self {
        Self::all(side)
    }
}

impl From<Color> for Border {
    fn from(color: Color) -> Self {
        Self::uniform(color, 1.0)
    }
}

impl std::ops::Mul<f32> for Border {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_border_style() {
        assert_eq!(BorderStyle::default(), BorderStyle::Solid);
    }

    #[test]
    fn test_border_side_creation() {
        let side = BorderSide::solid(Color::RED, 2.0);
        assert_eq!(side.color, Color::RED);
        assert_eq!(side.width, 2.0);
        assert_eq!(side.style, BorderStyle::Solid);
        assert!(!side.is_none());

        let none = BorderSide::NONE;
        assert!(none.is_none());
    }

    #[test]
    fn test_border_side_modifications() {
        let side = BorderSide::solid(Color::RED, 2.0);

        let with_color = side.with_color(Color::BLUE);
        assert_eq!(with_color.color, Color::BLUE);
        assert_eq!(with_color.width, 2.0);

        let with_width = side.with_width(4.0);
        assert_eq!(with_width.width, 4.0);
        assert_eq!(with_width.color, Color::RED);

        let with_style = side.with_style(BorderStyle::None);
        assert_eq!(with_style.style, BorderStyle::None);

        let scaled = side.scale(2.0);
        assert_eq!(scaled.width, 4.0);
        assert_eq!(scaled.color, Color::RED);
    }

    #[test]
    fn test_border_side_conversions() {
        let from_color: BorderSide = Color::RED.into();
        assert_eq!(from_color.color, Color::RED);
        assert_eq!(from_color.width, 1.0);

        let stroke = Stroke::new(2.0, Color::BLUE.to_egui());
        let from_stroke: BorderSide = stroke.into();
        assert_eq!(from_stroke.color, Color::BLUE);
        assert_eq!(from_stroke.width, 2.0);

        let back_to_stroke = from_stroke.to_egui_stroke();
        assert_eq!(back_to_stroke.width, 2.0);
        assert_eq!(back_to_stroke.color, Color::BLUE.to_egui());
    }

    #[test]
    fn test_border_creation() {
        let all = Border::all(BorderSide::solid(Color::RED, 2.0));
        assert_eq!(all.top.width, 2.0);
        assert_eq!(all.right.width, 2.0);
        assert_eq!(all.bottom.width, 2.0);
        assert_eq!(all.left.width, 2.0);
        assert!(all.is_uniform());

        let uniform = Border::uniform(Color::RED, 2.0);
        assert_eq!(uniform, all);

        let none = Border::NONE;
        assert!(none.is_none());
    }

    #[test]
    fn test_border_symmetric() {
        let vertical = BorderSide::solid(Color::RED, 2.0);
        let horizontal = BorderSide::solid(Color::BLUE, 3.0);
        let symmetric = Border::symmetric(vertical, horizontal);

        assert_eq!(symmetric.top, vertical);
        assert_eq!(symmetric.bottom, vertical);
        assert_eq!(symmetric.left, horizontal);
        assert_eq!(symmetric.right, horizontal);
    }

    #[test]
    fn test_border_only() {
        let side = BorderSide::solid(Color::RED, 2.0);

        let only_top = Border::only_top(side);
        assert_eq!(only_top.top, side);
        assert!(only_top.right.is_none());
        assert!(only_top.bottom.is_none());
        assert!(only_top.left.is_none());

        let only_right = Border::only_right(side);
        assert_eq!(only_right.right, side);
        assert!(only_right.top.is_none());

        let only_bottom = Border::only_bottom(side);
        assert_eq!(only_bottom.bottom, side);

        let only_left = Border::only_left(side);
        assert_eq!(only_left.left, side);
    }

    #[test]
    fn test_border_dimensions() {
        let border = Border::new(
            BorderSide::solid(Color::RED, 1.0),
            BorderSide::solid(Color::RED, 2.0),
            BorderSide::solid(Color::RED, 3.0),
            BorderSide::solid(Color::RED, 4.0),
        );

        assert_eq!(border.horizontal_width(), 4.0); // top + bottom = 1 + 3
        assert_eq!(border.vertical_width(), 6.0);   // left + right = 4 + 2
        assert_eq!(border.dimensions(), Size::new(6.0, 4.0));
    }

    #[test]
    fn test_border_uniform_detection() {
        let uniform = Border::uniform(Color::RED, 2.0);
        assert!(uniform.is_uniform());
        assert_eq!(uniform.uniform_side(), Some(BorderSide::solid(Color::RED, 2.0)));

        let non_uniform = Border::new(
            BorderSide::solid(Color::RED, 1.0),
            BorderSide::solid(Color::RED, 2.0),
            BorderSide::solid(Color::RED, 1.0),
            BorderSide::solid(Color::RED, 1.0),
        );
        assert!(!non_uniform.is_uniform());
        assert_eq!(non_uniform.uniform_side(), None);
    }

    #[test]
    fn test_border_operations() {
        let border = Border::uniform(Color::RED, 2.0);

        let scaled = border.scale(2.0);
        assert_eq!(scaled.top.width, 4.0);
        assert_eq!(scaled.right.width, 4.0);

        let product = border * 3.0;
        assert_eq!(product.top.width, 6.0);
    }

    #[test]
    fn test_border_merge() {
        let border1 = Border::new(
            BorderSide::solid(Color::RED, 1.0),
            BorderSide::solid(Color::RED, 4.0),
            BorderSide::solid(Color::RED, 2.0),
            BorderSide::solid(Color::RED, 3.0),
        );

        let border2 = Border::new(
            BorderSide::solid(Color::BLUE, 3.0),
            BorderSide::solid(Color::BLUE, 2.0),
            BorderSide::solid(Color::BLUE, 4.0),
            BorderSide::solid(Color::BLUE, 1.0),
        );

        let max_merged = border1.merge_max(&border2);
        assert_eq!(max_merged.top.width, 3.0);  // max(1, 3)
        assert_eq!(max_merged.right.width, 4.0); // max(4, 2)
        assert_eq!(max_merged.bottom.width, 4.0); // max(2, 4)
        assert_eq!(max_merged.left.width, 3.0);  // max(3, 1)

        let min_merged = border1.merge_min(&border2);
        assert_eq!(min_merged.top.width, 1.0);  // min(1, 3)
        assert_eq!(min_merged.right.width, 2.0); // min(4, 2)
        assert_eq!(min_merged.bottom.width, 2.0); // min(2, 4)
        assert_eq!(min_merged.left.width, 1.0);  // min(3, 1)
    }

    #[test]
    fn test_border_conversions() {
        let from_side: Border = BorderSide::solid(Color::RED, 2.0).into();
        assert_eq!(from_side, Border::uniform(Color::RED, 2.0));

        let from_color: Border = Color::BLUE.into();
        assert_eq!(from_color, Border::uniform(Color::BLUE, 1.0));
    }
}
