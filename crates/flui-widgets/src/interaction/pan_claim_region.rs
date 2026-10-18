//! A subtree's declared pan claim (ADR-0208 rule 1, CSS `touch-action`).

use flui_interaction::GestureArena;
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
///
/// The region is one identity while its claim and the enclosing arena stay:
/// a rebuild keeps the gestures inside it, and their contacts, running.
#[derive(Clone, StatefulView)]
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

impl StatefulView for PanClaimRegion {
    type State = PanClaimRegionState;

    fn create_state(&self) -> Self::State {
        PanClaimRegionState {
            claim: self.claim,
            region: None,
        }
    }
}

/// A mounted [`PanClaimRegion`]'s arena handle: the enclosing handle it was
/// derived from, and the region's own.
pub struct PanClaimRegionState {
    claim: PanClaim,
    region: Option<(GestureArena, GestureArena)>,
}

impl std::fmt::Debug for PanClaimRegionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanClaimRegionState")
            .field("claim", &self.claim)
            .finish_non_exhaustive()
    }
}

impl PanClaimRegionState {
    /// Derive the region's handle from `enclosing` unless the current one
    /// already is.
    fn derive(&mut self, enclosing: GestureArena) {
        let current = self
            .region
            .as_ref()
            .is_some_and(|(derived_from, _)| derived_from.admits_like(&enclosing));
        if !current {
            let region = enclosing.with_pan_claim(self.claim);
            self.region = Some((enclosing, region));
        }
    }
}

impl ViewState<PanClaimRegion> for PanClaimRegionState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.derive(GestureArenaScope::dependent_of(ctx));
    }

    /// An enclosing scope that hands out another arena handle gives the
    /// region a new one.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.derive(GestureArenaScope::dependent_of(ctx));
    }

    /// Another claim is another region.
    fn did_update_view(&mut self, _old_view: &PanClaimRegion, new_view: &PanClaimRegion) {
        if new_view.claim != self.claim {
            self.claim = new_view.claim;
            if let Some((enclosing, _)) = self.region.take() {
                self.derive(enclosing);
            }
        }
    }

    fn build(&self, view: &PanClaimRegion, _ctx: &dyn BuildContext) -> impl IntoView {
        let (_, region) = self
            .region
            .as_ref()
            .expect("BUG: a mounted PanClaimRegion derived its handle in init_state");
        GestureArenaScope::new(region.clone(), view.child.clone())
    }
}
