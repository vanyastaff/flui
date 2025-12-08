//! Listenable, change notification, and event bubbling types.
//!
//! This module provides:
//! - **Listenable**: Observer pattern for change notification (like Flutter's `ChangeNotifier`)
//! - **Notification**: Event bubbling up the view tree (like DOM event bubbling)
//!
//! # Change Notification
//!
//! ```rust
//! use flui_foundation::notifier::{ChangeNotifier, Listenable};
//! use std::sync::Arc;
//!
//! let notifier = ChangeNotifier::new();
//! let id = notifier.add_listener(Arc::new(|| println!("Changed!")));
//! notifier.notify_listeners();
//! ```
//!
//! # Event Bubbling
//!
//! ```rust
//! use flui_foundation::notifier::{Notification, DynNotification};
//! use flui_foundation::ElementId;
//!
//! #[derive(Debug, Clone)]
//! struct ButtonClicked { button_id: String }
//! impl Notification for ButtonClicked {}
//!
//! let notification = ButtonClicked { button_id: "ok".into() };
//! let dyn_notif: &dyn DynNotification = &notification;
//! ```

use parking_lot::Mutex;
use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::id::{ElementId, ListenerId};

/// A listener callback function.
pub type ListenerCallback = Arc<dyn Fn() + Send + Sync>;

/// An object that maintains a list of listeners.
///
/// Similar to Flutter's `Listenable`.
/// Uses interior mutability for thread-safe listener management.
pub trait Listenable {
    /// Register a listener callback.
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId;

    /// Remove a previously registered listener.
    fn remove_listener(&self, id: ListenerId);

    /// Remove all listeners.
    fn remove_all_listeners(&self);
}

