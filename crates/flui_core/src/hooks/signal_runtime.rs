//! Signal runtime for thread-local signal storage.
//!
//! Provides Copy-based signals with thread-local arena storage.
//! This eliminates the need for `.clone()` while maintaining thread-safety.

use super::signal::{SignalId, SubscriptionId};
use parking_lot::Mutex;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// Type-erased signal data stored in the runtime.
struct SignalDataErased {
    /// Type ID for safe downcasting
    type_id: TypeId,

    /// Type-erased value (Arc<Mutex<T>> wrapped in Box<dyn Any>)
    value: Box<dyn Any + Send + Sync>,

    /// Type-erased subscribers
    /// Maps SubscriptionId -> callback
    subscribers: Arc<Mutex<HashMap<SubscriptionId, Arc<dyn Fn() + Send + Sync>>>>,

    /// Type name for debugging
    #[cfg(debug_assertions)]
    type_name: &'static str,
}

impl std::fmt::Debug for SignalDataErased {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(debug_assertions)]
        {
            f.debug_struct("SignalDataErased")
                .field("type_name", &self.type_name)
                .field("type_id", &self.type_id)
                .field("subscribers_count", &self.subscribers.lock().len())
                .finish_non_exhaustive()
        }
        #[cfg(not(debug_assertions))]
        {
            f.debug_struct("SignalDataErased")
                .field("type_id", &self.type_id)
                .field("subscribers_count", &self.subscribers.lock().len())
                .finish_non_exhaustive()
        }
    }
}

/// Typed signal data (stored inside SignalDataErased).
pub struct SignalData<T> {
    /// The actual value (thread-safe)
    pub value: Arc<Mutex<T>>,

    /// Subscribers that get notified on changes
    #[allow(dead_code)]
    subscribers: Arc<Mutex<HashMap<SubscriptionId, Arc<dyn Fn() + Send + Sync>>>>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for SignalData<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalData")
            .field("value", &self.value)
            .field("subscribers_count", &self.subscribers.lock().len())
            .finish_non_exhaustive()
    }
}

impl<T: Send + 'static> SignalData<T> {
    #[allow(dead_code)]
    fn new(
        initial: T,
        subscribers: Arc<Mutex<HashMap<SubscriptionId, Arc<dyn Fn() + Send + Sync>>>>,
    ) -> Self {
        Self {
            value: Arc::new(Mutex::new(initial)),
            subscribers,
        }
    }
}

/// Global signal runtime that stores all signal data.
///
/// This is a thread-local singleton that manages the lifecycle of all signals.
/// Signals store only their ID and access data through this runtime.
#[derive(Debug)]
pub struct SignalRuntime {
    /// Map of SignalId -> SignalDataErased
    /// Uses Mutex for thread-safety (signals can be accessed from multiple threads)
    signals: Mutex<HashMap<SignalId, SignalDataErased>>,
}

impl SignalRuntime {
    /// Create a new signal runtime
    fn new() -> Self {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "[SIGNAL_RUNTIME] new() called - creating SignalRuntime from thread {:?}",
            std::thread::current().id()
        );

        Self {
            signals: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new signal with initial value
    ///
    /// Returns the SignalId for the newly created signal.
    pub fn create_signal<T: Clone + Send + 'static>(&self, initial: T) -> SignalId {
        let id = SignalId::new();

        // Create shared subscribers map
        let subscribers = Arc::new(Mutex::new(HashMap::new()));

        // Store the initial value in an Arc<Mutex<T>>
        let value_arc = Arc::new(Mutex::new(initial));

        // Type-erase and store
        let erased = SignalDataErased {
            type_id: TypeId::of::<T>(),
            value: Box::new(value_arc),
            subscribers,
            #[cfg(debug_assertions)]
            type_name: std::any::type_name::<T>(),
        };

        self.signals.lock().insert(id, erased);

        #[cfg(debug_assertions)]
        {
            let signal_count = self.signals.lock().len();
            tracing::trace!(
                "[SIGNAL_RUNTIME] Created signal {:?} with type {} from thread {:?}. Total signals: {}",
                id,
                std::any::type_name::<T>(),
                std::thread::current().id(),
                signal_count
            );
        }

        id
    }

