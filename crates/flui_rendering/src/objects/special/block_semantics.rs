//! RenderBlockSemantics - blocks descendant semantics from being merged

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderBlockSemantics
#[derive(Debug, Clone, Copy)]
pub struct BlockSemanticsData {
    /// Whether to block semantics merging
    pub blocking: bool,
}

impl BlockSemanticsData {
    /// Create new block semantics data
    pub fn new(blocking: bool) -> Self {
        Self { blocking }
    }
}

impl Default for BlockSemanticsData {
    fn default() -> Self {
        Self::new(true)
    }
}

/// RenderObject that blocks descendant semantics from being merged
///
/// Prevents an ancestor MergeSemantics from combining this subtree's
/// semantic information.
///
/// Useful when you want descendant widgets to have separate semantic nodes
/// even if an ancestor requests merging.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::special::BlockSemanticsData};
///
/// // Prevent merging for interactive child elements
/// let mut block = SingleRenderBox::new(BlockSemanticsData::new(true));
/// ```
pub type RenderBlockSemantics = SingleRenderBox<BlockSemanticsData>;

// ===== Public API =====

impl RenderBlockSemantics {
    /// Check if blocking semantics
    pub fn blocking(&self) -> bool {
        self.data().blocking
    }

    /// Set whether to block semantics
    pub fn set_blocking(&mut self, blocking: bool) {
        if self.data().blocking != blocking {
            self.data_mut().blocking = blocking;
            // In a full implementation, would notify semantics system
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderBlockSemantics {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Layout child with same constraints (pass-through)
        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            ctx.layout_child(child_id, constraints)
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
    fn test_block_semantics_data_new() {
        let data = BlockSemanticsData::new(true);
        assert!(data.blocking);

        let data = BlockSemanticsData::new(false);
        assert!(!data.blocking);
    }

    #[test]
    fn test_block_semantics_data_default() {
        let data = BlockSemanticsData::default();
        assert!(data.blocking);
    }

    #[test]
    fn test_render_block_semantics_new() {
        let block = SingleRenderBox::new(BlockSemanticsData::new(true));
        assert!(block.blocking());
    }

    #[test]
    fn test_render_block_semantics_set_blocking() {
        let mut block = SingleRenderBox::new(BlockSemanticsData::new(true));

        block.set_blocking(false);
        assert!(!block.blocking());

        block.set_blocking(true);
        assert!(block.blocking());
    }

    #[test]
    fn test_render_block_semantics_layout() {
        use flui_core::testing::mock_render_context;

        let block = SingleRenderBox::new(BlockSemanticsData::new(true));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = block.layout(constraints, &ctx);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
