//! [`SliverToBoxAdapter`] — adapts a single box-protocol child into a sliver so
//! it can sit inside a [`Viewport`](crate::Viewport).

use flui_objects::RenderSliverToBoxAdapter;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Wraps a single box (non-sliver) child as a sliver, so an ordinary widget can
/// be placed inside a scrolling [`Viewport`](crate::Viewport).
///
/// Flutter parity: `widgets/sliver.dart` `SliverToBoxAdapter` over
/// `RenderSliverToBoxAdapter`. The box child is laid out with an unbounded main
/// axis and its main-axis size becomes the sliver's scroll extent.
#[derive(Clone, Debug, Default)]
pub struct SliverToBoxAdapter {
    child: Child,
}

impl SliverToBoxAdapter {
    /// Create an adapter with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the box child to adapt into a sliver.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverToBoxAdapter {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverToBoxAdapter;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverToBoxAdapter::new()
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {
        // The adapter carries no configuration — nothing to update.
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

impl_render_view!(SliverToBoxAdapter);
