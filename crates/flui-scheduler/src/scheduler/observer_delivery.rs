//! Observer containment and ordered lifecycle notification after state commit.

use crate::wake_delivery::FailureSignal;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
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

pub(super) fn preserve_first_failure(
    custody: &FailureCustody<'_>,
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    result: std::thread::Result<()>,
) {
    if let Err(payload) = result {
        custody.signal.set(true);
        if first.is_some() {
            drop(payload);
        } else {
            *first = Some(payload);
        }
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
        let mut first = None;
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
                preserve_first_failure(
                    &custody,
                    &mut first,
                    catch_unwind(AssertUnwindSafe(|| scheduler.request_frame())),
                );
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
                    preserve_first_failure(
                        &custody,
                        &mut first,
                        catch_unwind(AssertUnwindSafe(|| callback(state))),
                    );
                }
                // A callback may remove its last registered owner while running.
                // Preserve an earlier failure before retiring its opaque captures.
                if first.is_some() {
                    std::mem::forget(callback);
                } else {
                    preserve_first_failure(
                        &custody,
                        &mut first,
                        catch_unwind(AssertUnwindSafe(|| drop(callback))),
                    );
                }
            }
        }
        if let Some(first) = first {
            std::panic::resume_unwind(first);
        }
    }
}
