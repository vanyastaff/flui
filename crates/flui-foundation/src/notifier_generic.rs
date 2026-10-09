//! `Notifier<Arg>` — a generic, typed, hardened notification channel.
//!
//! Generalizes [`crate::notifier::ChangeNotifier`] (which is effectively
//! `Notifier<()>`) to lend an argument to each listener. Reuses the
//! same firing discipline proven in [`crate::notifier::ChangeNotifier::notify_listeners`]:
//! snapshot under a borrow, registration-order firing, release before callbacks,
//! per-callback `catch_unwind`, remove-during-notify skip, and a dispose guard.
//!
//! It is also the core `ChangeNotifier` itself is seated on: `ChangeNotifier`
//! wraps a `Notifier<()>` and adds only its own seams (the branded
//! use-after-dispose message, and `remove_listener` tolerating a disposed
//! receiver via [`Notifier::remove_even_if_disposed`]).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use crate::id::ListenerId;
use crate::panic::PanicRecovery;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A listener callback that borrows its argument for the duration of the call.
pub type ArgCallback<Arg> = Rc<dyn Fn(&Arg) + 'static>;

/// Framework relay that borrows the enclosing delivery's failure custody.
#[doc(hidden)]
pub type RecoveryCallback<Arg> = Rc<dyn Fn(&Arg, &mut PanicRecovery)>;

/// A callback envelope withdrawn by a framework owner before retirement.
#[doc(hidden)]
pub struct NotificationCallback<Arg>(CallbackKind<Arg>);

enum CallbackKind<Arg> {
    User(ArgCallback<Arg>),
    Relay(RecoveryCallback<Arg>),
}

type OwnedCallback<Arg> = Rc<NotificationCallback<Arg>>;

impl<Arg> NotificationCallback<Arg> {
    /// Package a withdrawn user callback for enclosing-owner retirement.
    #[doc(hidden)]
    pub fn user(callback: ArgCallback<Arg>) -> Rc<Self> {
        Rc::new(Self(CallbackKind::User(callback)))
    }
    /// Package a refused framework relay for enclosing-owner retirement.
    #[doc(hidden)]
    pub fn relay(callback: RecoveryCallback<Arg>) -> Rc<Self> {
        Rc::new(Self(CallbackKind::Relay(callback)))
    }
    fn invoke(&self, arg: &Arg, recovery: &mut PanicRecovery) {
        match &self.0 {
            CallbackKind::User(callback) => callback(arg),
            CallbackKind::Relay(callback) => callback(arg, recovery),
        }
    }
}

impl<Arg> fmt::Debug for NotificationCallback<Arg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotificationCallback")
            .finish_non_exhaustive()
    }
}

/// The physical shared owner, including retirement of the final listener set.
struct ListenerStorage<Arg> {
    entries: RefCell<HashMap<ListenerId, OwnedCallback<Arg>>>,
    delivery_depth: Cell<usize>,
    failed: Arc<AtomicBool>,
    retired: RefCell<Vec<(ListenerId, OwnedCallback<Arg>)>>,
}

impl<Arg> Drop for ListenerStorage<Arg> {
    fn drop(&mut self) {
        // Empty the field before any capture can unwind through its drop glue.
        // Final Rc ownership gives exclusive access without holding a borrow.
        let mut callbacks = std::mem::take(self.retired.get_mut());
        callbacks.extend(std::mem::take(self.entries.get_mut()));
        retire_callbacks(callbacks);
    }
}

/// Separate callback envelopes must never become one aggregate drop boundary.
struct RetiringListeners<Arg>(Vec<(ListenerId, OwnedCallback<Arg>)>);

impl<Arg> Drop for RetiringListeners<Arg> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // A callback already failed. Its untouched successors must not
            // introduce another destructor failure while that panic propagates.
            std::mem::forget(std::mem::take(&mut self.0));
        }
    }
}

fn retire_listeners<Arg>(listeners: HashMap<ListenerId, OwnedCallback<Arg>>) {
    retire_callbacks(listeners.into_iter().collect());
}

fn retire_callbacks<Arg>(callbacks: Vec<(ListenerId, OwnedCallback<Arg>)>) {
    if std::thread::panicking() {
        // Preserve an incoming unwind without running opaque captures at all.
        std::mem::forget(callbacks);
        return;
    }
    let mut retiring = RetiringListeners(callbacks);
    retiring.0.sort_unstable_by_key(|(id, _)| *id);
    retiring.0.reverse();
    while let Some((_, callback)) = retiring.0.pop() {
        drop(callback);
    }
}

