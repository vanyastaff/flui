//! Border types for styling

use crate::geometry::traits::{NumericUnit, Unit};
use crate::geometry::Pixels;
use crate::styling::Color;

/// Style of a border.
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
    #[inline]
    pub const fn is_solid(&self) -> bool {
        matches!(self, BorderStyle::Solid)
    }

    /// Returns true if this style is none.
    #[inline]
    pub const fn is_none(&self) -> bool {
        matches!(self, BorderStyle::None)
    }
}

/// A single side of a border.
///
/// Generic over unit type `T` for full type safety. Use `BorderSide<Pixels>` for UI borders.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{BorderSide, BorderStyle, Color};
/// use flui_types::geometry::px;
///
/// // Simple solid border
/// let side = BorderSide::new(Color::BLACK, px(2.0), BorderStyle::Solid);
///
/// // With custom stroke alignment (centered on border)
/// let side = BorderSide::with_stroke_align(Color::RED, px(1.0), BorderStyle::Solid, 0.5);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderSide<T: Unit> {
    /// The color of this side of the border.
    pub color: Color,

    /// The width of this side of the border.
    pub width: T,

    /// The style of this side of the border.
    pub style: BorderStyle,

    /// The relative position of the stroke on a border side.
    ///
    /// Values typically range from 0.0 (inside) to 1.0 (outside).
    /// 0.5 represents the stroke centered on the border.
    pub stroke_align: f32,
}

impl<T: Unit> BorderSide<T> {
    /// Creates a border side.
    ///
    /// # Arguments
    ///
    /// * `color` - The color of the border
    /// * `width` - The width of the border
    /// * `style` - The style of the border
    #[inline]
    pub const fn new(color: Color, width: T, style: BorderStyle) -> Self {
        Self {
            color,
            width,
            style,
            stroke_align: 0.0, // inside by default
        }
    }

    /// Creates a border side with custom stroke alignment.
    #[inline]
    pub const fn with_stroke_align(
        color: Color,
        width: T,
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

    /// Creates a border side with no border.
    #[inline]
    pub fn none() -> Self {
        Self {
            color: Color::BLACK,
            width: T::zero(),
            style: BorderStyle::None,
            stroke_align: 0.0,
        }
    }

    /// Creates a copy of this border side with the given color.
    #[inline]
    pub const fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// Creates a copy of this border side with the given width.
    #[inline]
    pub const fn with_width(self, width: T) -> Self {
        Self { width, ..self }
    }

    /// Creates a copy of this border side with the given style.
    #[inline]
    pub const fn with_style(self, style: BorderStyle) -> Self {
        Self { style, ..self }
    }

    /// Creates a copy of this border side with the given stroke alignment.
    #[inline]
    pub const fn with_stroke_alignment(self, stroke_align: f32) -> Self {
        Self {
            stroke_align,
            ..self
        }
    }
}

impl BorderSide<Pixels> {
    /// A hairline border side (width = 0.0).
    ///
    /// This is the default border side, with black color.
    pub const HAIRLINE: Self = Self {
        color: Color::BLACK,
        width: Pixels::ZERO,
        style: BorderStyle::Solid,
        stroke_align: 0.0,
    };

    /// A border side with no border.
    pub const NONE: Self = Self {
        color: Color::BLACK,
        width: Pixels::ZERO,
        style: BorderStyle::None,
        stroke_align: 0.0,
    };

    /// Returns true if this border side is effectively visible.
    ///
    /// A border is visible if its style is solid and its width is greater than 0.
    #[inline]
    pub fn is_visible(&self) -> bool {
        use crate::geometry::px;
        self.style.is_solid() && self.width > px(0.0)
    }
}

impl<T: NumericUnit> BorderSide<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    /// Linearly interpolate between two border sides.
    ///
    /// If the two sides have different styles, the interpolation switches
    /// abruptly at t = 0.5.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);

        if t < 0.5 {
            Self {
                color: Color::lerp(a.color, b.color, t),
                width: a.width * (1.0 - t) + b.width * t,
                style: a.style,
                stroke_align: a.stroke_align + (b.stroke_align - a.stroke_align) * t,
            }
        } else {
            Self {
                color: Color::lerp(a.color, b.color, t),
                width: a.width * (1.0 - t) + b.width * t,
                style: b.style,
                stroke_align: a.stroke_align + (b.stroke_align - a.stroke_align) * t,
            }
        }
    }

    /// Scale the width of this border side by the given factor.
    #[inline]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            width: self.width * factor,
            ..*self
        }
    }
}

impl<T: Unit> Default for BorderSide<T> {
    #[inline]
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            width: T::zero(),
            style: BorderStyle::Solid,
            stroke_align: 0.0,
        }
    }
}

/// Physical position of a border side on a box.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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
    #[inline]
    pub const fn all() -> [Self; 4] {
        [Self::Top, Self::Right, Self::Bottom, Self::Left]
    }

    /// Returns true if this is a horizontal position (Top or Bottom)
    #[inline]
    pub const fn is_horizontal(&self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    /// Returns true if this is a vertical position (Left or Right)
    #[inline]
    pub const fn is_vertical(&self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}
