//! Flex layout types

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FlexFit {
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
    #[must_use]
    pub const fn is_tight(&self) -> bool {
        matches!(self, FlexFit::Tight)
    }

    #[must_use]
    pub const fn is_loose(&self) -> bool {
        matches!(self, FlexFit::Loose)
    }

    #[must_use]
    pub const fn flip(&self) -> Self {
        match self {
            FlexFit::Tight => FlexFit::Loose,
            FlexFit::Loose => FlexFit::Tight,
        }
    }
}
