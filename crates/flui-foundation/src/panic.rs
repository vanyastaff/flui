//! Panic-payload text extraction and the `BUG:` internal-invariant classifier.
//!
//! Every `catch_unwind` boundary that reads a caught payload needs the
//! same two decidable operations — pull whatever text the payload carries
//! (or honestly admit it has none), and decide whether that text asserts a
//! FLUI-owned invariant rather than reporting a caller- or application-code
//! failure. This is their one home: a boundary that reads payload text
//! calls these two functions and never downcasts the payload itself, so
//! what one boundary reports is what every boundary reports. Boundaries
//! that only `resume_unwind` or discard the payload have no reason to
//! call either.
//!
//! See `docs/PANIC-POLICY.md`'s "The `BUG:` message convention" for the
//! policy [`is_internal_invariant`] mechanizes.

use std::any::Any;

/// The text of a panic payload, when it has any.
///
/// `panic!("literal")` produces a `&'static str` payload; `panic!("{}", x)`
/// and every other formatted `panic!` produce a `String`; anything else
/// (constructed with [`std::panic::panic_any`]) is opaque and reported as
/// `None` — never guessed at.
#[must_use]
pub fn payload_text(payload: &(dyn Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

/// Whether `text` asserts a FLUI-owned internal invariant rather than a
/// caller- or application-code failure.
///
/// This classifies only — it never decides how a caught panic is routed
/// (contained, re-raised, logged at what level); that stays a per-site
/// decision. It says whether `text` was written for the `BUG:` convention
/// documented in `docs/PANIC-POLICY.md`: every production-path `expect()`
/// that asserts an invariant FLUI itself must maintain is prefixed
/// `"BUG: "`, so a user hitting it knows immediately the fault is FLUI's,
/// not theirs.
#[must_use]
pub fn is_internal_invariant(text: &str) -> bool {
    text.starts_with("BUG:")
}

/// Retain a discarded opaque panic payload so containment can continue safely.
///
/// A payload may own several fields with panicking destructors. Dropping it
/// inside `catch_unwind` can still abort when a second field panics during the
/// first field's unwind. Exceptional-path retention deliberately leaks the
/// payload; it does not run arbitrary destruction after a caught failure.
/// Exact `&'static str` and `String` payloads are released instead: their
/// destruction cannot invoke user code. No other payload type is inspected
/// for whether its destructor might be safe.
/// Boundaries that propagate the original failure should use `resume_unwind`
/// instead. This operation is for a failure already reported or superseded.
pub fn retain_opaque_payload(payload: Box<dyn Any + Send>) {
    if payload.is::<&'static str>() || payload.is::<String>() {
        drop(payload);
    } else {
        std::mem::forget(payload);
    }
}

/// Carries the first failure across nested callback delivery and retirement.
///
/// Framework delivery seams borrow this context from their enclosing owner.
/// Opaque ownership retires normally until a failure is caught; afterwards it
/// is retained. Call [`Self::finish`] only once all accepted work is delivered.
/// An aggregate that double-panics before containment regains control still
/// follows Rust's abort semantics.
#[doc(hidden)]
pub struct PanicRecovery {
    incoming: bool,
    first: Option<Box<dyn Any + Send>>,
}

impl std::fmt::Debug for PanicRecovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanicRecovery")
            .field("has_failure", &self.has_failure())
            .finish_non_exhaustive()
    }
}

impl Default for PanicRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl PanicRecovery {
    #[must_use]
    pub fn new() -> Self {
        Self {
            incoming: std::thread::panicking(),
            first: None,
        }
    }

    pub(crate) fn inherit_failure(&mut self) {
        self.incoming = true;
    }

    pub fn run(&mut self, action: impl FnOnce()) {
        self.run_with(|_| action());
    }

    pub fn run_with(&mut self, action: impl FnOnce(&mut Self)) {
        if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(self)))
        {
            self.capture(payload);
        }
    }

    pub(crate) fn capture(&mut self, payload: Box<dyn Any + Send>) {
        if self.has_failure() {
            retain_opaque_payload(payload);
        } else {
            self.first = Some(payload);
        }
    }

    #[must_use]
    pub fn has_failure(&self) -> bool {
        self.incoming || self.first.is_some()
    }

    pub fn retire<T>(&mut self, value: T) {
        if self.has_failure() {
            std::mem::forget(value);
        } else {
            self.run(|| drop(value));
        }
    }

    pub fn finish(mut self) {
        if let Some(payload) = self.first.take() {
            std::panic::resume_unwind(payload);
        }
    }
}

impl Drop for PanicRecovery {
    fn drop(&mut self) {
        if let Some(payload) = self.first.take() {
            retain_opaque_payload(payload);
        }
    }
}
