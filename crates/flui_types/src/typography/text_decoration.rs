//! Text decoration types.

use crate::Color;

/// Text decoration (underline, overline, line-through).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Creates a new text decoration from flags.
    #[inline]
    #[must_use]
    pub const fn new(flags: u8) -> Self {
        Self { flags }
    }

    /// Combines multiple decorations.
    #[must_use]
    pub const fn combine(decorations: &[Self]) -> Self {
        let mut flags = 0;
        let mut i = 0;
        while i < decorations.len() {
            flags |= decorations[i].flags;
            i += 1;
        }
        Self { flags }
    }

    /// Returns true if this decoration contains underline.
    #[inline]
    #[must_use]
    pub const fn has_underline(&self) -> bool {
        self.flags & Self::UNDERLINE.flags != 0
    }

    /// Returns true if this decoration contains overline.
    #[inline]
    #[must_use]
    pub const fn has_overline(&self) -> bool {
        self.flags & Self::OVERLINE.flags != 0
    }

    /// Returns true if this decoration contains line-through.
    #[inline]
    #[must_use]
    pub const fn has_line_through(&self) -> bool {
        self.flags & Self::LINE_THROUGH.flags != 0
    }

    /// Returns true if this decoration is empty.
    #[inline]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.flags == 0
    }
}

impl Default for TextDecoration {
    fn default() -> Self {
        Self::NONE
    }
}

/// Style of text decoration line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextDecorationStyle {
    /// Solid line.
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

/// How text overflows should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextOverflow {
    /// Clip the overflowing text.
    #[default]
    Clip,
    /// Fade the overflowing text to transparent.
    Fade,
    /// Add an ellipsis to indicate overflow.
    Ellipsis,
    /// Display all text, even if it overflows.
    Visible,
}

/// How to measure the width of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextWidthBasis {
    /// Width is based on the parent container.
    #[default]
    Parent,
    /// Width is based on the longest line.
    LongestLine,
}

/// How text height is calculated.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextHeightBehavior {
    /// Whether to apply height to the first line ascent.
    pub apply_height_to_first_ascent: bool,
    /// Whether to apply height to the last line descent.
    pub apply_height_to_last_descent: bool,
}

impl Default for TextHeightBehavior {
    fn default() -> Self {
        Self {
            apply_height_to_first_ascent: true,
            apply_height_to_last_descent: true,
        }
    }
}

impl TextHeightBehavior {
    /// Creates a new text height behavior.
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

/// Distribution of leading (line spacing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextLeadingDistribution {
    /// Leading is distributed proportionally.
    #[default]
    Proportional,
    /// Leading is distributed evenly.
    Even,
}

/// Complete text decoration configuration.
#[derive(Debug, Clone, PartialEq)]
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
    /// Creates a new text decoration configuration.
    #[inline]
    #[must_use]
    pub fn new(decoration: TextDecoration) -> Self {
        Self {
            decoration,
            ..Default::default()
        }
    }

    /// Sets the decoration style.
    #[inline]
    #[must_use]
    pub fn with_style(mut self, style: TextDecorationStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the decoration color.
    #[inline]
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the decoration thickness.
    #[inline]
    #[must_use]
    pub fn with_thickness(mut self, thickness: f64) -> Self {
        self.thickness = Some(thickness);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_decoration_none() {
        let decoration = TextDecoration::NONE;
        assert!(decoration.is_none());
        assert!(!decoration.has_underline());
        assert!(!decoration.has_overline());
        assert!(!decoration.has_line_through());
    }

    #[test]
    fn test_text_decoration_underline() {
        let decoration = TextDecoration::UNDERLINE;
        assert!(!decoration.is_none());
        assert!(decoration.has_underline());
        assert!(!decoration.has_overline());
        assert!(!decoration.has_line_through());
    }

    #[test]
    fn test_text_decoration_combine() {
        let decoration = TextDecoration::combine(&[
            TextDecoration::UNDERLINE,
            TextDecoration::LINE_THROUGH,
        ]);
        assert!(!decoration.is_none());
        assert!(decoration.has_underline());
        assert!(!decoration.has_overline());
        assert!(decoration.has_line_through());
    }

    #[test]
    fn test_text_decoration_style_default() {
        assert_eq!(TextDecorationStyle::default(), TextDecorationStyle::Solid);
    }

    #[test]
    fn test_text_overflow_default() {
        assert_eq!(TextOverflow::default(), TextOverflow::Clip);
    }

    #[test]
    fn test_text_width_basis_default() {
        assert_eq!(TextWidthBasis::default(), TextWidthBasis::Parent);
    }

    #[test]
    fn test_text_height_behavior_default() {
        let behavior = TextHeightBehavior::default();
        assert!(behavior.apply_height_to_first_ascent);
        assert!(behavior.apply_height_to_last_descent);
    }

    #[test]
    fn test_text_height_behavior_constants() {
        assert!(!TextHeightBehavior::DISABLE_ALL.apply_height_to_first_ascent);
        assert!(!TextHeightBehavior::DISABLE_ALL.apply_height_to_last_descent);

        assert!(!TextHeightBehavior::DISABLE_FIRST.apply_height_to_first_ascent);
        assert!(TextHeightBehavior::DISABLE_FIRST.apply_height_to_last_descent);

        assert!(TextHeightBehavior::DISABLE_LAST.apply_height_to_first_ascent);
        assert!(!TextHeightBehavior::DISABLE_LAST.apply_height_to_last_descent);
    }

    #[test]
    fn test_text_leading_distribution_default() {
        assert_eq!(
            TextLeadingDistribution::default(),
            TextLeadingDistribution::Proportional
        );
    }

    #[test]
    fn test_text_decoration_config_builder() {
        let color = Color::rgba(255, 0, 0, 255);
        let config = TextDecorationConfig::new(TextDecoration::UNDERLINE)
            .with_style(TextDecorationStyle::Dashed)
            .with_color(color)
            .with_thickness(2.0);

        assert!(config.decoration.has_underline());
        assert_eq!(config.style, TextDecorationStyle::Dashed);
        assert_eq!(config.color, Some(color));
        assert_eq!(config.thickness, Some(2.0));
    }

    #[test]
    fn test_text_decoration_default() {
        assert_eq!(TextDecoration::default(), TextDecoration::NONE);
    }
}
