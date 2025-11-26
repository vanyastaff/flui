//! RenderBlockSemantics - blocks descendant semantics from being merged

use crate::core::{
    RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::Size;

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

impl RenderBox<Single> for RenderBlockSemantics {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        // Layout child with same constraints (pass-through)
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        // Paint child directly (pass-through)
        ctx.paint_child(child_id, ctx.offset);
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
