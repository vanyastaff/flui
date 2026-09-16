//! `ListenerRegistry<S>` — unified value + status listener registry with lazy
//! first/last edge hooks and RAII [`ListenerSubscription`] teardown.
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

/// Which channel a [`ListenerSubscription`] belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Channel {
    Value,
    Status,
}

type EdgeHook = Box<dyn FnMut() + Send>;

/// An installed edge hook, shared so it can be called with the SLOT lock
/// (`on_first`/`on_last`) already released. See `after_add`/`after_remove`.
type SharedHook = Arc<Mutex<EdgeHook>>;

struct RegistryInner<S> {
    /// Zero-arg value channel — reuses the existing hardened `ChangeNotifier`.
    value: ChangeNotifier,
    /// Typed status channel carrying `S` (e.g. `AnimationStatus`).
    status: Notifier<S>,
    /// Total live listeners across both channels — drives the lazy edges.
    count: AtomicUsize,
    on_first: Mutex<Option<SharedHook>>,
    on_last: Mutex<Option<SharedHook>>,
}

impl<S> RegistryInner<S> {
    /// Bump the shared count; fire `on_first` on the 0 → 1 transition.
    ///
    /// The hook runs with the `on_first` SLOT lock already released: it is
    /// arbitrary owner code (the module doc's "subscribe to parent"), and a
    /// hook that calls [`ListenerRegistry::set_on_first_listener`] from
    /// inside itself — re-arming for the next 0 → 1 edge — must neither
    /// deadlock on that same lock nor lose the re-arm. Cloning the `Arc`
    /// under the slot lock and releasing before calling achieves both: a
    /// re-arm mid-call replaces the OUTER `Option` slot (a different lock,
    /// already free by then) with a new `SharedHook`, leaving the Arc this
    /// call is still running under untouched until it returns.
    ///
    /// What a hook must NOT do is drive this registry's count back across
    /// its OWN edge from inside its body: the per-hook mutex is held for the
    /// whole call, so a nested crossing of the same edge clones the same
    /// `Arc` and re-locks it on the same thread, a deadlock on the
    /// non-reentrant `parking_lot::Mutex`. An `on_first` hook cannot reach
    /// its own edge (the count is already 1 while it runs, so an add inside
    /// it is 1 → 2); an `on_last` hook that adds and then drops a listener
    /// crosses 1 → 0 again and deadlocks; see [`Self::after_remove`].
    fn after_add(&self) {
        if self.count.fetch_add(1, Ordering::AcqRel) == 0 {
            let hook = self.on_first.lock().clone();
            if let Some(hook) = hook {
                (*hook.lock())();
            }
        }
    }

    /// Drop the shared count; fire `on_last` on the 1 → 0 transition. Same
    /// released-before-calling discipline as [`Self::after_add`].
    ///
    /// An `on_last` hook must not add and then drop a listener from inside
    /// its own body: that second 1 → 0 crossing re-enters this method while
    /// the hook's own per-hook mutex is still held by the outer call, and
    /// `hook.lock()` self-deadlocks. Re-arming through
    /// [`ListenerRegistry::set_on_last_listener`] is the supported way to
    /// change what the next edge does.
    fn after_remove(&self) {
        if self.count.fetch_sub(1, Ordering::AcqRel) == 1 {
            let hook = self.on_last.lock().clone();
            if let Some(hook) = hook {
                (*hook.lock())();
            }
        }
    }
}

/// Object-safe removal hook so a non-generic [`ListenerSubscription`] can tear itself
/// down without naming `S`.
trait RemoveFrom: Send + Sync {
    fn remove(&self, channel: Channel, id: ListenerId);
}

