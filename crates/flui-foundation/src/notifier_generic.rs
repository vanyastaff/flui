//! `Notifier<Arg>` — a generic, typed, hardened notification channel.
//!
//! Generalizes [`crate::notifier::ChangeNotifier`] (which is effectively
//! `Notifier<()>`) to deliver a `Clone` argument to each listener. Reuses the
//! same firing discipline proven in [`crate::notifier::ChangeNotifier::notify_listeners`]:
//! snapshot-under-lock, registration-order firing, drop-lock before callbacks,
//! per-callback `catch_unwind`, remove-during-notify skip, and a dispose guard.
//!
//! This is the substrate the animation crate composes into a
//! [`crate::listener_registry::ListenerRegistry`]: the *value* channel is a
//! `Notifier<()>` and the *status* channel is a `Notifier<AnimationStatus>`.
//!
//! `ChangeNotifier` is intentionally left untouched (it has many workspace
//! consumers); a later consolidation can re-seat it on `Notifier<()>`.

use std::{
    collections::HashMap,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;

use crate::id::ListenerId;

/// A listener callback that receives a `Clone` argument by value.
pub type ArgCallback<Arg> = Arc<dyn Fn(Arg) + Send + Sync + 'static>;

/// A generic, typed, hardened notification channel. See module docs.
///
/// Cloning shares the same underlying listener set, id counter, and disposed
/// flag (`Arc`-backed), so a callback holding its own clone observes disposal
/// performed elsewhere — matching `ChangeNotifier`'s semantics.
#[derive(Clone)]
pub struct Notifier<Arg> {
    listeners: Arc<Mutex<HashMap<ListenerId, ArgCallback<Arg>>>>,
    next_id: Arc<AtomicUsize>,
    is_disposed: Arc<AtomicBool>,
}

impl<Arg> Default for Notifier<Arg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Arg> fmt::Debug for Notifier<Arg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Notifier")
            .field("listeners", &self.listeners.lock().len())
            .field("is_disposed", &self.is_disposed())
            .finish_non_exhaustive()
    }
}

impl<Arg> Notifier<Arg> {
    /// Create an empty notifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicUsize::new(1)),
            is_disposed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn mint_id(&self) -> ListenerId {
        ListenerId::new(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Whether [`dispose`](Self::dispose) has been called (shared across clones).
    #[must_use]
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.is_disposed.load(Ordering::Acquire)
    }

    /// Debug-panics if disposed; release degrades to `tracing::warn!` + no-op.
    /// Returns `true` if the caller should early-return (disposed in release).
    #[inline]
    fn check_disposed(&self) -> bool {
        if self.is_disposed.load(Ordering::Acquire) {
            #[cfg(debug_assertions)]
            panic!("Notifier used after dispose: once dispose() is called the channel is unusable");
            // Unreachable in debug (the panic above diverges); reached only in
            // release, where use-after-dispose degrades to a warn + no-op.
            #[allow(unreachable_code)]
            {
                tracing::warn!("Notifier used after dispose");
                return true;
            }
        }
        false
    }

    /// Register a listener; returns its id.
    pub fn add(&self, listener: ArgCallback<Arg>) -> ListenerId {
        if self.check_disposed() {
            return self.mint_id();
        }
        let id = self.mint_id();
        self.listeners.lock().insert(id, listener);
        id
    }

    /// Remove a previously registered listener. No-op if absent.
    ///
    /// Unlike `ChangeNotifier::remove_listener` (which tolerates post-dispose
    /// removal for Flutter parity), this generic notifier keeps its disposed
    /// gate: it has no Flutter reference contract, and `ListenerRegistry`'s
    /// Status-channel guard depends on the current shape. Re-seat on the
    /// parity semantics deliberately if the planned `ChangeNotifier` →
    /// `Notifier<()>` consolidation lands.
    pub fn remove(&self, id: ListenerId) {
        if self.check_disposed() {
            return;
        }
        self.listeners.lock().remove(&id);
    }

    /// Remove all listeners.
    pub fn remove_all(&self) {
        if self.check_disposed() {
            return;
        }
        self.listeners.lock().clear();
    }

    /// Number of registered listeners.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.listeners.lock().len()
    }

    /// Whether there are no listeners.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.listeners.lock().is_empty()
    }

    /// Discard listeners and mark disposed. Idempotent (second call is a no-op).
    pub fn dispose(&self) {
        if self.is_disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.listeners.lock().clear();
    }
}

