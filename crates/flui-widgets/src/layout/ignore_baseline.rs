//! [`IgnoreBaseline`] — hides its child's baseline from a baseline-aligning
//! parent.

use flui_objects::RenderIgnoreBaseline;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// Hides `child`'s baseline from the parent, so a baseline-aligning parent
/// treats this subtree as having no baseline at all.
///
/// A [`Row`](crate::Row) with `CrossAxisAlignment::Baseline` shifts its
/// children down until their baselines meet, and grows to the tallest ascent
/// plus the deepest descent. Wrapping a child here takes it out of that
/// computation entirely: it is neither shifted nor allowed to stretch the row,
/// and sits flush at the cross start. Everything else — size, paint,
/// hit-testing, intrinsics — passes straight through.
///
/// A single-child render widget over `RenderIgnoreBaseline` with no
/// configuration of its own.
#[derive(Clone, Debug)]
pub struct IgnoreBaseline {
    child: Child,
    site: CreationSite,
}

impl IgnoreBaseline {
    /// Creates a childless baseline-hiding wrapper.
    #[must_use]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Sets the child whose baseline is hidden.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl Default for IgnoreBaseline {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::new()
    }
}

impl RenderView for IgnoreBaseline {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIgnoreBaseline;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIgnoreBaseline::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // The render object carries no configuration, so there is nothing
        // to push.
        flui_rendering::RenderUpdateImpact::NONE
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(IgnoreBaseline, site = site);
