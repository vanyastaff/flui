//! Text alignment types.

/// Horizontal text alignment.
///
/// # Memory Safety
/// - Zero-sized enum with no allocations
/// - Const-evaluable methods
///
/// # Type Safety
/// - `#[must_use]` on all pure methods
/// - Direction-aware alignment (Start/End)
///
/// # Examples
///
/// ```
/// use flui_types::typography::{TextAlign, TextDirection};
///
/// let align = TextAlign::Start;
/// assert_eq!(align.resolve(TextDirection::Ltr), TextAlign::Left);
/// assert_eq!(align.resolve(TextDirection::Rtl), TextAlign::Right);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlign {
    /// Align text to the left edge.
    #[default]
    Left,
    /// Align text to the right edge.
    Right,
    /// Center text horizontally.
    Center,
    /// Justify text (stretch lines to fill width).
    Justify,
    /// Align to the start edge (left in LTR, right in RTL).
    Start,
    /// Align to the end edge (right in LTR, left in RTL).
    End,
}

impl TextAlign {
    /// Resolves direction-dependent alignment to absolute alignment
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::{TextAlign, TextDirection};
    ///
    /// assert_eq!(TextAlign::Start.resolve(TextDirection::Ltr), TextAlign::Left);
    /// assert_eq!(TextAlign::Start.resolve(TextDirection::Rtl), TextAlign::Right);
    /// assert_eq!(TextAlign::Center.resolve(TextDirection::Ltr), TextAlign::Center);
    /// ```
    #[inline]
    #[must_use]
    pub const fn resolve(&self, direction: TextDirection) -> Self {
        match self {
            Self::Start => match direction {
                TextDirection::Ltr => Self::Left,
                TextDirection::Rtl => Self::Right,
            },
            Self::End => match direction {
                TextDirection::Ltr => Self::Right,
                TextDirection::Rtl => Self::Left,
            },
            _ => *self,
        }
    }

    /// Returns true if this alignment is direction-dependent
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextAlign;
    ///
    /// assert!(TextAlign::Start.is_direction_dependent());
    /// assert!(TextAlign::End.is_direction_dependent());
    /// assert!(!TextAlign::Left.is_direction_dependent());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_direction_dependent(&self) -> bool {
        matches!(self, Self::Start | Self::End)
    }

    /// Returns the horizontal offset factor for this alignment
    ///
    /// Returns 0.0 for left, 0.5 for center, 1.0 for right.
    /// Useful for rendering calculations.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextAlign;
    ///
    /// assert_eq!(TextAlign::Left.horizontal_factor(), 0.0);
    /// assert_eq!(TextAlign::Center.horizontal_factor(), 0.5);
    /// assert_eq!(TextAlign::Right.horizontal_factor(), 1.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn horizontal_factor(&self) -> f32 {
        match self {
            Self::Left | Self::Justify => 0.0,
            Self::Center => 0.5,
            Self::Right => 1.0,
            Self::Start => 0.0, // Assume LTR if not resolved
            Self::End => 1.0,
        }
    }
}

/// Vertical text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlignVertical {
    /// Align text to the top.
    Top,
    /// Center text vertically.
    #[default]
    Center,
    /// Align text to the bottom.
    Bottom,
}

impl TextAlignVertical {
    /// Returns the vertical offset factor for this alignment
    ///
    /// Returns 0.0 for top, 0.5 for center, 1.0 for bottom.
    /// Useful for rendering calculations.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextAlignVertical;
    ///
    /// assert_eq!(TextAlignVertical::Top.vertical_factor(), 0.0);
    /// assert_eq!(TextAlignVertical::Center.vertical_factor(), 0.5);
    /// assert_eq!(TextAlignVertical::Bottom.vertical_factor(), 1.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn vertical_factor(&self) -> f32 {
        match self {
            Self::Top => 0.0,
            Self::Center => 0.5,
            Self::Bottom => 1.0,
        }
    }
}

/// Text baseline for vertical alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextBaseline {
    /// Alphabetic baseline (default for most scripts).
    #[default]
    Alphabetic,
    /// Ideographic baseline (for CJK characters).
    Ideographic,
}

/// Text direction (left-to-right or right-to-left).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextDirection {
    /// Left-to-right text direction.
    #[default]
    Ltr,
    /// Right-to-left text direction.
    Rtl,
}

impl TextDirection {
    /// Returns true if this is left-to-right.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextDirection;
    ///
    /// assert!(TextDirection::Ltr.is_ltr());
    /// assert!(!TextDirection::Rtl.is_ltr());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_ltr(&self) -> bool {
        matches!(self, Self::Ltr)
    }

    /// Returns true if this is right-to-left.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextDirection;
    ///
    /// assert!(TextDirection::Rtl.is_rtl());
    /// assert!(!TextDirection::Ltr.is_rtl());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_rtl(&self) -> bool {
        matches!(self, Self::Rtl)
    }

    /// Returns the opposite direction
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextDirection;
    ///
    /// assert_eq!(TextDirection::Ltr.opposite(), TextDirection::Rtl);
    /// assert_eq!(TextDirection::Rtl.opposite(), TextDirection::Ltr);
    /// ```
    #[inline]
    #[must_use]
    pub const fn opposite(&self) -> Self {
        match self {
            Self::Ltr => Self::Rtl,
            Self::Rtl => Self::Ltr,
        }
    }
}

/// Text affinity for cursor positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAffinity {
    /// Cursor has affinity for the upstream (previous) character.
    #[default]
    Upstream,
    /// Cursor has affinity for the downstream (next) character.
    Downstream,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_align_default() {
        assert_eq!(TextAlign::default(), TextAlign::Left);
    }

    #[test]
    fn test_text_align_vertical_default() {
        assert_eq!(TextAlignVertical::default(), TextAlignVertical::Center);
    }

    #[test]
    fn test_text_baseline_default() {
        assert_eq!(TextBaseline::default(), TextBaseline::Alphabetic);
    }

    #[test]
    fn test_text_direction_default() {
        assert_eq!(TextDirection::default(), TextDirection::Ltr);
    }

    #[test]
    fn test_text_direction_checks() {
        assert!(TextDirection::Ltr.is_ltr());
        assert!(!TextDirection::Ltr.is_rtl());
        assert!(!TextDirection::Rtl.is_ltr());
        assert!(TextDirection::Rtl.is_rtl());
    }

    #[test]
    fn test_text_affinity_default() {
        assert_eq!(TextAffinity::default(), TextAffinity::Upstream);
    }

    #[test]
    fn test_text_align_variants() {
        let variants = [
            TextAlign::Left,
            TextAlign::Right,
            TextAlign::Center,
            TextAlign::Justify,
            TextAlign::Start,
            TextAlign::End,
        ];
        // Ensure all variants are distinct
        for (i, v1) in variants.iter().enumerate() {
            for (j, v2) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(v1, v2);
                } else {
                    assert_ne!(v1, v2);
                }
            }
        }
    }

    #[test]
    fn test_text_align_vertical_variants() {
        let variants = [
            TextAlignVertical::Top,
            TextAlignVertical::Center,
            TextAlignVertical::Bottom,
        ];
        for (i, v1) in variants.iter().enumerate() {
            for (j, v2) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(v1, v2);
                } else {
                    assert_ne!(v1, v2);
                }
            }
        }
    }
}
