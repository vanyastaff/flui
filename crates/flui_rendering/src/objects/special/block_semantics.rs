//! RenderBlockSemantics - blocks descendant semantics from being merged

use flui_types::Size;
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::BoxedLayer;

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
/// use flui_rendering::RenderBlockSemantics;
///
/// // Prevent merging for interactive child elements
/// let mut block = RenderBlockSemantics::new(true);
/// ```
#[derive(Debug)]
pub struct RenderBlockSemantics {
    /// Block semantics data
    pub blocking: bool,
}

// ===== Public API =====

impl RenderBlockSemantics {
    /// Create new RenderBlockSemantics
    pub fn new(blocking: bool) -> Self {
        Self { blocking }
    }

    /// Check if blocking semantics
    pub fn blocking(&self) -> bool {
        self.blocking
    }

    /// Set whether to block semantics
    pub fn set_blocking(&mut self, blocking: bool) {
        if self.blocking != blocking {
            self.blocking = blocking;
            // In a full implementation, would notify semantics system
        }
    }
}

// ===== RenderObject Implementation =====

impl RenderObject for RenderBlockSemantics {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints (pass-through)
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Paint child directly (pass-through)
        let child = cx.child();
        cx.capture_child_layer(child)
    }
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
        let block = RenderBlockSemantics::new(true);
        assert!(block.blocking());
    }

    #[test]
    fn test_render_block_semantics_set_blocking() {
        let mut block = RenderBlockSemantics::new(true);

        block.set_blocking(false);
        assert!(!block.blocking());

        block.set_blocking(true);
        assert!(block.blocking());
    }
}
