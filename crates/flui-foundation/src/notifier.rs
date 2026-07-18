//! Listenable and change notification types.
//!
//! This module provides the observer pattern for reactive UI updates,
//! similar to Flutter's `ChangeNotifier` system in foundation.
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

use std::{
    collections::HashMap,
    fmt,
    ops::Deref,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;

use crate::id::ListenerId;

/// A listener callback function.
// Audit I-16: explicit `+ 'static` bound on the listener callback —
// pre-cycle the implicit `'static` was confusing (callback types
// stored long-term must be static, but the trait-object syntax
// elided it). Explicit doc avoids ambiguity for callers
// constructing the Arc.
pub type ListenerCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// An object that maintains a list of listeners.
///
/// Similar to Flutter's `Listenable`.
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
    /// has been disposed (Flutter parity: `ChangeNotifier.removeListener`
    /// tolerates a disposed receiver so teardown code can always detach).
    fn remove_listener(&self, id: ListenerId);

    /// Remove all listeners.
    fn remove_all_listeners(&self);
}

/// An interface for subclasses of [`Listenable`] that expose a value.
///
/// Similar to Flutter's `ValueListenable<T>`.
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
/// Similar to Flutter's `ChangeNotifier`.
///
/// # Disposal
///
/// After [`dispose`] has been called, [`add_listener`] and
/// [`notify_listeners`] panic in debug builds via `debug_assert!` and
/// degrade to a `tracing::warn!` + no-op in release builds. Mirrors
/// Flutter's `ChangeNotifier.dispose` and `_debugAssertNotDisposed` semantics
/// (`flutter/lib/src/foundation/change_notifier.dart:181`, :376).
///
/// [`remove_listener`] is the deliberate exception: it carries no disposed
/// check in either build profile, matching `ChangeNotifier.removeListener`
/// upstream, which has no `debugAssertNotDisposed` so that teardown code can
/// detach from an already-disposed listenable. It is always a silent no-op
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
    listeners: Arc<Mutex<HashMap<ListenerId, ListenerCallback>>>,
    next_id: Arc<AtomicUsize>,
    is_disposed: Arc<AtomicBool>,
}

impl Default for ChangeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ChangeNotifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangeNotifier")
            .field("listeners_count", &self.listeners.lock().len())
            .finish_non_exhaustive()
    }
}

