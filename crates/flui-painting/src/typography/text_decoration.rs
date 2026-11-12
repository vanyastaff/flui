//! Text decoration, the lines drawn along a span's text (ADR-0310), and the
//! paragraph-level text options.

use core::ops::BitOr;

use crate::styling::Color;

/// Which lines a [`TextDecoration`] draws along its text.
///
/// A set: combine lines with `|` or [`union`](Self::union).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(try_from = "u8", into = "u8")
)]
pub struct TextDecorationLines(u8);

impl TextDecorationLines {
    /// No line.
    pub const NONE: Self = Self(0);
    /// A line under the text, at the position its font gives.
    pub const UNDERLINE: Self = Self(1);
    /// A line over the text, at its font's ascent.
    pub const OVERLINE: Self = Self(1 << 1);
    /// A line through the text, at its font's strikeout position.
    pub const LINE_THROUGH: Self = Self(1 << 2);

    #[cfg(feature = "serde")]
    const ALL: u8 = Self::UNDERLINE.0 | Self::OVERLINE.0 | Self::LINE_THROUGH.0;

    /// The lines of both sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every line of `other` is in this set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set draws no line.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for TextDecorationLines {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

#[cfg(feature = "serde")]
impl TryFrom<u8> for TextDecorationLines {
    type Error = &'static str;

    fn try_from(bits: u8) -> Result<Self, Self::Error> {
        if bits & !Self::ALL == 0 {
            Ok(Self(bits))
        } else {
            Err("a text decoration names a line that does not exist")
        }
    }
}

#[cfg(feature = "serde")]
impl From<TextDecorationLines> for u8 {
    fn from(lines: TextDecorationLines) -> Self {
        lines.0
    }
}

/// The shape a decoration's lines are drawn in.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TextDecorationStyle {
    /// A single solid line.
    #[default]
    Solid,
    /// Two solid lines, a line's thickness apart.
    Double,
    /// Round dots as wide as the line is thick.
    Dotted,
    /// Dashes four times as long as the line is thick.
    Dashed,
    /// A wave around the line's centre.
    Wavy,
}

/// How thick a decoration's lines are, as a multiple of the thickness the
/// text's font recommends: finite, positive and at most
/// [`DecorationThickness::MAX`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(try_from = "f32", into = "f32")
)]
pub struct DecorationThickness(f32);

impl DecorationThickness {
    /// The font's own thickness.
    pub const FROM_FONT: Self = Self(1.0);

    /// The thickest scale: sixteen times the font's thickness, about an em.
    pub const MAX: Self = Self(16.0);

    /// `scale` times the font's thickness, or `None` unless `scale` is
    /// finite, positive and at most [`Self::MAX`].
    #[must_use]
    pub fn try_new(scale: f64) -> Option<Self> {
        // In range, the narrowing only rounds.
        (scale.is_finite() && scale > 0.0 && scale <= f64::from(Self::MAX.0))
            .then_some(Self(scale as f32))
    }

    /// The multiple of the font's thickness.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Default for DecorationThickness {
    fn default() -> Self {
        Self::FROM_FONT
    }
}

#[cfg(feature = "serde")]
impl TryFrom<f32> for DecorationThickness {
    type Error = &'static str;

    fn try_from(scale: f32) -> Result<Self, Self::Error> {
        Self::try_new(f64::from(scale))
            .ok_or("a decoration thickness is finite, positive and at most 16")
    }
}

#[cfg(feature = "serde")]
impl From<DecorationThickness> for f32 {
    fn from(thickness: DecorationThickness) -> Self {
        thickness.0
    }
}

/// The lines drawn along a span's text: which lines, their shape, colour and
/// thickness (ADR-0310).
///
/// Each line sits where the text's font puts it: an underline at the font's
/// underline position and thickness, a line-through at its strikeout, an
/// overline at its ascent with the underline's thickness. Underlines and
/// overlines are painted under the glyphs, a line-through over them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct TextDecoration {
    /// The lines drawn.
    pub lines: TextDecorationLines,
    /// Their shape.
    pub style: TextDecorationStyle,
    /// Their colour; `None` draws them in the colour of the glyphs they run
    /// along.
    pub color: Option<Color>,
    /// Their thickness, as a multiple of the font's.
    pub thickness: DecorationThickness,
}

impl TextDecoration {
    /// No decoration: a span styled with it draws none, whatever it
    /// inherits.
    pub const NONE: Self = Self::new(TextDecorationLines::NONE);
    /// A solid underline in the glyphs' colour.
    pub const UNDERLINE: Self = Self::new(TextDecorationLines::UNDERLINE);
    /// A solid overline in the glyphs' colour.
    pub const OVERLINE: Self = Self::new(TextDecorationLines::OVERLINE);
    /// A solid line-through in the glyphs' colour.
    pub const LINE_THROUGH: Self = Self::new(TextDecorationLines::LINE_THROUGH);

