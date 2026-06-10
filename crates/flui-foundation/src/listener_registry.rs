//! `ListenerRegistry<S>` — unified value + status listener registry with lazy
//! first/last edge hooks and RAII [`Subscription`] teardown.
//!
//! Collapses Flutter's four-mixin listener lattice (`AnimationLazyListenerMixin`
//! XOR `AnimationEagerListenerMixin`, plus `AnimationLocalListenersMixin` and
//! `AnimationLocalStatusListenersMixin`, all sharing one listener count) into a
//! single composed type. An animation embeds one registry and its `Listenable`
//! impl becomes a one-line delegation.
//!
//! # Lazy edges
//!
//! When the total listener count (value + status) crosses 0 → 1, the
//! `on_first_listener` hook fires; combinators wire "subscribe to my parent"
//! there. When it crosses 1 → 0, `on_last_listener` fires; combinators tear the
//! parent subscription down. This is the structural fix for the historical
//! dead-combinator-listener bug: the owner cannot forget to wire what the
//! registry drives, and the subscription cannot outlive its need.
//!
//! # Ordering contract
//!
//! Owners MUST install `on_first_listener` / `on_last_listener` at construction,
//! before any external listener is added. A hook installed after the count has
//! already left zero will not observe the edge it missed.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicUsize, Ordering},
};

use parking_lot::Mutex;

use crate::id::ListenerId;
use crate::notifier::{ChangeNotifier, Listenable};
use crate::notifier_generic::{ArgCallback, Notifier};

/// Which channel a [`Subscription`] belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Channel {
    Value,
    Status,
}

type EdgeHook = Box<dyn FnMut() + Send>;

struct RegistryInner<S> {
    /// Zero-arg value channel — reuses the existing hardened `ChangeNotifier`.
    value: ChangeNotifier,
    /// Typed status channel carrying `S` (e.g. `AnimationStatus`).
    status: Notifier<S>,
    /// Total live listeners across both channels — drives the lazy edges.
    count: AtomicUsize,
    on_first: Mutex<Option<EdgeHook>>,
    on_last: Mutex<Option<EdgeHook>>,
}

impl<S> RegistryInner<S> {
    /// Bump the shared count; fire `on_first` on the 0 → 1 transition.
    fn after_add(&self) {
        if self.count.fetch_add(1, Ordering::AcqRel) == 0
            && let Some(hook) = self.on_first.lock().as_mut()
        {
            hook();
        }
    }

    /// Drop the shared count; fire `on_last` on the 1 → 0 transition.
    fn after_remove(&self) {
        if self.count.fetch_sub(1, Ordering::AcqRel) == 1
            && let Some(hook) = self.on_last.lock().as_mut()
        {
            hook();
        }
    }
}

/// Object-safe removal hook so a non-generic [`Subscription`] can tear itself
/// down without naming `S`.
trait RemoveFrom: Send + Sync {
    fn remove(&self, channel: Channel, id: ListenerId);
}

impl<S: Send + Sync + 'static> RemoveFrom for RegistryInner<S> {
    fn remove(&self, channel: Channel, id: ListenerId) {
        match channel {
            Channel::Value => self.value.remove_listener(id),
            Channel::Status => self.status.remove(id),
        }
        self.after_remove();
    }
}

/// Unified value + status listener registry. See module docs.
///
/// `S` is the status argument type (e.g. `AnimationStatus`). Cloning shares the
/// same underlying state (`Arc`-backed).
pub struct ListenerRegistry<S> {
    inner: Arc<RegistryInner<S>>,
}

impl<S> Default for ListenerRegistry<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Clone for ListenerRegistry<S> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<S> std::fmt::Debug for ListenerRegistry<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenerRegistry")
            .field("listener_count", &self.listener_count())
            .finish_non_exhaustive()
    }
}

impl<S> ListenerRegistry<S> {
    /// Create an empty registry with no edge hooks installed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                value: ChangeNotifier::new(),
                status: Notifier::new(),
                count: AtomicUsize::new(0),
                on_first: Mutex::new(None),
                on_last: Mutex::new(None),
            }),
        }
    }

    /// Install the hook fired when the total listener count crosses 0 → 1.
    /// Owners wire "subscribe to parent" here. See the ordering contract.
    pub fn set_on_first_listener(&self, f: impl FnMut() + Send + 'static) {
        *self.inner.on_first.lock() = Some(Box::new(f));
    }

    /// Install the hook fired when the total listener count crosses 1 → 0.
    /// Owners tear down the parent subscription here.
    pub fn set_on_last_listener(&self, f: impl FnMut() + Send + 'static) {
        *self.inner.on_last.lock() = Some(Box::new(f));
    }

    /// Total registered listeners across both channels.
    #[must_use]
    #[inline]
    pub fn listener_count(&self) -> usize {
        self.inner.count.load(Ordering::Acquire)
    }

    /// Whether any listener is registered on either channel.
    #[must_use]
    #[inline]
    pub fn has_listeners(&self) -> bool {
        self.listener_count() > 0
    }

    /// Fire all value listeners (zero-arg). Cheap: the value channel carries no
    /// argument, so no per-listener clone occurs.
    pub fn notify_value(&self) {
        self.inner.value.notify_listeners();
    }

    /// Dispose both channels. Subsequent notifies debug-panic (no-op in release).
    pub fn dispose(&self) {
        self.inner.value.dispose();
        self.inner.status.dispose();
    }
}