fn retire_listener<Arg>(listener: Option<OwnedCallback<Arg>>) {
    if std::thread::panicking() {
        std::mem::forget(listener);
    } else {
        drop(listener);
    }
}

/// A generic, typed, hardened notification channel. See module docs.
///
/// Cloning shares the same underlying listener set, id counter, and disposed
/// flag (`Rc`-backed), so a callback holding its own clone observes disposal
/// performed elsewhere — matching `ChangeNotifier`'s semantics.
///
/// Removal, clearing, disposal and final-owner destruction drop callbacks
/// outside the borrow, in registration order. After the first destructor panic,
/// or while the thread is already panicking, the remaining callbacks are
/// retained rather than dropped (ADR-0127).
pub struct Notifier<Arg> {
    listeners: Rc<ListenerStorage<Arg>>,
    next_id: Rc<Cell<usize>>,
    is_disposed: Rc<Cell<bool>>,
}

impl<Arg> Clone for Notifier<Arg> {
    fn clone(&self) -> Self {
        Self {
            listeners: Rc::clone(&self.listeners),
            next_id: Rc::clone(&self.next_id),
            is_disposed: Rc::clone(&self.is_disposed),
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
            .field("listeners", &self.listeners.entries.borrow_mut().len())
            .field("is_disposed", &self.is_disposed())
            .finish_non_exhaustive()
    }
}

impl<Arg> Notifier<Arg> {
    /// Create an empty notifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            listeners: Rc::new(ListenerStorage {
                entries: RefCell::new(HashMap::new()),
                delivery_depth: Cell::new(0),
                failed: Arc::new(AtomicBool::new(false)),
                retired: RefCell::new(Vec::new()),
            }),
            next_id: Rc::new(Cell::new(1)),
            is_disposed: Rc::new(Cell::new(false)),
        }
    }

    /// `pub(crate)` so [`crate::notifier::ChangeNotifier`]'s release-mode
    /// use-after-dispose path can hand out an unregistered id without
    /// tripping this notifier's own (differently-worded) disposed check.
    pub(crate) fn mint_id(&self) -> ListenerId {
        let id = self.next_id.get();
        self.next_id.set(
            id.checked_add(1)
                .expect("BUG: listener identities exhausted"),
        );
        ListenerId::new(id)
    }

    /// Whether [`dispose`](Self::dispose) has been called (shared across clones).
    #[must_use]
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    /// Debug-panics if disposed; release degrades to `tracing::warn!` + no-op.
    /// Returns `true` if the caller should early-return (disposed in release).
    #[inline]
    fn check_disposed(&self) -> bool {
        if self.is_disposed.get() {
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
        self.admit_callback(NotificationCallback::user(listener))
    }

    /// Register a framework relay in the same ordered notification channel.
    #[doc(hidden)]
    pub fn add_with_recovery(&self, listener: RecoveryCallback<Arg>) -> ListenerId {
        if self.check_disposed() {
            return self.mint_id();
        }
        self.admit_callback(NotificationCallback::relay(listener))
    }

    fn admit_callback(&self, listener: OwnedCallback<Arg>) -> ListenerId {
        let id = self.mint_id();
        let evicted = self.listeners.entries.borrow_mut().insert(id, listener);
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
        mutate: impl FnOnce(&mut HashMap<ListenerId, OwnedCallback<Arg>>) -> T,
    ) -> T {
        mutate(&mut self.listeners.entries.borrow_mut())
    }

    /// Remove a previously registered listener. No-op if absent.
    ///
    /// Unlike `ChangeNotifier::remove_listener` (which tolerates post-dispose
    /// removal), this generic notifier keeps its disposed gate.
    /// The tolerant behaviour lives in [`Self::remove_even_if_disposed`],
    /// which `ChangeNotifier` delegates to.
    pub fn remove(&self, id: ListenerId) {
        if self.check_disposed() {
            return;
        }
        self.retire_removed(id, self.extract_locked(|listeners| listeners.remove(&id)));
    }

    /// [`Self::remove`] without the disposed gate: always a silent no-op on a
    /// disposed channel (whose listener map is already empty).
    ///
    /// This is the flavour `ChangeNotifier::remove_listener` needs, so that
    /// teardown code can always detach without tripping a use-after-dispose
    /// check.
    pub fn remove_even_if_disposed(&self, id: ListenerId) {
        self.retire_removed(id, self.extract_locked(|listeners| listeners.remove(&id)));
    }

    #[doc(hidden)]
    #[must_use]
    pub fn take_callback(&self, id: ListenerId) -> Option<OwnedCallback<Arg>> {
        self.extract_locked(|listeners| listeners.remove(&id))
    }

    #[doc(hidden)]
    pub fn take_all_callbacks(&self) -> Vec<OwnedCallback<Arg>> {
        let mut callbacks: Vec<_> = self.extract_locked(std::mem::take).into_iter().collect();
        callbacks.sort_unstable_by_key(|(id, _)| *id);
        callbacks
            .into_iter()
            .map(|(_, callback)| callback)
            .collect()
    }

    #[doc(hidden)]
    #[must_use]
    pub fn dispose_and_take_callbacks(&self) -> Vec<OwnedCallback<Arg>> {
        self.is_disposed.set(true);
        self.take_all_callbacks()
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
        self.retire_removed_map(self.extract_locked(std::mem::take));
    }

    /// Number of registered listeners.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.listeners.entries.borrow_mut().len()
    }

    /// Whether there are no listeners.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.listeners.entries.borrow_mut().is_empty()
    }

    /// Capture membership when a framework owner commits a queued notification.
    #[doc(hidden)]
    #[must_use]
    pub fn listener_ids(&self) -> Vec<ListenerId> {
        let mut ids: Vec<_> = self.listeners.entries.borrow().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Carry this active channel's enclosing failure into reentrant owner cleanup.
    #[doc(hidden)]
    pub fn inherit_failure(&self, recovery: &mut PanicRecovery) {
        if self.listeners.delivery_depth.get() != 0 && self.listeners.failed.load(Ordering::Relaxed)
        {
            recovery.inherit_failure();
        }
    }

    /// Discard listeners and mark disposed. Idempotent (second call is a no-op).
    pub fn dispose(&self) {
        if self.is_disposed.replace(true) {
            return;
        }
        self.retire_removed_map(self.extract_locked(std::mem::take));
    }

    fn retire_removed(&self, id: ListenerId, callback: Option<OwnedCallback<Arg>>) {
        if self.listeners.delivery_depth.get() != 0 {
            if let Some(callback) = callback {
                self.listeners.retired.borrow_mut().push((id, callback));
            }
        } else {
            retire_listener(callback);
        }
    }

    fn retire_removed_map(&self, callbacks: HashMap<ListenerId, OwnedCallback<Arg>>) {
        if self.listeners.delivery_depth.get() != 0 {
            self.listeners.retired.borrow_mut().extend(callbacks);
        } else {
            retire_listeners(callbacks);
        }
    }
}

