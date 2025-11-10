//! RenderExcludeSemantics - excludes child from semantics tree

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::BoxedLayer;
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

impl Render for RenderExcludeSemantics {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child with same constraints (pass-through)
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Paint child directly (pass-through)
        tree.paint_child(child_id, offset)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
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
