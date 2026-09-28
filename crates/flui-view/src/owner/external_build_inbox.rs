//! Shared external build queue and its panic-safe frame-wake delivery state.

use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicUsize, Ordering},
};

use flui_foundation::{ElementId, RebuildReasons};
use parking_lot::Mutex;

#[derive(Default)]
struct WakeState {
    active_threads: HashSet<std::thread::ThreadId>,
    reentrant_threads: HashSet<std::thread::ThreadId>,
}

fn discard_panic_payload(payload: Box<dyn Any + Send>) {
    if let Err(drop_panic) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
    {
        std::mem::forget(drop_panic);
    }
}

/// Shared external work and wake-retry state for one build owner.
#[derive(Default)]
pub(crate) struct ExternalBuildInbox {
    pending: Mutex<HashMap<ElementId, RebuildReasons>>,
    requested_wake: AtomicUsize,
    delivered_wake: AtomicUsize,
    wake_state: Mutex<WakeState>,
}

impl ExternalBuildInbox {
    pub(crate) fn lock(&self) -> parking_lot::MutexGuard<'_, HashMap<ElementId, RebuildReasons>> {
        self.pending.lock()
    }

    pub(crate) fn try_len(&self) -> Option<usize> {
        self.pending.try_lock().map(|pending| pending.len())
    }

    fn request_generation(&self) -> usize {
        let previous = self
            .requested_wake
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                generation.checked_add(1)
            })
            .expect("BUG: external build wake generation exhausted");
        previous + 1
    }

    fn pending_generation(&self) -> Option<usize> {
        let requested = self.requested_wake.load(Ordering::Acquire);
        (self.delivered_wake.load(Ordering::Acquire) < requested).then_some(requested)
    }

    /// Request a frame for fresh work or retry a wake that previously
    /// panicked after work was committed.
    ///
    /// Calls never wait behind an arbitrary external wake hook. Concurrent
    /// callers may race delivery, and each successful hook acknowledges only
    /// the work generation captured before that hook began. A same-thread
    /// reentrant caller cannot invoke the hook recursively, so its outer call
    /// makes at most one compensating attempt before returning or resuming the
    /// first panic.
    pub(crate) fn request_frame_if_needed(
        &self,
        has_fresh_work: bool,
        request_frame: Option<&(dyn Fn() + Send + Sync)>,
    ) {
        let Some(request_frame) = request_frame else {
            return;
        };
        if has_fresh_work {
            self.request_generation();
        }
        let Some(mut generation) = self.pending_generation() else {
            return;
        };

        let current_thread = std::thread::current().id();
        {
            let mut state = self.wake_state.lock();
            if !state.active_threads.insert(current_thread) {
                state.reentrant_threads.insert(current_thread);
                return;
            }
        }

        let mut first_panic = None;
        let mut may_compensate = true;
        loop {
            let wake = std::panic::catch_unwind(std::panic::AssertUnwindSafe(request_frame));
            let reentered = {
                let mut state = self.wake_state.lock();
                state.reentrant_threads.remove(&current_thread)
            };

            match wake {
                Ok(()) => {
                    self.delivered_wake.fetch_max(generation, Ordering::Release);
                }
                Err(payload) => {
                    if first_panic.is_none() {
                        first_panic = Some(payload);
                    } else {
                        discard_panic_payload(payload);
                    }
                }
            }

            let compensation = (reentered && may_compensate)
                .then(|| self.pending_generation())
                .flatten();
            if let Some(pending_generation) = compensation {
                may_compensate = false;
                generation = pending_generation;
                continue;
            }

            self.wake_state
                .lock()
                .active_threads
                .remove(&current_thread);
            if let Some(payload) = first_panic {
                std::panic::resume_unwind(payload);
            }
            return;
        }
    }
}
