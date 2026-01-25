//! Wrap layout types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WrapAlignment {
    #[default]
    Start,

    /// Place children at the end of each run.
    End,

    /// Place children in the center of each run.
    Center,

    /// Place children with equal space between them in each run.
    ///
    /// The first child is at the start, the last child is at the end.
    SpaceBetween,

    /// Place children with equal space around them in each run.
    ///
    /// Half-sized spaces at the start and end.
    SpaceAround,

    /// Place children with equal space between them and at edges.
    SpaceEvenly,
}

impl WrapAlignment {
    #[must_use]
    pub const fn uses_spacing(&self) -> bool {
        matches!(
            self,
            WrapAlignment::SpaceBetween | WrapAlignment::SpaceAround | WrapAlignment::SpaceEvenly
        )
    }

    #[must_use]
    pub const fn is_edge_aligned(&self) -> bool {
        matches!(self, WrapAlignment::Start | WrapAlignment::End)
    }

    #[must_use]
    pub const fn is_centered(&self) -> bool {
        matches!(self, WrapAlignment::Center)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WrapCrossAlignment {
    #[default]
    Start,

    /// Place runs at the end of the cross axis.
    End,

    /// Place runs in the center of the cross axis.
    Center,
}

impl WrapCrossAlignment {
    #[must_use]
    pub const fn is_edge_aligned(&self) -> bool {
        matches!(self, WrapCrossAlignment::Start | WrapCrossAlignment::End)
    }

    #[must_use]
    pub const fn is_centered(&self) -> bool {
        matches!(self, WrapCrossAlignment::Center)
    }
}