/// A class that can be extended or mixed in that provides a change notification API.
///
/// Similar to Flutter's `ChangeNotifier`.
#[derive(Clone)]
pub struct ChangeNotifier {
    listeners: Arc<Mutex<HashMap<ListenerId, ListenerCallback>>>,
    next_id: Arc<AtomicUsize>,
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
        }
    }

    /// Generate a new unique listener ID.
    fn next_id(&self) -> ListenerId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        ListenerId::new(id)
    }

    /// Call all the registered listeners.
    pub fn notify_listeners(&self) {
        let listeners = self.listeners.lock();
        for callback in listeners.values() {
            callback();
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
        let id = self.next_id();
        self.listeners.lock().insert(id, listener);
        id
    }

    fn remove_listener(&self, id: ListenerId) {
        self.listeners.lock().remove(&id);
    }

    fn remove_all_listeners(&self) {
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
    /// Note: This does NOT notify listeners. Call `notify()` manually if needed.
    #[inline]
    pub const fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// Consumes the notifier and returns the inner value.
    #[must_use]
    #[inline]
    pub fn into_value(self) -> T {
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

impl<T: Clone + Default> Default for ValueNotifier<T> {
    #[inline]
    fn default() -> Self {
        Self::new(T::default())
    }
}

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

impl<T: Clone> Listenable for ValueNotifier<T> {
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

/// A listenable that merges multiple listenables.
///
/// Similar to Flutter's `Listenable.merge()`.
pub struct MergedListenable {
    #[allow(dead_code)]
    listenables: Vec<Box<dyn Listenable + Send>>,
    notifier: ChangeNotifier,
}

impl fmt::Debug for MergedListenable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MergedListenable")
            .field("source_count", &self.listenables.len())
            .field("listeners", &self.notifier.len())
            .finish()
    }
}

impl MergedListenable {
    /// Create a new merged listenable from multiple listenables.
    #[must_use]
    pub fn new(listenables: Vec<Box<dyn Listenable + Send>>) -> Self {
        Self {
            listenables,
            notifier: ChangeNotifier::new(),
        }
    }

    /// Create an empty merged listenable
    #[must_use]
    #[inline]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Notify all listeners.
    #[inline]
    pub fn notify(&self) {
        self.notifier.notify_listeners();
    }

    /// Returns the number of merged listenables
    #[must_use]
    #[inline]
    pub fn source_count(&self) -> usize {
        self.listenables.len()
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

impl Default for MergedListenable {
    #[inline]
    fn default() -> Self {
        Self::empty()
    }
}

impl Listenable for MergedListenable {
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

// ============================================================================
// EVENT BUBBLING (Notification)
// ============================================================================

/// Base trait for notifications that bubble up the widget tree.
///
/// Notifications are events that propagate from child to parent through the element tree.
/// Any widget can dispatch a notification, and ancestor widgets can listen for it.
///
/// # Thread Safety
///
/// All notifications must be `Send + Sync` to work in FLUI's multi-threaded environment.
///
/// # Example
///
/// ```rust
/// use flui_foundation::notifier::Notification;
/// use flui_foundation::ElementId;
///
/// #[derive(Debug, Clone)]
/// struct MyNotification { data: String }
///
/// impl Notification for MyNotification {
///     fn visit_ancestor(&self, element_id: ElementId) -> bool {
///         false // continue bubbling
///     }
/// }
/// ```
pub trait Notification: Any + Send + Sync + fmt::Debug {
    /// Called when visiting an ancestor element during bubbling.
    ///
    /// # Returns
    ///
    /// - `true`: Stop notification from bubbling further
    /// - `false`: Allow notification to continue bubbling (default)
    fn visit_ancestor(&self, _element_id: ElementId) -> bool {
        false
    }
}

/// Object-safe notification trait for type erasure.
///
/// Automatically implemented for all `Notification` types.
pub trait DynNotification: Send + Sync + fmt::Debug {
    /// Called when visiting an ancestor element during bubbling.
    fn visit_ancestor(&self, element_id: ElementId) -> bool;

    /// Get notification as Any for downcasting.
    fn as_any(&self) -> &dyn Any;
}

impl<T: Notification> DynNotification for T {
    fn visit_ancestor(&self, element_id: ElementId) -> bool {
        Notification::visit_ancestor(self, element_id)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_listener_id() {
        let id1 = ListenerId::new(1);
        let id2 = ListenerId::new(2);

        assert!(id1 < id2);
        assert_eq!(id1.get(), 1);
        assert_eq!(format!("{}", id1), "ListenerId(1)");
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
        let debug = format!("{:?}", notifier);
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
        let debug = format!("{:?}", notifier);
        assert!(debug.contains("ValueNotifier"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn test_value_notifier_display() {
        let notifier = ValueNotifier::new(42);
        assert_eq!(format!("{}", notifier), "42");
    }

    #[test]
    fn test_value_notifier_default() {
        let notifier: ValueNotifier<i32> = ValueNotifier::default();
        assert_eq!(*notifier, 0);
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

    #[test]
    fn test_merged_listenable() {
        let notifier1 = ChangeNotifier::new();
        let notifier2 = ChangeNotifier::new();

        let merged = MergedListenable::new(vec![Box::new(notifier1), Box::new(notifier2)]);

        assert_eq!(merged.source_count(), 2);
        assert!(merged.is_empty());

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let _ = merged.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(!merged.is_empty());
        assert_eq!(merged.len(), 1);

        merged.notify();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_merged_listenable_default() {
        let merged = MergedListenable::default();
        assert_eq!(merged.source_count(), 0);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_merged_listenable_debug() {
        let merged = MergedListenable::default();
        let debug = format!("{:?}", merged);
        assert!(debug.contains("MergedListenable"));
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

    // ========================================================================
    // Notification tests
    // ========================================================================

    #[derive(Debug, Clone)]
    struct TestNotification {
        value: i32,
    }

    impl Notification for TestNotification {}

    #[derive(Debug, Clone)]
    struct CustomBubblingNotification {
        stop_at: ElementId,
    }

    impl Notification for CustomBubblingNotification {
        fn visit_ancestor(&self, element_id: ElementId) -> bool {
            element_id == self.stop_at
        }
    }

    #[test]
    fn test_notification_trait() {
        let notification = TestNotification { value: 42 };
        let element_id = ElementId::new(1);
        assert!(!Notification::visit_ancestor(&notification, element_id));
    }

    #[test]
    fn test_custom_bubbling_logic() {
        let stop_at = ElementId::new(10);
        let other_element = ElementId::new(5);

        let notification = CustomBubblingNotification { stop_at };

        assert!(Notification::visit_ancestor(&notification, stop_at));
        assert!(!Notification::visit_ancestor(&notification, other_element));
    }

    #[test]
    fn test_dyn_notification_downcast() {
        let notification = TestNotification { value: 42 };
        let dyn_notification: &dyn DynNotification = &notification;

        let downcasted = dyn_notification.as_any().downcast_ref::<TestNotification>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }

    #[test]
    fn test_notification_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<TestNotification>();
        assert_sync::<TestNotification>();
    }
}
