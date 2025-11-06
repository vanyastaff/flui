//! RenderScrollView - Render object for scrollable widgets
//!
//! Handles layout of scrollable content. This is a simplified version
//! that lays out the child with relaxed constraints and clips to viewport.
//!
//! **Note:** Scroll event handling will be added in a future update via Layer.handle_event()

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::layer::{BoxedLayer, ClipRectLayer};
use flui_types::layout::Axis;
use flui_types::{BoxConstraints, Offset, Rect, Size};

/// RenderScrollView - handles scrolling of a single child
///
/// **Current implementation:** Lays out child with infinite constraints
/// in scroll direction and clips to viewport.
///
/// **Future:** Will support scroll events and programmatic scrolling
/// through ScrollController.
#[derive(Debug)]
pub struct RenderScrollView {
    /// Scroll direction (Vertical or Horizontal)
    direction: Axis,

    /// Whether to reverse the scroll direction
    _reverse: bool,

    /// Viewport size (our constrained size)
    viewport_size: Size,
}

impl RenderScrollView {
    /// Create a new RenderScrollView
    pub fn new(direction: Axis, reverse: bool) -> Self {
        Self {
            direction,
            _reverse: reverse,
            viewport_size: Size::zero(),
        }
    }
}

impl SingleRender for RenderScrollView {
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Create constraints for child - infinite in scroll direction
        let child_constraints = match self.direction {
            Axis::Vertical => BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                0.0,
                f32::INFINITY, // Infinite height for vertical scroll
            ),
            Axis::Horizontal => BoxConstraints::new(
                0.0,
                f32::INFINITY, // Infinite width for horizontal scroll
                constraints.min_height,
                constraints.max_height,
            ),
        };

        // Layout child with infinite constraint
        let child_size = tree.layout_child(child_id, child_constraints);

        // Our size is constrained by viewport
        self.viewport_size = constraints.constrain(child_size);

        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderScrollView::layout: direction={:?}, child_size={:?}, viewport_size={:?}",
            self.direction,
            child_size,
            self.viewport_size
        );

        self.viewport_size
    }

    fn paint(
        &self,
        tree: &ElementTree,
        child_id: ElementId,
        offset: Offset,
    ) -> BoxedLayer {
        // Paint child at offset (no scrolling in this basic version)
        let child_layer = tree.paint_child(child_id, offset);

        // For now, just return child_id layer without clipping
        // TODO: Add ClipRectLayer when we support single-child layers with clipping
        tree.paint_child(child_id, offset)
    }

    fn metadata(&self) -> Option<&dyn std::any::Any> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_scroll_view_new() {
        let render = RenderScrollView::new(Axis::Vertical, false);
        assert_eq!(render.direction, Axis::Vertical);
    }
}
