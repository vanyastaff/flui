//! RenderAbsorbPointer - prevents pointer events from reaching children

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{constraints::BoxConstraints, Offset, Size};

/// RenderObject that prevents pointer events from reaching its child
///
/// When absorbing is true, this widget consumes all pointer events,
/// preventing them from reaching the child. The child is still painted
/// but doesn't receive events.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAbsorbPointer;
///
/// let mut absorb = RenderAbsorbPointer::new(true);
/// ```
#[derive(Debug)]
pub struct RenderAbsorbPointer {
    /// Whether to absorb pointer events
    pub absorbing: bool,
}

impl RenderAbsorbPointer {
    /// Create new RenderAbsorbPointer
    pub fn new(absorbing: bool) -> Self {
        Self { absorbing }
    }

    /// Check if absorbing pointer events
    pub fn absorbing(&self) -> bool {
        self.absorbing
    }

    /// Set whether to absorb pointer events
    ///
    /// This doesn't affect layout or paint, only hit testing.
    pub fn set_absorbing(&mut self, absorbing: bool) {
        self.absorbing = absorbing;
        // Note: In a full implementation, this would mark needs hit test update
    }
}

impl Default for RenderAbsorbPointer {
    fn default() -> Self {
        Self { absorbing: true }
    }
}

impl SingleRender for RenderAbsorbPointer {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child normally - absorbing only affects hit testing
        tree.paint_child(child_id, offset)

        // TODO: In a real implementation, we would:
        // 1. Register hit test behavior during hit testing phase
        // 2. Return true from hit_test to absorb events
        // 3. Prevent events from propagating to child
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_absorb_pointer_new() {
        let absorb = RenderAbsorbPointer::new(true);
        assert!(absorb.absorbing());

        let absorb = RenderAbsorbPointer::new(false);
        assert!(!absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_default() {
        let absorb = RenderAbsorbPointer::default();
        assert!(absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_set_absorbing() {
        let mut absorb = RenderAbsorbPointer::new(true);

        absorb.set_absorbing(false);
        assert!(!absorb.absorbing());

        absorb.set_absorbing(true);
        assert!(absorb.absorbing());
    }
}
