//! Listenable and notification types
//!
//! This module provides types for observable values and change notifications,
//! similar to Flutter's foundation library.

use std::sync::{Arc, Mutex, Weak};
use std::collections::HashMap;

/// A listener callback function.
pub type ListenerCallback = Arc<dyn Fn() + Send + Sync>;

/// An object that maintains a list of listeners.
///
/// Similar to Flutter's `Listenable`.
pub trait Listenable {
    /// Register a listener callback.
    ///
    /// The callback will be called whenever `notify_listeners()` is called.
    fn add_listener(&mut self, listener: ListenerCallback) -> ListenerId;

    /// Remove a previously registered listener.
    fn remove_listener(&mut self, id: ListenerId);

    /// Remove all listeners.
    fn remove_all_listeners(&mut self);
}

/// Unique identifier for a listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(usize);

/// A class that can be extended or mixed in that provides a change notification API.
///
/// Similar to Flutter's `ChangeNotifier`.
#[derive(Clone)]
pub struct ChangeNotifier {
    listeners: Arc<Mutex<HashMap<ListenerId, ListenerCallback>>>,
    next_id: usize,
}

impl Default for ChangeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeNotifier {
    /// Create a new change notifier.
    pub fn new() -> Self {
        Self {
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_id: 0,
        }
    }

    /// Generate a new unique listener ID.
    fn next_id(&mut self) -> ListenerId {
        let id = ListenerId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Call all the registered listeners.
    ///
    /// Call this method whenever the object changes, to notify clients that the object has been updated.
    pub fn notify_listeners(&self) {
        let listeners = self.listeners.lock().unwrap();
        for callback in listeners.values() {
            callback();
        }
    }

    /// Whether any listeners are currently registered.
    pub fn has_listeners(&self) -> bool {
        !self.listeners.lock().unwrap().is_empty()
    }

    /// The number of listeners currently registered.
    pub fn listener_count(&self) -> usize {
        self.listeners.lock().unwrap().len()
    }
}

impl Listenable for ChangeNotifier {
    fn add_listener(&mut self, listener: ListenerCallback) -> ListenerId {
        let id = self.next_id();
        self.listeners.lock().unwrap().insert(id, listener);
        id
    }

    fn remove_listener(&mut self, id: ListenerId) {
        self.listeners.lock().unwrap().remove(&id);
    }

    fn remove_all_listeners(&mut self) {
        self.listeners.lock().unwrap().clear();
    }
}

/// A ChangeNotifier that holds a single value.
///
/// Similar to Flutter's `ValueNotifier`.
#[derive(Clone)]
pub struct ValueNotifier<T: Clone> {
    value: T,
    notifier: ChangeNotifier,
}

impl<T: Clone> ValueNotifier<T> {
    /// Create a new value notifier with an initial value.
    pub fn new(value: T) -> Self {
        Self {
            value,
            notifier: ChangeNotifier::new(),
        }
    }

    /// Get the current value.
    pub fn value(&self) -> &T {
        &self.value
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
}

impl<T: Clone> Listenable for ValueNotifier<T> {
    fn add_listener(&mut self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&mut self, id: ListenerId) {
        self.notifier.remove_listener(id)
    }

    fn remove_all_listeners(&mut self) {
        self.notifier.remove_all_listeners()
    }
}

/// A listenable that merges multiple listenables.
///
/// Similar to Flutter's `Listenable.merge()`.
pub struct MergedListenable {
    listenables: Vec<Box<dyn Listenable + Send>>,
    notifier: ChangeNotifier,
}

impl MergedListenable {
    /// Create a new merged listenable from multiple listenables.
    pub fn new(listenables: Vec<Box<dyn Listenable + Send>>) -> Self {
        Self {
            listenables,
            notifier: ChangeNotifier::new(),
        }
    }

    /// Notify all listeners.
    pub fn notify(&self) {
        self.notifier.notify_listeners();
    }
}

impl Listenable for MergedListenable {
    fn add_listener(&mut self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&mut self, id: ListenerId) {
        self.notifier.remove_listener(id)
    }

    fn remove_all_listeners(&mut self) {
        self.notifier.remove_all_listeners()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_change_notifier() {
        let mut notifier = ChangeNotifier::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // Add listener
        let counter_clone = Arc::clone(&counter);
        let _id = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(notifier.has_listeners());
        assert_eq!(notifier.listener_count(), 1);

        // Notify
        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_change_notifier_remove() {
        let mut notifier = ChangeNotifier::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let id = notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Remove listener
        notifier.remove_listener(id);
        assert!(!notifier.has_listeners());

        notifier.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Still 1, not incremented
    }

    #[test]
    fn test_value_notifier() {
        let mut notifier = ValueNotifier::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        // Set value (should notify)
        notifier.set_value(5);
        assert_eq!(*notifier.value(), 5);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Set same value (should not notify)
        notifier.set_value(5);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Force set (should notify even with same value)
        notifier.set_value_force(5);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_value_notifier_update() {
        let mut notifier = ValueNotifier::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        notifier.add_listener(Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        // Update value
        notifier.update(|val| *val += 10);
        assert_eq!(*notifier.value(), 10);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_multiple_listeners() {
        let mut notifier = ChangeNotifier::new();
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        let c1 = Arc::clone(&counter1);
        let c2 = Arc::clone(&counter2);

        notifier.add_listener(Arc::new(move || {
            c1.fetch_add(1, Ordering::SeqCst);
        }));

        notifier.add_listener(Arc::new(move || {
            c2.fetch_add(2, Ordering::SeqCst);
        }));

        notifier.notify_listeners();

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 2);
    }
}
