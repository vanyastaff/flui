//! [`ClipOval`] — clips its child to the oval inscribed in its bounds.

use flui_objects::RenderClipOval;
use flui_rendering::protocol::BoxProtocol;
use flui_types::painting::Clip;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Clips its child to the axis-aligned oval inscribed in this widget's bounds
/// (a circle when the bounds are square — the common avatar case).
///
/// Flutter parity: `widgets/basic.dart` `ClipOval` over `RenderClipOval`.
/// Layout is a pass-through; only painting is clipped. `clip_behavior` defaults
/// to [`Clip::AntiAlias`] (Flutter's `ClipOval` default — smooth edges).
#[derive(Clone, Debug)]
pub struct ClipOval {
    clip_behavior: Clip,
    child: Child,
}

impl Default for ClipOval {
    fn default() -> Self {
        Self {
            clip_behavior: Clip::AntiAlias,
            child: Child::empty(),
        }
    }
}

impl ClipOval {
    /// Create an oval clip with Flutter's default `AntiAlias` behavior.
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

impl RenderView for ClipOval {
    type Protocol = BoxProtocol;
    type RenderObject = RenderClipOval;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderClipOval::new(self.clip_behavior)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
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

impl_render_view!(ClipOval);
