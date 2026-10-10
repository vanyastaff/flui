//! Durable delivery of coalesced frame demand without holding a lock over a hook.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

#[derive(Default)]
struct State {
    pending: bool,
    token: Option<Arc<()>>,
    active: HashMap<ThreadId, bool>,
    failure_signal: Option<Weak<FailureSignal>>,
}

/// Failure metadata only: no payload, callback or owner execution authority.
#[derive(Debug, Default)]
pub(crate) struct FailureSignal(AtomicBool);

impl FailureSignal {
    pub(crate) fn get(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(crate) fn set(&self, failed: bool) {
        self.0.store(failed, Ordering::Release);
    }
}

#[derive(Default)]
pub(crate) struct WakeDelivery {
    state: Mutex<State>,
}

impl WakeDelivery {
    pub(crate) fn bind_failure_signal(&self, signal: Option<Weak<FailureSignal>>) {
        self.state.lock().failure_signal = signal;
    }
    /// Fresh demand supersedes in-flight acknowledgements. Identical demand
    /// retries unpaid delivery; a caller with no hook can never acknowledge it.
    /// Token allocations occur only on first delivery or overlapping fresh work.
    pub(crate) fn request(
        &self,
        fresh: impl FnOnce() -> bool,
        read_hook: impl FnMut() -> Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        self.request_preserving_failure(false, fresh, read_hook);
    }

    /// A recovery delivery borrows an earlier operation's failure custody.
    /// A successful hook can uninstall itself, so its captures also need
    /// retention even when this delivery catches no new failure.
    pub(crate) fn request_preserving_failure(
        &self,
        preserve_failure: bool,
        fresh: impl FnOnce() -> bool,
        mut read_hook: impl FnMut() -> Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        let preserve_failure = preserve_failure || std::thread::panicking();
        let thread = std::thread::current().id();
        let (mut token, hook, failure_signal) = {
            let mut state = self.state.lock();
            if fresh() {
                state.pending = true;
                if !state.active.is_empty() {
                    state.token = Some(Arc::new(()));
                }
            }
            if !state.pending {
                return;
            }
            // Read the installed hook within the demand transition. Otherwise
            // installation can retry before a caller using an old None snapshot
            // records its demand, leaving that demand asleep indefinitely.
            let Some(hook) = read_hook() else {
                return;
            };
            if let Some(reentered) = state.active.get_mut(&thread) {
                *reentered = true;
                drop(state);
                return;
            }
            let token = Arc::clone(state.token.get_or_insert_with(|| Arc::new(())));
            state.active.insert(thread, false);
            let failure_signal = state.failure_signal.as_ref().and_then(Weak::upgrade);
            (token, hook, failure_signal)
        };
        let mut first_panic = None;
        // Keep both owning envelopes until delivery bookkeeping is closed.
        // Switching hooks must not run the displaced callback's capture Drop.
        let mut compensation_hook: Option<Arc<dyn Fn() + Send + Sync>> = None;
        loop {
            let current_hook = compensation_hook.as_ref().unwrap_or(&hook);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| current_hook()));
            let succeeded = outcome.is_ok();
            if let Err(payload) = outcome {
                if let Some(signal) = &failure_signal {
                    signal.set(true);
                }
                if first_panic.is_none() {
                    first_panic = Some(payload);
                } else {
                    // Opaque aggregate drop glue can panic twice and abort even
                    // inside catch_unwind. Retain secondary payloads rather than
                    // risk replacing the authoritative first failure.
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
            }
            let compensation = {
                let mut state = self.state.lock();
                if succeeded
                    && state
                        .token
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &token))
                {
                    state.pending = false;
                }
                let reentered = state
                    .active
                    .get_mut(&thread)
                    .expect("BUG: active delivery owns its thread entry");
                let retry = *reentered && compensation_hook.is_none();
                *reentered = false;
                let next = if retry && state.pending {
                    // Hook installation is itself a retry opportunity. Select
                    // its current callback rather than repeat a displaced one.
                    read_hook().map(|hook| {
                        (
                            Arc::clone(
                                state
                                    .token
                                    .as_ref()
                                    .expect("BUG: active delivery has a receipt"),
                            ),
                            hook,
                        )
                    })
                } else {
                    None
                };
                if next.is_none() {
                    state.active.remove(&thread);
                }
                next
            };
            if let Some((next, next_hook)) = compensation {
                token = next;
                compensation_hook = Some(next_hook);
            } else {
                if preserve_failure {
                    std::mem::forget(hook);
                    std::mem::forget(compensation_hook);
                    if let Some(payload) = first_panic {
                        flui_foundation::panic::retain_opaque_payload(payload);
                    }
                    return;
                }
                if let Some(payload) = first_panic {
                    // A hook can uninstall itself before panicking. Its opaque
                    // capture bundle may have panicking aggregate drop glue,
                    // so retain our owning envelope before resuming the failure.
                    std::mem::forget(hook);
                    std::mem::forget(compensation_hook);
                    std::panic::resume_unwind(payload);
                }
                if failure_signal.as_ref().is_some_and(|signal| signal.get()) {
                    std::mem::forget(hook);
                    std::mem::forget(compensation_hook);
                    return;
                }
                // Normal retirement can itself raise the first failure. Retire
                // the initial envelope after removing the active entry; if it
                // panics, retain the compensation envelope before resuming it.
                if let Err(payload) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(hook)))
                {
                    if let Some(signal) = &failure_signal {
                        signal.set(true);
                    }
                    std::mem::forget(compensation_hook);
                    std::panic::resume_unwind(payload);
                }
                if failure_signal.as_ref().is_some_and(|signal| signal.get()) {
                    std::mem::forget(compensation_hook);
                } else if let Err(payload) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        drop(compensation_hook)
                    }))
                {
                    if let Some(signal) = &failure_signal {
                        signal.set(true);
                    }
                    std::panic::resume_unwind(payload);
                }
                return;
            }
        }
    }

    /// Consuming a demand and clearing its issuance latch are one transition.
    pub(crate) fn consume(&self, clear_latch: impl FnOnce()) {
        let mut state = self.state.lock();
        clear_latch();
        state.pending = false;
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn is_unlocked(&self) -> bool {
        self.state.try_lock().is_some()
    }
}
