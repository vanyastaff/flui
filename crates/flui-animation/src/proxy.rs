//! `ProxyAnimation` - wraps another animation, allowing hot-swapping.

use crate::animation::{
    Animation, ParentSubscription, Retirement, StatusCallback, Terminal, link_parent,
};
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use parking_lot::{Mutex, RwLock};
use std::fmt;
use std::sync::Arc;

/// Proxy-owned status-listener registry.
///
/// Status listeners must be owned by the proxy (not delegated to the current
/// parent): after `set_parent`, listeners registered on the old parent would
/// be orphaned there and `remove_status_listener` would target the new parent
/// with a foreign id. A single forwarder per parent fans out to this registry
/// and is migrated on every swap.
struct StatusListeners {
    listeners: Vec<(ListenerId, StatusCallback)>,
    next_id: usize,
}

impl StatusListeners {
    fn new() -> Self {
        Self {
            listeners: Vec::new(),
            // Listener ids start at 1 so a zero id can never collide.
            next_id: 1,
        }
    }
}

/// Snapshot-then-fire so user callbacks run without the registry lock held
/// (a callback may re-enter add/remove_status_listener).
fn fan_out_status(listeners: &Mutex<StatusListeners>, status: AnimationStatus) {
    let snapshot: Vec<Terminal<StatusCallback>> = listeners
        .lock()
        .listeners
        .iter()
        .map(|(_, cb)| Terminal::new(Arc::clone(cb)))
        .collect();
    for cb in snapshot {
        cb(status);
    }
}

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
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller1 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     &scheduler,
/// ));
///
/// let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f64>>);
///
/// // Later, swap to a different animation
/// let controller2 = Arc::new(AnimationController::new(
///     Duration::from_millis(500),
///     &scheduler,
/// ));
/// proxy.set_parent(controller2 as Arc<dyn Animation<f64>>);
/// ```
#[derive(Clone)]
pub struct ProxyAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    inner: Arc<ProxyOwner<T>>,
}

struct ProxyOwner<T: Clone + Send + Sync + 'static> {
    parent: RwLock<Terminal<Arc<dyn Animation<T>>>>,
    notifier: Terminal<Arc<ChangeNotifier>>,
    parent_sub: RwLock<Terminal<Arc<ParentSubscription>>>,
    status_listeners: Terminal<Arc<Mutex<StatusListeners>>>,
    status_sub: RwLock<Terminal<Arc<ParentSubscription>>>,
}

impl<T: Clone + Send + Sync + 'static> Drop for ProxyOwner<T> {
    fn drop(&mut self) {
        let parent = self.parent.get_mut().withdraw();
        let notifier = self.notifier.withdraw();
        let value_sub = self.parent_sub.get_mut().withdraw();
        let status_sub = self.status_sub.get_mut().withdraw();
        let listeners = self.status_listeners.withdraw();
        let callbacks: Vec<_> = std::mem::take(&mut listeners.lock().listeners)
            .into_iter()
            .map(|(_, callback)| Terminal::new(callback))
            .collect();
        let mut retirement = Retirement::new();
        value_sub.detach(&mut retirement);
        status_sub.detach(&mut retirement);
        retirement.retire(value_sub);
        retirement.retire(status_sub);
        retirement.retire(parent);
        for callback in callbacks {
            retirement.retire(callback);
        }
        retirement.retire(listeners);
        retirement.retire(notifier);
        retirement.finish();
    }
}

impl Drop for StatusListeners {
    fn drop(&mut self) {
        let callbacks: Vec<_> = std::mem::take(&mut self.listeners)
            .into_iter()
            .map(|(_, callback)| Terminal::new(callback))
            .collect();
        let mut retirement = Retirement::new();
        for callback in callbacks {
            retirement.retire(callback);
        }
        retirement.finish();
    }
}

/// Subscribe a status forwarder on `parent` that fans out to `listeners`.
///
/// Holds only a `Weak` to the registry so the subscription never keeps the
/// proxy alive; returns the teardown handle that removes the forwarder.
fn link_parent_status<T>(
    parent: &Arc<dyn Animation<T>>,
    listeners: &Arc<Mutex<StatusListeners>>,
) -> Arc<ParentSubscription>
where
    T: Clone + Send + Sync + 'static,
{
    let weak = Arc::downgrade(listeners);
    let id = parent.add_status_listener(Arc::new(move |status| {
        if let Some(listeners) = weak.upgrade() {
            fan_out_status(&listeners, status);
        }
    }));
    let parent = Arc::clone(parent);
    ParentSubscription::new(move || parent.remove_status_listener(id))
}

