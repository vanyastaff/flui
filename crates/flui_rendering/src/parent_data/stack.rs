//! Stack parent data - stores alignment for positioned children

use flui_types::{Offset, layout::Alignment};

/// Parent data for children of RenderStack
///
/// This data is attached to children of stack containers to control
/// how they are positioned.
///
/// ## Offset Caching
///
/// The `offset` field is calculated during layout and cached here to avoid
/// recalculation during paint() and hit_test(). This is an optimization
/// similar to FlexParentData.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StackParentData {
    /// Alignment within the stack (for non-positioned children)
    pub alignment: Option<Alignment>,

    /// Position from left edge (for positioned children)
    pub left: Option<f32>,

    /// Position from top edge (for positioned children)
    pub top: Option<f32>,

    /// Position from right edge (for positioned children)
    pub right: Option<f32>,

    /// Position from bottom edge (for positioned children)
    pub bottom: Option<f32>,

    /// Width override (for positioned children)
    pub width: Option<f32>,

    /// Height override (for positioned children)
    pub height: Option<f32>,

    /// Cached child offset (calculated during layout)
    ///
    /// This is set by RenderStack during layout() and read during paint() and hit_test()
    /// to avoid recalculating the position from left/top/right/bottom values.
    pub offset: Offset,
}

impl StackParentData {
    /// Create new stack parent data
    pub fn new() -> Self {
        Self::default()
    }

    /// Create positioned stack parent data
    pub fn positioned(
        left: Option<f32>,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
        width: Option<f32>,
        height: Option<f32>,
    ) -> Self {
        Self {
            alignment: None,
            left,
            top,
            right,
            bottom,
            width,
            height,
            offset: Offset::ZERO,
        }
    }

    /// Check if this child is positioned
    pub fn is_positioned(&self) -> bool {
        self.left.is_some()
            || self.top.is_some()
            || self.right.is_some()
            || self.bottom.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }
}

impl Default for StackParentData {
    fn default() -> Self {
        Self {
            alignment: None,
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
            offset: Offset::ZERO,
        }
    }
}

// Implement ParentData trait from flui_core
impl flui_core::render::ParentData for StackParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn flui_core::render::ParentDataWithOffset> {
        Some(self)
    }
}

// Implement ParentDataWithOffset trait from flui_core
impl flui_core::render::ParentDataWithOffset for StackParentData {
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
    fn test_stack_parent_data_default() {
        let data = StackParentData::default();
        assert_eq!(data.alignment, None);
        assert_eq!(data.left, None);
        assert_eq!(data.top, None);
        assert_eq!(data.right, None);
        assert_eq!(data.bottom, None);
        assert_eq!(data.width, None);
        assert_eq!(data.height, None);
        assert!(!data.is_positioned());
    }

    #[test]
    fn test_stack_parent_data_new() {
        let data = StackParentData::new();
        assert!(!data.is_positioned());
    }

    #[test]
    fn test_stack_parent_data_positioned() {
        let data = StackParentData::positioned(
            Some(10.0),
            Some(20.0),
            None,
            None,
            None,
            None,
        );
        assert_eq!(data.left, Some(10.0));
        assert_eq!(data.top, Some(20.0));
        assert_eq!(data.right, None);
        assert_eq!(data.bottom, None);
        assert!(data.is_positioned());
    }

    #[test]
    fn test_stack_parent_data_fully_positioned() {
        let data = StackParentData::positioned(
            Some(10.0),
            Some(20.0),
            Some(30.0),
            Some(40.0),
            Some(100.0),
            Some(200.0),
        );
        assert_eq!(data.left, Some(10.0));
        assert_eq!(data.top, Some(20.0));
        assert_eq!(data.right, Some(30.0));
        assert_eq!(data.bottom, Some(40.0));
        assert_eq!(data.width, Some(100.0));
        assert_eq!(data.height, Some(200.0));
        assert!(data.is_positioned());
    }

    #[test]
    fn test_is_positioned_with_width_only() {
        let mut data = StackParentData::default();
        data.width = Some(100.0);
        assert!(data.is_positioned());
    }

    #[test]
    fn test_is_positioned_with_height_only() {
        let mut data = StackParentData::default();
        data.height = Some(100.0);
        assert!(data.is_positioned());
    }

    #[test]
    fn test_is_positioned_with_left_only() {
        let mut data = StackParentData::default();
        data.left = Some(10.0);
        assert!(data.is_positioned());
    }

    #[test]
    fn test_is_positioned_with_alignment() {
        let mut data = StackParentData::default();
        data.alignment = Some(Alignment::center());
        // Alignment alone doesn't make it positioned
        assert!(!data.is_positioned());
    }
}
