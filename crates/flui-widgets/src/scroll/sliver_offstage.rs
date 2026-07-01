//! [`SliverOffstage`] — a sliver that hides its subtree without removing it
//! from layout.

use flui_objects::RenderSliverOffstage;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// A sliver that, when `offstage` is `true`, collapses its reported geometry to
/// zero, skips painting, and is unreachable by hit-testing — while still laying
/// out its child (Flutter parity).
///
/// When `offstage` is `false` it behaves as a transparent single-child proxy.
///
/// Flutter parity: `widgets/sliver.dart` `SliverOffstage` over
/// `RenderSliverOffstage` (`rendering/proxy_sliver.dart`). Lives inside a
/// [`Viewport`](crate::Viewport).
///
/// **Note:** the box-protocol equivalent is [`Offstage`](crate::Offstage) from
/// the interaction module.
#[derive(Clone, Debug)]
pub struct SliverOffstage {
    offstage: bool,
    child: Child,
}

impl SliverOffstage {
    /// Create a sliver offstage widget with the given `offstage` flag.
    ///
    /// When `offstage = true` the child is hidden (zero geometry, no paint, no
    /// hit-testing). When `offstage = false` the child is fully visible.
    pub fn new(offstage: bool) -> Self {
        Self {
            offstage,
            child: Child::empty(),
        }
    }

    /// Convenience: create a hidden sliver (`offstage = true`).
    pub fn hidden() -> Self {
        Self::new(true)
    }

    /// Convenience: create a visible sliver (`offstage = false`).
    pub fn visible() -> Self {
        Self::new(false)
    }

    /// Set the sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverOffstage {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverOffstage;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverOffstage::new(self.offstage)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_offstage(self.offstage);
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

impl_render_view!(SliverOffstage);
