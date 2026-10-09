//! `ProxyAnimation` - wraps another animation, allowing hot-swapping.

use crate::animation::{
    Animation, ParentSubscription, Retirement, StatusCallback, Terminal, link_parent,
};
use crate::status::AnimationStatus;
use flui_foundation::panic::RecoveryScope;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

/// An animation that can be hot-swapped for another animation.
///
/// `ProxyAnimation` forwards all calls to its parent animation, but allows
/// the parent to be changed dynamically. This is useful when you need to
/// change the animation being used without recreating the entire widget tree.
///
/// # Examples
///
/// ```
/// use flui_animation::{ProxyAnimation, AnimationController, Animation};
/// use flui_scheduler::UpdateScheduler;
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller1 = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
///
/// let proxy = ProxyAnimation::new(controller1.clone() as Rc<dyn Animation<f64>>);
///
/// // Later, swap to a different animation
/// let controller2 = Rc::new(AnimationController::builder(Duration::from_millis(500)).build());
/// proxy.set_parent(controller2 as Rc<dyn Animation<f64>>);
/// ```
#[derive(Clone)]
pub struct ProxyAnimation<T>
where
    T: Clone + 'static,
{
    inner: Rc<ProxyOwner<T>>,
}

struct ProxyOwner<T: Clone + 'static> {
    parent: RefCell<Terminal<Rc<dyn Animation<T>>>>,
    notifier: Terminal<Rc<ChangeNotifier>>,
    parent_sub: RefCell<Terminal<Rc<ParentSubscription>>>,
    status_listeners: Terminal<Rc<flui_foundation::Notifier<AnimationStatus>>>,
    status_sub: RefCell<Terminal<Rc<ParentSubscription>>>,
    delivering: Cell<bool>,
    pending: RefCell<VecDeque<ProxyDelivery<T>>>,
    retired: RefCell<
        Vec<Terminal<Rc<flui_foundation::notifier_generic::NotificationCallback<AnimationStatus>>>>,
    >,
}

enum ProxyDelivery<T: Clone + 'static> {
    Retire {
        parent: Terminal<Rc<dyn Animation<T>>>,
        previous: Terminal<Rc<dyn Animation<T>>>,
        value_sub: Terminal<Rc<ParentSubscription>>,
        status_sub: Terminal<Rc<ParentSubscription>>,
    },
    Value,
    Status(AnimationStatus, Vec<ListenerId>),
}

impl<T: Clone + 'static> Drop for ProxyOwner<T> {
    fn drop(&mut self) {
        let mut retirement = Retirement::new();
        self.notifier.inherit_failure(&mut retirement.scope());
        self.status_listeners
            .inherit_failure(&mut retirement.scope());
        let parent = self.parent.get_mut().withdraw();
        let notifier = self.notifier.withdraw();
        let value_sub = self.parent_sub.get_mut().withdraw();
        let status_sub = self.status_sub.get_mut().withdraw();
        let listeners = self.status_listeners.withdraw();
        let pending = self.pending.get_mut().drain(..).collect::<Vec<_>>();
        let retired = std::mem::take(self.retired.get_mut());
        let callbacks = listeners.dispose_and_take_callbacks();
        let value_callbacks = notifier.dispose_and_take_listeners();
        value_sub.detach(&mut retirement.scope());
        status_sub.detach(&mut retirement.scope());
        retirement.retire(value_sub);
        retirement.retire(status_sub);
        retirement.retire(parent);
        for delivery in pending {
            retirement.retire(delivery);
        }
        for callback in retired {
            retirement.retire(callback);
        }
        for callback in callbacks {
            retirement.retire(callback);
        }
        for callback in value_callbacks {
            retirement.retire(Terminal::new(callback));
        }
        retirement.retire(listeners);
        retirement.retire(notifier);
        retirement.finish();
    }
}

