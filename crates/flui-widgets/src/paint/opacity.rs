//! [`Opacity`] — makes its child partially transparent.

use flui_objects::RenderOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// Makes its child partially transparent.
///
/// `opacity` is clamped to `0.0..=1.0`; `0.0` paints nothing (but the child is
/// still laid out and interactive unless wrapped in `IgnorePointer`).
#[derive(Clone)]
pub struct Opacity {
    alpha: f64,
    child: Child,
    site: CreationSite,
}

impl std::fmt::Debug for Opacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opacity")
            .field("opacity", &self.alpha)
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

impl Opacity {
    /// Create an `Opacity` with the given opacity (clamped to `0.0..=1.0`).
    #[track_caller]
    pub fn new(opacity: f64) -> Self {
        Self {
            alpha: opacity,
            child: Child::empty(),
            site: CreationSite::here(),
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
        RenderOpacity::new(self.alpha)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_opacity(self.alpha);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Opacity, site = site);
