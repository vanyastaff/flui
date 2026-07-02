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

#[cfg(test)]
mod tests {
    use flui_rendering::delegates::{AspectRatioDelegate, CenterLayoutDelegate};
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn create_render_object_installs_the_given_delegate() {
        let render_object =
            CustomSingleChildLayout::new(Arc::new(CenterLayoutDelegate)).create_render_object();
        assert!(
            render_object
                .delegate()
                .as_any()
                .is::<CenterLayoutDelegate>(),
            "create_render_object must install the exact delegate passed to new()",
        );
    }

    #[test]
    fn update_render_object_replaces_the_delegate() {
        let mut render_object =
            CustomSingleChildLayout::new(Arc::new(CenterLayoutDelegate)).create_render_object();
        assert!(
            render_object
                .delegate()
                .as_any()
                .is::<CenterLayoutDelegate>()
        );

        CustomSingleChildLayout::new(Arc::new(AspectRatioDelegate::new(2.0)))
            .update_render_object(&mut render_object);

        assert!(
            render_object
                .delegate()
                .as_any()
                .is::<AspectRatioDelegate>(),
            "update_render_object must replace the delegate with the new instance",
        );
    }

    #[test]
    fn debug_reports_the_delegate_and_child_presence() {
        let widget = CustomSingleChildLayout::new(Arc::new(CenterLayoutDelegate));
        let debug = format!("{widget:?}");
        assert!(
            debug.contains("has_child: false"),
            "Debug output must report has_child, got: {debug}",
        );
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        let empty = CustomSingleChildLayout::new(Arc::new(CenterLayoutDelegate));
        assert!(!empty.has_children());

        let with_child =
            CustomSingleChildLayout::new(Arc::new(CenterLayoutDelegate)).child(SizedBox::shrink());
        assert!(with_child.has_children());
    }
}