    /// Get the current value of a signal
    pub fn get<T: Clone + Send + 'static>(&self, id: SignalId) -> T {
        let value_arc = {
            let signals = self.signals.lock();

            #[cfg(debug_assertions)]
            {
                tracing::trace!(
                    "[SIGNAL_RUNTIME] get() called for signal {:?} from thread {:?}",
                    id,
                    std::thread::current().id()
                );
                tracing::trace!(
                    "[SIGNAL_RUNTIME] Available signals: {:?}",
                    signals.keys().collect::<Vec<_>>()
                );
            }

            let erased = signals.get(&id).unwrap_or_else(|| {
                panic!(
                    "Signal {:?} not found! This is a framework bug. Available signals: {:?}",
                    id,
                    signals.keys().collect::<Vec<_>>()
                )
            });

            // Type check
            let expected_type_id = TypeId::of::<T>();
            if erased.type_id != expected_type_id {
                #[cfg(debug_assertions)]
                panic!(
                    "Signal type mismatch! Expected {}, got {}",
                    std::any::type_name::<T>(),
                    erased.type_name
                );
                #[cfg(not(debug_assertions))]
                panic!(
                    "Signal type mismatch! Expected {:?}, got {:?}",
                    expected_type_id, erased.type_id
                );
            }

            // Downcast and clone the Arc
            Arc::clone(
                erased
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Type check passed but downcast failed - this is a bug"),
            )
        }; // signals lock dropped here

        let result = value_arc.lock().clone();
        result
    }

    /// Get value with a closure (avoids clone)
    pub fn with<T: Send + 'static, R>(&self, id: SignalId, f: impl FnOnce(&T) -> R) -> R {
        let value_arc = {
            let signals = self.signals.lock();
            let erased = signals
                .get(&id)
                .unwrap_or_else(|| panic!("Signal {:?} not found!", id));

            Arc::clone(
                erased
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Signal type mismatch"),
            )
        };

        let guard = value_arc.lock();
        f(&*guard)
    }

    /// Set the signal to a new value
    pub fn set<T: Send + 'static>(&self, id: SignalId, value: T) {
        let (value_arc, subscribers) = {
            let signals = self.signals.lock();
            let erased = signals
                .get(&id)
                .unwrap_or_else(|| panic!("Signal {:?} not found!", id));

            let value_arc = Arc::clone(
                erased
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Signal type mismatch"),
            );
            let subscribers = Arc::clone(&erased.subscribers);

            (value_arc, subscribers)
        }; // signals lock dropped here

        *value_arc.lock() = value;

        // Notify subscribers
        Self::notify_subscribers_internal(&subscribers);

        #[cfg(debug_assertions)]
        tracing::trace!("[SIGNAL_RUNTIME] Signal {:?} value changed", id);
    }

    /// Update signal with a function
    pub fn update<T: Clone + Send + 'static>(&self, id: SignalId, f: impl FnOnce(T) -> T) {
        let old_value = self.get(id);
        let new_value = f(old_value);
        self.set(id, new_value);
    }

    /// Update signal by mutating in place
    pub fn update_mut<T: Send + 'static>(&self, id: SignalId, f: impl FnOnce(&mut T)) {
        let (value_arc, subscribers) = {
            let signals = self.signals.lock();
            let erased = signals
                .get(&id)
                .unwrap_or_else(|| panic!("Signal {:?} not found!", id));

            let value_arc = Arc::clone(
                erased
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Signal type mismatch"),
            );
            let subscribers = Arc::clone(&erased.subscribers);

            (value_arc, subscribers)
        }; // signals lock dropped here

        f(&mut *value_arc.lock());

        // Notify subscribers
        Self::notify_subscribers_internal(&subscribers);
    }

    /// Subscribe to signal changes
    pub fn subscribe(
        &self,
        id: SignalId,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> SubscriptionId {
        let signals = self.signals.lock();
        let erased = signals.get(&id).expect("Signal not found");

        let sub_id = SubscriptionId::new();
        erased.subscribers.lock().insert(sub_id, Arc::new(callback));

        #[cfg(debug_assertions)]
        tracing::trace!(
            "[SIGNAL_RUNTIME] Subscribed to signal {:?} with id {:?}",
            id,
            sub_id
        );

        sub_id
    }

    /// Unsubscribe from signal changes
    pub fn unsubscribe(&self, id: SignalId, sub_id: SubscriptionId) {
        let signals = self.signals.lock();
        if let Some(erased) = signals.get(&id) {
            erased.subscribers.lock().remove(&sub_id);

            #[cfg(debug_assertions)]
            tracing::trace!(
                "[SIGNAL_RUNTIME] Unsubscribed from signal {:?} with id {:?}",
                id,
                sub_id
            );
        }
    }

    /// Notify all subscribers of a signal
    fn notify_subscribers_internal(
        subscribers: &Arc<Mutex<HashMap<SubscriptionId, Arc<dyn Fn() + Send + Sync>>>>,
    ) {
        // Clone all subscriber Arc's to avoid holding the lock during callbacks
        let callbacks: Vec<_> = subscribers.lock().values().cloned().collect();

        #[cfg(debug_assertions)]
        if !callbacks.is_empty() {
            tracing::trace!("[SIGNAL_RUNTIME] Notifying {} subscribers", callbacks.len());
        }

        // Call each subscriber - safe because we own Arc clones
        for subscriber in callbacks {
            subscriber();
        }
    }

    /// Remove a signal from the runtime (cleanup)
    pub fn remove_signal(&self, id: SignalId) {
        let mut signals = self.signals.lock();
        if signals.remove(&id).is_some() {
            #[cfg(debug_assertions)]
            {
                let signal_count = signals.len();
                tracing::trace!(
                    "[SIGNAL_RUNTIME] Removed signal {:?} from thread {:?}. Remaining signals: {}",
                    id,
                    std::thread::current().id(),
                    signal_count
                );
            }
        }
    }

    /// Get subscriber count for debugging
    #[cfg(debug_assertions)]
    pub fn subscriber_count(&self, id: SignalId) -> usize {
        let signals = self.signals.lock();
        signals
            .get(&id)
            .map(|erased| erased.subscribers.lock().len())
            .unwrap_or(0)
    }
}

