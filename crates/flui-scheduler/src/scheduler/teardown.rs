//! Terminal scheduler retirement after its last strong UI reference is gone.

use crate::wake_delivery::FailureSignal;
use flui_foundation::panic::FirstPanic;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::{SchedulerClosed, SchedulerInner, UpdateScheduler};
use crate::async_driver::RetirePanic;

pub(super) struct ExecutionRelease {
    pub(super) preserve_failure: bool,
    pub(super) failure_signal: Arc<FailureSignal>,
    pub(super) failure: Rc<RefCell<Option<RetirePanic>>>,
}

impl UpdateScheduler {
    /// Only a terminal release allocates a result sink. Actual destruction
    /// happens after Rc closes weak upgrades, before any user cleanup runs.
    pub(crate) fn release_execution(
        self,
        preserve_failure: bool,
        failure_signal: Arc<FailureSignal>,
    ) -> Option<RetirePanic> {
        if Rc::strong_count(&self.inner) != 1 {
            drop(self);
            return None;
        }
        let failure = Rc::new(RefCell::new(None));
        *self.inner.execution_release.borrow_mut() = Some(ExecutionRelease {
            preserve_failure,
            failure_signal,
            failure: Rc::clone(&failure),
        });
        drop(self);
        failure.borrow_mut().take()
    }
}

struct Retirement<'a> {
    /// A failure was already unwinding or held when retirement began: it stays
    /// first, and every failure retirement raises is discarded (ADR-0281).
    preserve_failure: bool,
    signal: &'a FailureSignal,
    first: FirstPanic,
}

impl Retirement<'_> {
    fn observe(&mut self, payload: RetirePanic) {
        self.signal.set(true);
        self.first.record(payload);
    }

    fn retire<T>(&mut self, value: T) {
        // Contained after any earlier failure too: a destructor panic competes
        // and the first failure stays (ADR-0281).
        if self.first.contain(move || drop(value)).is_none() {
            self.signal.set(true);
        }
    }

    /// The failure retirement raised first, unless an earlier one stays first;
    /// then the held payload is destroyed under containment as `first` drops.
    fn into_failure(self) -> Option<RetirePanic> {
        if self.preserve_failure {
            None
        } else {
            self.first.into_payload()
        }
    }
}

impl Drop for SchedulerInner {
    fn drop(&mut self) {
        self.wake
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        let execution = self.execution_release.get_mut().take();
        let local_signal = FailureSignal::default();
        let signal = execution
            .as_ref()
            .map_or(&local_signal, |release| release.failure_signal.as_ref());
        let incoming = std::thread::panicking()
            || execution
                .as_ref()
                .is_some_and(|release| release.preserve_failure);

        // Detach every opaque collection before invocation or retirement. Rc's
        // strong count is already zero: callback reentry cannot revive the UI
        // scheduler or register work into collections being destroyed.
        let post_frame = self
            .callbacks
            .post_frame
            .get_mut()
            .detach_unclaimed_for_retirement();
        let listeners = std::mem::take(self.callbacks.lifecycle_listeners.get_mut());
        let hook = self.wake.on_frame_scheduled.lock().take();
        let waiters = self.frame.completion_waiters.get_mut().drain();
        let mut delivery =
            crate::completion_wake::WakeBatch::with_failure_signal("scheduler teardown", signal);

        for notifier in waiters {
            let Some(state) = notifier.state.upgrade() else {
                continue;
            };
            let waker = {
                let mut state = state.lock();
                state.completed = Some(Err(SchedulerClosed));
                state.waker.take()
            };
            if let Some(waker) = waker {
                delivery.wake(waker);
            }
        }

        let mut retirement = Retirement {
            preserve_failure: incoming,
            signal,
            first: FirstPanic::new(),
        };
        if let Some(payload) = delivery.into_failure() {
            retirement.observe(payload);
        }
        for value in post_frame {
            retirement.retire(value);
        }
        for value in listeners {
            retirement.retire(value);
        }
        retirement.retire(hook);

        if let Some(payload) = retirement.into_failure() {
            if let Some(release) = &execution {
                *release.failure.borrow_mut() = Some(payload);
            } else {
                // Ordinary Drop cannot re-raise into a potentially unwinding
                // owner. Explicit execution release receives the failure above.
                crate::completion_wake::discard_reported(payload, "scheduler teardown");
            }
        }
    }
}
