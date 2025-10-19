//! Wrap layout types

/// How the runs in a wrap layout should be placed in the main axis.
///
/// Similar to Flutter's `WrapAlignment`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::WrapAlignment;
///
/// let start = WrapAlignment::Start;
/// let space_between = WrapAlignment::SpaceBetween;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WrapAlignment {
    /// Place children at the start of each run.
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
    /// Returns true if this alignment uses spacing.
    #[inline]
    #[must_use]
    pub const fn uses_spacing(&self) -> bool {
        matches!(self, WrapAlignment::SpaceBetween | WrapAlignment::SpaceAround | WrapAlignment::SpaceEvenly)
    }

    /// Returns true if this alignment is edge-aligned.
    #[inline]
    #[must_use]
    pub const fn is_edge_aligned(&self) -> bool {
        matches!(self, WrapAlignment::Start | WrapAlignment::End)
    }

    /// Returns true if this alignment centers content.
    #[inline]
    #[must_use]
    pub const fn is_centered(&self) -> bool {
        matches!(self, WrapAlignment::Center)
    }
}

/// How the runs themselves should be placed in the cross axis.
///
/// Similar to Flutter's `WrapCrossAlignment`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::WrapCrossAlignment;
///
/// let start = WrapCrossAlignment::Start;
/// let center = WrapCrossAlignment::Center;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WrapCrossAlignment {
    /// Place runs at the start of the cross axis.
    #[default]
    Start,

    /// Place runs at the end of the cross axis.
    End,

    /// Place runs in the center of the cross axis.
    Center,
}

impl WrapCrossAlignment {
    /// Returns true if this alignment is edge-aligned.
    #[inline]
    #[must_use]
    pub const fn is_edge_aligned(&self) -> bool {
        matches!(self, WrapCrossAlignment::Start | WrapCrossAlignment::End)
    }

    /// Returns true if this alignment centers content.
    #[inline]
    #[must_use]
    pub const fn is_centered(&self) -> bool {
        matches!(self, WrapCrossAlignment::Center)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_alignment_default() {
        let default = WrapAlignment::default();
        assert_eq!(default, WrapAlignment::Start);
    }

    #[test]
    fn test_wrap_cross_alignment_default() {
        let default = WrapCrossAlignment::default();
        assert_eq!(default, WrapCrossAlignment::Start);
    }

    #[test]
    fn test_wrap_alignment_variants() {
        // Just ensure all variants exist
        let _variants = [
            WrapAlignment::Start,
            WrapAlignment::End,
            WrapAlignment::Center,
            WrapAlignment::SpaceBetween,
            WrapAlignment::SpaceAround,
            WrapAlignment::SpaceEvenly,
        ];
    }

    #[test]
    fn test_wrap_cross_alignment_variants() {
        // Just ensure all variants exist
        let _variants = [
            WrapCrossAlignment::Start,
            WrapCrossAlignment::End,
            WrapCrossAlignment::Center,
        ];
    }
}
