//! RenderConstrainedBox - applies additional constraints to a child

use flui_core::render::{BoxProtocol, LayoutContext, PaintContext};
use flui_core::render::{Optional, RenderBox};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that applies additional constraints to its child
///
/// This allows you to enforce minimum or maximum sizes on a child widget.
/// The child is laid out with constraints that are the intersection of
/// the incoming constraints and the additional constraints.
///
/// # Without Child
///
/// When no child is present, enforces the minimum size from additional constraints.
/// This can be used to reserve space.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderConstrainedBox;
/// use flui_types::constraints::BoxConstraints;
///
/// let constraints = BoxConstraints::tight_for(100.0, 100.0);
/// let constrained = RenderConstrainedBox::new(constraints);
/// ```
#[derive(Debug)]
pub struct RenderConstrainedBox {
    /// Additional constraints to apply
    pub additional_constraints: BoxConstraints,
}

impl RenderConstrainedBox {
    /// Create new RenderConstrainedBox with additional constraints
    pub fn new(additional_constraints: BoxConstraints) -> Self {
        Self {
            additional_constraints,
        }
    }

    /// Set new additional constraints
    pub fn set_additional_constraints(&mut self, constraints: BoxConstraints) {
        self.additional_constraints = constraints;
    }
}

impl Default for RenderConstrainedBox {
    fn default() -> Self {
        Self::new(BoxConstraints::UNCONSTRAINED)
    }
}

impl RenderBox<Optional> for RenderConstrainedBox {
    fn layout(&mut self, ctx: LayoutContext<'_, Optional, BoxProtocol>) -> Size {
        let constraints = ctx.constraints;

        // Enforce additional constraints by intersecting with incoming constraints
        let child_constraints = constraints.enforce(self.additional_constraints);

        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderConstrainedBox::layout: incoming={:?}, additional={:?}, child_constraints={:?}",
            constraints,
            self.additional_constraints,
            child_constraints
        );

        let size = if let Some(child_id) = ctx.children.get() {
            // Layout child with combined constraints
            ctx.layout_child(child_id, child_constraints)
        } else {
            // No child - return minimum size from additional constraints
            Size::new(child_constraints.min_width, child_constraints.min_height)
        };

        #[cfg(debug_assertions)]
        tracing::debug!("RenderConstrainedBox::layout: result size={:?}", size);

        size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Optional>) {
        // If we have a child, paint it at our offset
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(child_id, ctx.offset);
        }
        // If no child, nothing to paint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_constrained_box_new() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let constrained = RenderConstrainedBox::new(constraints);
        assert_eq!(constrained.additional_constraints, constraints);
    }

    #[test]
    fn test_render_constrained_box_default() {
        let constrained = RenderConstrainedBox::default();
        assert_eq!(
            constrained.additional_constraints,
            BoxConstraints::UNCONSTRAINED
        );
    }

    #[test]
    fn test_render_constrained_box_set_constraints() {
        let constraints1 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let mut constrained = RenderConstrainedBox::new(constraints1);

        let constraints2 = BoxConstraints::tight(Size::new(200.0, 200.0));
        constrained.set_additional_constraints(constraints2);
        assert_eq!(constrained.additional_constraints, constraints2);
    }
}
