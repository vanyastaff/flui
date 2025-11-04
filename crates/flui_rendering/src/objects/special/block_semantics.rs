//! RenderBlockSemantics - blocks descendant semantics from being merged

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

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

impl SingleRender for RenderBlockSemantics {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child with same constraints (pass-through)
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child directly (pass-through)
        tree.paint_child(child_id, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_block_semantics_new() {
        let block = RenderBlockSemantics::new(true);
        assert!(block.blocking);
    }

    #[test]
    fn test_render_block_semantics_set_blocking() {
        let mut block = RenderBlockSemantics::new(true);
        block.set_blocking(false);
        assert!(!block.blocking);

        block.set_blocking(true);
        assert!(block.blocking);
    }
}
