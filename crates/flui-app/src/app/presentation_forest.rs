//! `PresentationForest` — the insertion-ordered collection of
//! [`PresentationState`]s one `UiRealm` owns (ADR-0043 §1).
//!
//! Production topology is exactly one presentation per realm today: opening
//! a second independent window installs a second REALM (`AppRuntime`'s
//! `RealmId`-keyed map, #555 PR-1), not a second presentation inside an
//! existing one. This type exists so that shape — the realm-level composite
//! `GlobalKey` registry, the per-presentation pump/teardown/reassemble
//! machinery — is written once, generally, over `Vec<PresentationState>`
//! rather than hand-specialized to "exactly one", so a later slice (window
//! grouping / `WindowPolicy`, #555 PR-4/5) can lift the ratchet below
//! without re-deriving this plumbing.
//!
//! Iteration order is insertion (mount) order: a plain `Vec`, never
//! reordered. Pump, reassemble fan-out, and the composite registry all rely
//! on this — see their own docs for why mount order is the observable
//! contract, not an implementation accident.

use flui_foundation::PresentationId;

use super::presentation::PresentationState;

/// The insertion-ordered set of presentations one `UiRealm` owns.
///
/// # Ratchet
///
/// [`Self::install`] is the production entry point and panics if the forest
/// already holds a presentation — registry-pinned
/// (`multi-presentation-forest-gate`, `docs/runtime-contract.toml`) as the
/// mechanical 1×N ratchet: production topology stays exactly one
/// presentation per realm until a later slice's addressed routing lifts it.
/// [`Self::push_for_test`] is the only bypass, and only compiles under
/// `cfg(test)`.
pub(crate) struct PresentationForest {
    presentations: Vec<PresentationState>,
}

impl PresentationForest {
    /// Construct a forest holding exactly one presentation — the only
    /// production shape until the ratchet in [`Self::install`] lifts.
    pub(crate) fn single(presentation: PresentationState) -> Self {
        Self {
            presentations: vec![presentation],
        }
    }

    /// Install another presentation into this forest.
    ///
    /// **Ratchet:** panics with `BUG:` if the forest is not currently empty
    /// — there is no production call site for this today (opening a second
    /// window installs a second realm, not a second presentation), so this
    /// exists to fail loudly the moment one is added ahead of the addressed
    /// routing (#555 PR-4/5) that makes a true forest safe to drive.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller until the len()<=1 ratchet lifts \
                      (#555 PR-4/5's addressed routing); push_for_test is the \
                      isolation-suite bypass"
        )
    )]
    pub(crate) fn install(&mut self, presentation: PresentationState) {
        assert!(
            self.presentations.is_empty(),
            "BUG: PresentationForest::install called while the ratchet is \
             still closed -- production topology is exactly one presentation \
             per realm until #555 PR-4/5 lifts the len()<=1 gate; tests use \
             push_for_test to bypass it deliberately"
        );
        self.presentations.push(presentation);
    }

    /// Isolation-suite-only bypass of [`Self::install`]'s ratchet, so a test
    /// can drive a genuine multi-presentation realm ahead of the addressed
    /// routing that makes doing so safe in production.
    #[cfg(test)]
    pub(crate) fn push_for_test(&mut self, presentation: PresentationState) {
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "addressed lookup by id has no production caller until \
                      #555 PR-4's addressed entry points land"
        )
    )]
    pub(crate) fn get(&self, id: PresentationId) -> Option<&PresentationState> {
        self.presentations.iter().find(|p| p.id() == id)
    }

    /// Remove and return the presentation with the given id, if present.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller until a presentation-close path \
                      (#555 PR-4/5) removes a member from a genuine N>1 forest; \
                      teardown of the realm's SOLE presentation today drops \
                      the whole forest instead of removing a member from it"
        )
    )]
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

    #[test]
    #[should_panic(expected = "BUG: PresentationForest::install called")]
    fn install_panics_once_the_forest_already_holds_a_presentation() {
        let mut forest = PresentationForest::single(presentation(1));
        forest.install(presentation(2));
    }

    #[test]
    fn push_for_test_bypasses_the_ratchet() {
        let mut forest = PresentationForest::single(presentation(1));
        forest.push_for_test(presentation(2));

        assert_eq!(forest.len(), 2);
    }

    #[test]
    fn iter_yields_presentations_in_mount_order() {
        let mut forest = PresentationForest::single(presentation(1));
        forest.push_for_test(presentation(2));
        forest.push_for_test(presentation(3));

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
        forest.push_for_test(second);

        assert!(forest.get(second_id).is_some());
        let removed = forest.remove(second_id).expect("present");
        assert_eq!(removed.id(), second_id);
        assert!(forest.get(second_id).is_none());
        assert_eq!(forest.len(), 1);
    }
}
