//! [`IntrinsicHeight`] — sizes child to its maximum intrinsic height.

use flui_objects::RenderIntrinsicHeight;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Sizes its child to the child's maximum intrinsic height.
///
/// Useful when a widget should be exactly as tall as its natural content.
/// When the parent's height is already tight, that tight value propagates
/// directly without querying the child.
///
/// Flutter parity: `widgets/basic.dart` `IntrinsicHeight` over
/// [`RenderIntrinsicHeight`].
#[derive(Clone, Debug)]
pub struct IntrinsicHeight {
    child: Child,
}

impl IntrinsicHeight {
    /// Creates the widget.
    pub fn new() -> Self {
        Self {
            child: Child::empty(),
        }
    }

    /// Sets the child view.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl Default for IntrinsicHeight {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderView for IntrinsicHeight {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIntrinsicHeight;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderIntrinsicHeight::new()
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {
        // No configuration fields to synchronize.
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

impl_render_view!(IntrinsicHeight);
