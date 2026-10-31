//! Panic containment for synchronous dispatch phases.
//!
//! A pointer route, a recognizer and the binding each run user code in phases
//! whose mandatory cleanup must finish before a failure propagates.
//! [`RoutePanic`] holds the first such failure until that cleanup is done.

use std::any::Any;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use crate::retain::Retain;

/// The first panic captured while invoking a resolved pointer route, as
/// [`InteractionDispatchHandle::invoke_pointer_route`](crate::pointer::InteractionDispatchHandle::invoke_pointer_route)
/// returns it.
///
/// The dispatch owner must finish its mandatory cleanup (close the arena on
/// Down, sweep on Up, release on Up/Cancel, release an ephemeral route) and
/// then call [`resume`](Self::resume) so the panic propagates under the
/// repository panic policy.
#[must_use = "a captured route panic must be resumed after dispatch cleanup"]
pub struct RoutePanic {
    payload: Box<dyn Any + Send>,
}

impl RoutePanic {
    /// Run one synchronous dispatch phase and retain its return value unless it
    /// unwinds.
    pub(crate) fn try_run<T>(run: impl FnOnce() -> T) -> Result<T, Self> {
        catch_unwind(AssertUnwindSafe(run)).map_err(|payload| Self { payload })
    }

    /// Capture an unwind from one synchronous dispatch phase.
    pub(crate) fn capture(run: impl FnOnce()) -> Option<Self> {
        Self::try_run(run).err()
    }

    /// Keep transaction ordering deterministic when several phases panic.
    ///
    /// The earliest payload is resumed after mandatory cleanup; later payloads
    /// are reported but deliberately cannot replace it.
    pub(crate) fn preserve_first(
        first: &mut Option<Self>,
        candidate: Option<Self>,
        phase: &'static str,
    ) {
        let Some(candidate) = candidate else {
            return;
        };
        if first.is_none() {
            *first = Some(candidate);
        } else {
            // Panic payloads are arbitrary user values and may themselves
            // panic in Drop. Discarding a secondary payload normally could
            // therefore replace the first panic (or abort during unwind).
            // Only a known inert payload is released.
            candidate.retain();
            // Diagnostics are user code too. Retire the superseded payload
            // before reporting it, and contain a failing subscriber without
            // recursively reporting that diagnostic's own failure.
            Self::capture(|| {
                tracing::error!(
                    phase,
                    "dispatch phase panicked after an earlier phase; only the first panic is resumed"
                );
            })
            .retain();
        }
    }

    /// Continue unwinding with the captured payload.
    pub fn resume(self) -> ! {
        resume_unwind(self.payload)
    }
}

impl Retain for RoutePanic {
    /// Retain a failure already superseded or reported (ADR-0119).
    fn retain(self) {
        flui_foundation::panic::discard_payload(self.payload);
    }
}

impl fmt::Debug for RoutePanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutePanic").finish_non_exhaustive()
    }
}
