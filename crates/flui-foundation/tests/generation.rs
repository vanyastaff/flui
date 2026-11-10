//! `Generation` and `GenSlots` — the slot generation policy (the packed
//! keys' ABA contract).

use flui_foundation::generation::GenSlots;
use std::num::NonZeroU32;

use flui_foundation::generation::{DOMAIN_BIT, Generation, ReservedGenerationBit};
use flui_foundation::{ElementId, PresentationId, RenderId};

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

#[test]
fn the_last_generation_stops_below_the_reserved_domain_bit() {
    assert_eq!(Generation::LAST.as_u32(), 0x7FFF_FFFF);
    assert!(Generation::LAST.is_last());
    assert_eq!(Generation::LAST.next(), None, "no step past the last");
    let before_last = Generation::try_from(
        NonZeroU32::new(Generation::LAST.as_u32() - 1).expect("the value is non-zero"),
    )
    .expect("the generation before the last is admitted");
    assert!(!before_last.is_last());
    assert_eq!(before_last.next(), Some(Generation::LAST));

    let mut slots = GenSlots::new();
    slots.seed_generation(0, Generation::LAST.as_u32() - 1);
    assert_eq!(
        slots.mint_recycled(0),
        Generation::LAST,
        "the last recycle reaches LAST"
    );
    assert_eq!(slots.peek(0), None, "a slot at LAST closes at its release");
}

#[test]
fn a_raw_generation_that_sets_the_reserved_bit_is_refused() {
    let cases = [
        (Generation::LAST.as_u32(), true),
        (Generation::LAST.as_u32() + 1, false),
        (u32::MAX, false),
        (1, true),
    ];
    for (raw, admitted) in cases {
        let raw = NonZeroU32::new(raw).expect("the case is non-zero");
        let answer = Generation::try_from(raw);
        assert_eq!(answer.is_ok(), admitted, "raw generation {raw}");
        if !admitted {
            assert_eq!(answer, Err(ReservedGenerationBit(raw)));
        }
    }
}

#[test]
fn a_packed_key_never_carries_or_decodes_the_reserved_domain_bit() {
    let render = RenderId::new_gen(u32::MAX, Generation::LAST);
    assert_eq!(render.as_u64() & DOMAIN_BIT, 0);
    assert_eq!(RenderId::from_u64(render.as_u64()), Some(render));
    assert_eq!(render.generation(), Generation::LAST);
    let element = ElementId::new_gen(u32::MAX, Generation::LAST);
    assert_eq!(element.as_u64() & DOMAIN_BIT, 0);
    let presentation = PresentationId::new_gen(u32::MAX, Generation::LAST);
    assert_eq!(
        PresentationId::from_u64(presentation.as_u64()),
        Some(presentation)
    );

    let tagged = render.as_u64() | DOMAIN_BIT;
    assert_eq!(RenderId::from_u64(tagged), None);
    assert_eq!(PresentationId::from_u64(tagged), None);
    assert_eq!(
        RenderId::from_u64(DOMAIN_BIT | 1),
        None,
        "the domain bit alone is no generation either"
    );
}
