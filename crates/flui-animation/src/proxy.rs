//! `ProxyAnimation` - wraps another animation, allowing hot-swapping.

use crate::animation::{Animation, ParentSubscription, StatusCallback, link_parent};
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
    let snapshot: Vec<StatusCallback> = listeners
        .lock()
        .listeners
        .iter()
        .map(|(_, cb)| Arc::clone(cb))
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
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
/// let controller1 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     scheduler.clone(),
/// ));
///
/// let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f32>>);
///
/// // Later, swap to a different animation
/// let controller2 = Arc::new(AnimationController::new(
///     Duration::from_millis(500),
///     scheduler,
/// ));
/// proxy.set_parent(controller2 as Arc<dyn Animation<f32>>);
/// ```
#[derive(Clone)]
pub struct ProxyAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    parent: Arc<RwLock<Arc<dyn Animation<T>>>>,
    notifier: Arc<ChangeNotifier>,
    /// Re-emits the current parent's value changes; swapped (old removed) on
    /// `set_parent`, removed entirely on the last clone's drop.
    parent_sub: Arc<RwLock<Arc<ParentSubscription>>>,
    /// Proxy-owned status listeners; fed by a per-parent forwarder.
    status_listeners: Arc<Mutex<StatusListeners>>,
    /// Removes the status forwarder from the current parent; swapped on
    /// `set_parent`, removed entirely on the last clone's drop.
    status_sub: Arc<RwLock<Arc<ParentSubscription>>>,
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
        let notifier = Arc::new(ChangeNotifier::new());
        let parent_sub = link_parent(&parent, &notifier);
        let status_listeners = Arc::new(Mutex::new(StatusListeners::new()));
        let status_sub = link_parent_status(&parent, &status_listeners);
        Self {
            parent: Arc::new(RwLock::new(parent)),
            notifier,
            parent_sub: Arc::new(RwLock::new(parent_sub)),
            status_listeners,
            status_sub: Arc::new(RwLock::new(status_sub)),
        }
    }

    /// Get the current parent animation.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Arc<dyn Animation<T>> {
        self.parent.read().clone()
    }

    /// Set a new parent animation.
    ///
    /// Value listeners are always notified (the value type has no equality
    /// bound, so the proxy cannot compare old vs. new — Flutter only notifies
    /// on change). Status listeners are notified only when the status actually
    /// differs across the swap, matching Flutter's `ProxyAnimation.parent=`.
    pub fn set_parent(&self, new_parent: Arc<dyn Animation<T>>) {
        let old_status = self.parent.read().status();
        // Subscribe to the new parent first, then swap; replacing the stored
        // subscriptions drops the old ones, which removes the value listener
        // and status forwarder from the previous parent.
        let new_sub = link_parent(&new_parent, &self.notifier);
        let new_status_sub = link_parent_status(&new_parent, &self.status_listeners);
        let new_status = new_parent.status();
        *self.parent.write() = new_parent;
        *self.parent_sub.write() = new_sub;
        *self.status_sub.write() = new_status_sub;
        self.notifier.notify_listeners();
        if new_status != old_status {
            fan_out_status(&self.status_listeners, new_status);
        }
    }
}

impl<T> Animation<T> for ProxyAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    #[inline]
    fn value(&self) -> T {
        self.parent.read().value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.parent.read().status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let mut reg = self.status_listeners.lock();
        let id = ListenerId::new(reg.next_id);
        reg.next_id += 1;
        reg.listeners.push((id, callback));
        id
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.status_listeners
            .lock()
            .listeners
            .retain(|(listener_id, _)| *listener_id != id);
    }
}

impl<T> Listenable for ProxyAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
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
mod tests {
    use super::*;
    use crate::AnimationController;
    use flui_scheduler::Scheduler;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn test_proxy_animation() {
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));

        let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f32>>);

        controller1.set_value(0.5);
        assert_eq!(proxy.value(), 0.5);

        // Swap to a different animation
        let controller2 = Arc::new(AnimationController::new(
            Duration::from_millis(200),
            scheduler,
        ));
        controller2.set_value(0.75);
        proxy.set_parent(controller2.clone() as Arc<dyn Animation<f32>>);

        assert_eq!(proxy.value(), 0.75);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_proxy_animation_status() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let proxy = ProxyAnimation::new(controller.clone() as Arc<dyn Animation<f32>>);

        assert_eq!(proxy.status(), AnimationStatus::Dismissed);

        let _ = controller.forward();
        assert_eq!(proxy.status(), AnimationStatus::Forward);

        controller.dispose();
    }

    #[test]
    fn status_listeners_survive_set_parent() {
        // Status listeners registered on the proxy must keep firing after a
        // hot-swap; previously they stayed registered on the old parent and
        // never saw the new parent's transitions.
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        let controller2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f32>>);

        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = Arc::clone(&hits);
        let _id = proxy.add_status_listener(Arc::new(move |_status| {
            hits2.fetch_add(1, Ordering::SeqCst);
        }));

        // Both parents are Dismissed: the swap itself must NOT fire (status
        // unchanged across the swap, Flutter parity).
        proxy.set_parent(controller2.clone() as Arc<dyn Animation<f32>>);
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        // A transition on the NEW parent must reach the proxy's listener.
        let _ = controller2.forward();
        assert!(
            hits.load(Ordering::SeqCst) >= 1,
            "status listener must follow the proxy to the new parent"
        );

        // A transition on the OLD parent must no longer reach it.
        let before = hits.load(Ordering::SeqCst);
        let _ = controller1.forward();
        assert_eq!(
            hits.load(Ordering::SeqCst),
            before,
            "old parent must be unsubscribed after the swap"
        );

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn swap_with_status_change_fires_listeners() {
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        let controller2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));
        controller2.set_value(1.0); // Completed

        let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f32>>);

        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = Arc::clone(&seen);
        let _id = proxy.add_status_listener(Arc::new(move |status| {
            seen2.lock().push(status);
        }));

        // Dismissed -> Completed across the swap must fire once with the new
        // status (Flutter `ProxyAnimation.parent=` parity).
        proxy.set_parent(controller2.clone() as Arc<dyn Animation<f32>>);
        assert_eq!(seen.lock().as_slice(), &[AnimationStatus::Completed]);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn remove_status_listener_after_swap() {
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        let controller2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let proxy = ProxyAnimation::new(controller1.clone() as Arc<dyn Animation<f32>>);

        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = Arc::clone(&hits);
        let id = proxy.add_status_listener(Arc::new(move |_status| {
            hits2.fetch_add(1, Ordering::SeqCst);
        }));

        proxy.set_parent(controller2.clone() as Arc<dyn Animation<f32>>);
        // The id was issued by the proxy, so removal must work regardless of
        // which parent is current.
        proxy.remove_status_listener(id);

        let _ = controller2.forward();
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "removed status listener must not fire"
        );

        controller1.dispose();
        controller2.dispose();
    }
}
