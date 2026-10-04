//! The push vocabulary as store edits (ADR-0090 §2).
//!
//! winit, the one push-model source today, delivers
//! [`ImeEvent::{Enabled, Preedit, Commit, Disabled}`](ImeEvent). A
//! push event is not a second editing path: [`project_ime_event`] turns each
//! into edits made under a read-write lock on the field's [`TextStore`],
//! the same way a pull-model input method would make them.

use std::cell::Cell;
use std::rc::Rc;

use crate::ImeEvent;

use super::lock::{LockGrant, LockOutcome, LockTiming, TextStoreError};
use super::session::{Composition, Selection, TextStoreEdit};
use super::store::TextStore;
use super::utf16::{self, Utf16Offset, Utf16Range};

/// The edits one event makes, run inside the grant.
type Edit = Box<dyn FnOnce(&mut dyn TextStoreEdit) -> Result<(), TextStoreError>>;

/// Apply one push-model [`ImeEvent`] to `store` as edits under a read-write
/// lock.
///
/// - **`Preedit` with text** replaces the composition (or, when none is in
///   progress, the selection), marks the new text as the composition with
///   `hides_caret` set when `cursor` is `None`, and puts the caret at the
///   cursor's end — translated from a byte offset into the preedit to a
///   UTF-16 offset into the document, clamped forward to a `char` boundary
///   when malformed — or at the composition's end when there is no cursor.
/// - **`Preedit` with empty text** ends an active composition: its text is
///   deleted and the caret goes to the earlier of where it was and where the
///   composition started. With no composition it changes nothing (winit's
///   X11 backend sends one when the input method starts).
/// - **`Commit`** replaces the composition, or the selection when there is
///   none, ends the composition, and leaves the caret after the text.
/// - **`Disabled`** mid-composition strips the composed text the way an
///   empty `Preedit` does. That is winit's behaviour, applied here
///   explicitly: the store never strips on its own.
/// - **`Enabled`** edits nothing and returns [`LockOutcome::Granted`].
///
/// The lock is asynchronous: an event that arrives while the store cannot
/// grant a lock (inside a frame transaction, or inside another session) is
/// applied in order at the next commit anchor rather than dropped.
///
/// # Errors
///
/// The store's refusal of the lock, or an edit's error when the grant ran
/// before this returned. An edit that fails in a grant that runs later is
/// reported with `tracing::warn!`, since no caller is left to receive it.
pub fn project_ime_event(
    store: &dyn TextStore,
    event: &ImeEvent,
) -> Result<LockOutcome, TextStoreError> {
    let edit: Edit = match event {
        ImeEvent::Preedit { text, cursor } if !text.is_empty() => {
            let (text, cursor) = (text.clone(), *cursor);
            Box::new(move |session| preedit(session, &text, cursor))
        }
        ImeEvent::Preedit { .. } | ImeEvent::Disabled => Box::new(end_composition),
        ImeEvent::Commit(text) => {
            let text = text.clone();
            Box::new(move |session| commit(session, &text))
        }
        // `Enabled`, and any variant a later winit adds: nothing to edit.
        _ => return Ok(LockOutcome::Granted),
    };

    let failure: Rc<Cell<Option<TextStoreError>>> = Rc::new(Cell::new(None));
    let returned = Rc::new(Cell::new(false));
    let (grant_failure, grant_returned) = (Rc::clone(&failure), Rc::clone(&returned));
    let outcome = store.request_lock(
        LockGrant::read_write(move |session| {
            if let Err(error) = edit(session) {
                if grant_returned.get() {
                    tracing::warn!(?error, "a deferred IME edit could not be applied");
                } else {
                    grant_failure.set(Some(error));
                }
            }
        }),
        LockTiming::Async,
    );
    returned.set(true);
    match failure.take() {
        Some(error) => Err(error),
        None => outcome,
    }
}

fn preedit(
    session: &mut dyn TextStoreEdit,
    text: &str,
    cursor: Option<(usize, usize)>,
) -> Result<(), TextStoreError> {
    let region = session.composition().map_or_else(
        || session.selection().range(),
        |composition| composition.range,
    );
    let change = session.replace(region, text)?;
    let start = change.start;
    let range =
        Utf16Range::new(start, change.new_end).expect("BUG: an insertion ends after it starts");
    session.set_composition(Some(Composition {
        range,
        hides_caret: cursor.is_none(),
    }))?;
    let caret = match cursor {
        Some((_, end)) => {
            let end = text.ceil_char_boundary(end);
            Utf16Offset::new(start.get() + utf16::utf16_len(&text[..end]).get())
        }
        None => change.new_end,
    };
    session.set_selection(Selection::collapsed(caret))
}

fn end_composition(session: &mut dyn TextStoreEdit) -> Result<(), TextStoreError> {
    let Some(composition) = session.composition() else {
        return Ok(());
    };
    let caret = session.selection().active;
    let start = composition.range.start();
    session.replace(composition.range, "")?;
    session.set_composition(None)?;
    session.set_selection(Selection::collapsed(caret.min(start)))
}

fn commit(session: &mut dyn TextStoreEdit, text: &str) -> Result<(), TextStoreError> {
    let change = match session.composition() {
        Some(composition) => session.replace(composition.range, text)?,
        None => session.insert_at_selection(text)?,
    };
    session.set_composition(None)?;
    session.set_selection(Selection::collapsed(change.new_end))
}

#[cfg(test)]
mod tests {
    use super::super::{CommitGate, InMemoryTextStore};
    use super::*;

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
        Utf16Range::new(at(start), at(end)).expect("ordered")
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
}
