//! Durable delivery of coalesced frame demand without holding a lock over a hook.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread::ThreadId;

#[derive(Default)]
struct State {
    pending: bool,
    token: Option<Arc<()>>,
    active: HashMap<ThreadId, bool>,
}

#[derive(Default)]
pub(crate) struct WakeDelivery {
    state: Mutex<State>,
}

impl WakeDelivery {
    /// Fresh demand supersedes in-flight acknowledgements. Identical demand
    /// retries unpaid delivery; a caller with no hook can never acknowledge it.
    /// Token allocations occur only on first delivery or overlapping fresh work.
    pub(crate) fn request(
        &self,
        fresh: impl FnOnce() -> bool,
        read_hook: impl FnOnce() -> Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        let thread = std::thread::current().id();
        let (mut token, hook) = {
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
            (token, hook)
        };
        let mut first_panic = None;
        let mut may_compensate = true;
        loop {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook()));
            let succeeded = outcome.is_ok();
            if let Err(payload) = outcome {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                } else {
                    // Opaque aggregate drop glue can panic twice and abort even
                    // inside catch_unwind. Retain secondary payloads rather than
                    // risk replacing the authoritative first failure.
                    std::mem::forget(payload);
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
                let retry = *reentered && may_compensate;
                *reentered = false;
                if retry && state.pending {
                    state.token.clone()
                } else {
                    state.active.remove(&thread);
                    None
                }
            };
            if let Some(next) = compensation {
                token = next;
                may_compensate = false;
            } else {
                if let Some(payload) = first_panic {
                    // A hook can uninstall itself before panicking. Its opaque
                    // capture bundle may have panicking aggregate drop glue,
                    // so retain our owning envelope before resuming the failure.
                    std::mem::forget(hook);
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
