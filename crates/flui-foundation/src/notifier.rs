//! Listenable and change notification types.
//!
//! This module provides the observer pattern for reactive UI updates.
//!
//! - **Listenable**: Base trait for objects that notify listeners
//! - **`ChangeNotifier`**: Manages a list of listeners and notifies them
//! - **`ValueNotifier`**: A `ChangeNotifier` that holds a single value
//!
//! # Example
//!
//! ```rust
//! use std::{cell::Cell, rc::Rc};
//!
//! use flui_foundation::notifier::{ChangeNotifier, Listenable};
//!
//! let notifier = ChangeNotifier::new();
//! let count = Rc::new(Cell::new(0));
//! let count2 = Rc::clone(&count);
//! let _listening = notifier.listen(Rc::new(move || {
//!     count2.set(count2.get() + 1);
//! }));
//! notifier.notify_listeners();
//! assert_eq!(count.get(), 1);
//! ```
//!
//! # Note
//!
//! For event bubbling notifications (like `ScrollNotification`), see
//! `flui-view` which provides the `Notification` trait that integrates with
//! `BuildContext`.

use crate::panic::RecoveryScope;
use std::{fmt, ops::Deref, rc::Rc};

use crate::{id::ListenerId, notifier_generic::Notifier};

/// A listener callback function.
// Stored callbacks own their captures for the subscription's lifetime.
pub type ListenerCallback = Rc<dyn Fn() + 'static>;

/// A listener that runs inside the enclosing notification's [`RecoveryScope`].
///
/// Relays that forward a notification (an animation re-emitting its parent's
/// change, a builder rebuilding on a controller) register this form so the
/// first failure of the whole delivery stays authoritative.
pub type ListenerObserver = Rc<dyn Fn(&mut RecoveryScope<'_>)>;

/// An object that maintains a list of listeners.
///
/// Listener state belongs to the UI owner thread.
///
/// There are two variants of this interface:
///
/// - [`ValueListenable`]: A `Listenable` that also exposes a current value.
/// - [`ChangeNotifier`]: A concrete implementation that can be used directly.
///
/// # Example
///
/// ```rust
/// use std::{cell::Cell, rc::Rc};
///
/// use flui_foundation::notifier::{ChangeNotifier, Listenable};
///
/// let notifier = ChangeNotifier::new();
/// let count = Rc::new(Cell::new(0));
/// let count2 = Rc::clone(&count);
/// let listening = notifier.listen(Rc::new(move || {
///     count2.set(count2.get() + 1);
/// }));
/// notifier.notify_listeners();
/// assert_eq!(count.get(), 1);
/// listening.detach();
/// ```
pub trait Listenable {
    /// Register a relay that runs inside the notifying delivery's
    /// [`RecoveryScope`] (ADR-0219): a source forwards it to the channel that
    /// notifies, so the relay borrows that delivery's failure custody rather
    /// than a fresh one.
    ///
    /// Hold the returned [`Subscription`], and the relay fires on every
    /// notification; drop it, and the relay detaches from its own channel.
    fn listen_with_recovery(&self, relay: ListenerObserver) -> Subscription;

    /// Register a listener callback.
    ///
    /// An observer that ignores the scope; a panic in it is still contained
    /// by the notifying delivery. Hold the returned [`Subscription`], and the
    /// listener fires on every notification; drop it, and the listener
    /// detaches from its own channel.
    fn listen(&self, listener: ListenerCallback) -> Subscription;
}

/// An interface for subclasses of [`Listenable`] that expose a value.
///
/// This trait is implemented by [`ValueNotifier<T>`] and can be used
/// to accept any listenable that provides a current value.
///
/// # Example
///
/// ```rust
/// use flui_foundation::notifier::{Listenable, ValueListenable, ValueNotifier};
///
/// fn current<T: std::fmt::Debug>(
///     listenable: &impl ValueListenable<T>,
/// ) -> String {
///     format!("{:?}", listenable.value())
/// }
///
/// let notifier = ValueNotifier::new(42);
/// assert_eq!(current(&notifier), "42");
/// ```
pub trait ValueListenable<T>: Listenable {
    /// The current value of the object.
    ///
    /// When the value changes, the listeners registered with
    /// [`Listenable::listen`] will be invoked.
    fn value(&self) -> &T;
}

/// A class that can be extended or mixed in that provides a change notification
/// API.
///
/// # Disposal
///
/// After [`dispose`] has been called, [`add_listener`] and
/// [`notify_listeners`] panic in debug builds via `debug_assert!` and
/// degrade to a `tracing::warn!` + no-op in release builds.
///
/// [`remove_listener`] is the deliberate exception: it carries no disposed
/// check in either build profile, so that teardown code can detach from an
/// already-disposed listenable. It is always a silent no-op
/// once disposed (the listener map is already empty).
///
/// `is_disposed` is shared across clones via `Rc<Cell<bool>>` so that a
/// listener-callback holding its own clone sees disposal performed elsewhere.
///
/// [`dispose`]: ChangeNotifier::dispose
/// [`add_listener`]: ChangeNotifier::add_listener
/// [`notify_listeners`]: ChangeNotifier::notify_listeners
/// [`remove_listener`]: ChangeNotifier::remove_listener
#[derive(Clone)]
pub struct ChangeNotifier {
    /// The shared notification core — `ChangeNotifier` IS `Notifier<()>`
    /// plus its own seams: the `ChangeNotifier`-branded
    /// use-after-dispose message, and `remove_listener` tolerating a
    /// disposed receiver. The snapshot/ordering/`catch_unwind` firing
    /// discipline lives once, in [`Notifier::notify`].
    inner: Notifier<()>,
}

impl Default for ChangeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ChangeNotifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangeNotifier")
            .field("listeners_count", &self.inner.len())
            .finish_non_exhaustive()
    }
}

