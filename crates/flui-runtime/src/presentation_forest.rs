//! `PresentationForest` — the insertion-ordered collection of
//! [`PresentationState`]s one `UiRealm` owns (ADR-0043 §1).
//!
//! Production topology now allows any number of presentations per realm
//! (issue #555's addressed-routing slice lifts the mechanical 1×N ratchet this type used to
//! enforce): `UiRealm::install_presentation` is the production entry point
//! that assembles and installs a second (third, ...) presentation sharing
//! this realm's `GlobalKeyScope` and dispatch handles, with its own real
//! `WindowRegistry` mapping (`runner.rs::install_presentation_alongside`).
//! Opening a genuinely independent, unrelated window still installs a second
//! REALM (`AppRuntime`'s `RealmId`-keyed map) — this forest is for windows
//! that share one realm's GlobalKey scope, focus arbitration, and scheduler.
//!
//! Iteration order is insertion (mount) order: a plain `Vec`, never
//! reordered. Pump, reassemble fan-out, and the composite registry all rely
//! on this — see their own docs for why mount order is the observable
//! contract, not an implementation accident.

use flui_foundation::PresentationId;

use super::presentation::PresentationState;

/// The insertion-ordered set of presentations one `UiRealm` owns.
pub(crate) struct PresentationForest {
    presentations: Vec<PresentationState>,
}

impl std::fmt::Debug for PresentationForest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.presentations.iter().map(PresentationState::id))
            .finish()
    }
}

impl PresentationForest {
    pub(crate) fn take_all(&mut self) -> Vec<PresentationState> {
        std::mem::take(&mut self.presentations)
    }
    /// Construct a forest holding exactly one presentation — every realm's
    /// starting shape; [`Self::install`] grows it from there.
    pub(crate) fn single(presentation: PresentationState) -> Self {
        Self {
            presentations: vec![presentation],
        }
    }

    /// Install another presentation into this forest.
    ///
    /// Production entry point (issue #555 lifted the former `len()<=1`
    /// ratchet): `UiRealm::install_presentation` is the
    /// realm-level caller, and `runner.rs::install_presentation_alongside`
    /// is the caller that also mints this presentation's real
    /// `WindowRegistry` mapping — the two steps a hosted presentation needs
    /// to be genuinely dispatchable, not just forest-resident.
    pub(crate) fn install(&mut self, presentation: PresentationState) {
        self.presentations.push(presentation);
    }

    /// The number of presentations currently installed.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.presentations.len()
    }

    /// This realm's primary (and, until the ratchet lifts, only) production
    /// presentation.
    ///
    /// # Panics
    ///
    /// Panics if the forest is empty — a `UiRealm` never exists without at
    /// least one presentation; an empty forest is a construction bug, not a
    /// reachable runtime state.
    #[must_use]
    pub(crate) fn primary(&self) -> &PresentationState {
        self.presentations
            .first()
            .expect("BUG: PresentationForest is never empty for a live UiRealm")
    }

    /// Look up a presentation by its exact generational id.
    ///
    /// Production caller: [`super::ui_realm::UiRealm::close_presentation_entered`]'s
    /// step 2–3 phase resolves the presentation to close through this
    /// before removing it via [`Self::remove`] below.
    pub(crate) fn get(&self, id: PresentationId) -> Option<&PresentationState> {
        self.presentations.iter().find(|p| p.id() == id)
    }

    /// Remove and return the presentation with the given id, if present.
    ///
    /// Production caller: [`super::ui_realm::UiRealm::close_presentation_entered`]'s
    /// steps 4–6 — the only path today that removes a member from a genuine
    /// `N>1` forest rather than dropping the whole forest at once (the
    /// realm's own `Drop` still does the latter).
    pub(crate) fn remove(&mut self, id: PresentationId) -> Option<PresentationState> {
        let index = self.presentations.iter().position(|p| p.id() == id)?;
        Some(self.presentations.remove(index))
    }

    /// Iterate every presentation in mount (insertion) order.
    ///
    /// Mount order is the observable contract for pump processing, hot-reload
    /// reassemble fan-out, and the realm composite registry's try-in-order
    /// resolution — never re-sorted or reversed.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &PresentationState> {
        self.presentations.iter()
    }
}
