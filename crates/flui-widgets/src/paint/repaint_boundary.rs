//! [`RepaintBoundary`] — isolates its child's painting into its own layer.

use flui_objects::RenderRepaintBoundary;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Isolates its child into a separate compositing layer so repaints of the
/// child (or its siblings) don't force each other to re-paint.
///
/// Flutter parity: `widgets/basic.dart` `RepaintBoundary` over
/// `RenderRepaintBoundary`. Layout is a pass-through (the child's size); the
/// boundary only affects paint/compositing.
#[derive(Clone, Debug, Default)]
pub struct RepaintBoundary {
    child: Child,
}

impl RepaintBoundary {
    /// Create a repaint boundary with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the isolated child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for RepaintBoundary {
    type Protocol = BoxProtocol;
    type RenderObject = RenderRepaintBoundary;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderRepaintBoundary::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        // A repaint boundary carries no configuration — nothing to update.
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

impl_render_view!(RepaintBoundary);
