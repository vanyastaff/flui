//! [`ClipRect`] — clips its child to its own rectangular bounds.

use flui_objects::RenderClipRect;
use flui_rendering::protocol::BoxProtocol;
use flui_types::geometry::{Pixels, Rect};
use flui_types::painting::Clip;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Clips its child to this widget's rectangular bounds.
///
/// Flutter parity: `widgets/basic.dart` `ClipRect` over `RenderClipRect`.
/// Layout is a pass-through; only painting is clipped. `clip_behavior` defaults
/// to [`Clip::HardEdge`] (Flutter's `ClipRect` default).
#[derive(Clone, Debug)]
pub struct ClipRect {
    clip_behavior: Clip,
    clip_shape: Option<Rect<Pixels>>,
    child: Child,
}

impl Default for ClipRect {
    fn default() -> Self {
        Self {
            clip_behavior: Clip::HardEdge,
            clip_shape: None,
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

    /// The fixed rectangle to clip to, in the widget's own coordinates.
    ///
    /// Without it the clip is the widget's whole box. Flutter's equivalent is
    /// `ClipRect(clipper: CustomClipper<Rect>)`, a callback plus a
    /// hand-written `shouldReclip`; this is a value compared with `==`, which
    /// is what its own test clipper amounts to. See
    /// [`RenderClip::set_clip_shape`](flui_objects::RenderClipRect::set_clip_shape)
    /// for the full reasoning and for the size-dependent case it does not
    /// cover.
    #[must_use]
    pub fn clipper(mut self, shape: Rect<Pixels>) -> Self {
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

impl RenderView for ClipRect {
    type Protocol = BoxProtocol;
    type RenderObject = RenderClipRect;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut render_object = RenderClipRect::new(self.clip_behavior);
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

impl_render_view!(ClipRect);
