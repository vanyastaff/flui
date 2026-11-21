//! `ExcludeSemantics`: drops descendant semantics while keeping layout, paint and hit testing.

use flui_foundation::CreationSite;
use flui_objects::RenderExcludeSemantics;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Drops descendant semantics while keeping layout, paint, and hit testing.
#[derive(Clone, Debug)]
pub struct ExcludeSemantics {
    excluding: bool,
    child: Child,
    site: CreationSite,
}

impl Default for ExcludeSemantics {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(CreationSite::here())
    }
}

impl ExcludeSemantics {
    fn at(site: CreationSite) -> Self {
        Self {
            excluding: true,
            child: Child::empty(),
            site,
        }
    }

    /// Creates an exclude-semantics widget.
    #[track_caller]
    pub fn new() -> Self {
        Self::at(CreationSite::here())
    }

    /// Set whether descendants are excluded from semantics.
    #[must_use]
    pub fn excluding(mut self, excluding: bool) -> Self {
        self.excluding = excluding;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ExcludeSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderExcludeSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderExcludeSemantics::new(self.excluding)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_excluding(self.excluding);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ExcludeSemantics, site = site);