impl ChangeNotifier {
    /// Carry a failure this notifier's current delivery already holds into
    /// `recovery`, so a withdrawal made during that delivery retires its
    /// captures under containment and that failure stays the first.
    pub fn inherit_failure(&self, recovery: &mut RecoveryScope<'_>) {
        self.inner.inherit_failure(recovery);
    }
    /// Create a new change notifier.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: Notifier::new(),
        }
    }

    /// Returns `true` if [`dispose`](Self::dispose) has been called on this
    /// notifier (or any of its clones — the disposed state is shared).
    #[must_use]
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.inner.is_disposed()
    }

    /// Discards listeners and marks this notifier as disposed.
    ///
    /// After this is called, the notifier is not in a usable state:
    /// subsequent calls to [`add_listener`] or [`notify_listeners`] panic in
    /// debug builds via `debug_assert!` and degrade to a `tracing::warn!` +
    /// no-op in release builds. [`remove_listener`] is the deliberate
    /// exception — it stays a silent no-op after dispose.
    ///
    /// Disposal does NOT notify listeners; consumers must decide whether to notify before
    /// calling `dispose`.
    ///
    /// This method is **idempotent**: calling it again is a no-op (no panic).
    /// Takes `&self` rather than `&mut self` because the notifier is
    /// internally `Clone` over `Rc<...>` and a listener-callback may need
    /// to call `dispose` on its own clone (the snapshot-then-fire path in
    /// [`notify_listeners`] makes this reentrancy-safe).
    ///
    /// [`add_listener`]: ChangeNotifier::add_listener
    /// [`notify_listeners`]: Self::notify_listeners
    /// [`remove_listener`]: ChangeNotifier::remove_listener
    pub fn dispose(&self) {
        self.inner.dispose();
    }

    /// Withdraw a subscription without destroying its callback: the caller
    /// retires the returned envelope through its recovery scope, after
    /// [`inherit_failure`](Self::inherit_failure). `remove_listener` drops it
    /// instead, deferred to the end of an active delivery.
    #[must_use]
    pub fn take_listener(
        &self,
        id: ListenerId,
    ) -> Option<Rc<crate::notifier_generic::NotificationCallback<()>>> {
        self.inner.take_callback(id)
    }

    /// Withdraw every subscription, with the custody of
    /// [`take_listener`](Self::take_listener).
    #[must_use]
    pub fn take_listeners(&self) -> Vec<Rc<crate::notifier_generic::NotificationCallback<()>>> {
        self.inner.take_all_callbacks()
    }

    /// Hand `capture` to this notifier's delivery in progress, with the
    /// custody of [`Notifier::retire_after_delivery`](crate::notifier_generic::Notifier::retire_after_delivery).
    /// Returns it when no delivery is in progress.
    pub fn retire_after_delivery<T: 'static>(&self, capture: T) -> Option<T> {
        self.inner.retire_after_delivery(capture)
    }

    /// Dispose and withdraw subscriptions before their captures retire.
    #[doc(hidden)]
    #[must_use]
    pub fn dispose_and_take_listeners(
        &self,
    ) -> Vec<Rc<crate::notifier_generic::NotificationCallback<()>>> {
        self.inner.dispose_and_take_callbacks()
    }

    /// Debug-asserts that this notifier has not been disposed.
    ///
    /// In debug builds, panics with `"ChangeNotifier used after dispose"`
    /// via `debug_assert!`. In release builds, emits a `tracing::warn!` and
    /// returns `true` to indicate the caller should early-return as a no-op
    /// (release degrades gracefully).
    ///
    /// Returns `true` if the notifier is disposed (caller should no-op),
    /// `false` if usable.
    #[inline]
    fn check_disposed(&self) -> bool {
        if self.inner.is_disposed() {
            // This branded check runs BEFORE any delegation into the inner
            // [`Notifier`] so a disposed `ChangeNotifier` panics/warns with
            // its own documented message, not the generic channel's.
            //
            // cfg-explicit layout: debug panics immediately (hard contract
            // violation), release degrades gracefully with a warning.
            #[cfg(debug_assertions)]
            panic!(
                "ChangeNotifier used after dispose: once dispose() has been \
                 called, the notifier can no longer be used"
            );
            // The release-only block is unreachable in debug builds because the
            // `panic!` above diverges; the attribute below documents that. In release the
            // `panic!` is compiled out, the block IS reachable, and no lint
            // fires — hence `cfg_attr`, not a bare `expect`.
            #[cfg_attr(debug_assertions, expect(unreachable_code))]
            {
                tracing::warn!("ChangeNotifier used after dispose");
                return true;
            }
        }
        false
    }

    /// Call all the registered listeners.
    ///
    /// Snapshot semantics:
    ///
    /// - A registration-ordered snapshot is taken before callbacks run. No
    ///   state borrow is held across invocation or capture retirement.
    /// - Before each callback fires, the listener's registration is re-checked.
    ///   If the listener was removed during notify (e.g. a previous callback
    ///   called `remove_listener`), the callback is silently skipped. A
    ///   listener removed mid-iteration does NOT fire.
    /// - Each callback is wrapped in `catch_unwind(AssertUnwindSafe(|| ...))`.
    ///   If a callback panics, the panic payload is logged via
    ///   `tracing::error!` and iteration continues with the next listener.
    ///   One panicking listener does NOT abort the rest.
    ///   The first failure resumes after the healthy tail and retirement.
    ///
    /// Listeners fire in registration order (`ListenerId` ascending); the backing `HashMap` does not preserve
    /// insertion order, so the snapshot is sorted by id before firing.
    ///
    /// Post-snapshot *additions* are NOT fired in the current notify cycle;
    /// only listeners present at snapshot time and still
    /// registered when reached are invoked.
    ///
    /// # Disposal
    ///
    /// Panics in debug builds (no-ops in release) if called after
    /// [`dispose`](Self::dispose). The disposed-state check runs at the
    /// entry to this method. Disposal during notification withdraws all
    /// remaining subscriptions; their callbacks are skipped in that round.
    pub fn notify_listeners(&self) {
        if self.check_disposed() {
            return;
        }
        // The snapshot-then-fire loop (ordering, mid-notify removal skip,
        // per-callback `catch_unwind`, mid-flight-dispose honouring) lives
        // once, in the generic channel. The UNCHECKED variant on purpose:
        // the branded check above is this method's one documented entry
        // check, and a `dispose` racing in after it must behave as it always
        // did under the single-check semantics (the in-flight call proceeds)
        // rather than trip the inner channel's generically-worded gate.
        self.inner.notify_unchecked(&());
    }

    /// Notify inside an enclosing delivery's [`RecoveryScope`], for a relay
    /// that forwards a notification it is itself receiving.
    pub fn notify_listeners_with_recovery(&self, recovery: &mut RecoveryScope<'_>) {
        if !self.check_disposed() {
            self.inner.notify_with_recovery(&(), recovery);
        }
    }

    /// Observe a committed round after capturing membership, before delivery.
    ///
    /// Reentrant additions wait for the next round; removals and disposal silence
    /// the remaining selection. Observer failure shares the round's custody.
    #[doc(hidden)]
    pub fn notify_listeners_with_recovery_before(
        &self,
        recovery: &mut RecoveryScope<'_>,
        before: impl FnOnce(),
    ) {
        if !self.check_disposed() {
            self.inner
                .notify_unchecked_with_recovery_before(&(), recovery, before);
        }
    }

    /// Whether any listeners are currently registered
    #[must_use]
    #[inline]
    pub fn has_listeners(&self) -> bool {
        !self.inner.is_empty()
    }

    /// Subscribe with an owned [`Subscription`]: hold it, and the
    /// listener fires on every [`notify_listeners`](Self::notify_listeners);
    /// drop it, and the listener detaches from this notifier.
    pub fn listen(&self, listener: ListenerCallback) -> Subscription {
        self.inner.listen(listener)
    }

    /// Subscribe an owned relay that runs inside the notifying delivery's
    /// [`RecoveryScope`], the relay custody ADR-0219's registration owes.
    /// The id-form twin is [`add_with_recovery`](Self::add_with_recovery),
    /// for owners that retire through their own recovery scope.
    pub fn listen_with_recovery(&self, relay: ListenerObserver) -> Subscription {
        self.inner
            .listen_with_recovery(Rc::new(move |&(), recovery| relay(recovery)))
    }

    /// Checks if there are no listeners registered
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns the number of listeners currently registered
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl Listenable for ChangeNotifier {
    fn listen(&self, listener: ListenerCallback) -> Subscription {
        self.inner.listen(listener)
    }
    fn listen_with_recovery(&self, relay: ListenerObserver) -> Subscription {
        self.inner
            .listen_with_recovery(Rc::new(move |&(), recovery| relay(recovery)))
    }
}

