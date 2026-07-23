//! [`ClipRect`] — clips its child to its own rectangular bounds.

use flui_objects::RenderClipRect;
use flui_rendering::protocol::BoxProtocol;
use flui_types::painting::Clip;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Clips its child to this widget's rectangular bounds.
///
/// Flutter parity: `widgets/basic.dart` `ClipRect` over `RenderClipRect`.
/// Layout is a pass-through; only painting is clipped. `clip_behavior` defaults
/// to [`Clip::HardEdge`] (Flutter's `ClipRect` default).
#[derive(Clone, Debug)]
pub struct ClipRect {
    clip_behavior: Clip,
    child: Child,
}

impl Default for ClipRect {
    fn default() -> Self {
        Self {
            clip_behavior: Clip::HardEdge,
            child: Child::empty(),
        }
    }
}

impl ClipRect {
    /// Create a rectangular clip with Flutter's default `HardEdge` behavior.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the clip behavior (anti-aliasing / save-layer policy).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Set the clipped child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ClipRect {
    type Protocol = BoxProtocol;
    type RenderObject = RenderClipRect;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderClipRect::new(self.clip_behavior)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_clip_behavior(self.clip_behavior);
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

impl_render_view!(ClipRect);