impl<T> ProxyAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    /// Create a new proxy animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The initial parent animation
    #[must_use]
    pub fn new(parent: Arc<dyn Animation<T>>) -> Self {
        let parent = Terminal::new(parent);
        let notifier = Arc::new(ChangeNotifier::new());
        let parent_sub = link_parent(&parent, &notifier);
        let status_listeners = Arc::new(Mutex::new(StatusListeners::new()));
        let status_sub = link_parent_status(&parent, &status_listeners);
        Self {
            inner: Arc::new(ProxyOwner {
                parent: RwLock::new(parent),
                notifier: Terminal::new(notifier),
                parent_sub: RwLock::new(Terminal::new(parent_sub)),
                status_listeners: Terminal::new(status_listeners),
                status_sub: RwLock::new(Terminal::new(status_sub)),
            }),
        }
    }

    /// Get the current parent animation.
    #[inline]
    #[must_use]
    #[expect(
        clippy::clone_on_ref_ptr,
        reason = "this file is being reworked by the in-flight controller ownership change, which converts the site"
    )]
    pub fn parent(&self) -> Arc<dyn Animation<T>> {
        self.inner.parent.read().get().clone()
    }

    /// Set a new parent animation.
    ///
    /// Value listeners are always notified (the value type has no equality
    /// bound, so the proxy cannot compare old vs. new). Status listeners are
    /// notified only when the status actually differs across the swap.
    pub fn set_parent(&self, new_parent: Arc<dyn Animation<T>>) {
        let new_parent = Terminal::new(new_parent);
        let previous_parent = Terminal::new(self.parent());
        let old_status = previous_parent.status();
        // Subscribe to the new parent first, then swap; replacing the stored
        // subscriptions drops the old ones, which removes the value listener
        // and status forwarder from the previous parent.
        let new_sub = link_parent(&new_parent, &self.inner.notifier);
        let new_status_sub = link_parent_status(&new_parent, &self.inner.status_listeners);
        let new_status = new_parent.status();
        let old_parent = std::mem::replace(&mut *self.inner.parent.write(), new_parent);
        let old_parent_sub =
            std::mem::replace(&mut *self.inner.parent_sub.write(), Terminal::new(new_sub));
        let old_status_sub = std::mem::replace(
            &mut *self.inner.status_sub.write(),
            Terminal::new(new_status_sub),
        );
        // Drop the displaced subscriptions after the write guards release: a
        // `ParentSubscription` drop removes a listener from the (old) parent,
        // which takes the parent's own lock, so dropping it under a guard here
        // would invert the lock order.
        let mut retirement = Retirement::new();
        old_parent_sub.detach(&mut retirement);
        old_status_sub.detach(&mut retirement);
        retirement.retire(old_parent_sub);
        retirement.retire(old_status_sub);
        retirement.retire(old_parent);
        retirement.retire(previous_parent);
        retirement.finish();
        self.inner.notifier.notify_listeners();
        if new_status != old_status {
            fan_out_status(&self.inner.status_listeners, new_status);
        }
    }
}

impl<T> Animation<T> for ProxyAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    #[inline]
    fn value(&self) -> T {
        self.parent().value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.parent().status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let mut reg = self.inner.status_listeners.lock();
        let id = ListenerId::new(reg.next_id);
        reg.next_id += 1;
        reg.listeners.push((id, callback));
        id
    }

    fn remove_status_listener(&self, id: ListenerId) {
        let removed = {
            let mut listeners = self.inner.status_listeners.lock();
            listeners
                .listeners
                .iter()
                .position(|(candidate, _)| *candidate == id)
                .map(|index| listeners.listeners.remove(index).1)
        };
        drop(Terminal::new(removed));
    }
}

impl<T> Listenable for ProxyAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.inner.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.inner.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.inner.notifier.remove_all_listeners();
    }
}

impl<T> fmt::Debug for ProxyAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyAnimation")
            .field("value", &self.value())
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
#[expect(
    clippy::clone_on_ref_ptr,
    reason = "test fixtures share controllers; converted with the in-flight controller ownership change"
)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use flui_scheduler::UpdateScheduler;

    use std::time::Duration;

    #[test]
    fn swap_with_status_change_fires_listeners() {
        let scheduler = UpdateScheduler::new();
        let controller1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            &scheduler,
        ));
        let controller2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            &scheduler,
        ));
        controller2.set_value(1.0); // Completed

        let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f64>>);

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = Arc::clone(&seen);
        let _id = proxy.add_status_listener(Arc::new(move |status| {
            seen2.lock().push(status);
        }));

        // Dismissed -> Completed across the swap must fire once with the new
        // status.
        proxy.set_parent(controller2.clone() as Arc<dyn Animation<f64>>);
        assert_eq!(seen.lock().as_slice(), &[AnimationStatus::Completed]);

        controller1.dispose();
        controller2.dispose();
    }
}
