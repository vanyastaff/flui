//! Text decoration types.

use crate::Color;

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

    #[must_use]
    #[inline]
    pub const fn new(flags: u8) -> Self {
        Self { flags }
    }

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

    #[must_use]
    #[inline]
    pub const fn has_underline(&self) -> bool {
        self.flags & Self::UNDERLINE.flags != 0
    }

    #[must_use]
    #[inline]
    pub const fn has_overline(&self) -> bool {
        self.flags & Self::OVERLINE.flags != 0
    }

    #[must_use]
    #[inline]
    pub const fn has_line_through(&self) -> bool {
        self.flags & Self::LINE_THROUGH.flags != 0
    }

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
pub enum TextDecorationStyle {
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

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextOverflow {
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
pub enum TextWidthBasis {
    #[default]
    Parent,
    /// Width is based on the longest line.
    LongestLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextLeadingDistribution {
    #[default]
    Proportional,
    /// Leading is distributed evenly.
    Even,
}

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
    #[must_use]
    #[inline]
    pub fn new(decoration: TextDecoration) -> Self {
        Self {
            decoration,
            ..Default::default()
        }
    }

    #[must_use]
    #[inline]
    pub fn with_style(mut self, style: TextDecorationStyle) -> Self {
        self.style = style;
        self
    }

    #[must_use]
    #[inline]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    #[must_use]
    #[inline]
    pub fn with_thickness(mut self, thickness: f64) -> Self {
        self.thickness = Some(thickness);
        self
    }
}
