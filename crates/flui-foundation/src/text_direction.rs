//! The directionality in which text flows, shared by the paint, semantics and
//! reading-order paths (T11c; previously defined twice, in `flui-painting` and
//! `flui-semantics`, with hand-written mappings between them).

/// The directionality in which text flows.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(flui_foundation::inspect::Inspect)]
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
