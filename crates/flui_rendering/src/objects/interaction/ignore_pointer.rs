//! RenderIgnorePointer - makes widget ignore pointer events

use flui_types::Size;
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::BoxedLayer;

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

impl RenderObject for RenderIgnorePointer {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Paint child normally - ignoring only affects hit testing
        let child = cx.child();
        cx.capture_child_layer(child)

        // TODO: In a real implementation, we would:
        // 1. Register hit test behavior during hit testing phase
        // 2. Return false from hit_test to let events pass through
        // 3. Child doesn't receive events but widgets behind do
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
