//! [`SliverOpacity`] — fades a sliver child within a scroll viewport.

use flui_objects::RenderSliverOpacity;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Makes a **sliver** child partially transparent inside a
/// [`Viewport`](crate::Viewport).
///
/// Flutter parity: `widgets/sliver.dart` `SliverOpacity` over
/// `RenderSliverOpacity`. `opacity` is clamped to `0.0..=1.0`.
#[derive(Clone, Debug)]
pub struct SliverOpacity {
    opacity: f32,
    child: Child,
}

impl SliverOpacity {
    /// Create a `SliverOpacity` with the given opacity (clamped to `0.0..=1.0`).
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity,
            child: Child::empty(),
        }
    }

    /// Set the faded sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverOpacity {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverOpacity;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverOpacity::new(self.opacity)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_opacity(self.opacity);
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

impl_render_view!(SliverOpacity);
