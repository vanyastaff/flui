//! [`CustomSingleChildLayout`] — delegates single-child layout to a
//! [`SingleChildLayoutDelegate`].

use std::fmt;
use std::sync::Arc;

use flui_objects::RenderCustomSingleChildLayoutBox;
use flui_rendering::delegates::SingleChildLayoutDelegate;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// A widget that sizes and positions one child using a layout delegate.
///
/// Flutter parity: `widgets/basic.dart` `CustomSingleChildLayout` over
/// `RenderCustomSingleChildLayoutBox`.
#[derive(Clone)]
pub struct CustomSingleChildLayout {
    delegate: Arc<dyn SingleChildLayoutDelegate>,
    child: Child,
}

impl CustomSingleChildLayout {
    /// Creates a custom single-child layout with no child.
    pub fn new(delegate: Arc<dyn SingleChildLayoutDelegate>) -> Self {
        Self {
            delegate,
            child: Child::empty(),
        }
    }

    /// Sets the child laid out by the delegate.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl fmt::Debug for CustomSingleChildLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomSingleChildLayout")
            .field("delegate", &self.delegate)
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl RenderView for CustomSingleChildLayout {
    type Protocol = BoxProtocol;
    type RenderObject = RenderCustomSingleChildLayoutBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderCustomSingleChildLayoutBox::new(self.delegate.clone())
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_delegate(self.delegate.clone());
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

impl_render_view!(CustomSingleChildLayout);
