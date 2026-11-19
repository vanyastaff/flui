//! The push vocabulary as store edits (ADR-0090 §2).
//!
//! A push-model source delivers
//! [`ImeEvent::{Enabled, Preedit, Commit, Disabled}`](ImeEvent). A
//! push event is not a second editing path: [`project_ime_event`] turns each
//! into edits made under a read-write lock on the field's [`TextStore`],
//! the same way a pull-model input method would make them.

use std::cell::Cell;
use std::rc::Rc;

use crate::ImeEvent;

use super::contract::TextStore;
use super::lock::{LockGrant, LockOutcome, LockTiming, TextStoreError};
use super::session::{Composition, Selection, TextStoreEdit};
use super::utf16::{self, Utf16Offset, Utf16Range};

/// The edits one event makes, run inside the grant.
type Edit = Box<dyn FnOnce(&mut dyn TextStoreEdit) -> Result<(), TextStoreError>>;

/// Apply one push-model [`ImeEvent`] to `store` as edits under a read-write
/// lock.
///
/// - **`Preedit` with text** replaces the composition (or, when none is in
///   progress, the selection), marks the new text as the composition with
///   `hides_caret` set when `cursor` is `None`, and selects the cursor's span
///   inside it: the input method's target clause, so a field can mark the
///   part being converted, with the caret at its end. Each end is translated
///   from a byte offset into the preedit to a UTF-16 offset into the
///   document, clamped forward to a `char` boundary (and to the preedit's
///   end) when malformed. With no cursor the caret goes to the composition's
///   end.
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
/// before this returned. A grant the store queued runs after this returned:
/// its refusal goes to `late_refusal`, which the caller supplies to report it
/// (the contract does not log).
pub fn project_ime_event(
    store: &dyn TextStore,
    event: &ImeEvent,
    late_refusal: impl FnOnce(TextStoreError) + 'static,
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
        ImeEvent::Enabled => return Ok(LockOutcome::Granted),
    };
    edit_async(store, edit, late_refusal)
}

/// Run `edit` under an asynchronous read-write lock on `store`.
///
/// An edit that runs before this returns answers through the result: its
/// error, or else what the store did with the request. One the store queues
/// runs at a later commit anchor, after this returned, and hands its error to
/// `late_refusal`; a queued edit that succeeds, or a grant the store drops
/// unrun, calls nothing.
pub(super) fn edit_async(
    store: &dyn TextStore,
    edit: impl FnOnce(&mut dyn TextStoreEdit) -> Result<(), TextStoreError> + 'static,
    late_refusal: impl FnOnce(TextStoreError) + 'static,
) -> Result<LockOutcome, TextStoreError> {
    let failure: Rc<Cell<Option<TextStoreError>>> = Rc::new(Cell::new(None));
    // A grant that runs after this returned finds the caller gone.
    let caller = Rc::downgrade(&failure);
    let outcome = store.request_lock(
        LockGrant::read_write(move |session| {
            if let Err(error) = edit(session) {
                match caller.upgrade() {
                    Some(failure) => failure.set(Some(error)),
                    None => late_refusal(error),
                }
            }
        }),
        LockTiming::Async,
    );
    failure.take().map_or(outcome, Err)
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
        Utf16Range::try_new(start, change.new_end).expect("BUG: an insertion ends after it starts");
    session.set_composition(Some(Composition {
        range,
        hides_caret: cursor.is_none(),
    }))?;
    // The cursor's span is the input method's target clause, kept as the
    // selection inside the composition, as `setMarkedText:selectedRange:` and
    // `text-input-v3`'s cursor are.
    let selection = match cursor {
        Some((anchor, active)) => {
            let at = |byte: usize| {
                let byte = text.ceil_char_boundary(byte);
                Utf16Offset::new(start.get() + utf16::utf16_len(&text[..byte]).get())
            };
            Selection {
                anchor: at(anchor),
                active: at(active),
            }
        }
        None => Selection::collapsed(change.new_end),
    };
    session.set_selection(selection)
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
