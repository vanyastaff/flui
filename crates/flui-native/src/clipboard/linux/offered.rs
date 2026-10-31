//! The application's lazily offered selection, which both Linux hosts' primary
//! selections hold: a closure asked for the selected text only when another
//! client reads it, on the owner thread, with nothing of the host's held.
//!
//! Its captures are user code, so it is asked and dropped as ADR-0127 has a
//! callback asked and dropped: a closure that fails is retained rather than
//! dropped during the unwind, and closures dropped in sequence after one that
//! fails are retained too.

use std::rc::Rc;

use flui_platform::capability::Resolver;

/// What the application offers as its selection.
pub(crate) type SelectionText = Rc<dyn Fn() -> Option<String>>;
/// Drop the closures the host retired, in order. Once a destructor panics
/// the rest are retained, not dropped during the unwind, and the first
/// failure goes on (ADR-0127). Dropped by an unwind (a host torn down while
/// the application's panic unwinds), every closure is retained: the failure
/// unwinding stays the only one (ADR-0127 §2).
pub(crate) fn drop_retired(retired: Vec<SelectionText>) {
    if std::thread::panicking() {
        std::mem::forget(retired);
        return;
    }
    let mut retired = retired.into_iter();
    for text in retired.by_ref() {
        if let Err(failure) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(text)))
        {
            retired.for_each(std::mem::forget);
            std::panic::resume_unwind(failure);
        }
    }
}

/// Ask the application's closure for its text. A closure that panics is
/// retained rather than dropped during the unwind, which may be its last
/// owner: a capture's destructor that panicked then would abort (ADR-0127).
pub(crate) fn ask(text: SelectionText) -> Option<String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| text())) {
        Ok(answer) => answer,
        Err(failure) => {
            std::mem::forget(text);
            std::panic::resume_unwind(failure)
        }
    }
}

/// Answer the application's own read of its selection with what the closure
/// gives. A closure that panics leaves the read answered with nothing, and
/// that answer is posted under containment of its own before the closure's
/// failure goes on: an owner queue that fails then is not a second panic
/// during the unwind, which would abort, and the first failure stays the one
/// that goes on (ADR-0127 §2).
pub(crate) fn answer_with(text: SelectionText, answer: Resolver<Option<String>>) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ask(text))) {
        // An owner that retired has nobody left to answer.
        Ok(text) => drop(answer.resolve(text)),
        Err(failure) => {
            let settled =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answer.resolve(None)));
            if let Err(second) = settled {
                // Kept, not destroyed: its destructor could fail too.
                std::mem::forget(second);
            }
            std::panic::resume_unwind(failure)
        }
    }
}