impl<Arg> Notifier<Arg> {
    /// Fire every listener with `arg`, in registration order.
    ///
    /// Mirrors [`ChangeNotifier::notify_listeners`](crate::notifier::ChangeNotifier::notify_listeners):
    /// a snapshot of `(id, callback)` pairs is taken under a borrow, which is
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
    /// dispose-safe either way: disposing the channel skips its remaining
    /// snapshot entries, and a disposed listener map yields an empty snapshot.
    pub(crate) fn notify_unchecked(&self, arg: &Arg) {
        let mut recovery = PanicRecovery::new();
        if self.listeners.failed.load(Ordering::Relaxed) {
            recovery.inherit_failure();
        }
        self.notify_unchecked_with_recovery(arg, &mut recovery);
        recovery.finish();
    }

    /// Notify within an enclosing framework delivery's first-failure custody.
    #[doc(hidden)]
    pub fn notify_with_recovery(&self, arg: &Arg, recovery: &mut PanicRecovery) {
        if !self.check_disposed() {
            self.notify_unchecked_with_recovery(arg, recovery);
        }
    }

    pub(crate) fn notify_unchecked_with_recovery(&self, arg: &Arg, recovery: &mut PanicRecovery) {
        self.notify_selection(arg, None, recovery);
    }

    /// Deliver previously admitted membership, skipping removed subscriptions.
    #[doc(hidden)]
    pub fn notify_selected_with_recovery(
        &self,
        arg: &Arg,
        ids: &[ListenerId],
        recovery: &mut PanicRecovery,
    ) {
        if !self.check_disposed() {
            self.notify_selection(arg, Some(ids), recovery);
        }
    }

