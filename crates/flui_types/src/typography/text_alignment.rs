//! Text alignment types.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlign {
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
    #[must_use]
    #[inline]
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

    #[must_use]
    #[inline]
    pub const fn is_direction_dependent(&self) -> bool {
        matches!(self, Self::Start | Self::End)
    }

    #[must_use]
    #[inline]
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlignVertical {
    /// Align text to the top.
    Top,
    #[default]
    Center,
    /// Align text to the bottom.
    Bottom,
}

impl TextAlignVertical {
    #[must_use]
    #[inline]
    pub const fn vertical_factor(&self) -> f32 {
        match self {
            Self::Top => 0.0,
            Self::Center => 0.5,
            Self::Bottom => 1.0,
        }
    }
}

// Re-export TextBaseline from layout module (canonical source)
pub use crate::layout::TextBaseline;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextDirection {
    #[default]
    Ltr,
    /// Right-to-left text direction.
    Rtl,
}

impl TextDirection {
    #[must_use]
    #[inline]
    pub const fn is_ltr(&self) -> bool {
        matches!(self, Self::Ltr)
    }

    #[must_use]
    #[inline]
    pub const fn is_rtl(&self) -> bool {
        matches!(self, Self::Rtl)
    }

    #[must_use]
    #[inline]
    pub const fn opposite(&self) -> Self {
        match self {
            Self::Ltr => Self::Rtl,
            Self::Rtl => Self::Ltr,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAffinity {
    #[default]
    Upstream,
    /// Cursor has affinity for the downstream (next) character.
    Downstream,
}

#[cfg(test)]
mod tests {
    use super::*;

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
