//! `InputSequenceId` issue contract (ADR-0207 §4).

use std::collections::HashSet;
use std::num::NonZeroU64;

use flui_foundation::{
    AtomicIdMint, FrameCallbackId, IdExhausted, IdMint, InputSequenceId, ListenerId, TaskId,
};

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

#[test]
fn a_mint_never_issues_past_its_ceiling_or_below_a_reservation() {
    let mint = IdMint::<NonZeroU64>::new().ending_at(3);
    mint.reserve_through(1);
    assert_eq!(mint.next().map(NonZeroU64::get), Ok(2), "1 is reserved");
    mint.reserve_through(1);
    assert_eq!(
        mint.next().map(NonZeroU64::get),
        Ok(3),
        "a reservation never moves the mint backwards"
    );
    assert!(mint.is_exhausted());
    assert_eq!(mint.next(), Err(IdExhausted), "4 is past the ceiling");
    assert_eq!(mint.next(), Err(IdExhausted), "the refusal is permanent");
}

#[test]
fn drag_session_ids_refuse_permanently_after_the_last() {
    let mint = IdMint::<flui_foundation::DragSessionId>::starting_after(u64::MAX - 1);
    let last = mint.next().expect("one id remains");
    assert_eq!(last.get(), u64::MAX);
    for _ in 0..3 {
        assert_eq!(mint.next(), Err(IdExhausted));
    }
}

#[test]
fn the_atomic_mint_issues_the_same_sequence_the_owner_local_mint_does() {
    let owner_local = IdMint::<NonZeroU64>::new();
    let shared = AtomicIdMint::<NonZeroU64>::new();
    for _ in 0..1_000 {
        let owner_local = owner_local.next().expect("a fresh mint has ids to issue");
        let shared = shared.next().expect("a fresh mint has ids to issue");
        assert_eq!(owner_local, shared, "the twins issue the same identities");
    }
}

#[test]
fn the_atomic_mint_issues_ids_from_any_thread() {
    let shared = AtomicIdMint::<NonZeroU64>::new();
    let mut seen = HashSet::new();
    for _ in 0..1_000 {
        let id = shared.next().expect("a fresh mint has ids to issue");
        assert!(seen.insert(id), "{id:?} was issued twice");
    }
}

#[test]
fn an_exhausted_atomic_mint_refuses_permanently() {
    let mint = AtomicIdMint::<NonZeroU64>::starting_after(u64::MAX - 2);
    let second_to_last = mint.next().expect("two ids remain");
    let last = mint.next().expect("one id remains");
    assert_eq!(last.get(), u64::MAX);
    assert!(second_to_last < last);
    for _ in 0..3 {
        assert_eq!(
            mint.next(),
            Err(IdExhausted),
            "the mint wrapped or reissued"
        );
    }
}

#[test]
fn an_atomic_mint_admits_exactly_one_of_concurrent_requests_for_the_last_identity() {
    let shared = AtomicIdMint::<NonZeroU64>::starting_after(u64::MAX - 1);
    let admitted = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4).map(|_| scope.spawn(|| shared.next())).collect();
        workers
            .into_iter()
            .filter_map(|worker| worker.join().expect("workers refuse, never panic").ok())
            .collect::<Vec<_>>()
    });
    assert_eq!(admitted.len(), 1, "one id remains for four racers");
    assert_eq!(admitted[0].get(), u64::MAX);
    assert_eq!(shared.next(), Err(IdExhausted), "the refusal stays");
}

#[test]
fn a_pointer_width_bounded_id_refuses_past_its_type_ceiling() {
    let exhausted = IdMint::<ListenerId>::starting_after(usize::MAX as u64);
    assert_eq!(
        exhausted.next(),
        Err(IdExhausted),
        "starting at the type's ceiling is already exhausted"
    );
    let listener_mint = IdMint::<ListenerId>::starting_after(usize::MAX as u64 - 1);
    let last = listener_mint.next().expect("one id remains");
    assert_eq!(last.get(), usize::MAX);
    assert_eq!(
        listener_mint.next(),
        Err(IdExhausted),
        "refusal past the type's ceiling is permanent"
    );
    let task_mint = AtomicIdMint::<TaskId>::starting_after(usize::MAX as u64 - 1);
    assert_eq!(task_mint.next().expect("one id remains").get(), usize::MAX);
    assert_eq!(task_mint.next(), Err(IdExhausted));
}

#[test]
fn scheduler_and_listener_ids_are_minted_by_the_family() {
    let listener_mint = IdMint::<ListenerId>::new();
    let first = listener_mint.next().expect("a fresh mint has ids to issue");
    let second = listener_mint.next().expect("a fresh mint has ids to issue");
    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 2);
    assert!(first < second, "listener ids sort by issue order");

    let callbacks = AtomicIdMint::<FrameCallbackId>::new();
    let first = callbacks.next().expect("a fresh mint has ids to issue");
    let second = callbacks.next().expect("a fresh mint has ids to issue");
    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 2);
    assert!(first < second);

    let tasks = AtomicIdMint::<TaskId>::new();
    let first = tasks.next().expect("a fresh mint has ids to issue");
    let second = tasks.next().expect("a fresh mint has ids to issue");
    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 2);
    assert!(first < second);
}
