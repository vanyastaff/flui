//! `project_ime_event`: a push-model IME event applied to a store as edits.

use flui_interaction::testing::InMemoryTextStore;
use flui_platform::ImeEvent;
use flui_platform::text_store::{
    CommitGate, LockGrant, LockOutcome, LockTiming, Selection, TextStore, Utf16Offset, Utf16Range,
    project_ime_event,
};

/// Install a shut gate into `store`, as a frame transaction would.
fn shut_gate(store: &InMemoryTextStore) -> CommitGate {
    let gate = CommitGate::new();
    gate.set_open(false);
    store.set_commit_gate(gate.clone());
    gate
}

fn at(units: usize) -> Utf16Offset {
    Utf16Offset::new(units)
}

fn range(start: usize, end: usize) -> Utf16Range {
    Utf16Range::try_new(at(start), at(end)).expect("ordered")
}

fn preedit_event(text: &str, cursor: Option<(usize, usize)>) -> ImeEvent {
    ImeEvent::Preedit {
        text: text.to_owned(),
        cursor,
    }
}

fn apply(store: &InMemoryTextStore, event: &ImeEvent) {
    assert_eq!(
        project_ime_event(store, event),
        Ok(LockOutcome::Granted),
        "{event:?}"
    );
}

/// Select `anchor..active` through a platform session.
fn select(store: &InMemoryTextStore, anchor: usize, active: usize) {
    let _ = store.request_lock(
        LockGrant::read_write(move |session| {
            session
                .set_selection(Selection {
                    anchor: at(anchor),
                    active: at(active),
                })
                .expect("in range");
        }),
        LockTiming::Sync,
    );
}

fn composed(store: &InMemoryTextStore) -> Option<Utf16Range> {
    store.composition().map(|composition| composition.range)
}

fn preedit_replaces_the_selection_and_marks_the_composition() {
    let store = InMemoryTextStore::new("hello world");
    select(&store, 6, 11);
    apply(&store, &preedit_event("に", Some((3, 3))));
    assert_eq!(store.text(), "hello に");
    assert_eq!(composed(&store), Some(range(6, 7)));
    assert_eq!(store.selection(), Selection::collapsed(at(7)));

    // Growing and shrinking replace the composition, not the caret.
    apply(&store, &preedit_event("にほ", Some((6, 6))));
    apply(&store, &preedit_event("に", Some((3, 3))));
    assert_eq!(store.text(), "hello に");
    assert_eq!(composed(&store), Some(range(6, 7)));
}

fn malformed_preedit_cursor_clamps_instead_of_panicking() {
    let store = InMemoryTextStore::new("");
    // Byte 1 is inside "é" (two bytes): it moves forward to byte 2, one
    // unit into the preedit. Past the end clamps to the end.
    apply(&store, &preedit_event("éa", Some((1, 1))));
    assert_eq!(store.selection(), Selection::collapsed(at(1)));
    apply(&store, &preedit_event("éa", Some((0, 99))));
    assert_eq!(store.selection(), Selection::collapsed(at(2)));
    for (text, cursor, units) in [
        ("中a", 1, 1),
        ("中a", 2, 1),
        ("😀a", 1, 2),
        ("😀a", 2, 2),
        ("😀a", 3, 2),
        ("😀a", usize::MAX, 3),
        ("", usize::MAX, 0),
    ] {
        apply(&store, &preedit_event(text, Some((0, cursor))));
        assert_eq!(store.selection(), Selection::collapsed(at(units)));
    }
}

fn commit_replaces_the_composition() {
    let store = InMemoryTextStore::new("x");
    apply(&store, &preedit_event("とうきょう", None));
    apply(&store, &ImeEvent::Commit("東京".to_owned()));
    assert_eq!(store.text(), "x東京");
    assert_eq!(store.composition(), None);
    assert_eq!(store.selection(), Selection::collapsed(at(3)));
}

fn a_push_event_while_commits_are_closed_applies_in_order_at_the_next_anchor() {
    let store = InMemoryTextStore::new("");
    let gate = shut_gate(&store);
    for event in [
        preedit_event("と", Some((3, 3))),
        preedit_event("とう", Some((6, 6))),
        ImeEvent::Commit("東".to_owned()),
    ] {
        assert_eq!(
            project_ime_event(&*store, &event),
            Ok(LockOutcome::Deferred)
        );
    }
    assert_eq!(store.text(), "", "nothing applies inside the transaction");
    gate.set_open(true);
    assert_eq!(store.run_deferred_grants(), 3);
    assert_eq!(store.text(), "東");
    assert_eq!(store.composition(), None);
    assert_eq!(store.selection(), Selection::collapsed(at(1)));
}

/// IME projection into a store: preedit, malformed cursors, commit, and
/// events pushed while commits are closed.
#[test]
fn ime_events_project_onto_the_store_as_the_text_input_contract_says() {
    preedit_replaces_the_selection_and_marks_the_composition();
    malformed_preedit_cursor_clamps_instead_of_panicking();
    commit_replaces_the_composition();
    a_push_event_while_commits_are_closed_applies_in_order_at_the_next_anchor();
}
