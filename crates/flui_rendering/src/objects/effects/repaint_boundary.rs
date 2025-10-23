//! RenderRepaintBoundary - optimization boundary for repainting

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderRepaintBoundary
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepaintBoundaryData {
    /// Whether this boundary is currently active
    pub is_repaint_boundary: bool,
}

impl RepaintBoundaryData {
    /// Create new repaint boundary data
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
}

impl Default for RepaintBoundaryData {
    fn default() -> Self {
        Self::new()
    }
}

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
/// use flui_rendering::{SingleRenderBox, objects::effects::RepaintBoundaryData};
///
/// // Create repaint boundary for animated child
/// let mut boundary = SingleRenderBox::new(RepaintBoundaryData::new());
/// ```
pub type RenderRepaintBoundary = SingleRenderBox<RepaintBoundaryData>;

// ===== Public API =====

impl RenderRepaintBoundary {
    /// Get whether this is a repaint boundary
    pub fn is_repaint_boundary(&self) -> bool {
        self.data().is_repaint_boundary
    }

    /// Set whether this is a repaint boundary
    pub fn set_is_repaint_boundary(&mut self, is_boundary: bool) {
        if self.data().is_repaint_boundary != is_boundary {
            self.data_mut().is_repaint_boundary = is_boundary;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderRepaintBoundary {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child
        // In a real implementation with layer support, we would:
        // 1. Create a new paint layer if is_repaint_boundary is true
        // 2. Paint child to that layer
        // 3. Composite the layer with the parent
        //
        // This allows the framework to cache the layer and avoid
        // repainting the child if only the parent changes
        //
        // For now, we just paint the child directly
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }

        // Note: Full repaint boundary support requires:
        // - Layer-based rendering architecture
        // - Ability to cache and reuse layers
        // - Dirty region tracking
        // - Compositor integration
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repaint_boundary_data_new() {
        let data = RepaintBoundaryData::new();
        assert!(data.is_repaint_boundary);
    }

    #[test]
    fn test_repaint_boundary_data_inactive() {
        let data = RepaintBoundaryData::inactive();
        assert!(!data.is_repaint_boundary);
    }

    #[test]
    fn test_repaint_boundary_data_default() {
        let data = RepaintBoundaryData::default();
        assert!(data.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_new() {
        let boundary = SingleRenderBox::new(RepaintBoundaryData::new());
        assert!(boundary.is_repaint_boundary());
    }

    #[test]
    fn test_render_repaint_boundary_set_is_repaint_boundary() {
        let mut boundary = SingleRenderBox::new(RepaintBoundaryData::new());

        boundary.set_is_repaint_boundary(false);
        assert!(!boundary.is_repaint_boundary());
        assert!(boundary.needs_paint());
    }

    #[test]
    fn test_render_repaint_boundary_layout() {
        let mut boundary = SingleRenderBox::new(RepaintBoundaryData::new());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = boundary.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