impl ChangeNotifier {
    /// Create a new change notifier.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicUsize::new(1)),
            is_disposed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Generate a new unique listener ID.
    fn next_id(&self) -> ListenerId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        ListenerId::new(id)
    }

    /// Returns `true` if [`dispose`](Self::dispose) has been called on this
    /// notifier (or any of its clones — the disposed state is shared).
    #[must_use]
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.is_disposed.load(Ordering::Acquire)
    }

    /// Discards listeners and marks this notifier as disposed.
    ///
    /// After this is called, the notifier is not in a usable state:
    /// subsequent calls to [`add_listener`] or [`notify_listeners`] panic in
    /// debug builds via `debug_assert!` and degrade to a `tracing::warn!` +
    /// no-op in release builds. [`remove_listener`] is the deliberate
    /// exception — it stays a silent no-op after dispose (Flutter parity;
    /// see [`Listenable::remove_listener`]'s impl on this type).
    ///
    /// Mirrors Flutter's `ChangeNotifier.dispose` at
    /// `flutter/lib/src/foundation/change_notifier.dart:376`. Disposal does
    /// NOT notify listeners; consumers must decide whether to notify before
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
        // Idempotent: second call sees true and exits.
        if self.is_disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.listeners.lock().clear();
    }

    /// Debug-asserts that this notifier has not been disposed.
    ///
    /// In debug builds, panics with `"ChangeNotifier used after dispose"`
    /// via `debug_assert!`. In release builds, emits a `tracing::warn!` and
    /// returns `true` to indicate the caller should early-return as a no-op
    /// per plan §D7 (Flutter parity: release degrades gracefully).
    ///
    /// Returns `true` if the notifier is disposed (caller should no-op),
    /// `false` if usable.
    ///
    /// Mirrors Flutter's `_debugAssertNotDisposed`
    /// (`change_notifier.dart:181`).
    #[inline]
    fn check_disposed(&self) -> bool {
        if self.is_disposed.load(Ordering::Acquire) {
            // cfg-explicit layout: an earlier version combined
            // `debug_assert!(false, ..)` and `tracing::warn!` in one block,
            // which was misleading — in debug builds the assert diverges, so
            // the warn! below it was dead code; in release builds the assert
            // compiled out and only the warn! ran. Splitting on
            // `cfg(debug_assertions)` makes the intent unambiguous: debug
            // panics immediately (hard contract violation), release degrades
            // gracefully with a warning (Flutter parity).
            #[cfg(debug_assertions)]
            panic!(
                "ChangeNotifier used after dispose: once dispose() has been \
                 called, the notifier can no longer be used"
            );
            // The release-only block is unreachable in debug builds because the
            // `panic!` above diverges; `allow(unreachable_code)` documents that.
            #[allow(unreachable_code)]
            {
                tracing::warn!("ChangeNotifier used after dispose");
                return true;
            }
        }
        false
    }

    /// Call all the registered listeners.
    ///
    /// Snapshot semantics (Flutter parity, `ChangeNotifier.notifyListeners`):
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
    /// Listeners fire in registration order (`ListenerId` ascending), matching
    /// Flutter's array-order iteration; the backing `HashMap` does not preserve
    /// insertion order, so the snapshot is sorted by id before firing.
    ///
    /// Post-snapshot *additions* are NOT fired in the current notify cycle
    /// (same as Flutter); only listeners present at snapshot time and still
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
        // Stack-allocate the snapshot for the common case (1-4 listeners).
        // `SmallVec<[_; 4]>` keeps inline storage capacity 4 — when there are
        // ≤4 listeners the snapshot is purely stack memory; ≥5 listeners
        // spills to the heap. The snapshot carries `(ListenerId,
        // ListenerCallback)` pairs so each entry can be re-checked against
        // the live registration before firing.
        //
        // `SmallVec` is chosen over `tinyvec::ArrayVec` deliberately.
        // `ListenerCallback` is `Arc<dyn Fn() + Send + Sync>`, which does NOT
        // implement `Default`; `tinyvec` requires `T: Default` for every
        // element type, so it cannot store these callbacks. `SmallVec` imposes
        // no `Default` bound, so it is the only inline-storage option here.
        let mut snapshot: smallvec::SmallVec<[(ListenerId, ListenerCallback); 4]> = self
            .listeners
            .lock()
            .iter()
            .map(|(&id, cb)| (id, Arc::clone(cb)))
            .collect();
        // Fire in registration order (Flutter parity). `ListenerId` is assigned
        // monotonically by `next_id`, so sorting by id reproduces the order in
        // which listeners were added — the backing `HashMap` does not preserve
        // insertion order. This also makes the remove-during-notify contract
        // observe a deterministic ordering rather than arbitrary hash order.
        snapshot.sort_unstable_by_key(|(id, _)| *id);

        for (id, callback) in &snapshot {
            // Re-check registration before firing. Acquires the lock
            // briefly for the lookup and releases it before the callback runs,
            // so a listener individually removed mid-notify (by an earlier
            // callback's `remove_listener`) is skipped rather than invoked.
            //
            // Disposal is handled distinctly: `dispose()` clears the entire map
            // (and sets `is_disposed`), but FLUI guarantees that a `dispose()`
            // call made *mid-notify* by a listener does not abort the in-flight
            // snapshot — the remaining snapshot listeners still fire (see the
            // `dispose_during_notify_iteration_safe` reentrancy contract on
            // [`dispose`](Self::dispose)). We therefore only consult the live
            // map for the per-listener skip while the notifier is NOT disposed;
            // once disposed mid-flight, the snapshot is honoured to completion.
            if !self.is_disposed.load(Ordering::Acquire) && !self.listeners.lock().contains_key(id)
            {
                continue; // Individually removed during notify; skip.
            }
            // Isolate each callback's panic so one panicking listener does
            // not abort the remaining listeners. `AssertUnwindSafe` is sound
            // here: the callback is `Arc<dyn Fn() + Send + Sync>` invoked by
            // shared reference, and on unwind no borrowed state crosses the
            // boundary in a broken-invariant form — we only read the snapshot.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| callback())) {
                tracing::error!(
                    listener_id = ?id,
                    panic_payload = ?payload,
                    "ChangeNotifier listener panicked; continuing with remaining listeners"
                );
            }
        }
    }

    /// Whether any listeners are currently registered
    #[must_use]
    #[inline]
    pub fn has_listeners(&self) -> bool {
        !self.listeners.lock().is_empty()
    }

    /// Checks if there are no listeners registered
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.listeners.lock().is_empty()
    }

    /// Returns the number of listeners currently registered
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.listeners.lock().len()
    }
}

