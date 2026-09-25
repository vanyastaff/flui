//! [`AnchoredBox`]: the render node a view publishes so others can name it.

use flui_objects::{RenderSubtreeAnchor, SubtreeAnchor};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{Child, RenderView, impl_render_view};

/// A transparent single-child render proxy that publishes its render node's
/// id into a [`SubtreeAnchor`] while mounted.
///
/// Its only job is to have a `RenderId` someone else can name: a route's page
/// subtree, a `Hero`'s flight target, a focus node's rect provider, an
/// editable's IME cursor area. `BuildContext` walks ancestors only, so the
/// render node *below* a view has to be published from the render side
/// ([`RenderSubtreeAnchor`]'s `attach`/`detach`), which is what this view builds.
///
/// Framework seam, exported through [`crate::__private`] for the `flui-*`
/// widget crates; no semver guarantee.
#[derive(Debug, Clone)]
pub struct AnchoredBox {
    anchor: SubtreeAnchor,
    child: Child,
}

impl AnchoredBox {
    /// Anchor `child` into `anchor`, publishing the render node's id while mounted.
    pub fn new(anchor: SubtreeAnchor, child: impl IntoView) -> Self {
        Self::from_child(anchor, Child::some(child.into_view()))
    }

    /// [`Self::new`] for a child already held as a [`Child`].
    #[must_use]
    pub fn from_child(anchor: SubtreeAnchor, child: Child) -> Self {
        Self { anchor, child }
    }
}

impl RenderView for AnchoredBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSubtreeAnchor;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSubtreeAnchor::new(self.anchor.clone())
    }

    /// Rebinds the publication slot without changing geometry or child identity.
    /// Child reconciliation schedules any layout its own changes require.
    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_anchor(self.anchor.clone());
        flui_rendering::RenderUpdateImpact::NONE
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(AnchoredBox);
