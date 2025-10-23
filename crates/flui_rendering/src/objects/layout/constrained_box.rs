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
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderConstrainedBox {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        let additional = self.data().additional_constraints;

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with enforced constraints
        let size = if let Some(&child_id) = children_ids.first() {
            // Enforce additional constraints
            let child_constraints = constraints.enforce(additional);

            // Layout child via RenderContext
            let child_size = ctx.layout_child_cached(child_id, child_constraints, None);
            child_size
        } else {
            // No child - use smallest size that satisfies both constraints
            let enforced = additional.enforce(constraints);
            enforced.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Simply paint child at offset
        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
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
        assert!(constrained.needs_layout());
    }

    #[test]
    fn test_render_constrained_box_layout_no_child() {
        use flui_core::testing::mock_render_context;

        let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let constrained = SingleRenderBox::new(ConstrainedBoxData::new(additional));

        let incoming = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let (_tree, ctx) = mock_render_context();
        let size = constrained.layout(incoming, &ctx);

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
