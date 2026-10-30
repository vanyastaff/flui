//! The application's lazily offered selection, which both Linux hosts' primary
//! selections hold: a closure asked for the selected text only when another
//! client reads it, on the owner thread, with nothing of the host's held.
//!
//! Its captures are user code, so it is asked and dropped as ADR-0127 has a
//! callback asked and dropped: a closure that fails is retained rather than
//! dropped during the unwind, and closures dropped in sequence after one that
//! fails are retained too.

use std::rc::Rc;

/// What the application offers as its selection.
pub(crate) type SelectionText = Rc<dyn Fn() -> Option<String>>;

/// Drop the closures the host retired, in order. Once a destructor panics
/// the rest are retained, not dropped during the unwind, and the first
/// failure goes on (ADR-0127).
pub(crate) fn drop_retired(retired: Vec<SelectionText>) {
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
