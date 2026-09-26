//! Text alignment types.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How text is aligned horizontally within its container.
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
    /// Resolves `Start`/`End` to `Left`/`Right` for the given text direction.
    ///
    /// Direction-independent variants are returned unchanged.
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

    /// Returns `true` if this alignment depends on the text direction (`Start` or `End`).
    #[must_use]
    #[inline]
    pub const fn is_direction_dependent(&self) -> bool {
        matches!(self, Self::Start | Self::End)
    }

    /// Returns the horizontal alignment factor: 0.0 (left), 0.5 (center), or 1.0 (right).
    ///
    /// `Start` maps to 0.0 and `End` to 1.0 without direction resolution;
    /// call `resolve` first for RTL-aware alignment.
    #[must_use]
    #[inline]
    pub const fn horizontal_factor(&self) -> f32 {
        match self {
            Self::Left | Self::Justify | Self::Start => 0.0,
            Self::Center => 0.5,
            Self::Right | Self::End => 1.0,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How text is aligned vertically within its container.
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
    /// Returns the vertical alignment factor: 0.0 (top), 0.5 (center), or 1.0 (bottom).
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
/// The directionality in which text flows.
pub enum TextDirection {
    /// Left-to-right text direction.
    #[default]
    Ltr,
    /// Right-to-left text direction.
    Rtl,
}

impl TextDirection {
    /// Returns `true` if the direction is left-to-right.
    #[must_use]
    #[inline]
    pub const fn is_ltr(&self) -> bool {
        matches!(self, Self::Ltr)
    }

    /// Returns `true` if the direction is right-to-left.
    #[must_use]
    #[inline]
    pub const fn is_rtl(&self) -> bool {
        matches!(self, Self::Rtl)
    }

    /// Returns the opposite direction (`Ltr` <-> `Rtl`).
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
/// Which side of a text position the cursor associates with when the
/// position is ambiguous (e.g. at a line break or bidi boundary).
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

    /// `(resolved in LTR, resolved in RTL, direction dependent, factor)`.
    #[test]
    fn text_align() {
        use TextAlign::*;
        for (align, ltr, rtl, dependent, factor) in [
            (Left, Left, Left, false, 0.0),
            (Right, Right, Right, false, 1.0),
            (Center, Center, Center, false, 0.5),
            (Justify, Justify, Justify, false, 0.0),
            (Start, Left, Right, true, 0.0),
            (End, Right, Left, true, 1.0),
        ] {
            assert_eq!(align.resolve(TextDirection::Ltr), ltr, "{align:?}");
            assert_eq!(align.resolve(TextDirection::Rtl), rtl, "{align:?}");
            assert_eq!(align.is_direction_dependent(), dependent, "{align:?}");
            assert_eq!(align.horizontal_factor(), factor, "{align:?}");
        }
        assert_eq!(TextAlign::default(), Left);
    }

    #[test]
    fn vertical_factor() {
        use TextAlignVertical::*;
        assert_eq!(
            [Top, Center, Bottom].map(|v| v.vertical_factor()),
            [0.0, 0.5, 1.0]
        );
        assert_eq!(TextAlignVertical::default(), Center);
    }

    #[test]
    fn text_direction() {
        let (l, r) = (TextDirection::Ltr, TextDirection::Rtl);
        assert_eq!((l.is_ltr(), l.is_rtl(), l.opposite()), (true, false, r));
        assert_eq!((r.is_ltr(), r.is_rtl(), r.opposite()), (false, true, l));
        assert_eq!(TextDirection::default(), l);
        assert_eq!(TextAffinity::default(), TextAffinity::Upstream);
    }
}
