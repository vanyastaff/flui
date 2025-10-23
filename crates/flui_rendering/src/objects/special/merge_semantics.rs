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
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Layout child with same constraints (pass-through)
        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, constraints, None)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Paint child directly (pass-through)
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
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
        use flui_core::testing::mock_render_context;

        let merge = SingleRenderBox::new(MergeSemanticsData::new());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = merge.layout(constraints, &ctx);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
