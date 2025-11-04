//! Signal runtime - thread-local storage for signal values
//!
//! The runtime manages all signal values in a generational arena,
//! providing O(1) access and automatic cleanup.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use crate::signal::SignalId;

/// Callback type for signal subscribers
pub type SubscriberCallback = Arc<dyn Fn() + Send + Sync>;

/// Entry in the signal arena
struct SignalEntry {
    /// The value (type-erased)
    value: Box<dyn Any>,
    /// Subscribers to this signal
    subscribers: HashMap<usize, SubscriberCallback>,
    /// Next subscription ID
    next_sub_id: usize,
}

impl SignalEntry {
    fn new<T: 'static>(value: T) -> Self {
        Self {
            value: Box::new(value),
            subscribers: HashMap::new(),
            next_sub_id: 0,
        }
    }
}

/// Signal runtime - manages signal storage and subscriptions
///
/// This is a generational arena that stores all signal values for the current thread.
pub struct SignalRuntime {
    /// Signal storage (arena)
    signals: Vec<Option<SignalEntry>>,
    /// Free list for recycling IDs
    free_list: Vec<usize>,
}

impl SignalRuntime {
    /// Create a new runtime
    pub fn new() -> Self {
        Self {
            signals: Vec::new(),
            free_list: Vec::new(),
        }
    }

    /// Create a new signal and return its ID
    pub fn create_signal<T: 'static>(&mut self, value: T) -> SignalId {
        let entry = SignalEntry::new(value);

        // Try to reuse a free slot
        if let Some(id) = self.free_list.pop() {
            self.signals[id] = Some(entry);
            SignalId::new(id)
        } else {
            // Allocate new slot
            let id = self.signals.len();
            self.signals.push(Some(entry));
            SignalId::new(id)
        }
    }

    /// Get the value of a signal (cloned)
    pub fn get_signal<T: Clone + 'static>(&self, id: SignalId) -> T {
        self.signals
            .get(id.as_usize())
            .and_then(|opt| opt.as_ref())
            .and_then(|entry| entry.value.downcast_ref::<T>())
            .cloned()
            .expect("Signal not found or wrong type")
    }

    /// Access a signal's value with a closure (no cloning)
    pub fn with_signal<T: 'static, R>(&self, id: SignalId, f: impl FnOnce(&T) -> R) -> R {
        self.signals
            .get(id.as_usize())
            .and_then(|opt| opt.as_ref())
            .and_then(|entry| entry.value.downcast_ref::<T>())
            .map(f)
            .expect("Signal not found or wrong type")
    }

    /// Set the value of a signal
    pub fn set_signal<T: 'static>(&mut self, id: SignalId, value: T) {
        if let Some(Some(entry)) = self.signals.get_mut(id.as_usize()) {
            entry.value = Box::new(value);
        } else {
            panic!("Signal not found");
        }
    }

    /// Update a signal's value with a closure
    pub fn update_signal<T: 'static>(&mut self, id: SignalId, f: impl FnOnce(&mut T)) {
        if let Some(Some(entry)) = self.signals.get_mut(id.as_usize()) {
            if let Some(value) = entry.value.downcast_mut::<T>() {
                f(value);
            } else {
                panic!("Signal type mismatch");
            }
        } else {
            panic!("Signal not found");
        }
    }

    /// Subscribe to a signal
    pub fn subscribe(&mut self, id: SignalId, callback: SubscriberCallback) -> usize {
        if let Some(Some(entry)) = self.signals.get_mut(id.as_usize()) {
            let sub_id = entry.next_sub_id;
            entry.next_sub_id += 1;
            entry.subscribers.insert(sub_id, callback);
            sub_id
        } else {
            panic!("Signal not found");
        }
    }

    /// Unsubscribe from a signal
    pub fn unsubscribe(&mut self, id: SignalId, subscription_id: usize) {
        if let Some(Some(entry)) = self.signals.get_mut(id.as_usize()) {
            entry.subscribers.remove(&subscription_id);
        }
    }

    /// Notify all subscribers of a signal
    pub fn notify_subscribers(&self, id: SignalId) {
        if let Some(Some(entry)) = self.signals.get(id.as_usize()) {
            for callback in entry.subscribers.values() {
                callback();
            }
        }
    }

    /// Remove a signal and free its slot
    pub fn remove_signal(&mut self, id: SignalId) {
        if let Some(entry) = self.signals.get_mut(id.as_usize()) {
            *entry = None;
            self.free_list.push(id.as_usize());
        }
    }

    /// Get statistics about the runtime
    pub fn stats(&self) -> RuntimeStats {
        let total_signals = self.signals.len();
        let active_signals = self.signals.iter().filter(|e| e.is_some()).count();
        let free_slots = self.free_list.len();

        RuntimeStats {
            total_signals,
            active_signals,
            free_slots,
        }
    }
}

impl Default for SignalRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the signal runtime
#[derive(Debug, Clone, Copy)]
pub struct RuntimeStats {
    pub total_signals: usize,
    pub active_signals: usize,
    pub free_slots: usize,
}

// Thread-local runtime instance
thread_local! {
    static SIGNAL_RUNTIME: RefCell<SignalRuntime> = RefCell::new(SignalRuntime::new());
}

/// Execute a function with access to the signal runtime
pub fn with_runtime<R>(f: impl FnOnce(&mut SignalRuntime) -> R) -> R {
    SIGNAL_RUNTIME.with(|runtime| f(&mut runtime.borrow_mut()))
}

/// Get runtime statistics
pub fn runtime_stats() -> RuntimeStats {
    with_runtime(|runtime| runtime.stats())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get_signal() {
        let mut runtime = SignalRuntime::new();
        let id = runtime.create_signal(42);
        assert_eq!(runtime.get_signal::<i32>(id), 42);
    }

    #[test]
    fn test_set_signal() {
        let mut runtime = SignalRuntime::new();
        let id = runtime.create_signal(0);
        runtime.set_signal(id, 100);
        assert_eq!(runtime.get_signal::<i32>(id), 100);
    }

    #[test]
    fn test_update_signal() {
        let mut runtime = SignalRuntime::new();
        let id = runtime.create_signal(10);
        runtime.update_signal(id, |v: &mut i32| *v += 5);
        assert_eq!(runtime.get_signal::<i32>(id), 15);
    }

    #[test]
    fn test_with_signal() {
        let runtime = SignalRuntime::new();
        let mut runtime_mut = runtime;
        let id = runtime_mut.create_signal(String::from("Hello"));
        let len = runtime_mut.with_signal(id, |s: &String| s.len());
        assert_eq!(len, 5);
    }

    #[test]
    fn test_subscribe_and_notify() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let mut runtime = SignalRuntime::new();
        let id = runtime.create_signal(0);

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        runtime.subscribe(
            id,
            Arc::new(move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        );

        runtime.notify_subscribers(id);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        runtime.notify_subscribers(id);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
