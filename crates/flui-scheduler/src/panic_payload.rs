//! Panic-payload containment shared by [`crate::scheduler`], [`crate::async_driver`],
//! and [`crate::ticker`]'s already-unwinding recovery paths.
//!
//! Each of those three modules reaches a point where it has already traced a
//! caught panic and must now get rid of the payload without letting the
//! payload's OWN `Drop` raise a second, uncontained one. This single helper is
//! the one place that containment is implemented, so all three sites carry the
//! same guarantee instead of three hand-rolled copies that could drift.

use std::any::Any;

/// Drops `payload`, containing (and tracing) a second-order panic from the
/// payload's own `Drop` instead of letting it escape uncontained.
///
/// A panic payload is `Box<dyn Any + Send>` — it can own, or itself be, any
/// type, including one whose `Drop` panics. An ordinary `drop(payload)`
/// would let that second panic escape uncontained: during an unwind that is
/// a double panic (an abort with no diagnostic); outside one, it replaces
/// the failure this call site actually meant to report. Every call site
/// this exists for has already traced the original panic before calling
/// this; it only adds a SECOND trace, and only if dropping the payload
/// panics too.
///
/// The SECOND-order payload — what `panic_any` inside the first payload's
/// own `Drop::drop` raised — is contained the same way it got here in the
/// first place: leaked, never dropped. It is itself `Box<dyn Any + Send>`
/// and so can just as well own a type whose `Drop` ALSO panics; an ordinary
/// `drop` of it here would only move the uncontained-second-panic problem
/// this function exists to close down one more level instead of closing it.
pub(crate) fn discard_panic_payload(payload: Box<dyn Any + Send>, context: &'static str) {
    if let Err(drop_payload) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
    {
        tracing::error!(
            context,
            panic_msg = flui_foundation::panic::payload_text(&*drop_payload)
                .unwrap_or("(non-string panic payload)"),
            "a discarded panic payload's own Drop panicked; containing it here rather than \
             letting a second panic escape"
        );
        // A payload whose own Drop panics cannot be dropped safely -- leaking
        // it is the only containment left; see this function's own doc.
        std::mem::forget(drop_payload);
    }
}
