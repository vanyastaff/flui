//! `Notifier<Arg>` — a generic, typed, hardened notification channel.
//!
//! Generalizes [`crate::notifier::ChangeNotifier`] (which is effectively
//! `Notifier<()>`) to lend an argument to each listener. Reuses the
//! same firing discipline proven in [`crate::notifier::ChangeNotifier::notify_listeners`]:
//! snapshot-under-lock, registration-order firing, drop-lock before callbacks,
//! per-callback `catch_unwind`, remove-during-notify skip, and a dispose guard.
//!
//! This is the substrate the animation crate composes into a
//! [`crate::listener_registry::ListenerRegistry`]: the *value* channel is a
//! `Notifier<()>` and the *status* channel is a `Notifier<AnimationStatus>`.
//!
//! It is also the core `ChangeNotifier` itself is seated on: `ChangeNotifier`
//! wraps a `Notifier<()>` and adds only its own seams (the branded
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

/// A listener callback that borrows its argument for the duration of the call.
pub type ArgCallback<Arg> = Arc<dyn Fn(&Arg) + Send + Sync + 'static>;

/// A generic, typed, hardened notification channel. See module docs.
///
/// Cloning shares the same underlying listener set, id counter, and disposed
/// flag (`Arc`-backed), so a callback holding its own clone observes disposal
/// performed elsewhere — matching `ChangeNotifier`'s semantics.
pub struct Notifier<Arg> {
    listeners: Arc<Mutex<HashMap<ListenerId, ArgCallback<Arg>>>>,
    next_id: Arc<AtomicUsize>,
    is_disposed: Arc<AtomicBool>,
}

impl<Arg> Clone for Notifier<Arg> {
    fn clone(&self) -> Self {
        Self {
            listeners: Arc::clone(&self.listeners),
            next_id: Arc::clone(&self.next_id),
            is_disposed: Arc::clone(&self.is_disposed),
        }
    }
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
    /// removal), this generic notifier keeps its disposed gate:
    /// `ListenerRegistry`'s Status-channel guard depends on the current shape.
    /// The tolerant behaviour lives in [`Self::remove_even_if_disposed`],
    /// which `ChangeNotifier` delegates to.
    pub fn remove(&self, id: ListenerId) {
        if self.check_disposed() {
            return;
        }
        drop(self.extract_locked(|listeners| listeners.remove(&id)));
    }

    /// [`Self::remove`] without the disposed gate: always a silent no-op on a
    /// disposed channel (whose listener map is already empty).
    ///
    /// This is the flavour `ChangeNotifier::remove_listener` needs, so that
    /// teardown code can always detach without tripping a use-after-dispose
    /// check.
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
}

impl<Arg> Notifier<Arg> {
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
    /// Arguments are borrowed: notification never clones or destroys the value.
    /// After a caught listener panic, its payload and callback snapshot are
    /// retained rather than running opaque capture destructors. On a successful
    /// round, callbacks retire normally; the first retirement panic propagates
    /// with remaining envelopes retained. A single callback whose own capture
    /// aggregate double-panics during ordinary destruction remains subject to
    /// Rust's abort semantics, as does double-panic inside user callback code.
    pub fn notify(&self, arg: &Arg) {
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
    pub(crate) fn notify_unchecked(&self, arg: &Arg) {
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
        let mut snapshot = std::mem::ManuallyDrop::new(snapshot);
        let mut listener_failed = false;

        for (id, callback) in snapshot.iter() {
            // Skip a listener individually removed mid-notify (by an earlier
            // callback). Once disposed mid-flight, the snapshot is honoured to
            // completion (the disposed-state check ran at entry).
            if !self.is_disposed.load(Ordering::Acquire) && !self.listeners.lock().contains_key(id)
            {
                continue;
            }
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| callback(arg))) {
                listener_failed = true;
                let text = crate::panic::payload_text(&*payload)
                    .unwrap_or("<non-string panic payload>")
                    .to_owned();
                crate::panic::retain_opaque_payload(payload);
                tracing::error!(
                    listener_id = ?id,
                    panic_payload = text,
                    "Notifier listener panicked; continuing with remaining listeners"
                );
            }
        }
        if listener_failed {
            // Self-removal may leave this snapshot owning the last callback
            // envelope. Its opaque captures must not drop after containment.
            return;
        }
        // Retire separate envelopes one at a time. If one destructor fails,
        // the remaining snapshot must not add another failure during unwind.
        snapshot.reverse();
        while let Some((_, callback)) = snapshot.pop() {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(callback))) {
                std::panic::resume_unwind(payload);
            }
        }
        drop(std::mem::ManuallyDrop::into_inner(snapshot));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn panicking_listener_does_not_abort_rest() {
        let n: Notifier<()> = Notifier::new();
        let ran = Arc::new(AtomicUsize::new(0));
        let _ = n.add(Arc::new(|&()| panic!("boom")));
        let r = Arc::clone(&ran);
        let _ = n.add(Arc::new(move |&()| {
            r.fetch_add(1, Ordering::SeqCst);
        }));
        n.notify(&());
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    fn removed_during_notify_is_skipped() {
        let n: Notifier<()> = Notifier::new();
        let fired_b = Arc::new(AtomicUsize::new(0));
        let id_b_cell = Arc::new(Mutex::new(None::<ListenerId>));
        let n2 = n.clone();
        let cell2 = Arc::clone(&id_b_cell);
        let _a = n.add(Arc::new(move |&()| {
            let id = *cell2.lock();
            if let Some(id) = id {
                n2.remove(id);
            }
        }));
        let fb = Arc::clone(&fired_b);
        let id_b = n.add(Arc::new(move |&()| {
            fb.fetch_add(1, Ordering::SeqCst);
        }));
        let _prev = id_b_cell.lock().replace(id_b);
        n.notify(&());
        assert_eq!(fired_b.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn notifier_generic_contract() {
        crate::test_cases::run_cases(&[
            (
                "panicking listener does not abort rest",
                panicking_listener_does_not_abort_rest,
            ),
            (
                "removed during notify is skipped",
                removed_during_notify_is_skipped,
            ),
        ]);
    }
}
