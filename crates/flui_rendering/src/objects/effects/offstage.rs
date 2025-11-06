//! RenderOffstage - hides widget from display

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::{layer::pool, BoxedLayer};
use flui_types::{constraints::BoxConstraints, Offset, Size};

/// RenderObject that hides its child from display
///
/// When offstage is true:
/// - The child is not painted
/// - The child is laid out (to maintain its state)
/// - The size is reported as zero
///
/// This is different from Opacity(0) - the child doesn't take up space.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOffstage;
///
/// let offstage = RenderOffstage::new(true);
/// ```
#[derive(Debug)]
pub struct RenderOffstage {
    /// Whether the child is offstage (hidden)
    pub offstage: bool,
}

impl RenderOffstage {
    /// Create new RenderOffstage
    pub fn new(offstage: bool) -> Self {
        Self { offstage }
    }

    /// Set whether child is offstage
    pub fn set_offstage(&mut self, offstage: bool) {
        self.offstage = offstage;
    }
}

impl Default for RenderOffstage {
    fn default() -> Self {
        Self { offstage: true }
    }
}

impl SingleRender for RenderOffstage {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // SingleArity always has exactly one child - layout it to maintain state
        let child_size = tree.layout_child(child_id, constraints);

        // Report size as zero if offstage, otherwise use child size
        if self.offstage {
            Size::ZERO
        } else if child_size != Size::ZERO {
            child_size
        } else {
            constraints.smallest()
        }
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Don't paint if offstage
        if !self.offstage {
            tree.paint_child(child_id, offset)
        } else {
            // Return empty container layer when offstage
            Box::new(pool::acquire_container())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_offstage_new() {
        let offstage = RenderOffstage::new(true);
        assert!(offstage.offstage);

        let offstage = RenderOffstage::new(false);
        assert!(!offstage.offstage);
    }

    #[test]
    fn test_render_offstage_default() {
        let offstage = RenderOffstage::default();
        assert!(offstage.offstage);
    }

    #[test]
    fn test_render_offstage_set_offstage() {
        let mut offstage = RenderOffstage::new(true);
        offstage.set_offstage(false);
        assert!(!offstage.offstage);
    }
}
