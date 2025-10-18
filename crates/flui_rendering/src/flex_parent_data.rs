//! Parent data for flex layout
//!
//! Stores layout information for children in a flex layout (Row/Column).

use flui_core::ParentData;

/// Parent data for flex children
///
/// Stores the flex factor for flexible children and positioning information.
/// Similar to Flutter's FlexParentData.
#[derive(Debug, Clone, Copy)]
pub struct FlexParentData {
    /// The flex factor for this child
    ///
    /// - `None`: child is inflexible (sizes itself)
    /// - `Some(n)`: child is flexible and gets `n` units of remaining space
    pub flex: Option<i32>,

    /// Whether this child should fit the cross axis
    ///
    /// Used by CrossAxisAlignment::Stretch
    pub fit: FlexFit,
}

/// How a flexible child should fit in the cross axis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexFit {
    /// Child determines its own cross axis size
    Loose,

    /// Child fills the cross axis
    Tight,
}

impl FlexParentData {
    /// Create inflexible parent data (flex = None)
    pub const fn new() -> Self {
        Self {
            flex: None,
            fit: FlexFit::Loose,
        }
    }

    /// Create flexible parent data with the given flex factor
    pub const fn with_flex(flex: i32) -> Self {
        Self {
            flex: Some(flex),
            fit: FlexFit::Loose,
        }
    }

    /// Create flexible parent data with tight fit
    pub const fn with_flex_tight(flex: i32) -> Self {
        Self {
            flex: Some(flex),
            fit: FlexFit::Tight,
        }
    }

    /// Check if this child is flexible
    pub const fn is_flexible(&self) -> bool {
        self.flex.is_some()
    }

    /// Get the flex factor (returns 0 if not flexible)
    pub const fn flex_factor(&self) -> i32 {
        match self.flex {
            Some(f) => f,
            None => 0,
        }
    }
}

impl Default for FlexParentData {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentData for FlexParentData {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_parent_data_new() {
        let data = FlexParentData::new();
        assert_eq!(data.flex, None);
        assert_eq!(data.fit, FlexFit::Loose);
        assert!(!data.is_flexible());
        assert_eq!(data.flex_factor(), 0);
    }

    #[test]
    fn test_flex_parent_data_with_flex() {
        let data = FlexParentData::with_flex(2);
        assert_eq!(data.flex, Some(2));
        assert_eq!(data.fit, FlexFit::Loose);
        assert!(data.is_flexible());
        assert_eq!(data.flex_factor(), 2);
    }

    #[test]
    fn test_flex_parent_data_with_flex_tight() {
        let data = FlexParentData::with_flex_tight(3);
        assert_eq!(data.flex, Some(3));
        assert_eq!(data.fit, FlexFit::Tight);
        assert!(data.is_flexible());
        assert_eq!(data.flex_factor(), 3);
    }

    #[test]
    fn test_flex_parent_data_default() {
        let data = FlexParentData::default();
        assert_eq!(data.flex, None);
        assert!(!data.is_flexible());
    }
}
