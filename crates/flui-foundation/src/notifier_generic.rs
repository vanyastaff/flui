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
//! It is also the core `ChangeNotifier` itself is seated on: `ChangeNotifier`
//! wraps a `Notifier<()>` and adds only its Flutter-parity seams (the branded
//! use-after-dispose message, and `remove_listener` tolerating a disposed
//! receiver via [`Notifier::remove_even_if_disposed`]).

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

    /// `pub(crate)` so [`crate::notifier::ChangeNotifier`]'s release-mode
    /// use-after-dispose path can hand out an unregistered id without
    /// tripping this notifier's own (differently-worded) disposed check.
    pub(crate) fn mint_id(&self) -> ListenerId {
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
            // release, where use-after-dispose degrades to a warn + no-op. The
            // lint therefore only exists in debug builds — hence `cfg_attr`.
            #[cfg_attr(debug_assertions, expect(unreachable_code))]
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
        self.add_unchecked(listener)
    }

    /// [`Self::add`] without the disposed gate — for wrappers
    /// (`ChangeNotifier`) that run their own differently-branded entry check
    /// first. A second gate here would turn a `dispose` racing in between the
    /// wrapper's check and this call into a generically-worded debug panic,
    /// where the historical single-check semantics let the already-started
    /// call proceed.
    pub(crate) fn add_unchecked(&self, listener: ArgCallback<Arg>) -> ListenerId {
        let id = self.mint_id();
        let evicted = self.listeners.lock().insert(id, listener);
        debug_assert!(
            evicted.is_none(),
            "listener ids are monotonic (see mint_id) — a fresh id can never \
             collide with a live registration"
        );
        id
    }

    /// Runs `mutate` against the locked listener map and returns whatever it
    /// extracts (a removed listener, or the whole displaced map on a clear),
    /// only after this notifier's own lock has released.
    ///
    /// A removed/displaced [`ArgCallback`]'s `Drop` may run arbitrary user
    /// code: the last `Arc` clone of a listener closure reaching zero can
    /// destroy captured state whose own destructor calls back into this same
    /// notifier (e.g. another `remove`/`dispose`). `mutate` itself only
    /// touches the map while the guard is held; its *return value* is not
    /// dropped here — it is this function's tail expression, so the guard
    /// (a temporary scoped to this function body) releases before the
    /// caller ever receives, and can drop, the extracted value.
    fn extract_locked<T>(
        &self,
        mutate: impl FnOnce(&mut HashMap<ListenerId, ArgCallback<Arg>>) -> T,
    ) -> T {
        mutate(&mut self.listeners.lock())
    }

    /// Remove a previously registered listener. No-op if absent.
    ///
    /// Unlike `ChangeNotifier::remove_listener` (which tolerates post-dispose
    /// removal for Flutter parity), this generic notifier keeps its disposed
    /// gate: it has no Flutter reference contract, and `ListenerRegistry`'s
    /// Status-channel guard depends on the current shape. The parity
    /// behaviour lives in [`Self::remove_even_if_disposed`], which
    /// `ChangeNotifier` delegates to.
    pub fn remove(&self, id: ListenerId) {
        if self.check_disposed() {
            return;
        }
        drop(self.extract_locked(|listeners| listeners.remove(&id)));
    }

    /// [`Self::remove`] without the disposed gate: always a silent no-op on a
    /// disposed channel (whose listener map is already empty).
    ///
    /// This is the Flutter-parity flavour `ChangeNotifier::remove_listener`
    /// needs — `ChangeNotifier.removeListener` upstream carries no
    /// `debugAssertNotDisposed` so teardown code can always detach.
    pub fn remove_even_if_disposed(&self, id: ListenerId) {
        drop(self.extract_locked(|listeners| listeners.remove(&id)));
    }

    /// Remove all listeners.
    pub fn remove_all(&self) {
        if self.check_disposed() {
            return;
        }
        self.remove_all_unchecked();
    }

    /// [`Self::remove_all`] without the disposed gate — same single-check
    /// rationale as [`Self::add_unchecked`]; racing a `dispose` here is
    /// harmless (both clear the same map).
    pub(crate) fn remove_all_unchecked(&self) {
        drop(self.extract_locked(std::mem::take));
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
        drop(self.extract_locked(std::mem::take));
    }

    /// Test-only probe: `true` if `listeners` is currently free to lock.
    ///
    /// Backs a regression test for `extract_locked`'s drop-after-release
    /// ordering: a listener whose own `Drop` re-enters this same notifier
    /// (another `remove`/`dispose` call) must observe the lock already
    /// free, not deadlock on it.
    #[cfg(test)]
    pub(crate) fn is_unlocked(&self) -> bool {
        self.listeners.try_lock().is_some()
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
    /// backing `HashMap` is unordered, so the snapshot is sorted). Listeners
    /// *added* during a notify round are NOT fired in that round — only in the
    /// next one (same round-N-vs-round-N+1 rule as `notify_listeners`).
    pub fn notify(&self, arg: Arg) {
        if self.check_disposed() {
            return;
        }
        self.notify_unchecked(arg);
    }

    /// [`Self::notify`] without the entry disposed gate — same single-check
    /// rationale as [`Self::add_unchecked`]. The firing loop itself is
    /// dispose-safe either way: once the snapshot is taken it is honoured to
    /// completion, and a fully-disposed (empty) listener map simply yields an
    /// empty snapshot.
    pub(crate) fn notify_unchecked(&self, arg: Arg) {
        // Stack-allocate the snapshot for the common case (1-4 listeners);
        // ≥5 spills to the heap. `SmallVec` over `tinyvec::ArrayVec`
        // deliberately: the callbacks are `Arc<dyn Fn(..)>`, which does not
        // implement `Default`, and `tinyvec` requires `T: Default`.
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
                    panic_payload = crate::panic::payload_text(&*payload)
                        .unwrap_or("<non-string panic payload>"),
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
        let _prev = id_b_cell.lock().replace(id_b);
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

    /// A listener whose own `Drop` re-enters the notifier, probing whether
    /// the lock is still held.
    struct ReentrantDropCanary {
        notifier: Notifier<()>,
        observed_locked: Arc<AtomicBool>,
    }

    impl Drop for ReentrantDropCanary {
        fn drop(&mut self) {
            // Probe first, THEN decide whether to reenter: attempting the
            // reentrant call unconditionally would make a real regression
            // (the lock still held here) actually deadlock on it, turning
            // this test into a hang instead of a fast assertion failure.
            if self.notifier.is_unlocked() {
                // Reentrant call into the same notifier from inside a
                // removed listener's own destructor. `remove_even_if_disposed`
                // (not `remove`) because the `dispose` variant of this canary
                // runs after `is_disposed` is already `true`, and `remove`
                // would debug-panic on that gate — this canary only needs to
                // prove the LOCK is free to reacquire, independent of that
                // gate.
                self.notifier
                    .remove_even_if_disposed(ListenerId::new(999_999));
            } else {
                self.observed_locked.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Pins `Notifier::remove`'s extract-then-drop ordering (via
    /// `extract_locked`, backing both `remove` and
    /// `remove_even_if_disposed`): reverting to a bare
    /// `self.listeners.lock().remove(&id);` statement drops the removed
    /// callback while its own guard is still live, so a listener whose
    /// `Drop` re-enters this notifier deadlocks on `listeners`.
    #[test]
    fn remove_drops_the_removed_listener_after_releasing_the_lock() {
        let n: Notifier<()> = Notifier::new();
        let observed_locked = Arc::new(AtomicBool::new(false));
        let canary = ReentrantDropCanary {
            notifier: n.clone(),
            observed_locked: Arc::clone(&observed_locked),
        };
        let id = n.add(Arc::new(move |()| {
            let _keep_alive = &canary;
        }));

        n.remove(id);

        assert!(
            !observed_locked.load(Ordering::SeqCst),
            "the removed listener's Drop observed the notifier's lock still held"
        );
    }

    /// Pins `Notifier::dispose`'s extract-then-drop ordering (via
    /// `extract_locked`, backing both `remove_all_unchecked` and
    /// `dispose`): reverting to a bare `self.listeners.lock().clear();`
    /// statement drops every cleared callback while the guard is still
    /// live, so a listener whose `Drop` re-enters this notifier deadlocks
    /// on `listeners`.
    #[test]
    fn dispose_drops_every_removed_listener_after_releasing_the_lock() {
        let n: Notifier<()> = Notifier::new();
        let observed_locked = Arc::new(AtomicBool::new(false));
        let canary = ReentrantDropCanary {
            notifier: n.clone(),
            observed_locked: Arc::clone(&observed_locked),
        };
        let _id = n.add(Arc::new(move |()| {
            let _keep_alive = &canary;
        }));

        n.dispose();

        assert!(
            !observed_locked.load(Ordering::SeqCst),
            "a disposed listener's Drop observed the notifier's lock still held"
        );
    }
}
