//! RenderExcludeSemantics - excludes child from semantics tree

use flui_core::render::{
    RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::Size;

/// RenderObject that excludes its child from the semantics tree
///
/// When `excluding` is true, this and all descendants are invisible to
/// accessibility systems (screen readers, etc.).
///
/// Useful for decorative elements that don't need to be announced.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderExcludeSemantics;
///
/// // Exclude decorative icon from screen readers
/// let mut exclude = RenderExcludeSemantics::new(true);
/// ```
#[derive(Debug)]
pub struct RenderExcludeSemantics {
    /// Whether to exclude semantics
    pub excluding: bool,
}

// ===== Public API =====

impl RenderExcludeSemantics {
    /// Create new RenderExcludeSemantics
    pub fn new(excluding: bool) -> Self {
        Self { excluding }
    }

    /// Check if excluding semantics
    pub fn excluding(&self) -> bool {
        self.excluding
    }

    /// Set whether to exclude semantics
    pub fn set_excluding(&mut self, excluding: bool) {
        if self.excluding != excluding {
            self.excluding = excluding;
            // In a full implementation, would notify semantics system
        }
    }
}

// ===== RenderObject Implementation =====

impl RenderBox<Single> for RenderExcludeSemantics {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();
        // Layout child with same constraints (pass-through)
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();
        // Paint child directly (pass-through)
        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_exclude_semantics_new() {
        let exclude = RenderExcludeSemantics::new(true);
        assert!(exclude.excluding);
    }

    #[test]
    fn test_render_exclude_semantics_set_excluding() {
        let mut exclude = RenderExcludeSemantics::new(true);
        exclude.set_excluding(false);
        assert!(!exclude.excluding);

        exclude.set_excluding(true);
        assert!(exclude.excluding);
    }
}
