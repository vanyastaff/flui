//! RenderConstrainedBox - applies additional constraints to a child

use flui_core::render::{
    LayoutCx, PaintCx, RenderObject, SingleArity, SingleChild, SingleChildPaint,
};
use flui_engine::BoxedLayer;
use flui_types::{Size, constraints::BoxConstraints};

/// RenderObject that applies additional constraints to its child
///
/// This allows you to enforce minimum or maximum sizes on a child widget.
/// The child is laid out with constraints that are the intersection of
/// the incoming constraints and the additional constraints.
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

impl RenderObject for RenderConstrainedBox {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child();
        let constraints = cx.constraints();

        // SingleArity always has exactly one child
        // Enforce additional constraints
        let child_constraints = constraints.enforce(self.additional_constraints);
        cx.layout_child(child, child_constraints)
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Simply pass through child layer
        let child = cx.child();
        cx.capture_child_layer(child)
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
