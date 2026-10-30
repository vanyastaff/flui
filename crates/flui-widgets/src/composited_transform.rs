//! [`LinkLeader`] and [`LinkFollower`]: a subtree painted where another one is, across the tree.
//!
//! A follower mounted in an overlay stays on its leader through every scroll and transform
//! between them, resolved when the frame composites rather than one frame late, because the
//! offset is read from the layers (`RenderLeaderLayer`, `RenderFollowerLayer`), not from a
//! layout the follower waited for.

use flui_objects::{RenderFollowerLayer, RenderLeaderLayer};
use flui_painting::paint::Alignment;
use flui_rendering::layer::LayerLink;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{Child, RenderView, impl_render_view};

/// Publishes its child's layer under `link`, for a [`LinkFollower`] to follow.
#[derive(Debug, Clone)]
pub(crate) struct LinkLeader {
    link: LayerLink,
    child: Child,
}

impl LinkLeader {
    pub(crate) fn new(link: LayerLink, child: impl IntoView) -> Self {
        Self {
            link,
            child: Child::some(child.into_view()),
        }
    }
}

impl RenderView for LinkLeader {
    type Protocol = BoxProtocol;
    type RenderObject = RenderLeaderLayer;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderLeaderLayer::new(self.link)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_link(self.link)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(LinkLeader);

/// Paints its child with `follower_anchor` of its own box on `leader_anchor` of the leader's,
/// wherever the leader under `link` is; hidden while no leader publishes under it.
#[derive(Debug, Clone)]
pub(crate) struct LinkFollower {
    link: LayerLink,
    leader_anchor: Alignment,
    follower_anchor: Alignment,
    child: Child,
}

impl LinkFollower {
    pub(crate) fn new(
        link: LayerLink,
        leader_anchor: Alignment,
        follower_anchor: Alignment,
        child: impl IntoView,
    ) -> Self {
        Self {
            link,
            leader_anchor,
            follower_anchor,
            child: Child::some(child.into_view()),
        }
    }
}

impl RenderView for LinkFollower {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFollowerLayer;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderFollowerLayer::new(self.link)
            .with_show_when_unlinked(false)
            .with_leader_anchor(self.leader_anchor)
            .with_follower_anchor(self.follower_anchor)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_link(self.link)
            | render_object.set_leader_anchor(self.leader_anchor)
            | render_object.set_follower_anchor(self.follower_anchor)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(LinkFollower);