    fn notify_selection(
        &self,
        arg: &Arg,
        ids: Option<&[ListenerId]>,
        recovery: &mut PanicRecovery,
    ) {
        recovery.with_failure_latch(Arc::clone(&self.listeners.failed), |recovery| {
            self.listeners
                .delivery_depth
                .set(self.listeners.delivery_depth.get() + 1);
            // Stack-allocate the snapshot for the common case (1-4 listeners);
            // ≥5 spills to the heap. `SmallVec` over `tinyvec::ArrayVec`
            // deliberately: the callbacks are `Rc<dyn Fn(..)>`, which does not
            // implement `Default`, and `tinyvec` requires `T: Default`.
            let mut snapshot: smallvec::SmallVec<[(ListenerId, OwnedCallback<Arg>); 4]> = self
                .listeners
                .entries
                .borrow_mut()
                .iter()
                .filter(|(id, _)| ids.is_none_or(|ids| ids.binary_search(id).is_ok()))
                .map(|(&id, cb)| (id, Rc::clone(cb)))
                .collect();
            snapshot.sort_unstable_by_key(|(id, _)| *id);
            let mut snapshot = std::mem::ManuallyDrop::new(snapshot);
            for (id, callback) in snapshot.iter() {
                // Skip a listener individually removed mid-notify (by an earlier
                // callback), and skip the tail once the channel is disposed.
                if self.is_disposed.get() || !self.listeners.entries.borrow().contains_key(id) {
                    continue;
                }
                if let Err(payload) =
                    catch_unwind(AssertUnwindSafe(|| callback.invoke(arg, recovery)))
                {
                    let failure_text = crate::panic::payload_text(payload.as_ref())
                        .unwrap_or("<non-string panic payload>")
                        .to_owned();
                    // Own the failure before diagnostics or nested retirement.
                    recovery.capture(payload);
                    recovery.run(|| {
                        tracing::error!(
                            listener_id = ?id,
                            panic_payload = failure_text,
                            "Notifier listener panicked; continuing with remaining listeners"
                        );
                    });
                }
            }
            // Retire separate envelopes one at a time. If one destructor fails,
            // the remaining snapshot must not add another failure during unwind.
            snapshot.reverse();
            while let Some((_, callback)) = snapshot.pop() {
                recovery.retire(callback);
            }
            drop(std::mem::ManuallyDrop::into_inner(snapshot));
            if self.listeners.delivery_depth.get() == 1 {
                loop {
                    let mut retired = std::mem::take(&mut *self.listeners.retired.borrow_mut());
                    if retired.is_empty() {
                        break;
                    }
                    retired.sort_unstable_by_key(|(id, _)| *id);
                    for (_, callback) in retired {
                        recovery.retire(callback);
                    }
                }
                self.listeners.failed.store(false, Ordering::Relaxed);
            }
            self.listeners
                .delivery_depth
                .set(self.listeners.delivery_depth.get() - 1);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn panicking_listener_does_not_abort_rest() {
        let n: Notifier<()> = Notifier::new();
        let ran = Rc::new(AtomicUsize::new(0));
        let _ = n.add(Rc::new(|&()| panic!("boom")));
        let r = Rc::clone(&ran);
        let _ = n.add(Rc::new(move |&()| {
            r.fetch_add(1, Ordering::SeqCst);
        }));
        let failure = catch_unwind(AssertUnwindSafe(|| n.notify(&())))
            .expect_err("first listener failure propagates after the tail");
        assert_eq!(crate::panic::payload_text(&*failure), Some("boom"));
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    fn removed_during_notify_is_skipped() {
        let n: Notifier<()> = Notifier::new();
        let fired_b = Rc::new(AtomicUsize::new(0));
        let id_b_cell = Rc::new(RefCell::new(None::<ListenerId>));
        let n2 = n.clone();
        let cell2 = Rc::clone(&id_b_cell);
        let _a = n.add(Rc::new(move |&()| {
            let id = *cell2.borrow_mut();
            if let Some(id) = id {
                n2.remove(id);
            }
        }));
        let fb = Rc::clone(&fired_b);
        let id_b = n.add(Rc::new(move |&()| {
            fb.fetch_add(1, Ordering::SeqCst);
        }));
        let _prev = id_b_cell.borrow_mut().replace(id_b);
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
