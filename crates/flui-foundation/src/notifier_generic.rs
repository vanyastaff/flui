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

use crate::panic::RecoveryScope;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use crate::notifier::ListenerCallback;
use crate::panic::PanicRecovery;
use crate::sequence::IdMint;
use crate::sequence::ListenerId;
use smallvec::SmallVec;
use std::sync::atomic::{AtomicBool, Ordering};

/// A listener callback that borrows its argument for the duration of the call.
pub type ArgCallback<Arg> = Rc<dyn Fn(&Arg) + 'static>;

/// A callback that runs inside the notifying delivery's [`RecoveryScope`]: the
/// form a relay registers so the first failure of the whole delivery stays
/// authoritative (for example a custom animation's `observe_status`).
pub type RecoveryCallback<Arg> = Rc<dyn Fn(&Arg, &mut RecoveryScope<'_>)>;

/// A registered callback withdrawn from its channel, for the withdrawing owner
/// to retire through a [`RecoveryScope`] (see [`Notifier::take_callback`]).
pub struct NotificationCallback<Arg>(CallbackKind<Arg>);

enum CallbackKind<Arg> {
    User(ArgCallback<Arg>),
    Relay(RecoveryCallback<Arg>),
    /// A listener that takes no argument, held as it was given: no adapter
    /// is allocated to drop the argument.
    Plain(crate::notifier::ListenerCallback),
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
    fn invoke(&self, arg: &Arg, recovery: &mut RecoveryScope<'_>) {
        match &self.0 {
            CallbackKind::User(callback) => callback(arg),
            CallbackKind::Relay(callback) => callback(arg, recovery),
            CallbackKind::Plain(callback) => callback(),
        }
    }
}

impl<Arg> fmt::Debug for NotificationCallback<Arg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotificationCallback")
            .finish_non_exhaustive()
    }
}

/// The registrations of one channel, in registration order.
///
/// Identities are minted in ascending order, so the entries are sorted by
/// the order they arrive in: a lookup is a binary search, a delivery walks
/// them as they lie, and a channel with one listener holds it inline, with
/// no table beside it. A withdrawn registration leaves its slot empty until
/// the empty slots outnumber the live ones, so withdrawing every listener of
/// a large channel one at a time is linear in all, not quadratic.
struct Listeners<Arg> {
    slots: SmallVec<[(ListenerId, Option<OwnedCallback<Arg>>); 1]>,
    live: usize,
}

impl<Arg> Default for Listeners<Arg> {
    fn default() -> Self {
        Self {
            slots: SmallVec::new(),
            live: 0,
        }
    }
}

impl<Arg> Listeners<Arg> {
    fn len(&self) -> usize {
        self.live
    }

    fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Registers `callback` under `id`, which is newer than every identity
    /// this channel holds.
    fn admit(&mut self, id: ListenerId, callback: OwnedCallback<Arg>) {
        debug_assert!(
            self.slots.last().is_none_or(|(last, _)| *last < id),
            "listener ids are monotonic (see mint_id) — a fresh id is never \
             older than a registration"
        );
        self.slots.push((id, Some(callback)));
        self.live += 1;
    }

    fn slot(&self, id: ListenerId) -> Option<usize> {
        self.slots.binary_search_by_key(&id, |(slot, _)| *slot).ok()
    }

    fn contains(&self, id: ListenerId) -> bool {
        self.slot(id)
            .is_some_and(|index| self.slots[index].1.is_some())
    }

    /// Withdraws the registration `id` names and hands its callback out, for
    /// the caller to destroy outside the channel's borrow.
    fn withdraw(&mut self, id: ListenerId) -> Option<OwnedCallback<Arg>> {
        let index = self.slot(id)?;
        let callback = self.slots[index].1.take()?;
        self.live -= 1;
        // Only empty slots are removed here, and an empty slot owns nothing:
        // no destructor runs under the borrow.
        if self.slots.len() - self.live > self.live {
            self.slots.retain(|(_, callback)| callback.is_some());
        }
        Some(callback)
    }

    /// The live registrations, oldest first.
    fn iter(&self) -> impl Iterator<Item = (ListenerId, &OwnedCallback<Arg>)> {
        self.slots
            .iter()
            .filter_map(|(id, callback)| Some((*id, callback.as_ref()?)))
    }

