//! Value and status listener registration.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::panic::RecoveryScope;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};

use crate::StatusObserver;
use crate::channels::Withdrawn;
use crate::status::AnimationStatus;

use super::{
    AnimationController, AnimationControllerInner, ControllerDelivery, Opaque, RetiredSources,
};

impl AnimationController {
    pub(super) fn register_status_listener(&self, listener: StatusObserver) -> Option<ListenerId> {
        let mut listener = Opaque::new(listener);
        let inner = self.inner.borrow_mut();
        if inner.disposed {
            let mut retired = RetiredSources::new();
            retired.refused_status.push(listener);
            Self::retire_after(inner, retired);
            return None;
        }
        // Admission runs with the state borrow released: the channel's
        // exhausted-mint panic reaches reentering hooks while the public
        // source stays borrowable (ADR-0180). A dispose that lands between
        // the check and the admission is answered by the channel's own
        // disposed gate, which admits nothing and names no ticket.
        drop(inner);
        let observer = listener.take();
        let id = self
            .inner
            .borrow()
            .status_channel
            .add_with_recovery(Rc::new(move |status, recovery| observer(*status, recovery)));
        Some(id)
    }

    pub(super) fn withdraw_status_listener(
        source: &RefCell<AnimationControllerInner>,
        id: ListenerId,
        recovery: &mut RecoveryScope<'_>,
    ) -> Option<Withdrawn<AnimationStatus>> {
        let mut inner = source.borrow_mut();
        let taken = inner.status_channel.take_callback(id);
        if inner.delivering
            && let Some(callback) = taken
        {
            // A delivery round is draining: its tail retires the capture.
            let mut sources = RetiredSources::new();
            sources.status_callbacks.push(callback);
            inner
                .pending_delivery
                .push_back(ControllerDelivery::Retire(sources));
            return None;
        }
        drop(inner);
        source.borrow().status_channel.inherit_failure(recovery);
        taken
    }
}

impl Listenable for AnimationController {
    fn listen(&self, listener: ListenerCallback) -> flui_foundation::Subscription {
        let mut callback = Opaque::new(listener);
        let inner = self.inner.borrow_mut();
        if inner.disposed {
            // A disposed controller refuses admission: the callback retires
            // under this controller's own custody, and the caller holds the
            // inert subscription (ADR-0180's refusal contract).
            let callback = callback.take();
            let mut retired = RetiredSources::new();
            retired.value_callbacks.push(Opaque::new(
                flui_foundation::notifier_generic::NotificationCallback::user(Rc::new(
                    move |(): &()| callback(),
                )),
            ));
            Self::retire_after(inner, retired);
            return flui_foundation::Subscription::inert();
        }
        drop(inner);
        self.notifier.listen(callback.take())
    }
    fn listen_with_recovery(
        &self,
        relay: flui_foundation::notifier::ListenerObserver,
    ) -> flui_foundation::Subscription {
        let mut relay = Opaque::new(relay);
        let inner = self.inner.borrow_mut();
        if inner.disposed {
            let relay = relay.take();
            let mut retired = RetiredSources::new();
            retired.value_callbacks.push(Opaque::new(
                flui_foundation::notifier_generic::NotificationCallback::relay(Rc::new(
                    move |(): &(), recovery| relay(recovery),
                )),
            ));
            Self::retire_after(inner, retired);
            // The refusal admitted no registration: the subscription is inert.
            return flui_foundation::Subscription::inert();
        }
        drop(inner);
        self.notifier.listen_with_recovery(relay.take())
    }
}