impl ChangeNotifier {
    /// The channel's own registration of a plain callback, for owners that
    /// withdraw through [`take_listener`](Self::take_listener)'s id — the
    /// channel-level machinery the trait's `listen`-pair wraps.
    pub fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        if self.check_disposed() {
            return self.inner.mint_id();
        }
        self.inner.add_unchecked(Rc::new(move |&()| listener()))
    }

    /// The channel's own registration of a relay, whose id the channel's
    /// `take_callback` machinery withdraws (ADR-0219's channel level).
    pub fn add_with_recovery(&self, relay: ListenerObserver) -> ListenerId {
        if self.check_disposed() {
            return self.inner.mint_id();
        }
        self.inner
            .add_with_recovery(Rc::new(move |&(), recovery| relay(recovery)))
    }

    /// The channel's own tolerant withdrawal: a no-op on a disposed channel
    /// (its table is empty), so an owner's teardown or a
    /// [`Subscription`]'s replacement can always detach. Owned callers hold a
    /// [`Subscription`] instead; this is the channel-level machinery an owner
    /// of the whole channel retires its own registrations through.
    pub fn remove_listener(&self, id: ListenerId) {
        self.inner.remove_even_if_disposed(id);
    }

    /// Withdraw every listener with the tolerance of
    /// [`remove_listener`](Self::remove_listener).
    pub fn remove_all_listeners(&self) {
        self.inner.remove_all_unchecked();
    }
}

