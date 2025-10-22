//! RenderConstrainedBox - applies additional constraints to a child

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderConstrainedBox
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstrainedBoxData {
    /// Additional constraints to apply
    pub additional_constraints: BoxConstraints,
}

impl ConstrainedBoxData {
    /// Create new constrained box data
    pub fn new(additional_constraints: BoxConstraints) -> Self {
        Self { additional_constraints }
    }
}

/// RenderObject that applies additional constraints to its child
///
/// This allows you to enforce minimum or maximum sizes on a child widget.
/// The child is laid out with constraints that are the intersection of
/// the incoming constraints and the additional constraints.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::ConstrainedBoxData};
/// use flui_types::constraints::BoxConstraints;
///
/// let constraints = BoxConstraints::tight_for(100.0, 100.0);
/// let mut constrained = SingleRenderBox::new(ConstrainedBoxData::new(constraints));
/// ```
pub type RenderConstrainedBox = SingleRenderBox<ConstrainedBoxData>;

// ===== Public API =====

impl RenderConstrainedBox {
    /// Get the additional constraints
    pub fn additional_constraints(&self) -> BoxConstraints {
        self.data().additional_constraints
    }

    /// Set new additional constraints
    ///
    /// If constraints change, marks as needing layout.
    pub fn set_additional_constraints(&mut self, constraints: BoxConstraints) {
        if self.data().additional_constraints != constraints {
            self.data_mut().additional_constraints = constraints;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderConstrainedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let additional = self.data().additional_constraints;

        // Layout child with enforced constraints
        let size = if let Some(child) = self.child_mut() {
            // Enforce additional constraints
            let child_constraints = additional.enforce(constraints);

            // Layout child
            child.layout(child_constraints)
        } else {
            // No child - use smallest size that satisfies both constraints
            let enforced = additional.enforce(constraints);
            enforced.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Simply paint child at offset
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_constrained_box_new() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let constrained = SingleRenderBox::new(ConstrainedBoxData::new(constraints));
        assert_eq!(constrained.additional_constraints(), constraints);
    }

    #[test]
    fn test_render_constrained_box_set_constraints() {
        let constraints1 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let mut constrained = SingleRenderBox::new(ConstrainedBoxData::new(constraints1));

        let constraints2 = BoxConstraints::tight(Size::new(200.0, 200.0));
        constrained.set_additional_constraints(constraints2);
        assert_eq!(constrained.additional_constraints(), constraints2);
        assert!(RenderBoxMixin::needs_layout(&constrained));
    }

    #[test]
    fn test_render_constrained_box_layout_no_child() {
        let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let mut constrained = SingleRenderBox::new(ConstrainedBoxData::new(additional));

        let incoming = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = constrained.layout(incoming);

        // Should use smallest size that satisfies both constraints
        assert_eq!(size, Size::new(50.0, 50.0));
    }

    #[test]
    fn test_constrained_box_data_debug() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let data = ConstrainedBoxData::new(constraints);
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("ConstrainedBoxData"));
    }
}
