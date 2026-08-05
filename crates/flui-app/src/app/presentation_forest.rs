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

impl PresentationForest {
    /// Construct a forest holding exactly one presentation — every realm's
    /// starting shape; [`Self::install`] grows it from there.
    pub(crate) fn single(presentation: PresentationState) -> Self {
        Self {
            presentations: vec![presentation],
        }
    }

    /// Install another presentation into this forest.
    ///
    /// Production entry point (this slice lifted the former `len()<=1`
    /// ratchet, registry-pinned as `multi-presentation-forest-gate` in
    /// `docs/runtime-contract.toml`): `UiRealm::install_presentation` is the
    /// realm-level caller, and `runner.rs::install_presentation_alongside`
    /// is the caller that also mints this presentation's real
    /// `WindowRegistry` mapping — the two steps a hosted presentation needs
    /// to be genuinely dispatchable, not just forest-resident.
    ///
    /// `runner.rs::install_presentation_alongside` (the real, non-test
    /// caller reachability threads through) is desktop-only — on a target
    /// where that function is itself dead code (android/wasm32), this
    /// method loses its only non-test caller too.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "reachable only through UiRealm::install_presentation, whose one \
                      production caller (runner.rs::install_presentation_alongside) is \
                      desktop-only"
        )
    )]
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

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use flui_platform::traits::PlatformTextInput;
    use flui_rendering::pipeline::{PipelineCell, PipelineOwner};

    use super::*;

    fn presentation(generation: u32) -> PresentationState {
        PresentationState::new_for_test(
            PresentationId::new_gen(0, NonZeroU32::new(generation).expect("nonzero")),
            PipelineCell::new(PipelineOwner::new()),
            None::<Arc<dyn PlatformTextInput>>,
        )
    }

    #[test]
    fn single_holds_exactly_the_constructed_presentation() {
        let p = presentation(1);
        let id = p.id();
        let forest = PresentationForest::single(p);

        assert_eq!(forest.len(), 1);
        assert_eq!(forest.primary().id(), id);
    }

    /// The former mechanical ratchet (`len() <= 1`) is lifted: `install`
    /// now accepts any number of presentations through the SAME production
    /// entry point every isolation test uses, not a `cfg(test)`-only
    /// bypass. If reverted (the old `assert!(self.presentations.is_empty())`
    /// restored), this fails on the second `install` call instead of
    /// observing `len() == 2`.
    #[test]
    fn install_accepts_any_number_of_presentations() {
        let mut forest = PresentationForest::single(presentation(1));
        forest.install(presentation(2));
        forest.install(presentation(3));

        assert_eq!(forest.len(), 3);
    }

    #[test]
    fn iter_yields_presentations_in_mount_order() {
        let mut forest = PresentationForest::single(presentation(1));
        forest.install(presentation(2));
        forest.install(presentation(3));

        let ids: Vec<_> = forest.iter().map(PresentationState::id).collect();
        assert_eq!(
            ids,
            vec![
                PresentationId::new_gen(0, NonZeroU32::new(1).expect("nonzero")),
                PresentationId::new_gen(0, NonZeroU32::new(2).expect("nonzero")),
                PresentationId::new_gen(0, NonZeroU32::new(3).expect("nonzero")),
            ],
            "mount order is insertion order, never re-sorted"
        );
    }

    #[test]
    fn get_and_remove_address_by_exact_presentation_id() {
        let mut forest = PresentationForest::single(presentation(1));
        let second = presentation(2);
        let second_id = second.id();
        forest.install(second);

        assert!(forest.get(second_id).is_some());
        let removed = forest.remove(second_id).expect("present");
        assert_eq!(removed.id(), second_id);
        assert!(forest.get(second_id).is_none());
        assert_eq!(forest.len(), 1);
    }
}
