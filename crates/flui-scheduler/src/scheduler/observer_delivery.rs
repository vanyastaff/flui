//! Observer containment and ordered lifecycle notification after state commit.

use crate::wake_delivery::FailureSignal;
use flui_foundation::panic::FirstPanic;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::{Arc, Weak};

use super::{AppLifecycleState, UpdateScheduler};

#[derive(Default)]
pub(super) struct LifecycleDelivery {
    pending: RefCell<VecDeque<LifecycleTransition>>,
    draining: Cell<bool>,
}

struct LifecycleTransition {
    state: AppLifecycleState,
    request_frame: bool,
}

struct DrainGuard<'a>(&'a Cell<bool>);

/// Share containment with cancellation and nested execution, even between frames.
pub(super) struct FailureCustody<'a> {
    scheduler: &'a UpdateScheduler,
    signal: Arc<FailureSignal>,
    previous: Option<Weak<FailureSignal>>,
}

impl<'a> FailureCustody<'a> {
    pub(super) fn new(scheduler: &'a UpdateScheduler) -> Self {
        let previous = scheduler.inner.execution_failure.borrow().clone();
        let signal = previous
            .as_ref()
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| Arc::new(FailureSignal::default()));
        let receipt = Some(Arc::downgrade(&signal));
        scheduler
            .inner
            .execution_failure
            .borrow_mut()
            .clone_from(&receipt);
        scheduler
            .inner
            .wake
            .wake_delivery
            .bind_failure_signal(receipt.clone());
        let owner = scheduler.inner.execution_owner.borrow().upgrade();
        if let Some(owner) = owner {
            owner.bind_owner_failure_signal(receipt);
        }
        Self {
            scheduler,
            signal,
            previous,
        }
    }
}

impl Drop for FailureCustody<'_> {
    fn drop(&mut self) {
        self.scheduler
            .inner
            .execution_failure
            .borrow_mut()
            .clone_from(&self.previous);
        self.scheduler
            .inner
            .wake
            .wake_delivery
            .bind_failure_signal(self.previous.clone());
        let owner = self.scheduler.inner.execution_owner.borrow().upgrade();
        if let Some(owner) = owner {
            owner.bind_owner_failure_signal(self.previous.take());
        }
    }
}

impl Drop for DrainGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// Run `step` contained: a failure raises the custody's signal, and the first
/// one stays the one resumed (ADR-0281).
pub(super) fn contain_observer(
    custody: &FailureCustody<'_>,
    first: &mut FirstPanic,
    step: impl FnOnce(),
) {
    if first.contain(step).is_none() {
        custody.signal.set(true);
    }
}

impl LifecycleDelivery {
    pub(super) fn deliver(
        &self,
        scheduler: &UpdateScheduler,
        state: AppLifecycleState,
        request_frame: bool,
    ) {
        self.pending.borrow_mut().push_back(LifecycleTransition {
            state,
            request_frame,
        });
        if self.draining.replace(true) {
            return;
        }
        let _guard = DrainGuard(&self.draining);
        let custody = FailureCustody::new(scheduler);
        let mut first = FirstPanic::new();
        loop {
            let next = self.pending.borrow_mut().pop_front();
            let Some(LifecycleTransition {
                state,
                request_frame,
            }) = next
            else {
                break;
            };
            if request_frame {
                // The frame request runs a wake hook; delivery goes on.
                contain_observer(&custody, &mut first, || scheduler.request_frame());
            }
            let listeners: Vec<_> = scheduler
                .inner
                .callbacks
                .lifecycle_listeners
                .borrow()
                .iter()
                .map(|listener| (listener.id, listener.callback.clone()))
                .collect();
            for (id, callback) in listeners {
                let live = scheduler
                    .inner
                    .callbacks
                    .lifecycle_listeners
                    .borrow()
                    .iter()
                    .any(|listener| listener.id == id);
                if live {
                    // A listener's panic does not stop the remaining listeners.
                    contain_observer(&custody, &mut first, || callback(state));
                }
                // A callback may remove its last registered owner while running.
                // Its captures retire contained; an earlier failure stays first
                // (ADR-0281).
                contain_observer(&custody, &mut first, move || drop(callback));
            }
        }
        first.resume();
    }
}
