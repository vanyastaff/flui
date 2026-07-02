//! [`SizedOverflowBox`] — claims a fixed size while letting the child overflow.

use flui_objects::RenderSizedOverflowBox;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Alignment, Size};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Claims a specific size for itself while laying its child out under the
/// incoming (parent) constraints, which may allow the child to be a different
/// size and overflow.
///
/// The box's own size is always `constraints.constrain(requested_size)`.  The
/// child is aligned within that slot using `alignment`.
///
/// Flutter parity: `widgets/basic.dart` `SizedOverflowBox` over
/// [`RenderSizedOverflowBox`].
#[derive(Clone, Debug)]
pub struct SizedOverflowBox {
    alignment: Alignment,
    requested_size: Size,
    child: Child,
}

impl SizedOverflowBox {
    /// Creates the widget with center alignment.
    pub fn new(requested_size: Size) -> Self {
        Self {
            alignment: Alignment::CENTER,
            requested_size,
            child: Child::empty(),
        }
    }

    /// Sets the alignment of the child within the claimed slot.
    #[must_use]
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets the child view.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SizedOverflowBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedOverflowBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSizedOverflowBox::new(self.alignment, self.requested_size)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_alignment(self.alignment);
        render_object.set_requested_size(self.requested_size);
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

impl_render_view!(SizedOverflowBox);
