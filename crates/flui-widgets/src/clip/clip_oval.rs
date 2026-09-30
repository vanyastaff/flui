//! [`ClipOval`] — clips its child to the oval inscribed in its bounds.

use flui_objects::{Oval, RenderClipOval};
use flui_painting::paint::Clip;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Clips its child to the axis-aligned oval inscribed in this widget's bounds
/// (a circle when the bounds are square — the common avatar case).
///
/// Layout is a pass-through; only painting is clipped. `clip_behavior` defaults
/// to [`Clip::AntiAlias`] (smooth edges).
#[derive(Clone, Debug)]
pub struct ClipOval {
    clip_behavior: Clip,
    clip_shape: Option<Oval>,
    child: Child,
}

impl Default for ClipOval {
    fn default() -> Self {
        Self {
            clip_behavior: Clip::AntiAlias,
            clip_shape: None,
            child: Child::empty(),
        }
    }
}

impl ClipOval {
    /// Create an oval clip with the default `AntiAlias` behavior.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the clip behavior (anti-aliasing / save-layer policy).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// The fixed ellipse to clip to, inscribed in the given rectangle.
    ///
    /// Without it the clip is the ellipse inscribed in the widget's whole box.
    /// See [`ClipRect::clipper`](crate::ClipRect::clipper) for why this is a
    /// value rather than a callback.
    #[must_use]
    pub fn clipper(mut self, shape: Oval) -> Self {
        self.clip_shape = Some(shape);
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

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut render_object = RenderClipOval::new(self.clip_behavior);
        let _ = render_object.set_clip_shape(self.clip_shape);
        render_object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        impact |= render_object.set_clip_shape(self.clip_shape);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ClipOval);
