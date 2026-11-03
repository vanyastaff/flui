//! [`IgnorePointer`] — makes its subtree invisible to hit-testing.

use flui_objects::RenderIgnorePointer;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// During hit-testing, makes its child (and subtree) invisible to pointer
/// events while still laying it out and painting it.
///
/// `ignoring` defaults to `true`.
#[derive(Clone, Debug)]
pub struct IgnorePointer {
    ignoring: bool,
    child: Child,
    site: CreationSite,
}

impl Default for IgnorePointer {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(CreationSite::here())
    }
}

impl IgnorePointer {
    fn at(site: CreationSite) -> Self {
        Self {
            ignoring: true,
            child: Child::empty(),
            site,
        }
    }

    /// Create an `IgnorePointer` that ignores pointer events (`ignoring = true`).
    #[track_caller]
    pub fn new() -> Self {
        Self::at(CreationSite::here())
    }

    /// Set whether pointer events are ignored.
    #[must_use]
    pub fn ignoring(mut self, ignoring: bool) -> Self {
        self.ignoring = ignoring;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for IgnorePointer {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIgnorePointer;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIgnorePointer::new(self.ignoring)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_ignoring(self.ignoring);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(IgnorePointer, site = site);