/// A `ChangeNotifier` that holds a single value.
///
/// Reading, mutating and extracting the owned value do not require `Clone`.
/// Cloning the notifier requires `T: Clone`: it copies the value and shares
/// the listener channel.
///
/// Dropping the notifier drops the value, then its handle on the shared
/// channel, as std drops a struct's fields: a destructor that panics while
/// the thread is already unwinding aborts (ADR-0281). A clone keeps the
/// shared channel working.
///
/// Borrowed data inside `T` must outlive the notifier: declare the notifier
/// after its referent, or keep the referent alive longer. `T: 'static` is not
/// required.
///
/// ```
/// use flui_foundation::ValueNotifier;
/// let message = String::from("borrowed value");
/// let notifier = ValueNotifier::new(message.as_str());
/// let value = notifier.into_value();
/// assert_eq!(value, "borrowed value");
/// ```
///
/// An inner referent cannot expire before implicit notifier destruction:
///
/// ```compile_fail
/// use flui_foundation::ValueNotifier;
/// let _notifier;
/// {
///     let message = String::from("temporary");
///     _notifier = ValueNotifier::new(message.as_str());
/// }
/// ```
///
/// Moving the referent into the notifier instead of borrowing it compiles:
///
/// ```
/// use flui_foundation::ValueNotifier;
/// let _notifier;
/// {
///     let message = String::from("temporary");
///     _notifier = ValueNotifier::new(message);
/// }
/// ```
#[derive(Clone)]
pub struct ValueNotifier<T> {
    value: Option<T>,
    notifier: Option<ChangeNotifier>,
}

