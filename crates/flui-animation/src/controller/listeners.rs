//! Value and status listener registration.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::panic::RecoveryScope;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};

use super::{
    AnimationController, AnimationControllerInner, ControllerDelivery, Opaque, RetiredSources,
};
use crate::StatusObserver;

impl AnimationController {
    pub(super) fn register_status_listener(&self, callback: StatusObserver) -> Option<ListenerId> {
        let mut callback = Opaque::new(callback);
        let inner = self.inner.borrow_mut();
        if inner.disposed {
            let mut retired = RetiredSources::new();
            retired.callbacks.push(callback);
            Self::retire_after(inner, retired);
            return None;
        }
        let (id, mut inner) = Self::mint_listener_id(inner);
        inner.status_listeners.push((id, callback.take()));
        Some(id)
    }

    pub(super) fn withdraw_status_listener(
        source: &RefCell<AnimationControllerInner>,
        id: ListenerId,
        _recovery: &mut RecoveryScope<'_>,
    ) -> Option<Opaque<StatusObserver>> {
        let mut retired;
        {
            let mut inner = source.borrow_mut();
            retired = inner
                .status_listeners
                .iter()
                .position(|(candidate, _)| *candidate == id)
                .map(|index| Opaque::new(inner.status_listeners.remove(index).1));
            if inner.delivering
                && let Some(callback) = retired.take()
            {
                let mut sources = RetiredSources::new();
                sources.callbacks.push(callback);
                inner
                    .pending_delivery
                    .push_back(ControllerDelivery::Retire(sources));
            }
        }
        retired
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
            let (_id, inner) = Self::mint_listener_id(inner);
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
            let (_id, inner) = Self::mint_listener_id(inner);
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