impl<Arg: Clone> Notifier<Arg> {
    /// Fire every listener with `arg`, in registration order.
    ///
    /// Mirrors [`ChangeNotifier::notify_listeners`](crate::notifier::ChangeNotifier::notify_listeners):
    /// a snapshot of `(id, callback)` pairs is taken under lock, the lock is
    /// released before any callback fires (re-entrancy safe), each listener's
    /// live registration is re-checked so one removed mid-notify is skipped, and
    /// each callback is `catch_unwind`-isolated so a panicking listener does not
    /// abort the rest. Listeners fire in ascending [`ListenerId`] order (the
    /// backing `HashMap` is unordered, so the snapshot is sorted).
    pub fn notify(&self, arg: Arg) {
        if self.check_disposed() {
            return;
        }
        let mut snapshot: smallvec::SmallVec<[(ListenerId, ArgCallback<Arg>); 4]> = self
            .listeners
            .lock()
            .iter()
            .map(|(&id, cb)| (id, Arc::clone(cb)))
            .collect();
        snapshot.sort_unstable_by_key(|(id, _)| *id);

        for (id, callback) in &snapshot {
            // Skip a listener individually removed mid-notify (by an earlier
            // callback). Once disposed mid-flight, the snapshot is honoured to
            // completion (the disposed-state check ran at entry).
            if !self.is_disposed.load(Ordering::Acquire) && !self.listeners.lock().contains_key(id)
            {
                continue;
            }
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| callback(arg.clone()))) {
                tracing::error!(
                    listener_id = ?id,
                    panic_payload = ?payload,
                    "Notifier listener panicked; continuing with remaining listeners"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn delivers_arg_to_listener() {
        let n: Notifier<i32> = Notifier::new();
        let last = Arc::new(AtomicI32::new(0));
        let last2 = Arc::clone(&last);
        let _id = n.add(Arc::new(move |v: i32| last2.store(v, Ordering::SeqCst)));
        n.notify(7);
        assert_eq!(last.load(Ordering::SeqCst), 7);
    }

    #[test]
    fn fires_in_registration_order() {
        let n: Notifier<()> = Notifier::new();
        let log = Arc::new(Mutex::new(Vec::<u8>::new()));
        for k in 0u8..3 {
            let log = Arc::clone(&log);
            let _ = n.add(Arc::new(move |()| log.lock().push(k)));
        }
        n.notify(());
        assert_eq!(*log.lock(), vec![0, 1, 2]);
    }

    #[test]
    fn panicking_listener_does_not_abort_rest() {
        let n: Notifier<()> = Notifier::new();
        let ran = Arc::new(AtomicUsize::new(0));
        let _ = n.add(Arc::new(|()| panic!("boom")));
        let r = Arc::clone(&ran);
        let _ = n.add(Arc::new(move |()| {
            r.fetch_add(1, Ordering::SeqCst);
        }));
        n.notify(());
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn removed_during_notify_is_skipped() {
        let n: Notifier<()> = Notifier::new();
        let fired_b = Arc::new(AtomicUsize::new(0));
        let id_b_cell = Arc::new(Mutex::new(None::<ListenerId>));
        let n2 = n.clone();
        let cell2 = Arc::clone(&id_b_cell);
        let _a = n.add(Arc::new(move |()| {
            if let Some(id) = *cell2.lock() {
                n2.remove(id);
            }
        }));
        let fb = Arc::clone(&fired_b);
        let id_b = n.add(Arc::new(move |()| {
            fb.fetch_add(1, Ordering::SeqCst);
        }));
        *id_b_cell.lock() = Some(id_b);
        n.notify(());
        assert_eq!(fired_b.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn remove_and_len_and_dispose() {
        let n: Notifier<()> = Notifier::new();
        let id = n.add(Arc::new(|()| {}));
        assert_eq!(n.len(), 1);
        n.remove(id);
        assert_eq!(n.len(), 0);
        let _ = n.add(Arc::new(|()| {}));
        n.dispose();
        assert!(n.is_disposed());
        assert_eq!(n.len(), 0);
        n.dispose(); // idempotent — must not panic
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Notifier used after dispose")]
    fn notify_after_dispose_panics_in_debug() {
        let n: Notifier<()> = Notifier::new();
        n.dispose();
        n.notify(());
    }
}
