//! RenderMergeSemantics - merges descendant semantics into one node

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderMergeSemantics
#[derive(Debug, Clone, Copy)]
pub struct MergeSemanticsData {
    // Currently no additional data needed
    // Presence of this widget indicates merging should occur
}

impl MergeSemanticsData {
    /// Create new merge semantics data
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MergeSemanticsData {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject that merges descendant semantics into a single node
///
/// This combines all semantic information from descendants into one
/// semantic node for accessibility purposes.
///
/// Useful for complex widgets that should be treated as a single
/// interactive element by screen readers.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::special::MergeSemanticsData};
///
/// // Merge button label + icon into single semantic node
/// let mut merge = SingleRenderBox::new(MergeSemanticsData::new());
/// ```
pub type RenderMergeSemantics = SingleRenderBox<MergeSemanticsData>;

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderMergeSemantics {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints (pass-through)
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child directly (pass-through)
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_semantics_data_new() {
        let _data = MergeSemanticsData::new();
        // Just ensure it compiles
    }

    #[test]
    fn test_merge_semantics_data_default() {
        let _data = MergeSemanticsData::default();
        // Just ensure it compiles
    }

    #[test]
    fn test_render_merge_semantics_new() {
        let _merge = SingleRenderBox::new(MergeSemanticsData::new());
        // Just ensure it compiles
    }

    #[test]
    fn test_render_merge_semantics_layout() {
        let mut merge = SingleRenderBox::new(MergeSemanticsData::new());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = merge.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
