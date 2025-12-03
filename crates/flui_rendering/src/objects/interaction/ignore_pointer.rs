//! RenderIgnorePointer - makes widget ignore pointer events

use crate::core::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_interaction::HitTestResult;
use flui_types::{Offset, Size};

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
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> Size {
        // Layout child with same constraints
        ctx.layout_single_child()
            .unwrap_or_else(|_| ctx.constraints.smallest())
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Paint child normally - ignoring only affects hit testing
        let _ = ctx.paint_single_child(Offset::ZERO);
    }

    fn hit_test(&self, ctx: &BoxHitTestCtx<'_, Single>, result: &mut HitTestResult) -> bool {
        if self.ignoring {
            // Ignore pointer events - return false to let events pass through
            // This makes this widget and its children transparent to pointer events
            false // Events pass through to widgets behind
        } else {
            // Not ignoring - use default behavior (test children)
            ctx.hit_test_children(result)
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
