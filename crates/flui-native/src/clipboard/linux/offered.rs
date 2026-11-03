//! The application's lazily offered selection, which both Linux hosts' primary
//! selections hold: a closure asked for the selected text only when another
//! client reads it, on the owner thread, with nothing of the host's held.
//!
//! Its captures are user code, so after a caught failure a closure is still
//! dropped, under containment, and the first failure is the one that goes on
//! (ADR-0281).

use std::rc::Rc;

use flui_foundation::panic::{FirstPanic, discard_payload};
use flui_platform::{ClipboardRead, capability::Resolver};

use crate::host::host_text;

/// What the application offers as its selection.
pub(crate) type SelectionText = Rc<dyn Fn() -> Option<String>>;

/// Drop the closures the host retired, in order, each contained. Once a
/// destructor panics the rest are still dropped and the first failure goes
/// on (ADR-0281).
pub(crate) fn drop_retired(retired: Vec<SelectionText>) {
    let mut first = FirstPanic::new();
    for text in retired {
        first.contain(|| drop(text));
    }
    first.resume();
}

/// Ask the application's closure for its text. The closure, which may be its
/// last owner, is dropped after the call under containment, a panic included;
/// the call's panic goes on (ADR-0281).
pub(crate) fn ask(text: SelectionText) -> Option<String> {
    let mut first = FirstPanic::new();
    let answer = first.contain(|| text());
    first.contain(|| drop(text));
    first.resume();
    answer.flatten()
}

/// Answer the application's own read of its selection with what the closure
/// gives. A closure that panics leaves the read answered with nothing, and
/// that answer is posted under containment of its own before the closure's
/// failure goes on; a second failure there is discarded and the first one
/// stays the one that goes on (ADR-0281 §2).
pub(crate) fn answer_with(text: SelectionText, answer: Resolver<ClipboardRead>) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ask(text))) {
        // An owner that retired has nobody left to answer.
        Ok(text) => drop(answer.resolve(text.map_or(Ok(None), host_text))),
        Err(failure) => {
            let settled =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answer.resolve(Ok(None))));
            if let Err(second) = settled {
                discard_payload(second);
            }
            std::panic::resume_unwind(failure)
        }
    }
}
