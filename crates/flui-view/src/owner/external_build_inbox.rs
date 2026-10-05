//! Shared external build queue and its panic-safe frame-wake delivery state.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use flui_foundation::{ElementId, RebuildReasons};
use parking_lot::Mutex;

#[derive(Default)]
struct WakeState {
    active_threads: HashSet<std::thread::ThreadId>,
    reentrant_threads: HashSet<std::thread::ThreadId>,
}

type WakeToken = Arc<AtomicBool>;

/// Shared external work and wake-retry state for one build owner.
#[derive(Default)]
pub(crate) struct ExternalBuildInbox {
    closed: AtomicBool,
    pending: Mutex<HashMap<ElementId, RebuildReasons>>,
    current_wake: Mutex<Option<WakeToken>>,
    wake_state: Mutex<WakeState>,
}

impl ExternalBuildInbox {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        let mut pending = self.pending.lock();
        self.closed.store(true, Ordering::Release);
        pending.clear();
        drop(pending);
        let token = self.current_wake.lock().take();
        if let Some(token) = token {
            token.store(true, Ordering::Release);
        }
    }
    pub(crate) fn lock(&self) -> parking_lot::MutexGuard<'_, HashMap<ElementId, RebuildReasons>> {
        self.pending.lock()
    }

    pub(crate) fn try_len(&self) -> Option<usize> {
        self.pending.try_lock().map(|pending| pending.len())
    }

    fn request_token(&self) -> WakeToken {
        let token = Arc::new(AtomicBool::new(false));
        *self.current_wake.lock() = Some(Arc::clone(&token));
        token
    }

    fn pending_token(&self) -> Option<WakeToken> {
        self.current_wake
            .lock()
            .as_ref()
            .filter(|token| !token.load(Ordering::Acquire))
            .cloned()
    }

    /// Request a frame for fresh work or retry a wake that previously
    /// panicked after work was committed.
    ///
    /// Calls never wait behind an arbitrary external wake hook. Concurrent
    /// callers may race delivery, and each successful hook acknowledges only
    /// the work token captured before that hook began. A same-thread
    /// reentrant caller cannot invoke the hook recursively, so its outer call
    /// makes at most one compensating attempt before returning or resuming the
    /// first panic.
    pub(crate) fn request_frame_if_needed(
        &self,
        has_fresh_work: bool,
        request_frame: Option<&(dyn Fn() + Send + Sync)>,
    ) {
        if self.is_closed() {
            return;
        }
        let Some(request_frame) = request_frame else {
            return;
        };
        let requested = has_fresh_work.then(|| self.request_token());
        let Some(mut token) = requested.or_else(|| self.pending_token()) else {
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
                    token.store(true, Ordering::Release);
                }
                Err(payload) => {
                    if first_panic.is_none() {
                        first_panic = Some(payload);
                    } else {
                        flui_foundation::panic::retain_opaque_payload(payload);
                    }
                }
            }

            let compensation = (reentered && may_compensate)
                .then(|| self.pending_token())
                .flatten();
            if let Some(pending_token) = compensation {
                may_compensate = false;
                token = pending_token;
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