/// The value and channel withdrawn from a `ValueNotifier`, dropped in field
/// order: the value, then the channel handle.
struct RetiringValueNotifier<T> {
    value: Option<T>,
    notifier: Option<ChangeNotifier>,
}

impl<T> Drop for ValueNotifier<T> {
    fn drop(&mut self) {
        drop(self.extract_owned());
    }
}

impl<T> ValueNotifier<T> {
    /// Create a new value notifier with an initial value.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            value: Some(value),
            notifier: Some(ChangeNotifier::new()),
        }
    }

    fn notifier(&self) -> &ChangeNotifier {
        self.notifier
            .as_ref()
            .expect("BUG: a live ValueNotifier owns its listener channel")
    }

    fn extract_owned(&mut self) -> RetiringValueNotifier<T> {
        RetiringValueNotifier {
            value: self.value.take(),
            notifier: self.notifier.take(),
        }
    }

    /// Returns a reference to the current value.
    ///
    /// # Panics
    ///
    /// Panics if an internal ownership invariant is violated.
    #[must_use]
    #[inline]
    pub const fn value(&self) -> &T {
        self.value
            .as_ref()
            .expect("BUG: a live ValueNotifier owns its value")
    }

    /// Returns a mutable reference to the current value.
    ///
    /// Note: This does NOT notify listeners. Call `notify()` manually if
    /// needed.
    ///
    /// # Panics
    ///
    /// Panics if an internal ownership invariant is violated.
    #[inline]
    pub const fn value_mut(&mut self) -> &mut T {
        self.value
            .as_mut()
            .expect("BUG: a live ValueNotifier owns its value")
    }

    /// Consumes the notifier and returns the inner value.
    ///
    /// Disposes the shared listener channel before returning the value.
    ///
    /// # Panics
    ///
    /// Propagates a panic while disposing or retiring the listener channel.
    /// Panics if an internal ownership invariant is violated.
    #[must_use]
    #[inline]
    pub fn into_value(mut self) -> T {
        let mut retiring = self.extract_owned();
        retiring
            .notifier
            .as_ref()
            .expect("BUG: extraction owns the listener channel")
            .dispose();
        // Keep the value under custody until every channel owner has retired.
        drop(retiring.notifier.take());
        retiring
            .value
            .take()
            .expect("BUG: extraction owns the value")
    }

    /// Replaces the value and returns the old value.
    ///
    /// Notifies listeners if the new value is different from the old value.
    pub fn replace(&mut self, new_value: T) -> T
    where
        T: PartialEq,
    {
        let old_value = std::mem::replace(self.value_mut(), new_value);
        if self.value() != &old_value {
            self.notifier().notify_listeners();
        }
        old_value
    }

    /// Takes the value, replacing it with the default value.
    ///
    /// Notifies listeners.
    pub fn take(&mut self) -> T
    where
        T: Default,
    {
        let value = std::mem::take(self.value_mut());
        self.notifier().notify_listeners();
        value
    }

    /// Set a new value and notify listeners if the value changed.
    pub fn set_value(&mut self, new_value: T)
    where
        T: PartialEq,
    {
        if self.value() != &new_value {
            *self.value_mut() = new_value;
            self.notifier().notify_listeners();
        }
    }

    /// Set a new value without checking for equality.
    ///
    /// Always notifies listeners, even if the value didn't change.
    pub fn set_value_force(&mut self, new_value: T) {
        *self.value_mut() = new_value;
        self.notifier().notify_listeners();
    }

    /// Update the value using a function.
    ///
    /// Notifies listeners after the update.
    pub fn update<F>(&mut self, f: F)
    where
        F: FnOnce(&mut T),
    {
        f(self.value_mut());
        self.notifier().notify_listeners();
    }

    /// Manually notify all listeners.
    ///
    /// Useful when the value is mutated through `value_mut()`.
    #[inline]
    pub fn notify(&self) {
        self.notifier().notify_listeners();
    }

    /// Returns the number of listeners currently registered
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.notifier().len()
    }

    /// Checks if there are no listeners registered
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.notifier().is_empty()
    }

    /// Whether any listeners are currently registered
    #[must_use]
    #[inline]
    pub fn has_listeners(&self) -> bool {
        self.notifier().has_listeners()
    }
}

