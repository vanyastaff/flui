//! Flex layout types

/// How a flexible child is inscribed into the space allocated by a
/// flex layout.
///
/// Mirrors Flutter's `FlexFit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FlexFit {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Child is forced to fill the available space (the default).
    ///
    /// This is the behavior for `Expanded` widgets.
    #[default]
    Tight,

    /// Child can be smaller than the available space.
    ///
    /// The child widget can be at most as large as the available space
    /// (but is allowed to be smaller).
    /// This is the behavior for `Flexible` widgets.
    Loose,
}

impl FlexFit {
    /// Returns `true` if this is `FlexFit::Tight`.
    #[must_use]
    #[inline]
    pub const fn is_tight(&self) -> bool {
        matches!(self, FlexFit::Tight)
    }

    /// Returns `true` if this is `FlexFit::Loose`.
    #[must_use]
    #[inline]
    pub const fn is_loose(&self) -> bool {
        matches!(self, FlexFit::Loose)
    }

    /// Returns the opposite fit (`Tight` ↔ `Loose`).
    #[must_use]
    #[inline]
    pub const fn flip(&self) -> Self {
        match self {
            FlexFit::Tight => FlexFit::Loose,
            FlexFit::Loose => FlexFit::Tight,
        }
    }
}
