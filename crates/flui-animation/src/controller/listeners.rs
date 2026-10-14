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
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        let mut observer = Opaque::new(observer);
        let inner = self.inner.borrow_mut();
        if inner.disposed {
            let (id, inner) = Self::mint_listener_id(inner);
            let observer = observer.take();
            let mut retired = RetiredSources::new();
            retired.value_callbacks.push(Opaque::new(
                flui_foundation::notifier_generic::NotificationCallback::relay(Rc::new(
                    move |(): &(), recovery| observer(recovery),
                )),
            ));
            Self::retire_after(inner, retired);
            return id;
        }
        drop(inner);
        self.notifier.add_observer(observer.take())
    }
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        let mut callback = Opaque::new(callback);
        let inner = self.inner.borrow_mut();
        if inner.disposed {
            let (id, inner) = Self::mint_listener_id(inner);
            let callback = callback.take();
            let mut retired = RetiredSources::new();
            retired.value_callbacks.push(Opaque::new(
                flui_foundation::notifier_generic::NotificationCallback::user(Rc::new(
                    move |(): &()| callback(),
                )),
            ));
            Self::retire_after(inner, retired);
            return id;
        }
        drop(inner);
        self.notifier.add_listener(callback.take())
    }

    fn remove_listener(&self, id: ListenerId) {
        let callback = self.notifier.take_listener(id);
        self.retire_value_callbacks(callback);
    }

    fn remove_all_listeners(&self) {
        self.retire_value_callbacks(self.notifier.take_listeners());
    }
}
