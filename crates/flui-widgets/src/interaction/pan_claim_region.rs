//! A subtree's declared pan claim (ADR-0208 rule 1, CSS `touch-action`).

use flui_interaction::gesture::PanClaim;
use flui_view::BoxedView;
use flui_view::prelude::*;

use super::GestureArenaScope;

/// Declares which axes axial gestures outside this subtree may claim from the
/// free gestures (pans) inside it.
///
/// `PanClaim::None` keeps a map's or a signature pad's pan when it sits in a
/// pager or a list; `PanClaim::PanY` lets only an enclosing vertical scroll
/// take it. Nested regions intersect per axis, up to the nearest axial
/// gesture on that axis. Gestures inside the region join the enclosing
/// [`GestureArenaScope`]'s arena with the claim recorded.
#[derive(Clone, StatelessView)]
pub struct PanClaimRegion {
    claim: PanClaim,
    child: BoxedView,
}

impl PanClaimRegion {
    /// Declare `claim` for `child`'s gestures.
    #[must_use]
    pub fn new(claim: PanClaim, child: impl IntoView) -> Self {
        Self {
            claim,
            child: BoxedView(Box::new(child.into_view())),
        }
    }
}

impl std::fmt::Debug for PanClaimRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanClaimRegion")
            .field("claim", &self.claim)
            .finish_non_exhaustive()
    }
}

impl StatelessView for PanClaimRegion {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        GestureArenaScope::new(
            GestureArenaScope::of(ctx).with_pan_claim(self.claim),
            self.child.clone(),
        )
    }
}