impl<T: fmt::Debug> fmt::Debug for ValueNotifier<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueNotifier")
            .field("value", self.value())
            .field("listeners", &self.notifier().len())
            .finish()
    }
}

// `Default for ValueNotifier<T>` is removed intentionally. A defaulted
// notifier would have a default-constructed value AND a fresh identity (no
// listeners); two `ValueNotifier::<T>::default()` calls produce notifiers that
// are `==` by value yet are observably distinct objects. This violates the
// principle of least surprise. Construct explicitly via
// `ValueNotifier::new(value)`.

impl<T: PartialEq> PartialEq for ValueNotifier<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.value() == other.value()
    }
}

impl<T: Eq> Eq for ValueNotifier<T> {}

impl<T: fmt::Display> fmt::Display for ValueNotifier<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.value(), f)
    }
}

impl<T> Deref for ValueNotifier<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

impl<T> AsRef<T> for ValueNotifier<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.value()
    }
}

impl<T> Listenable for ValueNotifier<T> {
    fn listen(&self, listener: ListenerCallback) -> Subscription {
        self.notifier().listen(listener)
    }
    fn listen_with_recovery(&self, relay: ListenerObserver) -> Subscription {
        self.notifier().listen_with_recovery(relay)
    }
}

impl<T> ValueListenable<T> for ValueNotifier<T> {
    fn value(&self) -> &T {
        self.value()
    }
}

