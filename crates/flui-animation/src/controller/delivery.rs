//! Committed controller delivery debt and its reentrant FIFO drain.

use super::{
    AnimationController, AnimationControllerInner, AnimationStatus, ControllerDelivery,
    RetiredSources, RunDelivery, Terminal, ValueChange,
};
use flui_foundation::panic::RecoveryScope;

impl AnimationController {
    /// Commit delivery debt without invoking or retiring user code. Grouped
    /// admissions publish only after every participating owner is installed.
    pub(super) fn enqueue_delivery(
        status: AnimationStatus,
        delivery: Option<RunDelivery>,
        retired: RetiredSources,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
    ) -> bool {
        inner.enqueue_status_change(status);
        if let Some(delivery) = delivery {
            inner
                .pending_delivery
                .push_back(ControllerDelivery::Run(Terminal::new(delivery)));
        }
        inner
            .pending_delivery
            .push_back(ControllerDelivery::Retire(retired));
        if inner.settle_pending {
            inner.settle_pending = false;
            let generation = inner.run_generation;
            inner
                .pending_delivery
                .push_back(ControllerDelivery::SettleRun(generation));
        }
        let drain = !inner.delivering;
        inner.delivering = true;
        drop(inner);
        drain
    }

    pub(super) fn publish_delivery(
        &self,
        value_change: ValueChange,
        drain: bool,
        retirement: &mut RecoveryScope<'_>,
    ) {
        if value_change == ValueChange::Notify {
            retirement
                .run_with(|retirement| self.notifier.notify_listeners_with_recovery(retirement));
        }
        if drain {
            self.drain_delivery(retirement);
        }
    }

    /// The outermost caller drains accepted work; reentry only appends to it.
    fn drain_delivery(&self, retirement: &mut RecoveryScope<'_>) {
        let mut settled = false;
        loop {
            let delivery = {
                let mut inner = self.inner.borrow_mut();
                if settled
                    && matches!(
                        inner.pending_delivery.front(),
                        Some(ControllerDelivery::SettleRun(_))
                    )
                {
                    inner.delivering = false;
                    return;
                }
                let Some(delivery) = inner.pending_delivery.pop_front() else {
                    inner.delivering = false;
                    return;
                };
                delivery
            };
            match delivery {
                ControllerDelivery::RequestFrame => {
                    let probe = self.walk_probe();
                    if probe.has_run && !probe.parked {
                        let routes = self.inner.borrow().frame_routes.clone();
                        for route in routes {
                            if probe.live_running || route.requires_settlement(probe.behavior) {
                                route.request_frame(retirement);
                            }
                        }
                    }
                }
                ControllerDelivery::SettleRun(generation) => {
                    retirement.run_with(|retirement| {
                        settled |=
                            self.settle_run(generation, super::SettleReason::Clock, retirement);
                    });
                }
                ControllerDelivery::Status(status, listeners) => {
                    for (id, callback) in &listeners {
                        let live = {
                            let inner = self.inner.borrow_mut();
                            !inner.disposed
                                && inner
                                    .status_listeners
                                    .iter()
                                    .any(|(candidate, _)| candidate == id)
                        };
                        if live {
                            retirement.run_with(|recovery| callback.get().invoke(status, recovery));
                        }
                    }
                    for (_, callback) in listeners {
                        retirement.retire(callback);
                    }
                }
                ControllerDelivery::Run(delivery) => {
                    let retain = retirement.has_failure();
                    retirement.run(|| {
                        let delivery = delivery.into_inner();
                        if retain {
                            delivery.deliver_after_failure();
                        } else {
                            delivery.deliver();
                        }
                    });
                }
                ControllerDelivery::Retire(retired) => retirement.retire(retired),
            }
        }
    }
}