impl Listenable for ChangeNotifier {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        if self.check_disposed() {
            // Release-mode no-op: return a fresh id that is not registered.
            return self.next_id();
        }
        let id = self.next_id();
        self.listeners.lock().insert(id, listener);
        id
    }

    fn remove_listener(&self, id: ListenerId) {
        // Deliberately NO disposed check here — Flutter parity.
        // `ChangeNotifier.removeListener` in `change_notifier.dart` carries no
        // `debugAssertNotDisposed`, unlike `addListener`/`dispose` (and the
        // implicit assert `notifyListeners` inherits): its doc comment states
        // "This method returns immediately if [dispose] has been called," and
        // the rationale is explicit — "it is common that the owner of this
        // instance would be disposed a frame earlier than the listeners.
        // Allowing calls to this method after it is disposed makes it easier
        // for listeners to properly clean up." `dispose()` already cleared
        // `self.listeners`, so the lookup below is naturally a no-op; the id
        // simply isn't found.
        self.listeners.lock().remove(&id);
    }

    fn remove_all_listeners(&self) {
        if self.check_disposed() {
            return;
        }
        self.listeners.lock().clear();
    }
}

/// A `ChangeNotifier` that holds a single value.
///
/// Similar to Flutter's `ValueNotifier`.
#[derive(Clone)]
pub struct ValueNotifier<T: Clone> {
    value: T,
    notifier: ChangeNotifier,
}

impl<T: Clone> ValueNotifier<T> {
    /// Create a new value notifier with an initial value.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            value,
            notifier: ChangeNotifier::new(),
        }
    }

    /// Returns a reference to the current value.
    #[must_use]
    #[inline]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Returns a mutable reference to the current value.
    ///
    /// Note: This does NOT notify listeners. Call `notify()` manually if
    /// needed.
    #[inline]
    pub const fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// Consumes the notifier and returns the inner value.
    ///
    /// Audit I-20: calls `self.notifier.dispose()` before the inner
    /// `ChangeNotifier` is dropped so the dispose hook (PR #84
    /// template) fires once. Pre-cycle the listeners were silently
    /// dropped without the dispose protocol — any registered
    /// `assert_alive` guard on a sibling subscriber never saw the
    /// disposal event.
    #[must_use]
    #[inline]
    pub fn into_value(self) -> T {
        self.notifier.dispose();
        self.value
    }

    /// Replaces the value and returns the old value.
    ///
    /// Notifies listeners if the new value is different from the old value.
    pub fn replace(&mut self, new_value: T) -> T
    where
        T: PartialEq,
    {
        let old_value = std::mem::replace(&mut self.value, new_value);
        if self.value != old_value {
            self.notifier.notify_listeners();
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
        let value = std::mem::take(&mut self.value);
        self.notifier.notify_listeners();
        value
    }

    /// Set a new value and notify listeners if the value changed.
    pub fn set_value(&mut self, new_value: T)
    where
        T: PartialEq,
    {
        if self.value != new_value {
            self.value = new_value;
            self.notifier.notify_listeners();
        }
    }

    /// Set a new value without checking for equality.
    ///
    /// Always notifies listeners, even if the value didn't change.
    pub fn set_value_force(&mut self, new_value: T) {
        self.value = new_value;
        self.notifier.notify_listeners();
    }

    /// Update the value using a function.
    ///
    /// Notifies listeners after the update.
    pub fn update<F>(&mut self, f: F)
    where
        F: FnOnce(&mut T),
    {
        f(&mut self.value);
        self.notifier.notify_listeners();
    }

    /// Manually notify all listeners.
    ///
    /// Useful when the value is mutated through `value_mut()`.
    #[inline]
    pub fn notify(&self) {
        self.notifier.notify_listeners();
    }

    /// Returns the number of listeners currently registered
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.notifier.len()
    }

    /// Checks if there are no listeners registered
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.notifier.is_empty()
    }

    /// Whether any listeners are currently registered
    #[must_use]
    #[inline]
    pub fn has_listeners(&self) -> bool {
        self.notifier.has_listeners()
    }
}

impl<T: Clone + fmt::Debug> fmt::Debug for ValueNotifier<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueNotifier")
            .field("value", &self.value)
            .field("listeners", &self.notifier.len())
            .finish()
    }
}

// `Default for ValueNotifier<T>` is removed intentionally. A defaulted
// notifier would have a default-constructed value AND a fresh identity (no
// listeners); two `ValueNotifier::<T>::default()` calls produce notifiers that
// are `==` by value yet are observably distinct objects. This violates the
// principle of least surprise, and Flutter's `ValueNotifier` likewise has no
// default constructor. Construct explicitly via `ValueNotifier::new(value)`.

impl<T: Clone + PartialEq> PartialEq for ValueNotifier<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Clone + Eq> Eq for ValueNotifier<T> {}