impl<T> ValueNotifier<T> {
    /// Subscribe with an owned [`Subscription`]: hold it, and the
    /// listener fires on every [`set_value`](Self::set_value) that notifies;
    /// drop it, and the listener detaches from this notifier.
    pub fn listen(&self, listener: ListenerCallback) -> Subscription
    where
        T: 'static,
    {
        self.notifier().listen(listener)
    }
}

/// An owned registration on a [`Listenable`] source: hold it, and its
/// listener fires; drop it, and it detaches.
///
/// The subscription remembers the channel it subscribed on, so its detach
/// removes exactly the listener it registered: a [`ListenerId`]'s value alone
/// can never name another channel's registration (ids are minted per channel,
/// from one). A channel that was disposed, or is already gone, detaches
/// quietly: its listener table is empty and its id names nothing.
///
/// A detach that lands inside its channel's own notify round withdraws the
/// callback through that round's retirement: it destroys outside the channel's
/// borrow, at the round's tail, with the round's containment (ADR-0287).
///
/// ```rust
/// use std::{cell::Cell, rc::Rc};
/// use flui_foundation::ChangeNotifier;
///
/// let notifier = ChangeNotifier::new();
/// let hits = Rc::new(Cell::new(0));
/// let subscription = notifier.listen(Rc::new({
///     let hits = Rc::clone(&hits);
///     move || hits.set(hits.get() + 1)
/// }));
/// notifier.notify_listeners();
/// assert_eq!(hits.get(), 1);
///
/// drop(subscription);
/// notifier.notify_listeners();
/// assert_eq!(hits.get(), 1, "dropping the subscription detached the listener");
/// ```
#[must_use = "hold the subscription: dropping it detaches its listener"]
pub struct Subscription {
    /// The channel the listener registered on, weak so a subscription never
    /// pins a channel its outside has released. `None` for an inert
    /// subscription that never had one.
    channel: Option<std::rc::Weak<dyn crate::notifier_generic::SubscribedChannel>>,
    /// Some while the listener is still registered; `None` once detached.
    id: Option<ListenerId>,
    /// A removal the source itself supplies instead of a channel: the owned
    /// twin of ADR-0180's status-guard removal function, for a custom source
    /// whose storage is no foundation listener set.
    remove: Option<Box<dyn Fn()>>,
}

impl Subscription {
    /// A subscription bound to `channel`, cancelling on drop.
    #[doc(hidden)]
    pub(crate) fn for_channel(
        channel: std::rc::Weak<dyn crate::notifier_generic::SubscribedChannel>,
        id: ListenerId,
    ) -> Self {
        Self {
            channel: Some(channel),
            id: Some(id),
            remove: None,
        }
    }

    /// Detach now rather than at drop; idempotent with the drop that follows.
    /// A custom source's withdrawal runs its `remove` once.
    pub fn detach(mut self) {
        self.take_listener();
    }

    /// A subscription to a custom source: `remove` runs once, on the first
    /// detach or the drop that follows. The closure must commit the
    /// withdrawal it promises, as ADR-0180's status guard's removal does.
    pub fn for_removal(remove: impl Fn() + 'static) -> Self {
        Self {
            channel: None,
            id: None,
            remove: Some(Box::new(remove)),
        }
    }

    /// An inert subscription: nothing to detach, ever. For a source that
    /// admits no registration (a constant animation) and hands this form out
    /// instead of a listener id that names nothing.
    pub fn inert() -> Self {
        Self {
            channel: None,
            id: None,
            remove: None,
        }
    }

