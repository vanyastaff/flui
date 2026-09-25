//! Text decoration types.

use crate::Color;

/// A linear decoration to draw near the text (underline, overline, line-through).
///
/// Decorations are stored as a bitfield, so multiple decorations can be
/// combined via [`TextDecoration::combine`] (mirroring Flutter's
/// `TextDecoration.combine`).
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextDecoration {
    /// Bitfield of decoration flags.
    flags: u8,
}

impl TextDecoration {
    /// No decoration.
    pub const NONE: Self = Self { flags: 0 };
    /// Underline decoration.
    pub const UNDERLINE: Self = Self { flags: 1 << 0 };
    /// Overline decoration.
    pub const OVERLINE: Self = Self { flags: 1 << 1 };
    /// Line-through decoration.
    pub const LINE_THROUGH: Self = Self { flags: 1 << 2 };

    /// Creates a decoration from a raw bitfield of decoration flags.
    #[must_use]
    #[inline]
    pub const fn new(flags: u8) -> Self {
        Self { flags }
    }

    /// Combines multiple decorations into one by OR-ing their flags.
    #[must_use]
    #[inline]
    pub const fn combine(decorations: &[Self]) -> Self {
        let mut flags = 0;
        let mut i = 0;
        while i < decorations.len() {
            flags |= decorations[i].flags;
            i += 1;
        }
        Self { flags }
    }

    /// Returns `true` if the underline decoration is set.
    #[must_use]
    #[inline]
    pub const fn has_underline(&self) -> bool {
        self.flags & Self::UNDERLINE.flags != 0
    }

    /// Returns `true` if the overline decoration is set.
    #[must_use]
    #[inline]
    pub const fn has_overline(&self) -> bool {
        self.flags & Self::OVERLINE.flags != 0
    }

    /// Returns `true` if the line-through decoration is set.
    #[must_use]
    #[inline]
    pub const fn has_line_through(&self) -> bool {
        self.flags & Self::LINE_THROUGH.flags != 0
    }

    /// Returns `true` if no decoration is set.
    #[must_use]
    #[inline]
    pub const fn is_none(&self) -> bool {
        self.flags == 0
    }
}

impl Default for TextDecoration {
    #[inline]
    fn default() -> Self {
        Self::NONE
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// The style in which a text decoration line is drawn.
pub enum TextDecorationStyle {
    /// A single solid line.
    #[default]
    Solid,
    /// Double line.
    Double,
    /// Dotted line.
    Dotted,
    /// Dashed line.
    Dashed,
    /// Wavy line.
    Wavy,
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
    pub fn with_style(mut self, style: TextDecorationStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the decoration color.
    #[must_use]
    #[inline]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the decoration thickness.
    #[must_use]
    #[inline]
    pub fn with_thickness(mut self, thickness: f64) -> Self {
        self.thickness = Some(thickness);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(d: &TextDecoration) -> (bool, bool, bool, bool) {
        (
            d.has_underline(),
            d.has_overline(),
            d.has_line_through(),
            d.is_none(),
        )
    }

    #[test]
    fn decoration_flags() {
        assert_eq!(flags(&TextDecoration::NONE), (false, false, false, true));
        assert_eq!(
            flags(&TextDecoration::default()),
            (false, false, false, true)
        );
        assert_eq!(
            flags(&TextDecoration::UNDERLINE),
            (true, false, false, false)
        );
        assert_eq!(
            flags(&TextDecoration::OVERLINE),
            (false, true, false, false)
        );
        assert_eq!(
            flags(&TextDecoration::LINE_THROUGH),
            (false, false, true, false)
        );
        assert_eq!(
            flags(&TextDecoration::new(0b101)),
            (true, false, true, false)
        );
        let both = TextDecoration::combine(&[TextDecoration::UNDERLINE, TextDecoration::OVERLINE]);
        assert_eq!(flags(&both), (true, true, false, false));
        assert_eq!(
            flags(&TextDecoration::combine(&[])),
            (false, false, false, true)
        );
    }

    #[test]
    fn height_behavior() {
        assert_eq!(
            TextHeightBehavior::default(),
            TextHeightBehavior::new(true, true)
        );
        assert_eq!(
            TextHeightBehavior::DISABLE_ALL,
            TextHeightBehavior::new(false, false)
        );
        assert_eq!(
            TextHeightBehavior::DISABLE_FIRST,
            TextHeightBehavior::new(false, true)
        );
        assert_eq!(
            TextHeightBehavior::DISABLE_LAST,
            TextHeightBehavior::new(true, false)
        );
    }

    #[test]
    fn decoration_config() {
        let plain = TextDecorationConfig::default();
        assert!(plain.decoration.is_none() && plain.color.is_none() && plain.thickness.is_none());
        assert_eq!(plain.style, TextDecorationStyle::Solid);
        let config = TextDecorationConfig::new(TextDecoration::UNDERLINE)
            .with_style(TextDecorationStyle::Wavy)
            .with_color(Color::RED)
            .with_thickness(1.5);
        assert!(config.decoration.has_underline());
        assert_eq!(
            (config.style, config.color, config.thickness),
            (TextDecorationStyle::Wavy, Some(Color::RED), Some(1.5))
        );
    }
}
