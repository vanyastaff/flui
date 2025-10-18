//! Flex layout types

/// How a flex child is fit into the available space.
///
/// This determines whether a child should be given the maximum available space (tight)
/// or only as much as it needs (loose).
///
/// Similar to Flutter's `FlexFit`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::FlexFit;
///
/// let tight = FlexFit::Tight;
/// let loose = FlexFit::Loose;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FlexFit {
    /// Child is forced to fill the available space.
    ///
    /// The child widget must fill the space allocated by the flex layout.
    /// This is the default behavior for `Expanded` widgets.
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
    /// Returns true if this is a tight fit.
    pub const fn is_tight(&self) -> bool {
        matches!(self, FlexFit::Tight)
    }

    /// Returns true if this is a loose fit.
    pub const fn is_loose(&self) -> bool {
        matches!(self, FlexFit::Loose)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_fit_is_tight() {
        assert!(FlexFit::Tight.is_tight());
        assert!(!FlexFit::Loose.is_tight());
    }

    #[test]
    fn test_flex_fit_is_loose() {
        assert!(FlexFit::Loose.is_loose());
        assert!(!FlexFit::Tight.is_loose());
    }

    #[test]
    fn test_flex_fit_default() {
        let default = FlexFit::default();
        assert_eq!(default, FlexFit::Tight);
    }
}
