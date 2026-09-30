//! Border types for styling

use crate::styling::Color;
use flui_foundation::geometry::traits::{NumericUnit, Unit};

/// Style of a border.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
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
/// Generic over unit type `T` for full type safety. Use `BorderSide<f64>`
/// for UI borders.
///
/// # Examples
///
/// ```
/// use flui_painting::styling::{BorderSide, BorderStyle, Color};
///
/// // Simple solid border
/// let side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
///
/// // With custom stroke alignment (centered on border)
/// let side = BorderSide::with_stroke_align(Color::RED, 1.0, BorderStyle::Solid, 0.5);
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
    pub stroke_align: f64,
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
        stroke_align: f64,
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
    pub const fn with_stroke_alignment(self, stroke_align: f64) -> Self {
        Self {
            stroke_align,
            ..self
        }
    }
}

impl BorderSide<f64> {
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

    /// Returns true if this border side is effectively visible.
    ///
    /// A border is visible if its style is solid and its width is greater than
    /// 0.
    #[inline]
    pub fn is_visible(&self) -> bool {
        self.style.is_solid() && self.width > 0.0
    }
}

impl<T: NumericUnit> BorderSide<T>
where
    T: std::ops::Mul<f64, Output = T>,
{
    /// Linearly interpolate between two border sides (`t` clamped to `0..=1`).
    ///
    /// When the styles differ, a `None` side takes part as its own color
    /// made fully transparent and the result is `Solid`, so a border fades
    /// in or out instead of vanishing at `t = 0.5`.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        let width = a.width * (1.0 - t) + b.width * t;
        if width < T::zero() {
            return Self::none();
        }
        let stroke_align = a.stroke_align + (b.stroke_align - a.stroke_align) * t;
        #[expect(
            clippy::float_cmp,
            reason = "exact-equality shortcut for identical styles"
        )]
        if a.style == b.style && a.stroke_align == b.stroke_align {
            return Self {
                color: Color::lerp(a.color, b.color, t),
                width,
                style: a.style,
                stroke_align,
            };
        }
        let visible = |side: &Self| match side.style {
            BorderStyle::Solid => side.color,
            BorderStyle::None => side.color.with_alpha(0),
        };
        Self {
            color: Color::lerp(visible(&a), visible(&b), t),
            width,
            style: BorderStyle::Solid,
            stroke_align,
        }
    }

    /// Scale the width of this border side by the given factor.
    #[inline]
    pub fn scale(&self, factor: f64) -> Self {
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
