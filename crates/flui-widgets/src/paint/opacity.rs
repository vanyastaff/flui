//! [`Opacity`] — makes its child partially transparent.

use flui_objects::RenderOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Makes its child partially transparent.
///
/// Flutter parity: `widgets/basic.dart` `Opacity` over `RenderOpacity`.
/// `opacity` is clamped to `0.0..=1.0`; `0.0` paints nothing (but the child is
/// still laid out and interactive unless wrapped in `IgnorePointer`).
#[derive(Clone, Debug)]
pub struct Opacity {
    opacity: f32,
    child: Child,
}

impl Opacity {
    /// Create an `Opacity` with the given opacity (clamped to `0.0..=1.0`).
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity,
            child: Child::empty(),
        }
    }

    /// Set the child to fade.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Opacity {
    type Protocol = BoxProtocol;
    type RenderObject = RenderOpacity;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderOpacity::new(self.opacity)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_opacity(self.opacity);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Opacity);
