//! Text alignment types.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How text is aligned horizontally within its container.
#[derive(flui_foundation::inspect::Inspect)]
#[non_exhaustive]
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
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// How text is aligned vertically within its container.
#[non_exhaustive]
pub enum TextAlignVertical {
    /// Align text to the top.
    Top,
    /// Center text vertically.
    #[default]
    Center,
    /// Align text to the bottom.
    Bottom,
}

// Re-export TextBaseline from layout module (canonical source)
pub use crate::TextBaseline;

// Text flow direction lives in `flui_foundation::TextDirection`; the
// painting path re-exports it so `flui_painting::TextDirection` keeps working.
pub use flui_foundation::TextDirection;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Which side of a text position the cursor associates with when the
/// position is ambiguous (e.g. at a line break or bidi boundary).
#[expect(
    clippy::exhaustive_enums,
    reason = "a caret sits before or after the boundary it names"
)]
pub enum TextAffinity {
    /// Cursor has affinity for the upstream (previous) character.
    #[default]
    Upstream,
    /// Cursor has affinity for the downstream (next) character.
    Downstream,
}