    /// Every live registration, oldest first, leaving the channel empty.
    fn into_callbacks(self) -> impl Iterator<Item = (ListenerId, OwnedCallback<Arg>)> {
        self.slots
            .into_iter()
            .filter_map(|(id, callback)| Some((id, callback?)))
    }
}

/// The physical shared owner, including retirement of the final listener set.
struct ListenerStorage<Arg> {
    entries: RefCell<Listeners<Arg>>,
    delivery_depth: Cell<usize>,
    failed: AtomicBool,
    retired: RefCell<Vec<(ListenerId, OwnedCallback<Arg>)>>,
    /// Captures handed over by [`Notifier::retire_after_delivery`], retired
    /// after this round's own removed listeners.
    deferred: RefCell<Vec<Box<dyn Any>>>,
    /// Listener identities: the owner-local mint whose refusal is sticky, so
    /// a listener id is never issued twice.
    next_id: IdMint<ListenerId>,
    is_disposed: Cell<bool>,
}

impl<Arg> Drop for ListenerStorage<Arg> {
    fn drop(&mut self) {
        // Empty the field before any capture can unwind through its drop glue.
        // Final Rc ownership gives exclusive access without holding a borrow.
        let mut callbacks = std::mem::take(self.retired.get_mut());
        callbacks.extend(std::mem::take(self.entries.get_mut()).into_callbacks());
        let deferred = std::mem::take(self.deferred.get_mut());
        retire_callbacks(callbacks);
        drop(deferred);
    }
}

fn retire_listeners<Arg>(listeners: Listeners<Arg>) {
    retire_callbacks(listeners.into_callbacks().collect());
}

/// Drop callbacks in registration order. If one destructor panics, the rest
/// are still destroyed and the first panic propagates; one that panics during
/// that unwind aborts, as in std (ADR-0281).
fn retire_callbacks<Arg>(mut callbacks: Vec<(ListenerId, OwnedCallback<Arg>)>) {
    callbacks.sort_unstable_by_key(|(id, _)| *id);
    drop(callbacks);
}

/// The channel a [`Subscription`](crate::notifier::Subscription) holds as a
/// weak: the removal of its own listener. Sealed: the only implementations
/// are this crate's shared listener storages, so a subscription can only
/// detach through the channel it subscribed on — a [`ListenerId`]'s value
/// never names another channel's registration.
pub(crate) trait SubscribedChannel {
    fn detach_listener(&self, id: ListenerId);
}

impl<Arg: 'static> SubscribedChannel for ListenerStorage<Arg> {
    /// The removal that stays committed on every exit: the entry leaves the
    /// map under its borrow, the callback destroys outside it — through the
    /// delivery's retired list while a notify round is in progress, whose
    /// containment the round's tail provides (ADR-0287).
    fn detach_listener(&self, id: ListenerId) {
        let callback = self.entries.borrow_mut().withdraw(id);
        if let Some(callback) = callback {
            if self.delivery_depth.get() != 0 {
                self.retired.borrow_mut().push((id, callback));
            } else {
                drop(callback);
            }
        }
    }
}

/// A generic, typed, hardened notification channel. See module docs.
///
/// Cloning shares the same underlying listener set, id counter, and disposed
/// flag (`Rc`-backed), so a callback holding its own clone observes disposal
/// performed elsewhere — matching `ChangeNotifier`'s semantics.
///
/// Removal, clearing, disposal and final-owner destruction drop callbacks
/// outside the borrow, in registration order. If a destructor panics, the
/// rest are still dropped and the first panic propagates; one that panics
/// while the thread is already unwinding aborts, as in std (ADR-0281).
pub struct Notifier<Arg> {
    listeners: Rc<ListenerStorage<Arg>>,
}

