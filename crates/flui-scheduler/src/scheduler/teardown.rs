//! Terminal scheduler retirement after its last strong UI reference is gone.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use crate::wake_delivery::FailureSignal;

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
        let payload = failure.borrow_mut().take();
        payload
    }
}

struct Retirement<'a> {
    preserve_failure: bool,
    signal: &'a FailureSignal,
    first: Option<RetirePanic>,
}

impl Retirement<'_> {
    fn observe(&mut self, payload: RetirePanic) {
        self.signal.set(true);
        if self.preserve_failure || self.first.is_some() {
            flui_foundation::panic::retain_opaque_payload(payload);
        } else {
            self.first = Some(payload);
        }
    }

    fn retire<T>(&mut self, value: T) {
        if self.preserve_failure
            || self.first.is_some()
            || self.signal.get()
            || std::thread::panicking()
        {
            std::mem::forget(value);
        } else if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(value))) {
            self.observe(payload);
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
        let transient = std::mem::take(self.callbacks.transient.get_mut());
        let persistent = std::mem::take(self.callbacks.persistent.get_mut());
        let post_frame = self
            .callbacks
            .post_frame
            .get_mut()
            .detach_unclaimed_for_retirement();
        let microtasks = std::mem::take(self.callbacks.microtasks.get_mut());
        let listeners = std::mem::take(self.callbacks.lifecycle_listeners.get_mut());
        let timings = std::mem::take(self.binding.timings_callbacks.get_mut());
        let tasks = self.task_queue.detach_for_retirement();
        let hook = self.wake.on_frame_scheduled.lock().take();
        let waiters = self.frame.completion_waiters.get_mut().drain();
        let mut delivery = crate::completion_wake::WakeBatch::with_failure_signal(
            "scheduler teardown",
            incoming,
            signal,
        );

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
            first: None,
        };
        if let Some(payload) = delivery.into_failure() {
            retirement.observe(payload);
        }
        for value in transient {
            retirement.retire(value);
        }
        for value in persistent {
            retirement.retire(value);
        }
        for value in post_frame {
            retirement.retire(value);
        }
        for value in microtasks {
            retirement.retire(value);
        }
        for value in listeners {
            retirement.retire(value);
        }
        for value in timings {
            retirement.retire(value);
        }
        for value in tasks {
            retirement.retire(value);
        }
        retirement.retire(hook);

        if let Some(payload) = retirement.first {
            if let Some(release) = &execution {
                *release.failure.borrow_mut() = Some(payload);
            } else {
                // Ordinary Drop cannot re-raise into a potentially unwinding
                // owner. Explicit execution release receives the failure above.
                crate::completion_wake::retain_reported(payload, "scheduler teardown");
            }
        }
    }
}
