//! Wrap layout types

/// How children within a run should be placed in the main axis of a
/// `Wrap` layout.
///
/// Mirrors Flutter's `WrapAlignment`. Applies to each run (line)
/// independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WrapAlignment {
    /// Place children at the start of each run (the default).
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
    /// Returns `true` for the space-distributing variants
    /// (`SpaceBetween`, `SpaceAround`, `SpaceEvenly`).
    #[must_use]
    #[inline]
    pub const fn uses_spacing(&self) -> bool {
        matches!(
            self,
            WrapAlignment::SpaceBetween | WrapAlignment::SpaceAround | WrapAlignment::SpaceEvenly
        )
    }

    /// Returns `true` if children are packed against a run edge
    /// (`Start` or `End`).
    #[must_use]
    #[inline]
    pub const fn is_edge_aligned(&self) -> bool {
        matches!(self, WrapAlignment::Start | WrapAlignment::End)
    }

    /// Returns `true` if this is `Center`.
    #[must_use]
    #[inline]
    pub const fn is_centered(&self) -> bool {
        matches!(self, WrapAlignment::Center)
    }
}

/// How children within a run should be aligned relative to each other
/// in the cross axis of a `Wrap` layout.
///
/// Mirrors Flutter's `WrapCrossAlignment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WrapCrossAlignment {
    /// Place runs at the start of the cross axis (the default).
    #[default]
    Start,

    /// Place runs at the end of the cross axis.
    End,

    /// Place runs in the center of the cross axis.
    Center,
}

impl WrapCrossAlignment {
    /// Returns `true` if runs are packed against a cross-axis edge
    /// (`Start` or `End`).
    #[must_use]
    #[inline]
    pub const fn is_edge_aligned(&self) -> bool {
        matches!(self, WrapCrossAlignment::Start | WrapCrossAlignment::End)
    }

    /// Returns `true` if this is `Center`.
    #[must_use]
    #[inline]
    pub const fn is_centered(&self) -> bool {
        matches!(self, WrapCrossAlignment::Center)
    }
}
