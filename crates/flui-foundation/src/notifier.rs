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
//! use std::sync::{
//!     Arc,
//!     atomic::{AtomicU32, Ordering},
//! };
//!
//! use flui_foundation::notifier::{ChangeNotifier, Listenable};
//!
//! let notifier = ChangeNotifier::new();
//! let count = Arc::new(AtomicU32::new(0));
//! let count2 = Arc::clone(&count);
//! let _id = notifier.add_listener(Arc::new(move || {
//!     count2.fetch_add(1, Ordering::Relaxed);
//! }));
//! notifier.notify_listeners();
//! assert_eq!(count.load(Ordering::Relaxed), 1);
//! ```
//!
//! # Note
//!
//! For event bubbling notifications (like `ScrollNotification`), see
//! `flui-view` which provides the `Notification` trait that integrates with
//! `BuildContext`.

use std::{fmt, ops::Deref, sync::Arc};

use crate::{id::ListenerId, notifier_generic::Notifier};

/// A listener callback function.
// Audit I-16: explicit `+ 'static` bound on the listener callback —
// pre-cycle the implicit `'static` was confusing (callback types
// stored long-term must be static, but the trait-object syntax
// elided it). Explicit doc avoids ambiguity for callers
// constructing the Arc.
pub type ListenerCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// An object that maintains a list of listeners.
///
/// Uses interior mutability for thread-safe listener management.
///
/// There are two variants of this interface:
///
/// - [`ValueListenable`]: A `Listenable` that also exposes a current value.
/// - [`ChangeNotifier`]: A concrete implementation that can be used directly.
///
/// # Example
///
/// ```rust
/// use std::sync::{
///     Arc,
///     atomic::{AtomicU32, Ordering},
/// };
///
/// use flui_foundation::notifier::{ChangeNotifier, Listenable};
///
/// let notifier = ChangeNotifier::new();
/// let count = Arc::new(AtomicU32::new(0));
/// let count2 = Arc::clone(&count);
/// let id = notifier.add_listener(Arc::new(move || {
///     count2.fetch_add(1, Ordering::Relaxed);
/// }));
/// notifier.notify_listeners();
/// assert_eq!(count.load(Ordering::Relaxed), 1);
/// notifier.remove_listener(id);
/// ```
pub trait Listenable: Send + Sync {
    /// Register a listener callback.
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId;

    /// Remove a previously registered listener.
    ///
    /// A no-op if `id` is not registered — including after the listenable
    /// has been disposed (`ChangeNotifier::remove_listener` tolerates a
    /// disposed receiver so teardown code can always detach).
    fn remove_listener(&self, id: ListenerId);

    /// Remove all listeners.
    fn remove_all_listeners(&self);
}

/// An interface for subclasses of [`Listenable`] that expose a value.
///
/// This trait is implemented by [`ValueNotifier<T>`] and can be used
/// to accept any listenable that provides a current value.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_foundation::notifier::{Listenable, ValueListenable, ValueNotifier};
///
/// fn current<T: std::fmt::Debug + Clone + Send + Sync>(
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
    /// When the value changes, the callbacks registered with
    /// [`Listenable::add_listener`] will be invoked.
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
/// `is_disposed` is shared across clones via `Arc<AtomicBool>` so that a
/// listener-callback holding its own clone sees disposal performed elsewhere.
///
/// [`dispose`]: ChangeNotifier::dispose
/// [`add_listener`]: Listenable::add_listener
/// [`notify_listeners`]: ChangeNotifier::notify_listeners
/// [`remove_listener`]: Listenable::remove_listener
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
    /// exception — it stays a silent no-op after dispose (see
    /// [`Listenable::remove_listener`]'s impl on this type).
    ///
    /// Disposal does NOT notify listeners; consumers must decide whether to notify before
    /// calling `dispose`.
    ///
    /// This method is **idempotent**: calling it again is a no-op (no panic).
    /// Takes `&self` rather than `&mut self` because the notifier is
    /// internally `Clone` over `Arc<...>` and a listener-callback may need
    /// to call `dispose` on its own clone (the snapshot-then-fire path in
    /// [`notify_listeners`] makes this reentrancy-safe).
    ///
    /// [`add_listener`]: Listenable::add_listener
    /// [`notify_listeners`]: Self::notify_listeners
    /// [`remove_listener`]: Listenable::remove_listener
    pub fn dispose(&self) {
        self.inner.dispose();
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
    /// - A snapshot of `(id, callback)` pairs is taken under lock before any
    ///   callback fires. The lock is released before iteration, preventing
    ///   deadlocks when a callback calls `add_listener` or `remove_listener`
    ///   on the same notifier (re-entrancy).
    /// - Before each callback fires, the listener's registration is re-checked.
    ///   If the listener was removed during notify (e.g. a previous callback
    ///   called `remove_listener`), the callback is silently skipped. A
    ///   listener removed mid-iteration does NOT fire.
    /// - Each callback is wrapped in `catch_unwind(AssertUnwindSafe(|| ...))`.
    ///   If a callback panics, the panic payload is logged via
    ///   `tracing::error!` and iteration continues with the next listener.
    ///   One panicking listener does NOT abort the rest.
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
    /// entry to this method; once past it, the in-flight snapshot is
    /// immune to subsequent disposal until iteration completes — a
    /// listener-callback calling `dispose` mid-notify does NOT break the
    /// current iteration.
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

    /// Whether any listeners are currently registered
    #[must_use]
    #[inline]
    pub fn has_listeners(&self) -> bool {
        !self.inner.is_empty()
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
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        if self.check_disposed() {
            // Release-mode no-op: return a fresh id that is not registered.
            return self.inner.mint_id();
        }
        // Unchecked for the same reason as `notify_listeners`: the branded
        // check above is the one entry check; a racing `dispose` must not
        // produce the inner channel's generically-worded failure.
        self.inner.add_unchecked(Arc::new(move |&()| listener()))
    }

    fn remove_listener(&self, id: ListenerId) {
        // Deliberately NO disposed check here, unlike `add_listener`/`dispose`/
        // `notify_listeners`: it is common that the owner of this instance is
        // disposed a frame earlier than its listeners, and allowing removal
        // after dispose makes it easier for listeners to clean up.
        // `dispose()` already cleared the listener map, so the lookup below is
        // naturally a no-op; the id simply isn't found.
        self.inner.remove_even_if_disposed(id);
    }

    fn remove_all_listeners(&self) {
        if self.check_disposed() {
            return;
        }
        // Unchecked: same single-entry-check rationale as `add_listener`.
        self.inner.remove_all_unchecked();
    }
}

