//! Flex parent data - stores flex factor, fit, and offset for children

use flui_types::layout::FlexFit;
use flui_types::Offset;

/// Parent data for children of RenderFlex (Row/Column)
///
/// This data is attached to children of flex containers to control
/// how they are sized and positioned.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::parent_data::FlexParentData;
/// use flui_types::layout::FlexFit;
///
/// // For Expanded widget
/// let expanded = FlexParentData::expanded();
/// assert_eq!(expanded.flex, 1);
/// assert_eq!(expanded.fit, FlexFit::Tight);
///
/// // For Flexible widget
/// let flexible = FlexParentData::flexible();
/// assert_eq!(flexible.flex, 1);
/// assert_eq!(flexible.fit, FlexFit::Loose);
///
/// // Custom flex factor
/// let custom = FlexParentData::new(2, FlexFit::Tight);
/// assert_eq!(custom.flex, 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexParentData {
    /// The flex factor to use for this child.
    ///
    /// If non-zero, the child is flexible and will receive space according to its flex factor.
    /// Higher flex factors get proportionally more space.
    ///
    /// - `0`: Child is not flexible, uses intrinsic size
    /// - `1`: Child gets equal share of remaining space
    /// - `2+`: Child gets proportionally more space (2x gets twice as much as 1x)
    pub flex: i32,

    /// How the child should fit into the available space.
    ///
    /// - `FlexFit::Tight`: Child must fill allocated space (Expanded behavior)
    /// - `FlexFit::Loose`: Child can be smaller than allocated space (Flexible behavior)
    pub fit: FlexFit,

    /// Offset from parent's origin where this child should be painted/hit-tested
    ///
    /// Set during layout phase and used in paint and hit-test phases.
    /// This avoids recalculating child positions multiple times.
    pub offset: Offset,
}

impl FlexParentData {
    /// Create new flex parent data
    ///
    /// # Arguments
    ///
    /// * `flex` - The flex factor (0 = not flexible, 1+ = flexible)
    /// * `fit` - How the child should fit into the available space
    pub fn new(flex: i32, fit: FlexFit) -> Self {
        Self {
            flex,
            fit,
            offset: Offset::ZERO,
        }
    }

    /// Create flex parent data for Expanded widget (tight fit, flex=1)
    ///
    /// Expanded forces the child to fill all allocated space.
    pub fn expanded() -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Tight,
            offset: Offset::ZERO,
        }
    }

    /// Create flex parent data for Expanded widget with custom flex factor
    ///
    /// # Arguments
    ///
    /// * `flex` - The flex factor (higher = more space)
    pub fn expanded_with_flex(flex: i32) -> Self {
        Self {
            flex,
            fit: FlexFit::Tight,
            offset: Offset::ZERO,
        }
    }

    /// Create flex parent data for Flexible widget (loose fit, flex=1)
    ///
    /// Flexible allows the child to be smaller than allocated space.
    pub fn flexible() -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Loose,
            offset: Offset::ZERO,
        }
    }

    /// Create flex parent data for Flexible widget with custom flex factor
    ///
    /// # Arguments
    ///
    /// * `flex` - The flex factor (higher = more space)
    pub fn flexible_with_flex(flex: i32) -> Self {
        Self {
            flex,
            fit: FlexFit::Loose,
            offset: Offset::ZERO,
        }
    }

    /// Check if this child is flexible (flex > 0)
    pub fn is_flexible(&self) -> bool {
        self.flex > 0
    }

    /// Check if this child uses tight fit (must fill allocated space)
    pub fn is_tight(&self) -> bool {
        self.fit == FlexFit::Tight
    }
}

impl Default for FlexParentData {
    fn default() -> Self {
        Self {
            flex: 0,
            fit: FlexFit::Tight,
            offset: Offset::ZERO,
        }
    }
}

// Implement ParentData trait from flui_core
impl flui_core::render::ParentData for FlexParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn flui_core::render::ParentDataWithOffset> {
        Some(self)
    }
}

// Implement ParentDataWithOffset trait from flui_core
impl flui_core::render::ParentDataWithOffset for FlexParentData {
    fn offset(&self) -> flui_types::Offset {
        self.offset
    }

    fn set_offset(&mut self, offset: flui_types::Offset) {
        self.offset = offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_parent_data_default() {
        let data = FlexParentData::default();
        assert_eq!(data.flex, 0);
        assert_eq!(data.fit, FlexFit::Tight);
        assert!(!data.is_flexible());
    }

    #[test]
    fn test_flex_parent_data_expanded() {
        let data = FlexParentData::expanded();
        assert_eq!(data.flex, 1);
        assert_eq!(data.fit, FlexFit::Tight);
        assert!(data.is_flexible());
        assert!(data.is_tight());
    }

    #[test]
    fn test_flex_parent_data_flexible() {
        let data = FlexParentData::flexible();
        assert_eq!(data.flex, 1);
        assert_eq!(data.fit, FlexFit::Loose);
        assert!(data.is_flexible());
        assert!(!data.is_tight());
    }

    #[test]
    fn test_flex_parent_data_expanded_with_flex() {
        let data = FlexParentData::expanded_with_flex(3);
        assert_eq!(data.flex, 3);
        assert_eq!(data.fit, FlexFit::Tight);
        assert!(data.is_flexible());
    }

    #[test]
    fn test_flex_parent_data_flexible_with_flex() {
        let data = FlexParentData::flexible_with_flex(2);
        assert_eq!(data.flex, 2);
        assert_eq!(data.fit, FlexFit::Loose);
        assert!(data.is_flexible());
    }

    #[test]
    fn test_flex_parent_data_new() {
        let data = FlexParentData::new(5, FlexFit::Loose);
        assert_eq!(data.flex, 5);
        assert_eq!(data.fit, FlexFit::Loose);
    }

    #[test]
    fn test_is_flexible() {
        assert!(!FlexParentData::new(0, FlexFit::Tight).is_flexible());
        assert!(FlexParentData::new(1, FlexFit::Tight).is_flexible());
        assert!(FlexParentData::new(10, FlexFit::Tight).is_flexible());
    }

    #[test]
    fn test_is_tight() {
        assert!(FlexParentData::new(1, FlexFit::Tight).is_tight());
        assert!(!FlexParentData::new(1, FlexFit::Loose).is_tight());
    }
}
