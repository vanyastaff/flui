//! [`SliverIgnorePointer`] — a sliver that makes its subtree invisible to
//! pointer events.

use flui_objects::RenderSliverIgnorePointer;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// A sliver that, when `ignoring` is `true`, causes pointer events to pass
/// straight through to whatever is painted beneath it in the viewport.
///
/// Layout and paint are unconditional passthroughs; only hit-testing is
/// affected. When `ignoring` is `false` the child receives events normally.
///
/// Flutter parity: `widgets/sliver.dart` `SliverIgnorePointer` over
/// `RenderSliverIgnorePointer`. Lives inside a
/// [`Viewport`](crate::Viewport).
///
/// **Note:** the box-protocol equivalent is
/// [`IgnorePointer`](crate::IgnorePointer) from the interaction module.
#[derive(Clone, Debug)]
pub struct SliverIgnorePointer {
    ignoring: bool,
    child: Child,
}

impl SliverIgnorePointer {
    /// Create a sliver that conditionally ignores pointer events.
    ///
    /// When `ignoring = true` all pointer events pass through this sliver.
    /// When `ignoring = false` the child receives events normally.
    pub fn new(ignoring: bool) -> Self {
        Self {
            ignoring,
            child: Child::empty(),
        }
    }

    /// Set the sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverIgnorePointer {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverIgnorePointer;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverIgnorePointer::new(self.ignoring)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_ignoring(self.ignoring);
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

impl_render_view!(SliverIgnorePointer);
