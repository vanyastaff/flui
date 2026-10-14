//! `InputSequenceId` issue contract (ADR-0207 §4).

use std::collections::HashSet;

use flui_foundation::{IdExhausted, IdMint, InputSequenceId};

#[test]
fn input_sequence_ids_are_never_reused() {
    let mint = IdMint::<InputSequenceId>::new();
    let mut seen = HashSet::new();
    let mut last = None;
    for _ in 0..1_000 {
        let id = mint.next().expect("a fresh mint has ids to issue");
        assert!(seen.insert(id), "{id:?} was issued twice");
        assert!(last < Some(id), "{id:?} did not follow {last:?}");
        last = Some(id);
    }
}

#[test]
fn an_exhausted_sequence_mint_refuses_permanently() {
    let mint = IdMint::<InputSequenceId>::starting_after(u64::MAX - 2);
    let penultimate = mint.next().expect("two ids remain");
    let last = mint.next().expect("one id remains");
    assert_eq!(last.get(), u64::MAX);
    assert!(penultimate < last);
    for _ in 0..3 {
        assert_eq!(
            mint.next(),
            Err(IdExhausted),
            "the mint wrapped or reissued"
        );
    }
}