impl<S: Send + Sync + 'static> RemoveFrom for RegistryInner<S> {
    fn remove(&self, channel: Channel, id: ListenerId) {
        // After `dispose()` both channels have already cleared their listeners.
        // The Value channel (`ChangeNotifier::remove_listener`) tolerates
        // post-dispose removal per Flutter parity, so its guard below is merely
        // redundant; the Status channel (`Notifier::remove`) still debug-panics
        // on use-after-dispose, so ITS guard is load-bearing. A live
        // `ListenerSubscription` dropped after the registry is disposed must stay
        // safe — otherwise disposing while a subscription is alive aborts on its
        // drop — so skip the channel removal once disposed, while still
        // decrementing the shared count to keep the 1 → 0 edge exact.
        match channel {
            Channel::Value => {
                if !self.value.is_disposed() {
                    self.value.remove_listener(id);
                }
            }
            Channel::Status => {
                if !self.status.is_disposed() {
                    self.status.remove(id);
                }
            }
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
    ///
    /// A hook already installed (the ordering contract says this shouldn't
    /// happen, but nothing enforces it) is displaced, not merged; the
    /// displaced `Box<dyn FnMut>`'s `Drop` may run arbitrary captured-state
    /// teardown, so it is extracted from the guard before being dropped —
    /// never while `on_first`'s own lock is held. Calling this from INSIDE
    /// a currently-running `on_first` hook (a self-re-arm) is sound: it
    /// only ever touches this outer slot, never the running hook's own
    /// per-hook mutex — see `RegistryInner::after_add`'s own doc.
    pub fn set_on_first_listener(&self, f: impl FnMut() + Send + 'static) {
        let previous = self
            .inner
            .on_first
            .lock()
            .replace(Arc::new(Mutex::new(Box::new(f))));
        drop(previous);
    }

    /// Install the hook fired when the total listener count crosses 1 → 0.
    /// Owners tear down the parent subscription here. Same displaced-hook
    /// discipline, and the same self-re-arm soundness, as
    /// [`Self::set_on_first_listener`]. One constraint the `on_first` side
    /// does not have: the hook must not add and then drop a listener from
    /// inside its own body. That crosses 1 → 0 again while the hook's own
    /// per-hook mutex is held and deadlocks (see `RegistryInner::after_remove`).
    pub fn set_on_last_listener(&self, f: impl FnMut() + Send + 'static) {
        let previous = self
            .inner
            .on_last
            .lock()
            .replace(Arc::new(Mutex::new(Box::new(f))));
        drop(previous);
    }

    /// Test-only probe: `true` if `on_first`'s SLOT lock is currently free.
    ///
    /// Backs a regression test for `set_on_first_listener`'s
    /// extract-then-drop ordering: a previously-installed hook whose own
    /// `Drop` re-enters this method must observe the lock already released.
    #[cfg(test)]
    pub(crate) fn on_first_is_unlocked(&self) -> bool {
        self.inner.on_first.try_lock().is_some()
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
    /// Register a value listener (zero-arg). Returns a RAII [`ListenerSubscription`]
    /// that removes the listener — and may fire `on_last_listener` — on drop.
    #[must_use = "dropping the ListenerSubscription immediately removes the listener"]
    pub fn add_value_listener(
        &self,
        cb: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> ListenerSubscription {
        let id = self.inner.value.add_listener(cb);
        self.inner.after_add();
        ListenerSubscription {
            registry: Arc::downgrade(&self.inner) as Weak<dyn RemoveFrom>,
            channel: Channel::Value,
            id,
        }
    }

    /// Register a status listener (receives `S`). Returns a RAII [`ListenerSubscription`].
    #[must_use = "dropping the ListenerSubscription immediately removes the listener"]
    pub fn add_status_listener(&self, cb: ArgCallback<S>) -> ListenerSubscription {
        let id = self.inner.status.add(cb);
        self.inner.after_add();
        ListenerSubscription {
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
#[must_use = "dropping the ListenerSubscription immediately removes the listener"]
pub struct ListenerSubscription {
    registry: Weak<dyn RemoveFrom>,
    channel: Channel,
    id: ListenerId,
}

impl std::fmt::Debug for ListenerSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenerSubscription")
            .field("channel", &self.channel)
            .field("id", &self.id)
            .field("alive", &(self.registry.strong_count() > 0))
            .finish()
    }
}

impl Drop for ListenerSubscription {
    fn drop(&mut self) {
        if let Some(reg) = self.registry.upgrade() {
            reg.remove(self.channel, self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

    /// A hook that re-arms itself (calls `set_on_first_listener` from
    /// inside its own body) must neither deadlock on `on_first`'s slot lock
    /// nor lose the re-arm. Reverting `after_add` to call the hook while
    /// still holding that lock (the let-chain-scrutinee shape) hangs this
    /// test forever — a same-thread, non-reentrant `parking_lot::Mutex`
    /// deadlock, not a panic, so it was verified in scratch under `timeout`
    /// rather than left in the permanent suite as an unbounded hang.
    #[test]
    fn on_first_listener_hook_can_re_arm_itself_without_deadlocking() {
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let fires = Arc::new(AtomicUsize::new(0));
        let fires_for_hook = Arc::clone(&fires);
        let reg_for_hook = reg.clone();

        reg.set_on_first_listener(move || {
            fires_for_hook.fetch_add(1, Ordering::SeqCst);
            let fires_for_next = Arc::clone(&fires_for_hook);
            reg_for_hook.set_on_first_listener(move || {
                fires_for_next.fetch_add(1, Ordering::SeqCst);
            });
        });

        let s1 = reg.add_value_listener(Arc::new(|| {}));
        assert_eq!(fires.load(Ordering::SeqCst), 1, "the original hook fired");
        drop(s1);

        let s2 = reg.add_value_listener(Arc::new(|| {}));
        assert_eq!(
            fires.load(Ordering::SeqCst),
            2,
            "the re-armed hook must fire on the next 0 -> 1 edge, not be lost"
        );
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
            // reg dropped here; ListenerSubscription holds only a Weak.
        };
        drop(s); // upgrade() returns None — must not panic / use-after-free.
    }

    #[test]
    fn dropping_subscription_after_dispose_does_not_panic() {
        // Disposing the registry clears both channels; dropping a still-live
        // subscription afterwards must not debug-panic on the now-disposed
        // channel (that would abort if the drop ran during unwinding). The
        // shared count is still decremented.
        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let v = reg.add_value_listener(Arc::new(|| {}));
        let s = reg.add_status_listener(Arc::new(|_s: u8| {}));
        assert_eq!(reg.listener_count(), 2);
        reg.dispose();
        drop(v); // value channel disposed — removal must be skipped, not panic.
        drop(s); // status channel disposed — same.
        assert_eq!(reg.listener_count(), 0, "count still reaches zero");
    }

    /// Pins `set_on_first_listener`'s extract-then-drop ordering: reverting
    /// to a bare `*self.inner.on_first.lock() = Some(Box::new(f));`
    /// statement drops the DISPLACED hook (the one this call is overwriting)
    /// while that assignment's own guard is still live, so a hook whose
    /// `Drop` re-enters this registry deadlocks on `on_first`.
    #[test]
    fn set_on_first_listener_drops_the_displaced_hook_after_releasing_the_lock() {
        struct DropCanary {
            reg: ListenerRegistry<u8>,
            observed_locked: Arc<AtomicBool>,
        }
        impl Drop for DropCanary {
            fn drop(&mut self) {
                if !self.reg.on_first_is_unlocked() {
                    self.observed_locked.store(true, Ordering::SeqCst);
                }
            }
        }

        let reg: ListenerRegistry<u8> = ListenerRegistry::new();
        let observed_locked = Arc::new(AtomicBool::new(false));

        let canary = DropCanary {
            reg: reg.clone(),
            observed_locked: Arc::clone(&observed_locked),
        };
        reg.set_on_first_listener(move || {
            let _keep_alive = &canary;
        });

        // Overwrites the hook above, displacing (and dropping) it.
        reg.set_on_first_listener(|| {});

        assert!(
            !observed_locked.load(Ordering::SeqCst),
            "the displaced hook's Drop observed the lock still held"
        );
    }
}
