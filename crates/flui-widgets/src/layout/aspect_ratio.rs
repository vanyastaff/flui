//! [`AspectRatio`] — sizes its child to a given width:height ratio.

use flui_objects::{AspectRatioFactor, RenderAspectRatio};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// Sizes its child to a given `width / height` aspect ratio, fitting within the
/// incoming constraints.
///
/// Backed by `RenderAspectRatio`. The ratio must be finite and `> 0` (debug-asserted in the render object).
#[derive(Clone, Debug)]
pub struct AspectRatio {
    ratio: f64,
    child: Child,
    site: CreationSite,
}

impl AspectRatio {
    /// Create an `AspectRatio` enforcing `aspect_ratio` (= width / height).
    #[track_caller]
    pub fn new(aspect_ratio: f64) -> Self {
        Self {
            ratio: aspect_ratio,
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Set the child to size by the ratio.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for AspectRatio {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAspectRatio;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderAspectRatio::new(AspectRatioFactor::new_unchecked(self.ratio))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_aspect_ratio(AspectRatioFactor::new_unchecked(self.ratio));
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(AspectRatio, site = site);