/// A `ChangeNotifier` that holds a single value.
///
/// Reading, mutating and extracting the owned value do not require `Clone`.
/// Cloning the notifier requires `T: Clone`: it copies the value and shares
/// the listener channel.
/// Terminal retirement drops the value before its channel during ordinary
/// destruction; an incoming unwind retains both opaque owners. If channel
/// disposal fails during extraction, the value remains retained instead of
/// introducing a competing destructor failure.
///
/// Borrowed data inside `T` must remain valid until the notifier is destroyed.
/// Declare the notifier inside its referent's scope, after the referent, or
/// keep the referent alive longer. Extraction remains supported while the
/// referent is valid; no `T: 'static` bound is required.
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
#[derive(Clone)]
pub struct ValueNotifier<T> {
    value: Option<T>,
    notifier: Option<ChangeNotifier>,
}

/// Custody for the two separately owned terminal obligations, after their
/// physical owner has been emptied. Healthy paths drain both fields explicitly.
struct RetiringValueNotifier<T> {
    value: Option<T>,
    notifier: Option<ChangeNotifier>,
}

impl<T> Drop for RetiringValueNotifier<T> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            std::mem::forget(self.value.take());
            std::mem::forget(self.notifier.take());
        }
    }
}

impl<T> Drop for ValueNotifier<T> {
    fn drop(&mut self) {
        let mut retiring = self.extract_owned();
        if !std::thread::panicking() {
            // Match the former value-then-channel field order while preventing
            // the channel from retiring if the value's destruction fails.
            drop(retiring.value.take());
            drop(retiring.notifier.take());
        }
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

impl<T: Send + Sync> Listenable for ValueNotifier<T> {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier().add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier().remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier().remove_all_listeners();
    }
}

impl<T: Send + Sync> ValueListenable<T> for ValueNotifier<T> {
    fn value(&self) -> &T {
        self.value()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn test_change_notifier() {
        let notifier = ChangeNotifier::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let _id = notifier.add_listener(Arc::new(move || {
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
        let fired_2 = Arc::new(AtomicBool::new(false));
        let fired_3 = Arc::new(AtomicBool::new(false));
        let (fired_2c, fired_3c) = (Arc::clone(&fired_2), Arc::clone(&fired_3));

        let _ = notifier.add_listener(Arc::new(|| panic!("intentional test panic")));
        let _ = notifier.add_listener(Arc::new(move || {
            fired_2c.store(true, Ordering::SeqCst);
        }));
        let _ = notifier.add_listener(Arc::new(move || {
            fired_3c.store(true, Ordering::SeqCst);
        }));

        notifier.notify_listeners(); // must not abort

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
        let other_ran = Arc::new(AtomicUsize::new(0));
        let other_ran_clone = Arc::clone(&other_ran);

        // Listener #1: disposes the notifier mid-iteration.
        let _ = notifier.add_listener(Arc::new(move || {
            notifier_for_callback.dispose();
        }));
        // Listener #2: increments counter — proves iteration completes after
        // mid-flight dispose (snapshot was already taken).
        let _ = notifier.add_listener(Arc::new(move || {
            other_ran_clone.fetch_add(1, Ordering::SeqCst);
        }));

        // Must not panic — even though listener #1 sets is_disposed during
        // iteration, the snapshot is in-flight; the disposed-state check
        // ran at entry to notify_listeners, before the snapshot.
        notifier.notify_listeners();

        // Listener #2 must have run (it was in the snapshot taken before
        // listener #1 fired).
        assert_eq!(
            other_ran.load(Ordering::SeqCst),
            1,
            "snapshot-then-fire must complete iteration even if dispose called mid-flight"
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
