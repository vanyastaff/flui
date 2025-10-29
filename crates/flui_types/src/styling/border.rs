//! Border types for styling

use crate::styling::Color;

/// The style of a border side.
///
/// Similar to Flutter's `BorderStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BorderStyle {
    /// Draw the border as a solid line.
    #[default]
    Solid,

    /// Omit the border entirely.
    ///
    /// This is different from having a width of zero, as it affects
    /// how the border is rendered.
    None,
}

impl BorderStyle {
    /// Returns true if this style is solid.
    pub const fn is_solid(&self) -> bool {
        matches!(self, BorderStyle::Solid)
    }

    /// Returns true if this style is none.
    pub const fn is_none(&self) -> bool {
        matches!(self, BorderStyle::None)
    }
}

/// A side of a border of a box.
///
/// Similar to Flutter's `BorderSide`.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{BorderSide, BorderStyle, Color};
///
/// // Black solid border, 1px width
/// let side = BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid);
///
/// // No border
/// let none = BorderSide::none();
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderSide {
    /// The color of this side of the border.
    pub color: Color,

    /// The width of this side of the border, in logical pixels.
    pub width: f32,

    /// The style of this side of the border.
    pub style: BorderStyle,

    /// The relative position of the stroke on a [BorderSide] in an
    /// [OutlinedBorder] or [Border].
    ///
    /// Values typically range from 0.0 (inside) to 1.0 (outside).
    /// 0.5 represents the stroke centered on the border.
    pub stroke_align: f32,
}

impl BorderSide {
    /// Creates a border side.
    ///
    /// # Arguments
    ///
    /// * `color` - The color of the border
    /// * `width` - The width of the border in logical pixels
    /// * `style` - The style of the border
    pub const fn new(color: Color, width: f32, style: BorderStyle) -> Self {
        Self {
            color,
            width,
            style,
            stroke_align: 0.0, // inside by default
        }
    }

    /// Creates a border side with custom stroke alignment.
    pub const fn with_stroke_align(
        color: Color,
        width: f32,
        style: BorderStyle,
        stroke_align: f32,
    ) -> Self {
        Self {
            color,
            width,
            style,
            stroke_align,
        }
    }

    /// A hairline border side (width = 0.0).
    ///
    /// This is the default border side, with black color.
    pub const HAIRLINE: Self = Self {
        color: Color::BLACK,
        width: 0.0,
        style: BorderStyle::Solid,
        stroke_align: 0.0,
    };

    /// A border side with no border.
    pub const NONE: Self = Self {
        color: Color::BLACK,
        width: 0.0,
        style: BorderStyle::None,
        stroke_align: 0.0,
    };

    /// Creates a border side with no border.
    pub const fn none() -> Self {
        Self::NONE
    }

    /// Returns true if this border side is effectively invisible.
    ///
    /// A border is invisible if its style is None or its width is 0.0.
    pub fn is_visible(&self) -> bool {
        self.style.is_solid() && self.width > 0.0
    }

    /// Creates a copy of this border side with the given color.
    pub const fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// Creates a copy of this border side with the given width.
    pub const fn with_width(self, width: f32) -> Self {
        Self { width, ..self }
    }

    /// Creates a copy of this border side with the given style.
    pub const fn with_style(self, style: BorderStyle) -> Self {
        Self { style, ..self }
    }

    /// Creates a copy of this border side with the given stroke alignment.
    pub const fn with_stroke_alignment(self, stroke_align: f32) -> Self {
        Self {
            stroke_align,
            ..self
        }
    }

    /// Linearly interpolate between two border sides.
    ///
    /// If the two sides have different styles, the interpolation switches
    /// abruptly at t = 0.5.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);

        if t < 0.5 {
            Self {
                color: Color::lerp(a.color, b.color, t),
                width: a.width + (b.width - a.width) * t,
                style: a.style,
                stroke_align: a.stroke_align + (b.stroke_align - a.stroke_align) * t,
            }
        } else {
            Self {
                color: Color::lerp(a.color, b.color, t),
                width: a.width + (b.width - a.width) * t,
                style: b.style,
                stroke_align: a.stroke_align + (b.stroke_align - a.stroke_align) * t,
            }
        }
    }

    /// Scale the width of this border side by the given factor.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            width: self.width * factor,
            ..*self
        }
    }
}

impl Default for BorderSide {
    fn default() -> Self {
        Self::HAIRLINE
    }
}

