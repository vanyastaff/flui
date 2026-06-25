//! [`ConstrainedBox`] — imposes additional constraints on its child.

use flui_objects::RenderConstrainedBox;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Imposes additional [`BoxConstraints`] on its child.
///
/// Flutter parity: `widgets/basic.dart` `ConstrainedBox` over
/// `RenderConstrainedBox`. The additional constraints are *enforced against*
/// (intersected with) the constraints this box receives from its parent.
#[derive(Clone, Debug)]
pub struct ConstrainedBox {
    constraints: BoxConstraints,
    child: Child,
}

impl ConstrainedBox {
    /// Create a `ConstrainedBox` with the given additional constraints.
    pub fn new(constraints: BoxConstraints) -> Self {
        Self {
            constraints,
            child: Child::empty(),
        }
    }

    /// Set the constrained child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ConstrainedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderConstrainedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderConstrainedBox::new(self.constraints)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_additional_constraints(self.constraints);
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(ConstrainedBox);
