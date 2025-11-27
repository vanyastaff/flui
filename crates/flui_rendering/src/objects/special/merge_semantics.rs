//! RenderMergeSemantics - merges descendant semantics into one node
//!
//! Flutter reference: <https://api.flutter.dev/flutter/widgets/MergeSemantics-class.html>

use crate::core::{
    RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::Size;

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
/// use flui_rendering::RenderMergeSemantics;
///
/// // Merge button label + icon into single semantic node
/// let mut merge = RenderMergeSemantics::new();
/// ```
#[derive(Debug)]
pub struct RenderMergeSemantics {
    // Currently no additional data needed
    // Presence of this widget indicates merging should occur
}

impl RenderMergeSemantics {
    /// Create new RenderMergeSemantics
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RenderMergeSemantics {
    fn default() -> Self {
        Self::new()
    }
}

// ===== RenderObject Implementation =====

impl RenderBox<Single> for RenderMergeSemantics {
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
    fn test_render_merge_semantics_new() {
        let _merge = RenderMergeSemantics::new();
        // Just ensure it compiles
    }

    #[test]
    fn test_render_merge_semantics_default() {
        let _merge = RenderMergeSemantics::default();
        // Just ensure it compiles
    }
}