// Global signal runtime instance (thread-local for safety)
//
// This thread-local storage provides a separate signal runtime for each thread,
// ensuring thread-safety without requiring explicit synchronization when accessing
// signal data within the same thread.
thread_local! {
    /// Global signal runtime instance (thread-local for safety)
    pub static SIGNAL_RUNTIME: SignalRuntime = SignalRuntime::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get() {
        SIGNAL_RUNTIME.with(|runtime| {
            let id = runtime.create_signal(42i32);
            let value: i32 = runtime.get(id);
            assert_eq!(value, 42);
        });
    }

    #[test]
    fn test_set() {
        SIGNAL_RUNTIME.with(|runtime| {
            let id = runtime.create_signal(0i32);
            runtime.set(id, 100);
            assert_eq!(runtime.get::<i32>(id), 100);
        });
    }

    #[test]
    fn test_update() {
        SIGNAL_RUNTIME.with(|runtime| {
            let id = runtime.create_signal(5i32);
            runtime.update(id, |n| n * 2);
            assert_eq!(runtime.get::<i32>(id), 10);
        });
    }

    #[test]
    fn test_update_mut() {
        SIGNAL_RUNTIME.with(|runtime| {
            let id = runtime.create_signal(vec![1, 2, 3]);
            runtime.update_mut(id, |v: &mut Vec<i32>| v.push(4));
            runtime.with(id, |v: &Vec<i32>| {
                assert_eq!(v.len(), 4);
                assert_eq!(v[3], 4);
            });
        });
    }

    #[test]
    fn test_subscribe() {
        SIGNAL_RUNTIME.with(|runtime| {
            let id = runtime.create_signal(0i32);

            let call_count = Arc::new(Mutex::new(0));
            let call_count_clone = Arc::clone(&call_count);

            let _sub_id = runtime.subscribe(id, move || {
                *call_count_clone.lock() += 1;
            });

            runtime.set(id, 1);
            assert_eq!(*call_count.lock(), 1);

            runtime.set(id, 2);
            assert_eq!(*call_count.lock(), 2);
        });
    }

    #[test]
    fn test_unsubscribe() {
        SIGNAL_RUNTIME.with(|runtime| {
            let id = runtime.create_signal(0i32);

            let call_count = Arc::new(Mutex::new(0));
            let call_count_clone = Arc::clone(&call_count);

            let sub_id = runtime.subscribe(id, move || {
                *call_count_clone.lock() += 1;
            });

            runtime.set(id, 1);
            assert_eq!(*call_count.lock(), 1);

            // Unsubscribe
            runtime.unsubscribe(id, sub_id);

            runtime.set(id, 2);
            // Should not be called
            assert_eq!(*call_count.lock(), 1);
        });
    }

    #[test]
    #[should_panic(expected = "Signal type mismatch")]
    fn test_type_safety() {
        SIGNAL_RUNTIME.with(|runtime| {
            let id = runtime.create_signal(42i32);
            // Try to get as wrong type
            let _: String = runtime.get(id); // Should panic
        });
    }
}
