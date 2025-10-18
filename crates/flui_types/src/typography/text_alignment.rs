//! Text alignment types.

/// Horizontal text alignment.
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
    pub const fn is_ltr(&self) -> bool {
        matches!(self, Self::Ltr)
    }

    /// Returns true if this is right-to-left.
    pub const fn is_rtl(&self) -> bool {
        matches!(self, Self::Rtl)
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
