//! The push vocabulary as store edits (ADR-0090 §2).
//!
//! winit, the one push-model source today, delivers
//! [`ImeEvent::{Enabled, Preedit, Commit, Disabled}`](ImeEvent). A
//! push event is not a second editing path: [`project_ime_event`] turns each
//! into edits made under a read-write lock on the field's [`TextStore`],
//! the same way a pull-model input method would make them.

use std::cell::Cell;
use std::rc::Rc;

use flui_types::ImeEvent;

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
            let end = clamp_to_char_boundary(text, end);
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

/// `offset` moved forward to the nearest `char` boundary of `text`, and
/// clamped to its end: a preedit cursor is untrusted platform input.
fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    (offset..=text.len())
        .find(|&candidate| text.is_char_boundary(candidate))
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::super::InMemoryTextStore;
    use super::*;

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

    #[test]
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

    #[test]
    fn preedit_cursor_bytes_map_to_utf16_within_the_preedit() {
        let store = InMemoryTextStore::new("ab");
        apply(&store, &preedit_event("😀a", Some((4, 4))));
        // The composition starts at 2; "😀" is four bytes and two units.
        assert_eq!(composed(&store), Some(range(2, 5)));
        assert_eq!(store.selection(), Selection::collapsed(at(4)));
    }

    #[test]
    fn malformed_preedit_cursor_clamps_instead_of_panicking() {
        let store = InMemoryTextStore::new("");
        // Byte 1 is inside "é" (two bytes): it moves forward to byte 2, one
        // unit into the preedit. Past the end clamps to the end.
        apply(&store, &preedit_event("éa", Some((1, 1))));
        assert_eq!(store.selection(), Selection::collapsed(at(1)));
        apply(&store, &preedit_event("éa", Some((0, 99))));
        assert_eq!(store.selection(), Selection::collapsed(at(2)));
    }

    #[test]
    fn cursor_none_hides_the_caret_and_collapses_to_composition_end() {
        let store = InMemoryTextStore::new("x");
        apply(&store, &preedit_event("かな", None));
        let composition = store.composition().expect("composing");
        assert!(composition.hides_caret);
        assert_eq!(composition.range, range(1, 3));
        assert_eq!(store.selection(), Selection::collapsed(at(3)));

        apply(&store, &preedit_event("かな", Some((0, 0))));
        assert!(!store.composition().expect("composing").hides_caret);
    }

    #[test]
    fn empty_preedit_ends_an_active_composition() {
        let store = InMemoryTextStore::new("ab");
        apply(&store, &preedit_event("にほ", Some((6, 6))));
        apply(&store, &preedit_event("", None));
        assert_eq!(store.text(), "ab");
        assert_eq!(store.composition(), None);
        assert_eq!(store.selection(), Selection::collapsed(at(2)));
        assert_eq!(store.owner_notifications(), 2);
    }

    #[test]
    fn empty_preedit_without_composition_changes_nothing_and_does_not_notify() {
        let store = InMemoryTextStore::new("hello");
        select(&store, 4, 1);
        let before = store.owner_notifications();
        apply(&store, &preedit_event("", None));
        assert_eq!(store.text(), "hello");
        assert_eq!(
            store.selection(),
            Selection {
                anchor: at(4),
                active: at(1)
            },
            "a backward selection survives exactly"
        );
        assert_eq!(store.owner_notifications(), before);
    }

    #[test]
    fn x11_empty_start_then_empty_end_preserves_selection() {
        let store = InMemoryTextStore::new("a😀b");
        select(&store, 1, 3);
        apply(&store, &ImeEvent::Enabled);
        apply(&store, &preedit_event("", None));
        apply(&store, &preedit_event("", None));
        assert_eq!(store.text(), "a😀b");
        assert_eq!(
            store.selection(),
            Selection {
                anchor: at(1),
                active: at(3)
            }
        );
    }

    #[test]
    fn commit_replaces_the_composition() {
        let store = InMemoryTextStore::new("x");
        apply(&store, &preedit_event("とうきょう", None));
        apply(&store, &ImeEvent::Commit("東京".to_owned()));
        assert_eq!(store.text(), "x東京");
        assert_eq!(store.composition(), None);
        assert_eq!(store.selection(), Selection::collapsed(at(3)));
    }

    #[test]
    fn direct_commit_replaces_the_selection() {
        let store = InMemoryTextStore::new("hello world");
        select(&store, 0, 5);
        apply(&store, &ImeEvent::Commit("你好".to_owned()));
        assert_eq!(store.text(), "你好 world");
        assert_eq!(store.selection(), Selection::collapsed(at(2)));
    }

    #[test]
    fn disabled_mid_composition_strips_the_slice() {
        let store = InMemoryTextStore::new("ab");
        select(&store, 1, 1);
        apply(&store, &preedit_event("にほ", Some((6, 6))));
        assert_eq!(store.text(), "aにほb");
        apply(&store, &ImeEvent::Disabled);
        assert_eq!(store.text(), "ab");
        assert_eq!(store.composition(), None);
        assert_eq!(store.selection(), Selection::collapsed(at(1)));

        // With nothing composed, `Disabled` is inert.
        let before = store.owner_notifications();
        apply(&store, &ImeEvent::Disabled);
        assert_eq!(store.owner_notifications(), before);
    }

    #[test]
    fn a_caret_before_the_composition_stays_when_it_is_stripped() {
        let store = InMemoryTextStore::new("ab");
        apply(&store, &preedit_event("に", Some((3, 3))));
        select(&store, 0, 0);
        apply(&store, &ImeEvent::Disabled);
        assert_eq!(store.text(), "ab");
        assert_eq!(store.selection(), Selection::collapsed(at(0)));
    }

    #[test]
    fn a_push_event_while_commits_are_closed_applies_in_order_at_the_next_anchor() {
        let store = InMemoryTextStore::new("");
        store.set_commits_allowed(false);
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
        store.set_commits_allowed(true);
        assert_eq!(store.run_deferred_grants(), 3);
        assert_eq!(store.text(), "東");
        assert_eq!(store.composition(), None);
        assert_eq!(store.selection(), Selection::collapsed(at(1)));
    }

    #[test]
    fn enabled_takes_no_lock_and_edits_nothing() {
        let store = InMemoryTextStore::new("ab");
        // Commits closed: a lock request would be deferred, so `Granted`
        // shows that none was made.
        store.set_commits_allowed(false);
        assert_eq!(
            project_ime_event(&*store, &ImeEvent::Enabled),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(store.owner_notifications(), 0);
    }
}
