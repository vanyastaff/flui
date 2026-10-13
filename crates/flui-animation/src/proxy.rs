//! `ProxyAnimation` - wraps another animation, allowing hot-swapping.

use crate::animation::{Animation, ParentSubscription, Retirement, Terminal, link_parent};
use crate::channels::{Channels, Withdrawn};
use crate::status::AnimationStatus;
use flui_foundation::panic::RecoveryScope;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
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
/// use std::rc::Rc;
/// use std::time::Duration;
///
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
    channels: Channels,
    parent_sub: RefCell<Terminal<Rc<ParentSubscription>>>,
    status_sub: RefCell<Terminal<crate::StatusSubscription>>,
    delivering: Cell<bool>,
    pending: RefCell<VecDeque<ProxyDelivery<T>>>,
    retired: RefCell<Vec<Terminal<Withdrawn<AnimationStatus>>>>,
}

enum ProxyDelivery<T: Clone + 'static> {
    Retire {
        parent: Terminal<Rc<dyn Animation<T>>>,
        previous: Terminal<Rc<dyn Animation<T>>>,
        value_sub: Terminal<Rc<ParentSubscription>>,
        status_sub: Terminal<crate::StatusSubscription>,
    },
    Value,
    Status(AnimationStatus, Vec<ListenerId>),
}

impl<T: Clone + 'static> Drop for ProxyOwner<T> {
    fn drop(&mut self) {
        let mut retirement = Retirement::new();
        self.channels.inherit_failure(&mut retirement.scope());
        let parent = self.parent.get_mut().withdraw();
        let channels = self.channels.withdraw();
        let value_sub = self.parent_sub.get_mut().withdraw();
        let mut status_sub = self.status_sub.get_mut().withdraw();
        let pending = self.pending.get_mut().drain(..).collect::<Vec<_>>();
        let retired = std::mem::take(self.retired.get_mut());
        let disposed = channels.dispose();
        value_sub.detach(&mut retirement.scope());
        status_sub
            .get_mut()
            .cancel_with_recovery(&mut retirement.scope());
        retirement.retire(value_sub);
        retirement.retire(status_sub);
        retirement.retire(parent);
        for delivery in pending {
            retirement.retire(delivery);
        }
        for callback in retired {
            retirement.retire(callback);
        }
        disposed.retire_statuses_first(&mut retirement.scope());
        retirement.retire(channels);
        retirement.finish();
    }
}

impl<T> ProxyAnimation<T>
where
    T: Clone + 'static,
{
    /// Create a new proxy animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The initial parent animation
    #[must_use]
    pub fn new(parent: Rc<dyn Animation<T>>) -> Self {
        let parent = Terminal::new(parent);
        let channels = Channels::new();
        let parent_sub = link_parent(&parent, channels.values());
        let status_sub =
            crate::animation::link_parent_status(&parent, channels.statuses(), |status| status);
        Self {
            inner: Rc::new(ProxyOwner {
                parent: RefCell::new(parent),
                channels,
                parent_sub: RefCell::new(Terminal::new(parent_sub)),
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
        self.inner.channels.inherit_failure(&mut retirement.scope());
        let new_parent = Terminal::new(new_parent);
        let previous_parent = Terminal::new(self.parent());
        let old_status = previous_parent.status();
        // Subscribe to the new parent first, then swap; replacing the stored
        // subscriptions drops the old ones, which removes the value listener
        // and status forwarder from the previous parent.
        let new_sub = link_parent(&new_parent, self.inner.channels.values());
        let new_status_sub = crate::animation::link_parent_status(
            &new_parent,
            self.inner.channels.statuses(),
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
            let snapshot = self.inner.channels.statuses().listener_ids();
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
                    mut status_sub,
                } => {
                    value_sub.detach(retirement);
                    status_sub.get_mut().cancel_with_recovery(retirement);
                    retirement.retire(value_sub);
                    retirement.retire(status_sub);
                    retirement.retire(parent);
                    retirement.retire(previous);
                }
                ProxyDelivery::Value => retirement.run_with(|recovery| {
                    self.inner
                        .channels
                        .values()
                        .notify_listeners_with_recovery(recovery);
                }),
                ProxyDelivery::Status(status, callbacks) => {
                    retirement.run_with(|recovery| {
                        self.inner
                            .channels
                            .statuses()
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

impl<T: Clone + 'static> ProxyAnimation<T> {
    /// Withdraw a value listener within an enclosing framework cleanup round.
    /// Its outgoing captures retire through that round's first-failure context.
    #[doc(hidden)]
    pub fn remove_listener_with_recovery(&self, id: ListenerId, recovery: &mut RecoveryScope<'_>) {
        self.inner
            .channels
            .remove_listener_with_recovery(id, recovery);
    }
}

impl<T> Animation<T> for ProxyAnimation<T>
where
    T: Clone + 'static,
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

    fn observe_status(&self, observer: crate::StatusObserver) -> crate::StatusSubscription {
        let id = self.inner.channels.add_status(observer);
        crate::StatusSubscription::new(&self.inner, id, ProxyOwner::withdraw_status)
    }
}

impl<T: Clone + 'static> ProxyOwner<T> {
    fn withdraw_status(
        &self,
        id: ListenerId,
        recovery: &mut RecoveryScope<'_>,
    ) -> Option<Withdrawn<AnimationStatus>> {
        if self.delivering.get() {
            // A delivery round is draining: its tail retires the capture.
            if let Some(callback) = self.channels.statuses().take_callback(id) {
                self.retired.borrow_mut().push(Terminal::new(callback));
            }
            None
        } else {
            self.channels.take_status(id, recovery)
        }
    }
}

impl<T> Listenable for ProxyAnimation<T>
where
    T: Clone + 'static,
{
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.inner.channels.add_observer(observer)
    }

    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.inner.channels.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.inner.channels.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.inner.channels.remove_all_listeners();
    }
}

impl<T> fmt::Debug for ProxyAnimation<T>
where
    T: Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyAnimation")
            .field("parent", &self.parent())
            .finish_non_exhaustive()
    }
}