impl<T> ProxyAnimation<T>
where
    T: Clone + fmt::Debug + 'static,
{
    /// Create a new proxy animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The initial parent animation
    #[must_use]
    pub fn new(parent: Rc<dyn Animation<T>>) -> Self {
        let parent = Terminal::new(parent);
        let notifier = Rc::new(ChangeNotifier::new());
        let parent_sub = link_parent(&parent, &notifier);
        let status_listeners = Rc::new(flui_foundation::Notifier::new());
        let status_sub =
            crate::animation::link_parent_status(&parent, &status_listeners, |status| status);
        Self {
            inner: Rc::new(ProxyOwner {
                parent: RefCell::new(parent),
                notifier: Terminal::new(notifier),
                parent_sub: RefCell::new(Terminal::new(parent_sub)),
                status_listeners: Terminal::new(status_listeners),
                status_sub: RefCell::new(Terminal::new(status_sub)),
                delivering: Cell::new(false),
                pending: RefCell::new(VecDeque::new()),
                retired: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Get the current parent animation.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Rc<dyn Animation<T>> {
        self.inner.parent.borrow().get().clone()
    }

    /// Set a new parent animation.
    ///
    /// Value listeners are always notified (the value type has no equality
    /// bound, so the proxy cannot compare old vs. new). Status listeners are
    /// notified only when the status actually differs across the swap.
    pub fn set_parent(&self, new_parent: Rc<dyn Animation<T>>) {
        let mut retirement = Retirement::new();
        self.inner.notifier.inherit_failure(&mut retirement.scope());
        self.inner
            .status_listeners
            .inherit_failure(&mut retirement.scope());
        let new_parent = Terminal::new(new_parent);
        let previous_parent = Terminal::new(self.parent());
        let old_status = previous_parent.status();
        // Subscribe to the new parent first, then swap; replacing the stored
        // subscriptions drops the old ones, which removes the value listener
        // and status forwarder from the previous parent.
        let new_sub = link_parent(&new_parent, &self.inner.notifier);
        let new_status_sub = crate::animation::link_parent_status(
            &new_parent,
            &self.inner.status_listeners,
            |status| status,
        );
        let new_status = new_parent.status();
        let old_parent = std::mem::replace(&mut *self.inner.parent.borrow_mut(), new_parent);
        let old_parent_sub = std::mem::replace(
            &mut *self.inner.parent_sub.borrow_mut(),
            Terminal::new(new_sub),
        );
        let old_status_sub = std::mem::replace(
            &mut *self.inner.status_sub.borrow_mut(),
            Terminal::new(new_status_sub),
        );
        // Admission includes outgoing ownership before any parent teardown can
        // re-enter. Nested swaps append after this commit's notifications.
        let mut pending = self.inner.pending.borrow_mut();
        pending.push_back(ProxyDelivery::Retire {
            parent: old_parent,
            previous: previous_parent,
            value_sub: old_parent_sub,
            status_sub: old_status_sub,
        });
        pending.push_back(ProxyDelivery::Value);
        if new_status != old_status {
            let snapshot = self.inner.status_listeners.listener_ids();
            pending.push_back(ProxyDelivery::Status(new_status, snapshot));
        }
        drop(pending);
        self.drain(&mut retirement.scope());
        retirement.finish();
    }

    fn drain(&self, retirement: &mut RecoveryScope<'_>) {
        if self.inner.delivering.replace(true) {
            return;
        }
        loop {
            let delivery = self.inner.pending.borrow_mut().pop_front();
            let Some(delivery) = delivery else { break };
            match delivery {
                ProxyDelivery::Retire {
                    parent,
                    previous,
                    value_sub,
                    status_sub,
                } => {
                    value_sub.detach(retirement);
                    status_sub.detach(retirement);
                    retirement.retire(value_sub);
                    retirement.retire(status_sub);
                    retirement.retire(parent);
                    retirement.retire(previous);
                }
                ProxyDelivery::Value => retirement.run_with(|recovery| {
                    self.inner.notifier.notify_listeners_with_recovery(recovery);
                }),
                ProxyDelivery::Status(status, callbacks) => {
                    retirement.run_with(|recovery| {
                        self.inner
                            .status_listeners
                            .notify_selected_with_recovery(&status, &callbacks, recovery);
                    });
                }
            }
            let retired = std::mem::take(&mut *self.inner.retired.borrow_mut());
            for callback in retired {
                retirement.retire(callback);
            }
        }
        self.inner.delivering.set(false);
    }
}

impl<T> Animation<T> for ProxyAnimation<T>
where
    T: Clone + fmt::Debug + 'static,
{
    #[inline]
    fn value(&self) -> T {
        self.parent().value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.parent().status()
    }

    fn is_animating(&self) -> bool {
        self.parent().is_animating()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.inner
            .status_listeners
            .add(Rc::new(move |status| callback(*status)))
    }

    fn add_status_observer(&self, observer: crate::animation::StatusObserver) -> ListenerId {
        self.inner
            .status_listeners
            .add_with_recovery(Rc::new(move |status, recovery| observer(*status, recovery)))
    }

    fn remove_status_listener(&self, id: ListenerId) {
        if self.inner.delivering.get() {
            if let Some(callback) = self.inner.status_listeners.take_callback(id) {
                self.inner
                    .retired
                    .borrow_mut()
                    .push(Terminal::new(callback));
            }
        } else {
            let callback = self.inner.status_listeners.take_callback(id);
            let mut recovery = Retirement::new();
            self.inner
                .status_listeners
                .inherit_failure(&mut recovery.scope());
            self.inner.notifier.inherit_failure(&mut recovery.scope());
            recovery.retire(callback);
            recovery.finish();
        }
    }
}

impl<T> Listenable for ProxyAnimation<T>
where
    T: Clone + 'static,
{
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.inner.notifier.add_observer(observer)
    }

    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.inner.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        let callback = self.inner.notifier.take_listener(id);
        let mut recovery = Retirement::new();
        self.inner
            .status_listeners
            .inherit_failure(&mut recovery.scope());
        self.inner.notifier.inherit_failure(&mut recovery.scope());
        recovery.retire(callback);
        recovery.finish();
    }

    fn remove_all_listeners(&self) {
        let callbacks = self.inner.notifier.take_listeners();
        let mut recovery = Retirement::new();
        self.inner
            .status_listeners
            .inherit_failure(&mut recovery.scope());
        self.inner.notifier.inherit_failure(&mut recovery.scope());
        for callback in callbacks {
            recovery.retire(callback);
        }
        recovery.finish();
    }
}

impl<T> fmt::Debug for ProxyAnimation<T>
where
    T: Clone + fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyAnimation")
            .field("value", &self.value())
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;

    use std::time::Duration;

    #[test]
    fn swap_with_status_change_fires_listeners() {
        let controller1 = Rc::new(AnimationController::builder(Duration::from_millis(100)).build());
        let controller2 = Rc::new(AnimationController::builder(Duration::from_millis(100)).build());
        controller2.set_value(1.0); // Completed

        let proxy = ProxyAnimation::new(controller1.clone() as Rc<dyn Animation<f64>>);

        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen2 = Rc::clone(&seen);
        let _id = proxy.add_status_listener(Rc::new(move |status| {
            seen2.borrow_mut().push(status);
        }));

        // Dismissed -> Completed across the swap must fire once with the new
        // status.
        proxy.set_parent(controller2.clone() as Rc<dyn Animation<f64>>);
        assert_eq!(seen.borrow().as_slice(), &[AnimationStatus::Completed]);

        controller1.dispose();
        controller2.dispose();
    }
}