impl<T: Clone + fmt::Display> fmt::Display for ValueNotifier<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.value, f)
    }
}

impl<T: Clone> Deref for ValueNotifier<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: Clone> AsRef<T> for ValueNotifier<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.value
    }
}

impl<T: Clone + Send + Sync> Listenable for ValueNotifier<T> {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

impl<T: Clone + Send + Sync> ValueListenable<T> for ValueNotifier<T> {
    fn value(&self) -> &T {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn test_listener_id() {
        let id1 = ListenerId::new(1);
        let id2 = ListenerId::new(2);

        assert!(id1 < id2);
        assert_eq!(id1.get(), 1);
        assert_eq!(format!("{id1}"), "Listener(1)");
    }

    #[test]
    fn test_listener_id_conversions() {
        let id = ListenerId::new(42);
        assert_eq!(id.get(), 42);

        let n: usize = id.into();
        assert_eq!(n, 42);
    }

    #[test]
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

    #[test]
    fn test_change_notifier_debug() {
        let notifier = ChangeNotifier::new();
        let debug = format!("{notifier:?}");
        assert!(debug.contains("ChangeNotifier"));
    }

    #[test]
    fn test_change_notifier_remove() {
        let notifier = ChangeNotifier::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let id = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        notifier.remove_listener(id);
        assert!(!notifier.has_listeners());
        assert!(notifier.is_empty());

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_value_notifier() {
        let mut notifier = ValueNotifier::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let _ = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        notifier.set_value(5);
        assert_eq!(*notifier.value(), 5);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        notifier.set_value(5);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        notifier.set_value_force(5);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_value_notifier_deref() {
        let notifier = ValueNotifier::new(42);
        assert_eq!(*notifier, 42);

        let value: &i32 = notifier.as_ref();
        assert_eq!(*value, 42);
    }

    #[test]
    fn test_value_notifier_debug() {
        let notifier = ValueNotifier::new(42);
        let debug = format!("{notifier:?}");
        assert!(debug.contains("ValueNotifier"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn test_value_notifier_display() {
        let notifier = ValueNotifier::new(42);
        assert_eq!(format!("{notifier}"), "42");
    }

    #[test]
    fn valuenotifier_new_creates_distinct_notifiers() {
        // `Default for ValueNotifier<T>` was removed, so notifiers are
        // constructed explicitly with `new`. Two notifiers built from the
        // same value are `==` by value but are observably distinct objects
        // (independent listener registries) — the exact surprise a `Default`
        // impl would have hidden.
        let a = ValueNotifier::new(0u32);
        let b = ValueNotifier::new(0u32);
        assert_eq!(a, b, "equal by value");

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let _ = a.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));
        // `a` has a listener; `b` must not — distinct identities.
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 0);
        a.notify();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        b.notify();
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "b is a distinct notifier; notifying it must not touch a's listener"
        );
    }

    #[test]
    fn test_value_notifier_equality() {
        let notifier1 = ValueNotifier::new(42);
        let notifier2 = ValueNotifier::new(42);
        let notifier3 = ValueNotifier::new(100);

        assert_eq!(notifier1, notifier2);
        assert_ne!(notifier1, notifier3);
    }

    #[test]
    fn test_value_notifier_into_value() {
        let notifier = ValueNotifier::new(42);
        let value = notifier.into_value();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_value_notifier_take() {
        let mut notifier = ValueNotifier::new(42);
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let _ = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let value = notifier.take();
        assert_eq!(value, 42);
        assert_eq!(*notifier, 0);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_value_notifier_replace() {
        let mut notifier = ValueNotifier::new(10);
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let _ = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let old = notifier.replace(20);
        assert_eq!(old, 10);
        assert_eq!(*notifier, 20);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_value_notifier_value_mut() {
        let mut notifier = ValueNotifier::new(10);
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let _ = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        *notifier.value_mut() = 20;
        assert_eq!(*notifier, 20);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        notifier.notify();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_value_notifier_update() {
        let mut notifier = ValueNotifier::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let _ = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        notifier.update(|val| *val += 10);
        assert_eq!(*notifier.value(), 10);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn removed_listener_does_not_fire_during_notify() {
        // A listener removed *during* iteration (by a previously-fired
        // listener) must NOT fire. Given listeners A and B, where A removes B
        // mid-notify, B must not fire (post-removal skip).
        use std::sync::atomic::AtomicBool;

        use parking_lot::Mutex;

        let notifier = ChangeNotifier::new();
        let fired_b = Arc::new(AtomicBool::new(false));
        let fired_b_clone = Arc::clone(&fired_b);
        let notifier_clone = notifier.clone();

        let id_b_cell = Arc::new(Mutex::new(None::<ListenerId>));
        let id_b_cell_clone = Arc::clone(&id_b_cell);

        let id_a = notifier.add_listener(Arc::new(move || {
            if let Some(id) = *id_b_cell_clone.lock() {
                notifier_clone.remove_listener(id);
            }
        }));

        let id_b = notifier.add_listener(Arc::new(move || {
            fired_b_clone.store(true, Ordering::SeqCst);
        }));
        *id_b_cell.lock() = Some(id_b);

        notifier.notify_listeners();
        assert!(
            !fired_b.load(Ordering::SeqCst),
            "removed listener must not fire"
        );
        let _ = id_a;
    }

    #[test]
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

    #[test]
    fn notify_listeners_fires_all_when_no_panic() {
        // TRIANGULATE: 3 listeners, no panics, all 3 fire.
        let notifier = ChangeNotifier::new();
        let count = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let c = Arc::clone(&count);
            let _ = notifier.add_listener(Arc::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
            }));
        }
        notifier.notify_listeners();
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn notify_listeners_empty() {
        // TRIANGULATE: no listeners registered; no panic, no-op.
        let notifier = ChangeNotifier::new();
        notifier.notify_listeners();
        assert_eq!(notifier.len(), 0);
    }

    #[test]
    fn notify_listeners_skips_all_removed() {
        // TRIANGULATE: all listeners removed before notify; none fire.
        let notifier = ChangeNotifier::new();
        let count = Arc::new(AtomicUsize::new(0));
        let mut ids = Vec::new();
        for _ in 0..3 {
            let c = Arc::clone(&count);
            ids.push(notifier.add_listener(Arc::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
            })));
        }
        for id in ids {
            notifier.remove_listener(id);
        }
        notifier.notify_listeners();
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_multiple_listeners() {
        let notifier = ChangeNotifier::new();
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        let c1 = Arc::clone(&counter1);
        let c2 = Arc::clone(&counter2);

        let _ = notifier.add_listener(Arc::new(move || {
            c1.fetch_add(1, Ordering::SeqCst);
        }));

        let _ = notifier.add_listener(Arc::new(move || {
            c2.fetch_add(2, Ordering::SeqCst);
        }));

        notifier.notify_listeners();

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 2);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_listener_id_serde() {
        let id = ListenerId::new(42);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "42");

        let deserialized: ListenerId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    // ------------------------------------------------------------------
    // ChangeNotifier::dispose + disposed-state assertion
    //
    // Mirrors Flutter's `ChangeNotifier.dispose` at
    // flutter/lib/src/foundation/change_notifier.dart:181 (debugAssertNotDisposed)
    // and :376 (dispose).
    // ------------------------------------------------------------------

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ChangeNotifier used after dispose")]
    fn dispose_then_add_listener_debug_asserts() {
        let notifier = ChangeNotifier::new();
        notifier.dispose();
        // Must panic in debug builds (release degrades to tracing::warn! +
        // no-op; release-mode behavior is sanity-checked separately).
        let _ = notifier.add_listener(Arc::new(|| {}));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ChangeNotifier used after dispose")]
    fn dispose_then_notify_debug_asserts() {
        let notifier = ChangeNotifier::new();
        notifier.dispose();
        notifier.notify_listeners();
    }

    #[test]
    fn dispose_then_remove_listener_is_a_silent_no_op() {
        // Flutter parity: `ChangeNotifier.removeListener` carries no
        // `debugAssertNotDisposed`, unlike `addListener`/`notifyListeners`/
        // `dispose` — its doc comment explains that teardown code must be
        // able to detach from an already-disposed listenable. Must not
        // panic in debug OR release; behavior is identical in both.
        let notifier = ChangeNotifier::new();
        let id = notifier.add_listener(Arc::new(|| {}));
        notifier.dispose();
        notifier.remove_listener(id); // must not panic
        notifier.remove_listener(ListenerId::new(9999)); // unknown id: also fine
    }

    #[test]
    fn dispose_is_idempotent() {
        let notifier = ChangeNotifier::new();
        let _ = notifier.add_listener(Arc::new(|| {}));
        assert_eq!(notifier.len(), 1);

        notifier.dispose();
        assert_eq!(notifier.len(), 0, "dispose must clear listeners");
        assert!(
            notifier.is_disposed(),
            "is_disposed must be true after dispose"
        );

        // Second dispose is a no-op — must NOT panic.
        notifier.dispose();
        assert_eq!(notifier.len(), 0);
        assert!(notifier.is_disposed());
    }

    #[test]
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
}
