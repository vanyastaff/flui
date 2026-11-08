//! `Generation` and `GenSlots` — the slot generation policy (the packed
//! keys' ABA contract).

use flui_foundation::generation::GenSlots;
use flui_foundation::generation::Generation;

/// The ledger's carry of `slot`, as the packed value.
fn carry(slots: &GenSlots, slot: u32) -> u32 {
    slots
        .generation(slot)
        .map(Generation::as_u32)
        .expect("the slot is in the bookkeeping")
}

#[test]
fn a_fresh_slot_starts_at_its_first_generation() {
    let mut slots = GenSlots::new();
    let first = slots.mint_fresh();
    assert_eq!(carry(&slots, 0), 1, "a fresh slot starts at its first");
    assert!(!first.is_last());
    assert!(
        first.next().is_some(),
        "a first generation has a release step"
    );
    assert_eq!(
        Generation::FIRST.as_nonzero().get(),
        1,
        "the first generation packs the value one"
    );
}

#[test]
fn a_recycled_slot_steps_to_the_generation_it_mints_next() {
    let mut slots = GenSlots::new();
    slots.mint_fresh();
    let minted = slots.mint_recycled(0);
    assert_eq!(minted.as_u32(), 2, "the recycled slot steps past its carry");
    assert_eq!(carry(&slots, 0), 2, "the ledger carries the committed step");
    // A handle that carried the slot's previous occupant no longer matches.
    assert_ne!(slots.generation(0), Some(Generation::FIRST));
}

#[test]
fn an_exhausted_slot_has_no_release_step_and_closes_forever() {
    let mut slots = GenSlots::new();
    slots.seed_generation(3, Generation::LAST.as_u32());
    assert!(
        slots.generation(3).is_some_and(Generation::is_last),
        "the seeded terminal generation is the slot's carry"
    );
    assert_eq!(
        slots.peek(3),
        None,
        "a slot at its last generation has no release to commit"
    );
    let mut slots = slots.clone();
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slots.mint_recycled(3);
    }));
    assert!(
        refused.is_err(),
        "a closed slot is never claimed from circulation"
    );
    assert_eq!(
        slots.generation(3).map(Generation::as_u32),
        Some(Generation::LAST.as_u32()),
        "the closed slot keeps its final generation"
    );
}

#[test]
fn a_release_commit_applies_only_what_its_peek_minted() {
    let mut slots = GenSlots::new();
    let fresh = slots.mint_fresh();
    let release = slots.peek(0).expect("a fresh slot has a release step");
    slots.commit(release);
    assert_eq!(
        carry(&slots, 0),
        fresh.as_u32() + 1,
        "the commit applied the peeked step"
    );
    // A handle that carried the released occupant's generation no longer
    // matches the slot's live carry.
    let live = slots.generation(0).expect("a committed slot stays carried");
    assert_ne!(live, fresh);
}

#[test]
fn slots_the_bookkeeping_does_not_carry_yet_grow_in_as_fresh_firsts() {
    let mut slots = GenSlots::new();
    slots.seed_generation(2, 7);
    assert_eq!(carry(&slots, 2), 7);
    assert_eq!(carry(&slots, 0), 1, "the slots in between grow as fresh");
    assert_eq!(carry(&slots, 1), 1);
    assert_eq!(
        slots.generation(3),
        None,
        "the ledger carries exactly the slots it grew to"
    );
}