    /// `lines`, solid, in the glyphs' colour, at the font's thickness.
    #[must_use]
    pub const fn new(lines: TextDecorationLines) -> Self {
        Self {
            lines,
            style: TextDecorationStyle::Solid,
            color: None,
            thickness: DecorationThickness::FROM_FONT,
        }
    }

    /// The same lines drawn in `style`.
    #[must_use]
    pub const fn with_style(mut self, style: TextDecorationStyle) -> Self {
        self.style = style;
        self
    }

    /// The same lines drawn in `color`.
    #[must_use]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The same lines drawn at `thickness`.
    #[must_use]
    pub const fn with_thickness(mut self, thickness: DecorationThickness) -> Self {
        self.thickness = thickness;
        self
    }

    /// Whether the decoration draws no line.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.lines.is_empty()
    }
}

#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How visual text overflow is handled.
pub enum TextOverflow {
    /// Clip the overflowing text at its container boundary.
    #[default]
    Clip,
    /// Fade the overflowing text to transparent.
    Fade,
    /// Add an ellipsis to indicate overflow.
    Ellipsis,
    /// Display all text, even if it overflows.
    Visible,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How the width of a text paragraph is measured.
pub enum TextWidthBasis {
    /// Width fills the parent's width constraint (multiline text takes the full width).
    #[default]
    Parent,
    /// Width is based on the longest line.
    LongestLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How the `height` line-height multiplier applies to the ascent of the
/// first line and the descent of the last line of a paragraph.
pub struct TextHeightBehavior {
    /// Whether to apply height to the first line ascent.
    pub apply_height_to_first_ascent: bool,
    /// Whether to apply height to the last line descent.
    pub apply_height_to_last_descent: bool,
}

impl Default for TextHeightBehavior {
    #[inline]
    fn default() -> Self {
        Self {
            apply_height_to_first_ascent: true,
            apply_height_to_last_descent: true,
        }
    }
}

impl TextHeightBehavior {
    /// Creates a new text height behavior.
    #[inline]
    pub fn new(apply_to_first_ascent: bool, apply_to_last_descent: bool) -> Self {
        Self {
            apply_height_to_first_ascent: apply_to_first_ascent,
            apply_height_to_last_descent: apply_to_last_descent,
        }
    }

    /// Disables height application to all lines.
    pub const DISABLE_ALL: Self = Self {
        apply_height_to_first_ascent: false,
        apply_height_to_last_descent: false,
    };

    /// Disables height application to first line.
    pub const DISABLE_FIRST: Self = Self {
        apply_height_to_first_ascent: false,
        apply_height_to_last_descent: true,
    };

    /// Disables height application to last line.
    pub const DISABLE_LAST: Self = Self {
        apply_height_to_first_ascent: true,
        apply_height_to_last_descent: false,
    };
}

#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How the extra vertical space added by the `height` multiplier is
/// distributed above and below the text.
pub enum TextLeadingDistribution {
    /// Distribute leading according to the font's ascent/descent ratio.
    #[default]
    Proportional,
    /// Leading is distributed evenly.
    Even,
}

/// Full decoration description: which lines to draw plus their style,
/// color, and thickness.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextDecorationConfig {
    /// The decoration type.
    pub decoration: TextDecoration,
    /// The decoration style.
    pub style: TextDecorationStyle,
    /// The decoration color.
    pub color: Option<Color>,
    /// The decoration thickness.
    pub thickness: Option<f64>,
}

impl Default for TextDecorationConfig {
    #[inline]
    fn default() -> Self {
        Self {
            decoration: TextDecoration::NONE,
            style: TextDecorationStyle::default(),
            color: None,
            thickness: None,
        }
    }
}

impl TextDecorationConfig {
    /// Creates a config for the given decoration with default style, color, and thickness.
    #[must_use]
    #[inline]
    pub fn new(decoration: TextDecoration) -> Self {
        Self {
            decoration,
            ..Default::default()
        }
    }

    /// Sets the decoration style.
    #[must_use]
    #[inline]
    pub fn style(mut self, style: TextDecorationStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the decoration color.
    #[must_use]
    #[inline]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the decoration thickness.
    #[must_use]
    #[inline]
    pub fn thickness(mut self, thickness: f64) -> Self {
        self.thickness = Some(thickness);
        self
    }
}
