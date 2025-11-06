//! RenderRepaintBoundary - optimization boundary for repainting

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{constraints::BoxConstraints, Offset, Size};

/// RenderObject that creates a repaint boundary
///
/// This widget creates a separate paint layer, isolating the child's
/// repainting from its ancestors. When the child repaints, only this
/// subtree needs to be repainted, not the entire widget tree.
///
/// Useful for optimizing performance when a widget repaints frequently
/// (e.g., animations, videos, interactive elements).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderRepaintBoundary;
///
/// // Create repaint boundary for animated child
/// let boundary = RenderRepaintBoundary::new();
/// ```
#[derive(Debug)]
pub struct RenderRepaintBoundary {
    /// Whether this boundary is currently active
    pub is_repaint_boundary: bool,
}

impl RenderRepaintBoundary {
    /// Create new RenderRepaintBoundary
    pub fn new() -> Self {
        Self {
            is_repaint_boundary: true,
        }
    }

    /// Create inactive boundary
    pub fn inactive() -> Self {
        Self {
            is_repaint_boundary: false,
        }
    }

    /// Set whether this is a repaint boundary
    pub fn set_is_repaint_boundary(&mut self, is_boundary: bool) {
        self.is_repaint_boundary = is_boundary;
    }
}

impl Default for RenderRepaintBoundary {
    fn default() -> Self {
        Self::new()
    }
}

impl SingleRender for RenderRepaintBoundary {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // SingleArity always has exactly one child
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child
        // TODO: In a full implementation with layer caching support:
        // - Create a cached layer if is_repaint_boundary is true
        // - Reuse the cached layer on subsequent paints if child hasn't changed
        // - Mark the layer as dirty when the child needs repainting
        //
        // This allows the framework to cache the layer and avoid
        // repainting the child if only the parent changes
        //
        // For now, we just paint the child directly
        tree.paint_child(child_id, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_repaint_boundary_new() {
        let boundary = RenderRepaintBoundary::new();
        assert!(boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_inactive() {
        let boundary = RenderRepaintBoundary::inactive();
        assert!(!boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_default() {
        let boundary = RenderRepaintBoundary::default();
        assert!(boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_set_is_repaint_boundary() {
        let mut boundary = RenderRepaintBoundary::new();
        boundary.set_is_repaint_boundary(false);
        assert!(!boundary.is_repaint_boundary);
    }
}