    fn take_listener(&mut self) {
        let Some(id) = self.id.take() else {
            if let Some(remove) = self.remove.take() {
                remove();
            }
            return;
        };
        if let Some(channel) = self.channel.take().and_then(|channel| channel.upgrade()) {
            channel.detach_listener(id);
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.take_listener();
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.id {
            Some(id) => write!(f, "Subscription({id:?})"),
            None => f.write_str("Subscription(detached)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn test_change_notifier() {
        let notifier = ChangeNotifier::new();
        let counter = Rc::new(AtomicUsize::new(0));

        let counter_clone = Rc::clone(&counter);
        let _listening = notifier.listen(Rc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(notifier.has_listeners());
        assert!(!notifier.is_empty());
        assert_eq!(notifier.len(), 1);

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    fn listener_fires_after_panic() {
        // A panicking listener must NOT abort the remaining listeners.
        // Given 3 listeners: panic-1, listener-2, listener-3 — listener-2 and
        // listener-3 must still fire despite listener-1 panicking.
        use std::sync::atomic::AtomicBool;

        let notifier = ChangeNotifier::new();
        let fired_2 = Rc::new(AtomicBool::new(false));
        let fired_3 = Rc::new(AtomicBool::new(false));
        let (fired_2c, fired_3c) = (Rc::clone(&fired_2), Rc::clone(&fired_3));

        let _listening = notifier.listen(Rc::new(|| panic!("intentional test panic")));
        let _listening = notifier.listen(Rc::new(move || {
            fired_2c.store(true, Ordering::SeqCst);
        }));
        let _listening = notifier.listen(Rc::new(move || {
            fired_3c.store(true, Ordering::SeqCst);
        }));

        let failure =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| notifier.notify_listeners()))
                .expect_err("first failure propagates after the healthy tail");
        assert_eq!(
            crate::panic::payload_text(&*failure),
            Some("intentional test panic")
        );

        assert!(
            fired_2.load(Ordering::SeqCst),
            "listener-2 must fire after listener-1 panics"
        );
        assert!(
            fired_3.load(Ordering::SeqCst),
            "listener-3 must fire after listener-1 panics"
        );
    }

    // ------------------------------------------------------------------
    // ChangeNotifier::dispose + disposed-state assertion
    // ------------------------------------------------------------------

    fn dispose_during_notify_iteration_safe() {
        // Reentrancy guarantee: a listener-callback may call `dispose` on the
        // notifier mid-`notify_listeners`. The snapshot-then-fire path at
        // ChangeNotifier::notify_listeners captures the callback set under the
        // mutex before invoking; the in-flight iteration completes without
        // panic. After the iteration, `is_disposed == true` only affects
        // subsequent outside calls.
        let notifier = ChangeNotifier::new();
        let notifier_for_callback = notifier.clone();
        let other_ran = Rc::new(AtomicUsize::new(0));
        let other_ran_clone = Rc::clone(&other_ran);

        // Listener #1: disposes the notifier mid-iteration.
        let _listening = notifier.listen(Rc::new(move || {
            notifier_for_callback.dispose();
        }));
        // Listener #2: increments counter — proves iteration completes after
        // mid-flight dispose (snapshot was already taken).
        let _listening = notifier.listen(Rc::new(move || {
            other_ran_clone.fetch_add(1, Ordering::SeqCst);
        }));

        // Must not panic — even though listener #1 sets is_disposed during
        // iteration, the snapshot is in-flight; the disposed-state check
        // ran at entry to notify_listeners, before the snapshot.
        notifier.notify_listeners();

        // Disposal withdraws all subscriptions, including snapshot members.
        assert_eq!(
            other_ran.load(Ordering::SeqCst),
            0,
            "disposed subscriptions must be silent even in an older snapshot"
        );

        // After the iteration, the notifier is disposed.
        assert!(notifier.is_disposed());
        assert_eq!(notifier.len(), 0, "dispose cleared listeners");
    }

    #[test]
    fn change_notifier_contract() {
        crate::test_cases::run_cases(&[
            ("test change notifier", test_change_notifier),
            ("listener fires after panic", listener_fires_after_panic),
            (
                "dispose during notify iteration safe",
                dispose_during_notify_iteration_safe,
            ),
        ]);
    }
}
