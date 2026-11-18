//! Panic containment for synchronous dispatch phases.
//!
//! A pointer route, a recognizer and the binding each run user code in phases
//! whose mandatory cleanup must finish before a failure propagates. Each phase
//! runs under [`caught`] or [`try_run`], and the dispatch folds the phases'
//! failures into one [`FirstPanic`] with [`keep_first`] (ADR-0281): the first
//! is resumed once the cleanup is done, a later one is reported and discarded
//! under containment. A value withdrawn after a caught failure is still
//! destroyed, with its destructor under containment ([`drop_contained`],
//! ADR-0281 §3).

pub(crate) use flui_foundation::panic::{FirstPanic, drop_contained};

/// Run one synchronous dispatch phase and hold its panic, if it has one.
pub(crate) fn caught(run: impl FnOnce()) -> FirstPanic {
    let mut failure = FirstPanic::new();
    let _: Option<()> = failure.contain(run);
    failure
}

/// Run one synchronous dispatch phase and hand back its return value, or the
/// panic it raised instead.
pub(crate) fn try_run<T>(run: impl FnOnce() -> T) -> Result<T, FirstPanic> {
    let mut failure = FirstPanic::new();
    failure.contain(run).ok_or(failure)
}

/// Fold the failure of the phase named `phase` into `first`. The earliest
/// failure stays and is resumed after mandatory cleanup; a later one cannot
/// replace it: it is destroyed under containment (ADR-0281 §2), then
/// reported.
pub(crate) fn keep_first(first: &mut FirstPanic, later: FirstPanic, phase: &'static str) {
    if !later.is_held() {
        return;
    }
    if !first.is_held() {
        first.absorb(later);
        return;
    }
    drop(later);
    // A tracing subscriber is user code too: one that panics is contained,
    // and its own failure is not reported again.
    drop(caught(|| {
        tracing::error!(
            phase,
            "dispatch phase panicked after an earlier phase; only the first panic is resumed"
        );
    }));
}

/// Raise the panic [`try_run`] caught, where the caller has no value to return
/// in its place.
pub(crate) fn raise(failure: FirstPanic) -> ! {
    let payload = failure
        .into_payload()
        .expect("BUG: try_run's error holds the panic it caught");
    std::panic::resume_unwind(payload)
}
