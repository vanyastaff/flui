//! RenderIgnorePointer - makes widget ignore pointer events

use flui_interaction::HitTestResult;
use crate::core::{
    RenderBox, Single, {BoxLayoutCtx, HitTestContext},
};
use flui_types::Size;

/// RenderObject that makes its subtree ignore pointer events
///
/// When ignoring is true, this widget and its children don't respond to
/// pointer events. Unlike AbsorbPointer, events pass through to widgets behind.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderIgnorePointer;
///
/// let mut ignore = RenderIgnorePointer::new(true);
/// ```
#[derive(Debug)]
pub struct RenderIgnorePointer {
    /// Whether to ignore pointer events
    pub ignoring: bool,
}

impl RenderIgnorePointer {
    /// Create new RenderIgnorePointer
    pub fn new(ignoring: bool) -> Self {
        Self { ignoring }
    }

    /// Check if ignoring pointer events
    pub fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Set whether to ignore pointer events
    ///
    /// This doesn't affect layout or paint, only hit testing.
    pub fn set_ignoring(&mut self, ignoring: bool) {
        self.ignoring = ignoring;
        // Note: In a full implementation, this would mark needs hit test update
    }
}

impl Default for RenderIgnorePointer {
    fn default() -> Self {
        Self { ignoring: true }
    }
}

impl RenderBox<Single> for RenderIgnorePointer {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Single>) -> Size {
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = ctx.children.single();
        // Paint child normally - ignoring only affects hit testing
        ctx.paint_child(child_id, ctx.offset);
    }

    fn hit_test(
        &self,
        ctx: HitTestContext<'_, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool {
        if self.ignoring {
            // Ignore pointer events - return false to let events pass through
            // This makes this widget and its children transparent to pointer events
            false // Events pass through to widgets behind
        } else {
            // Not ignoring - use default behavior (test children)
            self.hit_test_children(&ctx, result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_ignore_pointer_new() {
        let ignore = RenderIgnorePointer::new(true);
        assert!(ignore.ignoring());

        let ignore = RenderIgnorePointer::new(false);
        assert!(!ignore.ignoring());
    }

    #[test]
    fn test_render_ignore_pointer_default() {
        let ignore = RenderIgnorePointer::default();
        assert!(ignore.ignoring());
    }

    #[test]
    fn test_render_ignore_pointer_set_ignoring() {
        let mut ignore = RenderIgnorePointer::new(true);

        ignore.set_ignoring(false);
        assert!(!ignore.ignoring());

        ignore.set_ignoring(true);
        assert!(ignore.ignoring());
    }
}
