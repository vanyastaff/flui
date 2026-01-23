//! Border types for styling

use crate::styling::Color;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BorderStyle {
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

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderSide {
    /// The color of this side of the border.
    pub color: Color,

    /// The width of this side of the border, in logical pixels.
    pub width: f32,

    /// The style of this side of the border.
    pub style: BorderStyle,

    /// The relative position of the stroke on a `BorderSide` in an
    /// `OutlinedBorder` or `Border`.
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