impl<Arg> Clone for Notifier<Arg> {
    fn clone(&self) -> Self {
        Self {
            listeners: Rc::clone(&self.listeners),
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
                entries: RefCell::new(Listeners::default()),
                delivery_depth: Cell::new(0),
                failed: AtomicBool::new(false),
                retired: RefCell::new(Vec::new()),
                deferred: RefCell::new(Vec::new()),
                next_id: IdMint::new(),
                is_disposed: Cell::new(false),
            }),
        }
    }

    /// `pub(crate)` so [`crate::notifier::ChangeNotifier`]'s release-mode
    /// use-after-dispose path can hand out an unregistered id without
    /// tripping this notifier's own (differently-worded) disposed check.
    pub(crate) fn mint_id(&self) -> ListenerId {
        self.listeners
            .next_id
            .next()
            .expect("BUG: listener identities exhausted")
    }

    /// Whether [`dispose`](Self::dispose) has been called (shared across clones).
    #[must_use]
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.listeners.is_disposed.get()
    }

    /// Debug-panics if disposed; release degrades to `tracing::warn!` + no-op.
    /// Returns `true` if the caller should early-return (disposed in release).
    #[inline]
    fn check_disposed(&self) -> bool {
        if self.listeners.is_disposed.get() {
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

    /// Subscribe with an owned [`Subscription`](crate::notifier::Subscription):
    /// hold it, and the listener fires; drop it, and the listener detaches
    /// from this channel.
    ///
    /// A removed-mid-notify listener is skipped for the rest of its round, as
    /// [`remove`](Self::remove) says; a subscription is no different. On a
    /// disposed channel the subscription detaches quietly: the list is empty
    /// and the id it carries names nothing.
    ///
    /// The channel's value borrow does not follow the subscription: its
    /// storage is held as a `'static` channel, so `Arg: 'static`.
    pub fn listen(&self, listener: ListenerCallback) -> crate::notifier::Subscription
    where
        Arg: 'static,
    {
        let id = if self.check_disposed() {
            self.mint_id()
        } else {
            self.add_plain_unchecked(listener)
        };
        let channel: Rc<dyn SubscribedChannel> = self.listeners.clone();
        crate::notifier::Subscription::for_channel(Rc::downgrade(&channel), id)
    }
    /// Subscribe a [`RecoveryCallback`] under an owned
    /// [`Subscription`](crate::notifier::Subscription): hold it, and the
    /// relay fires on every [`notify`](Self::notify), borrowing each
    /// delivery's recovery scope; drop it, and the relay detaches from this
    /// channel. The relay's custody matches [`add_with_recovery`](Self::add_with_recovery)'s.
    pub fn listen_with_recovery(
        &self,
        relay: RecoveryCallback<Arg>,
    ) -> crate::notifier::Subscription
    where
        Arg: 'static,
    {
        let id = self.add_with_recovery(relay);
        let channel: Rc<dyn SubscribedChannel> = self.listeners.clone();
        crate::notifier::Subscription::for_channel(Rc::downgrade(&channel), id)
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

    /// [`Self::add_unchecked`] for a listener that takes no argument: it is
    /// registered as given, with no adapter around it.
    pub(crate) fn add_plain_unchecked(
        &self,
        listener: crate::notifier::ListenerCallback,
    ) -> ListenerId {
        self.admit_callback(Rc::new(NotificationCallback(CallbackKind::Plain(listener))))
    }

    /// Register a [`RecoveryCallback`] in the same ordered notification channel
    /// as [`add`](Self::add).
    pub fn add_with_recovery(&self, listener: RecoveryCallback<Arg>) -> ListenerId {
        if self.check_disposed() {
            return self.mint_id();
        }
        self.admit_callback(NotificationCallback::relay(listener))
    }

    fn admit_callback(&self, listener: OwnedCallback<Arg>) -> ListenerId {
        let id = self.mint_id();
        self.listeners.entries.borrow_mut().admit(id, listener);
        id
    }

    /// Runs `mutate` against the locked listener list and returns whatever it
    /// extracts (a removed listener, or the whole displaced list on a clear),
    /// only after this notifier's own lock has released.
    ///
    /// A removed/displaced [`ArgCallback`]'s `Drop` may run arbitrary user
    /// code: the last `Arc` clone of a listener closure reaching zero can
    /// destroy captured state whose own destructor calls back into this same
    /// notifier (e.g. another `remove`/`dispose`). `mutate` itself only
    /// touches the list while the guard is held; its *return value* is not
    /// dropped here — it is this function's tail expression, so the guard
    /// (a temporary scoped to this function body) releases before the
    /// caller ever receives, and can drop, the extracted value.
    fn extract_locked<T>(&self, mutate: impl FnOnce(&mut Listeners<Arg>) -> T) -> T {
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
        self.retire_removed(id, self.extract_locked(|listeners| listeners.withdraw(id)));
    }

    /// [`Self::remove`] without the disposed gate: always a silent no-op on a
    /// disposed channel (whose listener list is already empty).
    ///
    /// This is the flavour `ChangeNotifier::remove_listener` needs, so that
    /// teardown code can always detach without tripping a use-after-dispose
    /// check.
    pub fn remove_even_if_disposed(&self, id: ListenerId) {
        self.retire_removed(id, self.extract_locked(|listeners| listeners.withdraw(id)));
    }

    /// Withdraw a registration without destroying its callback: the caller
    /// retires the returned envelope through its recovery scope, after
    /// [`inherit_failure`](Self::inherit_failure). This is the removal half of a
    /// custom source's recovery-aware subscription; [`remove`](Self::remove)
    /// drops the callback under its own scope instead.
    #[must_use]
    pub fn take_callback(&self, id: ListenerId) -> Option<OwnedCallback<Arg>> {
        self.extract_locked(|listeners| listeners.withdraw(id))
    }

    #[doc(hidden)]
    pub fn take_all_callbacks(&self) -> Vec<OwnedCallback<Arg>> {
        self.extract_locked(std::mem::take)
            .into_callbacks()
            .map(|(_, callback)| callback)
            .collect::<Vec<OwnedCallback<Arg>>>()
    }

    #[doc(hidden)]
    #[must_use]
    pub fn dispose_and_take_callbacks(&self) -> Vec<OwnedCallback<Arg>> {
        self.listeners.is_disposed.set(true);
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
        self.listeners
            .entries
            .borrow()
            .iter()
            .map(|(id, _)| id)
            .collect::<Vec<ListenerId>>()
    }

    /// Hand `capture` to this channel's delivery in progress: it retires with
    /// the round's own removed listeners, when the outermost delivery ends,
    /// under the round's containment. Returns it when no delivery is in
    /// progress, for the caller to retire now.
    ///
    /// An owner of several channels uses this to keep a capture withdrawn
    /// from one channel alive across another channel's round.
    pub fn retire_after_delivery<T: 'static>(&self, capture: T) -> Option<T> {
        if self.listeners.delivery_depth.get() == 0 {
            return Some(capture);
        }
        self.listeners.deferred.borrow_mut().push(Box::new(capture));
        None
    }

    /// Carry a failure this channel's current delivery already holds into
    /// `recovery`, so a withdrawal made during that delivery retires its
    /// captures under containment and that failure stays the first.
    pub fn inherit_failure(&self, recovery: &mut RecoveryScope<'_>) {
        if self.listeners.delivery_depth.get() != 0 && self.listeners.failed.load(Ordering::Relaxed)
        {
            recovery.inherit_failure();
        }
    }

    /// Discard listeners and mark disposed. Idempotent (second call is a no-op).
    pub fn dispose(&self) {
        if self.listeners.is_disposed.replace(true) {
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
            drop(callback);
        }
    }

    fn retire_removed_map(&self, callbacks: Listeners<Arg>) {
        if self.listeners.delivery_depth.get() != 0 {
            self.listeners
                .retired
                .borrow_mut()
                .extend(callbacks.into_callbacks());
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
    /// list keeps them in that order). Listeners
    /// *added* during a notify round are NOT fired in that round — only in the
    /// next one (same round-N-vs-round-N+1 rule as `notify_listeners`).
    /// Arguments are borrowed: notification never clones or destroys the value.
    /// A caught listener panic is held while the rest of the round runs; the
    /// snapshot's callbacks then retire one at a time under the round's
    /// containment, and the first panic propagates. A capture aggregate that
    /// panics again while one of its fields unwinds aborts, as in std
    /// (ADR-0281).
    pub fn notify(&self, arg: &Arg) {
        if self.check_disposed() {
            return;
        }
        self.notify_unchecked(arg);
    }

    /// [`Self::notify`] without the entry disposed gate — same single-check
    /// rationale as [`Self::add_unchecked`]. The firing loop itself is
    /// dispose-safe either way: disposing the channel skips its remaining
    /// snapshot entries, and a disposed listener list yields an empty snapshot.
    pub(crate) fn notify_unchecked(&self, arg: &Arg) {
        let mut recovery = PanicRecovery::new();
        if self.listeners.failed.load(Ordering::Relaxed) {
            recovery.inherit_failure();
        }
        self.notify_unchecked_with_recovery(arg, &mut recovery.scope());
        recovery.finish();
    }

    /// Notify inside an enclosing delivery's [`RecoveryScope`], for a relay that
    /// forwards a notification it is itself receiving.
    pub fn notify_with_recovery(&self, arg: &Arg, recovery: &mut RecoveryScope<'_>) {
        if !self.check_disposed() {
            self.notify_unchecked_with_recovery(arg, recovery);
        }
    }

    pub(crate) fn notify_unchecked_with_recovery(
        &self,
        arg: &Arg,
        recovery: &mut RecoveryScope<'_>,
    ) {
        self.notify_selection(arg, None, recovery, || {});
    }

    pub(crate) fn notify_unchecked_with_recovery_before(
        &self,
        arg: &Arg,
        recovery: &mut RecoveryScope<'_>,
        before: impl FnOnce(),
    ) {
        self.notify_selection(arg, None, recovery, before);
    }

    /// Deliver previously admitted membership, skipping removed subscriptions.
    #[doc(hidden)]
    pub fn notify_selected_with_recovery(
        &self,
        arg: &Arg,
        ids: &[ListenerId],
        recovery: &mut RecoveryScope<'_>,
    ) {
        if !self.check_disposed() {
            self.notify_selection(arg, Some(ids), recovery, || {});
        }
    }

    fn notify_selection(
        &self,
        arg: &Arg,
        ids: Option<&[ListenerId]>,
        recovery: &mut RecoveryScope<'_>,
        before: impl FnOnce(),
    ) {
        recovery.with_failure_latch(&self.listeners.failed, |recovery| {
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
                .borrow()
                .iter()
                .filter(|(id, _)| ids.is_none_or(|ids| ids.binary_search(id).is_ok()))
                .map(|(id, cb)| (id, Rc::clone(cb)))
                .collect();
            // Membership and capture custody precede optional observer reentry.
            // An observer failure joins the same round, preserving the tail.
            recovery.run(before);
            for (id, callback) in &snapshot {
                // Skip a listener individually removed mid-notify (by an earlier
                // callback), and skip the tail once the channel is disposed.
                if self.listeners.is_disposed.get()
                    || !self.listeners.entries.borrow().contains(*id)
                {
                    continue;
                }
                #[expect(
                    clippy::disallowed_methods,
                    reason = "ADR-0287: a listener's panic must not stop the remaining listeners"
                )]
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
            // Retire separate envelopes one at a time, each under the round's
            // containment: a destructor panic is a competing failure (ADR-0281).
            snapshot.reverse();
            while let Some((_, callback)) = snapshot.pop() {
                recovery.retire_callout_capture(callback);
            }
            drop(snapshot);
            if self.listeners.delivery_depth.get() == 1 {
                loop {
                    let mut retired = std::mem::take(&mut *self.listeners.retired.borrow_mut());
                    let deferred = std::mem::take(&mut *self.listeners.deferred.borrow_mut());
                    if retired.is_empty() && deferred.is_empty() {
                        break;
                    }
                    retired.sort_unstable_by_key(|(id, _)| *id);
                    for (_, callback) in retired {
                        recovery.retire_callout_capture(callback);
                    }
                    for capture in deferred {
                        recovery.retire_callout_capture(capture);
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

    /// Withdrawing leaves no listener behind and none out of order, across
    /// the points where the list drops its empty slots.
    fn withdrawals_keep_the_rest_in_registration_order() {
        let n: Notifier<()> = Notifier::new();
        let fired = Rc::new(RefCell::new(Vec::<usize>::new()));
        let ids: Vec<ListenerId> = (0..64_usize)
            .map(|index| {
                let fired = Rc::clone(&fired);
                n.add(Rc::new(move |&()| fired.borrow_mut().push(index)))
            })
            .collect();
        let mut expected: Vec<usize> = (0..64).collect();
        // Oldest first, then every third of the rest, then the newest.
        let order = (0..20_usize)
            .chain((20..60).step_by(3))
            .chain((60..64).rev());
        for index in order {
            n.remove(ids[index]);
            n.remove(ids[index]);
            expected.retain(|kept| *kept != index);
            assert_eq!(n.len(), expected.len());
            fired.borrow_mut().clear();
            n.notify(&());
            assert_eq!(*fired.borrow(), expected);
            assert_eq!(n.listener_ids().len(), expected.len());
        }
        let late = Rc::clone(&fired);
        let _ = n.add(Rc::new(move |&()| late.borrow_mut().push(usize::MAX)));
        fired.borrow_mut().clear();
        n.notify(&());
        assert_eq!(fired.borrow().last(), Some(&usize::MAX));
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
            (
                "withdrawals keep the rest in registration order",
                withdrawals_keep_the_rest_in_registration_order,
            ),
        ]);
    }
}
