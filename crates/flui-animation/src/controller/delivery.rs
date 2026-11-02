//! Committed controller delivery debt and its reentrant FIFO drain.

use super::{
    AnimationController, AnimationControllerInner, AnimationStatus, ControllerDelivery,
    RetiredSources, RetirementAction, RunDelivery, Terminal, ValueChange,
};
use flui_foundation::panic::{FailureClass, RecoveryScope};

impl AnimationController {
    /// Logical teardown has already committed. Physical custody waits for the
    /// active value, status, continuation or waiter round to finish catching its
    /// first failure. An idle controller owns and drains this cleanup itself.
    pub(crate) fn retire_with_delivery(
        &self,
        action: impl FnOnce(&mut RecoveryScope<'_>) + 'static,
        recovery: &mut RecoveryScope<'_>,
    ) {
        let action = Terminal::new(Box::new(action) as RetirementAction);
        self.enqueue_and_drain(ControllerDelivery::RetireOwned(action), recovery);
    }

    /// A registry has a known custody type, so waiting for the same delivery
    /// round does not require allocating an erased cleanup closure.
    pub(crate) fn retire_registry_with_delivery(
        &self,
        registry: Terminal<crate::Vsync>,
        recovery: &mut RecoveryScope<'_>,
    ) {
        self.enqueue_and_drain(ControllerDelivery::RetireRegistry(registry), recovery);
    }

    fn enqueue_and_drain(&self, delivery: ControllerDelivery, recovery: &mut RecoveryScope<'_>) {
        let mut inner = self.inner.borrow_mut();
        inner.pending_delivery.push_back(delivery);
        let drain = !inner.delivering;
        inner.delivering = true;
        drop(inner);
        if drain {
            self.drain_delivery(recovery);
        }
    }

    /// Commit delivery debt without invoking or retiring user code. Grouped
    /// admissions publish only after every participating owner is installed.
    pub(super) fn enqueue_delivery(
        status: AnimationStatus,
        delivery: Option<RunDelivery>,
        retired: RetiredSources,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
    ) -> bool {
        #[cfg(feature = "diagnostics")]
        if let Some(transition) =
            super::diagnostics::Transition::capture(&mut inner, delivery.as_ref())
        {
            inner
                .pending_delivery
                .push_back(ControllerDelivery::Diagnostic(transition));
        }
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
        retirement.run_classified(FailureClass::Delivery, |retirement| {
            if value_change == ValueChange::Notify {
                let mut profile = None;
                self.notifier
                    .notify_listeners_with_recovery_before(retirement, || {
                        // The round already owns its membership and capture custody.
                        // Span creation/entry may add, remove or dispose listeners.
                        profile = Some(crate::observability::ProfileSpan::listeners());
                    });
                if let Some(profile) = profile {
                    profile.close(retirement);
                }
            }
            if drain {
                self.drain_delivery(retirement);
            }
        });
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
                        Some(ControllerDelivery::SettleRun(generation))
                            if inner.accepts_settlement(*generation, &super::SettleReason::Clock)
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
                #[cfg(feature = "diagnostics")]
                ControllerDelivery::Diagnostic(transition) => transition.emit(),
                ControllerDelivery::RequestFrame => {
                    let probe = self.walk_probe();
                    if probe.has_run {
                        let routes = self.inner.borrow().frame_routes.clone();
                        for route in routes {
                            if route.requires_sample(&probe) {
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
                    let profile = if listeners.is_empty() {
                        None
                    } else {
                        Some(crate::observability::ProfileSpan::listeners())
                    };
                    for (id, callback) in &listeners {
                        let live = {
                            let inner = self.inner.borrow();
                            !inner.disposed
                                && inner
                                    .status_listeners
                                    .iter()
                                    .any(|(candidate, _)| candidate == id)
                        };
                        if live {
                            retirement.run_classified(FailureClass::Delivery, |recovery| {
                                (callback.get())(status, recovery);
                            });
                        }
                    }
                    for (_, callback) in listeners {
                        retirement.retire_classified(FailureClass::Delivery, callback);
                    }
                    if let Some(profile) = profile {
                        profile.close(retirement);
                    }
                }
                ControllerDelivery::Run(delivery) => {
                    let profile = crate::observability::ProfileSpan::listeners();
                    retirement.run_classified(FailureClass::Delivery, |_| {
                        delivery.into_inner().deliver();
                    });
                    profile.close(retirement);
                }
                ControllerDelivery::Retire(retired) => {
                    retirement.retire_classified(FailureClass::Delivery, retired);
                }
                ControllerDelivery::RetireRegistry(registry) => {
                    retirement.retire_classified(FailureClass::Delivery, registry);
                }
                ControllerDelivery::RetireOwned(action) => {
                    if retirement.has_failure() {
                        retirement.retire(action);
                    } else {
                        retirement.run_classified(FailureClass::Unknown, |recovery| {
                            action.into_inner()(recovery);
                        });
                    }
                }
            }
        }
    }
}