impl<S: Send + Sync + 'static> ListenerRegistry<S> {
    /// Register a value listener (zero-arg). Returns a RAII [`Subscription`]
    /// that removes the listener — and may fire `on_last_listener` — on drop.
    #[must_use = "dropping the Subscription immediately removes the listener"]
    pub fn add_value_listener(&self, cb: Arc<dyn Fn() + Send + Sync + 'static>) -> Subscription {
        let id = self.inner.value.add_listener(cb);
        self.inner.after_add();
        Subscription {
            registry: Arc::downgrade(&self.inner) as Weak<dyn RemoveFrom>,
            channel: Channel::Value,
            id,
        }
    }

    /// Register a status listener (receives `S`). Returns a RAII [`Subscription`].
    #[must_use = "dropping the Subscription immediately removes the listener"]
    pub fn add_status_listener(&self, cb: ArgCallback<S>) -> Subscription {
        let id = self.inner.status.add(cb);
        self.inner.after_add();
        Subscription {
            registry: Arc::downgrade(&self.inner) as Weak<dyn RemoveFrom>,
            channel: Channel::Status,
            id,
        }
    }
}

impl<S: Clone> ListenerRegistry<S> {
    /// Fire all status listeners with `status`.
    pub fn notify_status(&self, status: S) {
        self.inner.status.notify(status);
    }
}

/// RAII handle returned by the `add_*_listener` methods. Dropping it removes the
/// listener and updates the shared count, firing `on_last_listener` at the
/// 1 → 0 edge. Holds a [`Weak`] so a dropped registry is never resurrected.
#[must_use = "dropping the Subscription immediately removes the listener"]
pub struct Subscription {
    registry: Weak<dyn RemoveFrom>,
    channel: Channel,
    id: ListenerId,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("channel", &self.channel)
            .field("id", &self.id)
            .field("alive", &(self.registry.strong_count() > 0))
            .finish()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(reg) = self.registry.upgrade() {
            reg.remove(self.channel, self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync) {
        let c = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::clone(&c);
        (c, move || {
            c2.fetch_add(1, Ordering::SeqCst);
        })
    }

    #[test]
    fn first_listener_edge_fires_once() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let firsts = Arc::new(AtomicUsize::new(0));
        let f2 = Arc::clone(&firsts);
        reg.set_on_first_listener(move || {
            f2.fetch_add(1, Ordering::SeqCst);
        });
        let s1 = reg.add_value_listener(Arc::new(|| {}));
        let s2 = reg.add_value_listener(Arc::new(|| {}));
        assert_eq!(firsts.load(Ordering::SeqCst), 1, "first edge fires once");
        drop(s1);
        drop(s2);
    }

    #[test]
    fn last_listener_edge_fires_on_drop_to_zero() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let lasts = Arc::new(AtomicUsize::new(0));
        let l2 = Arc::clone(&lasts);
        reg.set_on_last_listener(move || {
            l2.fetch_add(1, Ordering::SeqCst);
        });
        let s1 = reg.add_value_listener(Arc::new(|| {}));
        let s2 = reg.add_status_listener(Arc::new(|_s: u8| {}));
        assert_eq!(lasts.load(Ordering::SeqCst), 0);
        drop(s1);
        assert_eq!(lasts.load(Ordering::SeqCst), 0, "still 1 listener");
        drop(s2);
        assert_eq!(lasts.load(Ordering::SeqCst), 1, "last edge at 1->0");
    }

    #[test]
    fn shared_count_spans_value_and_status() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let firsts = Arc::new(AtomicUsize::new(0));
        let f2 = Arc::clone(&firsts);
        reg.set_on_first_listener(move || {
            f2.fetch_add(1, Ordering::SeqCst);
        });
        let _s = reg.add_status_listener(Arc::new(|_s: u8| {}));
        let _v = reg.add_value_listener(Arc::new(|| {}));
        assert_eq!(firsts.load(Ordering::SeqCst), 1, "one shared first edge");
        assert_eq!(reg.listener_count(), 2);
    }

    #[test]
    fn notify_value_and_status_independent() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let (vc, vcb) = counter();
        let _v = reg.add_value_listener(Arc::new(vcb));
        let sc = Arc::new(AtomicUsize::new(0));
        let sc2 = Arc::clone(&sc);
        let _s = reg.add_status_listener(Arc::new(move |s: u8| {
            sc2.fetch_add(s as usize, Ordering::SeqCst);
        }));
        reg.notify_value();
        assert_eq!(vc.load(Ordering::SeqCst), 1);
        assert_eq!(
            sc.load(Ordering::SeqCst),
            0,
            "value notify must not fire status"
        );
        reg.notify_status(5);
        assert_eq!(sc.load(Ordering::SeqCst), 5);
        assert_eq!(
            vc.load(Ordering::SeqCst),
            1,
            "status notify must not fire value"
        );
    }

    #[test]
    fn drop_subscription_stops_delivery() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let (vc, vcb) = counter();
        let s = reg.add_value_listener(Arc::new(vcb));
        reg.notify_value();
        assert_eq!(vc.load(Ordering::SeqCst), 1);
        drop(s);
        assert_eq!(reg.listener_count(), 0, "drop decrements shared count");
        reg.notify_value();
        assert_eq!(vc.load(Ordering::SeqCst), 1, "dropped sub must not fire");
    }

    #[test]
    fn subscription_outliving_registry_is_safe() {
        let s = {
            let reg: ListenerRegistry<u8> = ListenerRegistry::new();
            reg.add_value_listener(Arc::new(|| {}))
            // reg dropped here; Subscription holds only a Weak.
        };
        drop(s); // upgrade() returns None — must not panic / use-after-free.
    }
}