/// Position of a border side in a box.
///
/// Used to identify which side of a border is being painted or styled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BorderPosition {
    /// Top side of the border
    Top,
    /// Right side of the border
    Right,
    /// Bottom side of the border
    Bottom,
    /// Left side of the border
    Left,
}

impl BorderPosition {
    /// Returns all border positions in order: Top, Right, Bottom, Left
    pub const fn all() -> [Self; 4] {
        [Self::Top, Self::Right, Self::Bottom, Self::Left]
    }

    /// Returns true if this is a horizontal position (Top or Bottom)
    pub const fn is_horizontal(&self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    /// Returns true if this is a vertical position (Left or Right)
    pub const fn is_vertical(&self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_border_style() {
        assert!(BorderStyle::Solid.is_solid());
        assert!(!BorderStyle::None.is_solid());
        assert!(BorderStyle::None.is_none());
        assert!(!BorderStyle::Solid.is_none());
    }

    #[test]
    fn test_border_style_default() {
        assert_eq!(BorderStyle::default(), BorderStyle::Solid);
    }

    #[test]
    fn test_border_side_new() {
        let side = BorderSide::new(Color::RED, 2.0, BorderStyle::Solid);
        assert_eq!(side.color, Color::RED);
        assert_eq!(side.width, 2.0);
        assert_eq!(side.style, BorderStyle::Solid);
        assert_eq!(side.stroke_align, 0.0);
    }

    #[test]
    fn test_border_side_constants() {
        assert_eq!(BorderSide::HAIRLINE.width, 0.0);
        assert_eq!(BorderSide::HAIRLINE.style, BorderStyle::Solid);

        assert_eq!(BorderSide::NONE.style, BorderStyle::None);
    }

    #[test]
    fn test_border_side_is_visible() {
        let visible = BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid);
        assert!(visible.is_visible());

        let invisible_width = BorderSide::new(Color::BLACK, 0.0, BorderStyle::Solid);
        assert!(!invisible_width.is_visible());

        let invisible_style = BorderSide::new(Color::BLACK, 1.0, BorderStyle::None);
        assert!(!invisible_style.is_visible());
    }

    #[test]
    fn test_border_side_with_methods() {
        let side = BorderSide::default();

        let colored = side.with_color(Color::BLUE);
        assert_eq!(colored.color, Color::BLUE);

        let wider = side.with_width(5.0);
        assert_eq!(wider.width, 5.0);

        let styled = side.with_style(BorderStyle::None);
        assert_eq!(styled.style, BorderStyle::None);

        let aligned = side.with_stroke_alignment(0.5);
        assert_eq!(aligned.stroke_align, 0.5);
    }

    #[test]
    fn test_border_side_lerp() {
        let a = BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid);
        let b = BorderSide::new(Color::WHITE, 5.0, BorderStyle::None);

        let mid = BorderSide::lerp(a, b, 0.5);
        assert_eq!(mid.width, 3.0);
        assert_eq!(mid.style, BorderStyle::None); // Switches at 0.5
    }

    #[test]
    fn test_border_side_scale() {
        let side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        let scaled = side.scale(2.5);
        assert_eq!(scaled.width, 5.0);
        assert_eq!(scaled.color, Color::BLACK);
    }

    #[test]
    fn test_border_side_default() {
        let default = BorderSide::default();
        assert_eq!(default, BorderSide::HAIRLINE);
    }

    #[test]
    fn test_border_position_all() {
        let positions = BorderPosition::all();
        assert_eq!(positions.len(), 4);
        assert_eq!(positions[0], BorderPosition::Top);
        assert_eq!(positions[1], BorderPosition::Right);
        assert_eq!(positions[2], BorderPosition::Bottom);
        assert_eq!(positions[3], BorderPosition::Left);
    }

    #[test]
    fn test_border_position_is_horizontal() {
        assert!(BorderPosition::Top.is_horizontal());
        assert!(BorderPosition::Bottom.is_horizontal());
        assert!(!BorderPosition::Left.is_horizontal());
        assert!(!BorderPosition::Right.is_horizontal());
    }

    #[test]
    fn test_border_position_is_vertical() {
        assert!(BorderPosition::Left.is_vertical());
        assert!(BorderPosition::Right.is_vertical());
        assert!(!BorderPosition::Top.is_vertical());
        assert!(!BorderPosition::Bottom.is_vertical());
    }
}
