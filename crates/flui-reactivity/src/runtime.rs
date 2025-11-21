//! Global signal runtime with DashMap for lock-free concurrent access.
//!
//! Provides Copy-based signals with a single global SignalRuntime.
//! This eliminates RuntimeId registry overhead and provides direct access.

use super::signal::{SignalId, SubscriptionId};
use crate::error::SignalError;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// Configuration for SignalRuntime limits and behavior.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Maximum number of signals that can exist in the runtime.
    /// Default: 100,000
    pub max_signals: usize,

    /// Maximum subscribers per signal before rejecting new subscriptions.
    /// Default: 1,000
    pub max_subscribers_per_signal: usize,

    /// Maximum nesting depth for computed signal dependencies.
    /// Default: 100
    pub max_computed_depth: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_signals: 100_000,
            max_subscribers_per_signal: 1000,
            max_computed_depth: 100,
        }
    }
}
/// Type alias for subscriber map to reduce complexity
type SubscriberMap = Arc<Mutex<HashMap<SubscriptionId, Arc<dyn Fn() + Send + Sync>>>>;

/// Type-erased signal data stored in the runtime.
struct SignalDataErased {
    /// Type ID for safe downcasting
    type_id: TypeId,

    /// Type-erased value (Arc<Mutex<T>> wrapped in Box<dyn Any>)
    value: Box<dyn Any + Send + Sync>,

    /// Type-erased subscribers
    /// Maps SubscriptionId -> callback
    subscribers: SubscriberMap,

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
    subscribers: SubscriberMap,
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
    fn new(initial: T, subscribers: SubscriberMap) -> Self {
        Self {
            value: Arc::new(Mutex::new(initial)),
            subscribers,
        }
    }
}

/// Global signal runtime that stores all signal data.
///
/// Uses DashMap for lock-free concurrent reads and writes.
/// Single global instance - all signals share the same namespace.
#[derive(Debug)]
pub struct SignalRuntime {
    /// Map of SignalId -> SignalDataErased
    /// DashMap provides concurrent lock-free access
    signals: DashMap<SignalId, SignalDataErased>,
    /// Runtime configuration (memory limits, etc.)
    config: RuntimeConfig,
}

impl SignalRuntime {
    /// Create a new signal runtime with default configuration
    fn new() -> Self {
        Self::with_config(RuntimeConfig::default())
    }

    /// Create a new signal runtime with custom configuration
    pub fn with_config(config: RuntimeConfig) -> Self {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "[SIGNAL_RUNTIME] Creating SignalRuntime with config: {:?}",
            config
        );

        Self {
            signals: DashMap::new(),
            config,
        }
    }

    /// Get the global signal runtime instance
    pub fn global() -> &'static Self {
        &SIGNAL_RUNTIME
    }

    /// Create a new signal with initial value
    ///
    /// Returns the SignalId for the newly created signal.
    ///
    /// # Panics
    ///
    /// Panics if the number of signals exceeds `RuntimeConfig::max_signals`.
    /// This prevents unbounded memory growth from signal leaks.
    pub fn create_signal<T: Clone + Send + 'static>(&self, initial: T) -> SignalId {
        // Check signal count limit BEFORE creating the signal
        let current_count = self.signals.len();
        if current_count >= self.config.max_signals {
            panic!(
                "Signal count limit exceeded: {} >= {}. This prevents memory exhaustion from signal leaks. \
                 Consider increasing RuntimeConfig::max_signals or fixing signal leaks.",
                current_count,
                self.config.max_signals
            );
        }

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

        self.signals.insert(id, erased);

        #[cfg(debug_assertions)]
        {
            let signal_count = self.signals.len();
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
        #[cfg(debug_assertions)]
        tracing::trace!(
            "[SIGNAL_RUNTIME] get() called for signal {:?} from thread {:?}",
            id,
            std::thread::current().id()
        );

        // Clone Arc while holding DashMap guard, then drop guard before locking
        let value_arc: Arc<Mutex<T>> = {
            let entry = self.signals.get(&id).unwrap_or_else(|| {
                panic!(
                    "Signal {:?} not found! This is a framework bug. Total signals: {}",
                    id,
                    self.signals.len()
                )
            });

            // Type check
            let expected_type_id = TypeId::of::<T>();
            if entry.type_id != expected_type_id {
                #[cfg(debug_assertions)]
                panic!(
                    "Signal type mismatch! Expected {}, got {}",
                    std::any::type_name::<T>(),
                    entry.type_name
                );
                #[cfg(not(debug_assertions))]
                panic!(
                    "Signal type mismatch! Expected {:?}, got {:?}",
                    expected_type_id, entry.type_id
                );
            }

            // Clone Arc before entry guard is dropped
            Arc::clone(
                entry
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Type check passed but downcast failed - this is a bug"),
            )
        }; // entry guard dropped here

        // Now lock and clone value (entry guard already dropped)
        let result = value_arc.lock().clone();
        result
    }

    /// Get value with a closure (avoids clone)
    pub fn with<T: Send + 'static, R>(&self, id: SignalId, f: impl FnOnce(&T) -> R) -> R {
        // Clone Arc while holding DashMap guard
        let value_arc: Arc<Mutex<T>> = {
            let entry = self
                .signals
                .get(&id)
                .unwrap_or_else(|| panic!("Signal {:?} not found!", id));

            Arc::clone(
                entry
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Signal type mismatch"),
            )
        }; // entry guard dropped here

        let guard = value_arc.lock();
        f(&*guard)
    }

    /// Set the signal to a new value
    pub fn set<T: Send + 'static>(&self, id: SignalId, value: T) {
        // Clone Arc and subscribers while holding DashMap guard
        let (value_arc, subscribers): (Arc<Mutex<T>>, SubscriberMap) = {
            let entry = self
                .signals
                .get(&id)
                .unwrap_or_else(|| panic!("Signal {:?} not found!", id));

            let value_arc = Arc::clone(
                entry
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Signal type mismatch"),
            );
            let subscribers = Arc::clone(&entry.subscribers);

            (value_arc, subscribers)
        }; // entry guard dropped here

        // Update value
        *value_arc.lock() = value;

        // Notify subscribers
        Self::notify_subscribers_internal(id, &subscribers);

        #[cfg(debug_assertions)]
        tracing::trace!("[SIGNAL_RUNTIME] Signal {:?} value changed", id);
    }

    /// Update signal with a function (atomic operation)
    pub fn update<T: Clone + Send + 'static>(&self, id: SignalId, f: impl FnOnce(T) -> T) {
        // Clone Arc and subscribers while holding DashMap guard
        let (value_arc, subscribers): (Arc<Mutex<T>>, SubscriberMap) = {
            let entry = self
                .signals
                .get(&id)
                .unwrap_or_else(|| panic!("Signal {:?} not found!", id));

            let value_arc = Arc::clone(
                entry
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Signal type mismatch"),
            );
            let subscribers = Arc::clone(&entry.subscribers);

            (value_arc, subscribers)
        }; // entry guard dropped here

        // Track whether value was actually changed
        let mut value_changed = false;

        // Update value atomically with panic safety
        let update_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut value_guard = value_arc.lock();
            let old_value = value_guard.clone();
            let new_value = f(old_value);
            *value_guard = new_value;
            value_changed = true; // Only set if closure completed successfully
        }));

        // Only notify if update succeeded and value actually changed
        if update_result.is_ok() && value_changed {
            Self::notify_subscribers_internal(id, &subscribers);
        } else if let Err(panic_err) = update_result {
            // Propagate panic without notifying subscribers
            std::panic::resume_unwind(panic_err);
        }
    }

    /// Update signal by mutating in place
    pub fn update_mut<T: Send + 'static>(&self, id: SignalId, f: impl FnOnce(&mut T)) {
        // Clone Arc and subscribers while holding DashMap guard
        let (value_arc, subscribers): (Arc<Mutex<T>>, SubscriberMap) = {
            let entry = self
                .signals
                .get(&id)
                .unwrap_or_else(|| panic!("Signal {:?} not found!", id));

            let value_arc = Arc::clone(
                entry
                    .value
                    .downcast_ref::<Arc<Mutex<T>>>()
                    .expect("Signal type mismatch"),
            );
            let subscribers = Arc::clone(&entry.subscribers);

            (value_arc, subscribers)
        }; // entry guard dropped here

        // Track whether mutation was actually performed
        let mut value_changed = false;

        // Mutate value with panic safety
        let update_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f(&mut *value_arc.lock());
            value_changed = true; // Only set if closure completed successfully
        }));

        // Only notify if update succeeded and value actually changed
        if update_result.is_ok() && value_changed {
            Self::notify_subscribers_internal(id, &subscribers);
        } else if let Err(panic_err) = update_result {
            // Propagate panic without notifying subscribers
            std::panic::resume_unwind(panic_err);
        }
    }

    /// Subscribe to signal changes
    ///
    /// # Errors
    ///
    /// Returns `SignalError::TooManySubscribers` if the signal already has
    /// `RuntimeConfig::max_subscribers_per_signal` subscribers. This prevents memory leaks
    /// from forgotten subscriptions.
    ///
    /// # Panics
    ///
    /// Panics if the signal is not found in the runtime.
    pub fn subscribe(
        &self,
        id: SignalId,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> Result<SubscriptionId, SignalError> {
        let entry = self.signals.get(&id).expect("Signal not found");

        let mut subscribers = entry.subscribers.lock();

        // Check subscriber limit to prevent memory leaks
        let max_subscribers = self.config.max_subscribers_per_signal;
        if subscribers.len() >= max_subscribers {
            tracing::error!(
                "[SIGNAL_RUNTIME] Signal {:?} exceeded max subscribers ({})",
                id,
                max_subscribers
            );
            return Err(SignalError::TooManySubscribers {
                signal_id: id,
                max: max_subscribers,
            });
        }

        let sub_id = SubscriptionId::new();
        subscribers.insert(sub_id, Arc::new(callback));

        #[cfg(debug_assertions)]
        tracing::trace!(
            "[SIGNAL_RUNTIME] Subscribed to signal {:?} with id {:?} (total: {})",
            id,
            sub_id,
            subscribers.len()
        );

        Ok(sub_id)
    }

    /// Unsubscribe from signal changes
    pub fn unsubscribe(&self, id: SignalId, sub_id: SubscriptionId) {
        if let Some(entry) = self.signals.get(&id) {
            entry.subscribers.lock().remove(&sub_id);

            #[cfg(debug_assertions)]
            tracing::trace!(
                "[SIGNAL_RUNTIME] Unsubscribed from signal {:?} with id {:?}",
                id,
                sub_id
            );
        }
    }

    /// Manually trigger notification to all subscribers without changing the value.
    ///
    /// Used internally by Computed signals to propagate dirty flags.
    pub(crate) fn notify_subscribers(&self, id: SignalId) {
        if let Some(entry) = self.signals.get(&id) {
            let subscribers = Arc::clone(&entry.subscribers);
            Self::notify_subscribers_internal(id, &subscribers);
        }
    }

    /// Notify all subscribers of a signal
    fn notify_subscribers_internal(signal_id: SignalId, subscribers: &SubscriberMap) {
        // Clone subscriber map to capture for batch notification
        let subscribers_clone = Arc::clone(subscribers);

        // Queue notification if batching (with deduplication by SignalId)
        crate::batch::queue_notification(signal_id, move || {
            // Clone all subscriber Arc's to avoid holding the lock during callbacks
            let callbacks: Vec<_> = subscribers_clone.lock().values().cloned().collect();

            for callback in callbacks {
                callback();
            }
        });
    }

    /// Legacy notify implementation (for compatibility)
    #[allow(dead_code)]
    fn notify_subscribers_internal_immediate(subscribers: &SubscriberMap) {
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
        if self.signals.remove(&id).is_some() {
            #[cfg(debug_assertions)]
            {
                let signal_count = self.signals.len();
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
        self.signals
            .get(&id)
            .map(|entry| entry.subscribers.lock().len())
            .unwrap_or(0)
    }
}

/// Drop implementation for SignalRuntime
///
/// Ensures proper cleanup by clearing all subscriptions before dropping signals.
/// This prevents potential memory leaks from subscription callbacks holding references.
impl Drop for SignalRuntime {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "[SIGNAL_RUNTIME] Dropping SignalRuntime with {} signals",
            self.signals.len()
        );

        // Clear all subscriptions before dropping signals
        // This ensures subscription callbacks don't hold dangling references
        for entry in self.signals.iter() {
            entry.value().subscribers.lock().clear();
        }

        // Now clear all signals
        self.signals.clear();

        #[cfg(debug_assertions)]
        tracing::debug!("[SIGNAL_RUNTIME] SignalRuntime dropped successfully");
    }
}

/// Global singleton SignalRuntime
///
/// All signals are stored in this single global instance.
/// DashMap provides lock-free concurrent access for maximum performance.
static SIGNAL_RUNTIME: Lazy<SignalRuntime> = Lazy::new(|| {
    tracing::debug!("Initializing global SignalRuntime");
    SignalRuntime::new()
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get() {
        let runtime = SignalRuntime::global();
        let id = runtime.create_signal(42i32);
        let value: i32 = runtime.get(id);
        assert_eq!(value, 42);
    }

    #[test]
    fn test_set() {
        let runtime = SignalRuntime::global();
        let id = runtime.create_signal(0i32);
        runtime.set(id, 100);
        assert_eq!(runtime.get::<i32>(id), 100);
    }

    #[test]
    fn test_update() {
        let runtime = SignalRuntime::global();
        let id = runtime.create_signal(5i32);
        runtime.update(id, |n: i32| n * 2);
        assert_eq!(runtime.get::<i32>(id), 10);
    }

    #[test]
    fn test_update_mut() {
        let runtime = SignalRuntime::global();
        let id = runtime.create_signal(vec![1, 2, 3]);
        runtime.update_mut(id, |v: &mut Vec<i32>| v.push(4));
        runtime.with(id, |v: &Vec<i32>| {
            assert_eq!(v.len(), 4);
            assert_eq!(v[3], 4);
        });
    }

    #[test]
    fn test_subscribe() {
        let runtime = SignalRuntime::global();
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
    }

    #[test]
    fn test_unsubscribe() {
        let runtime = SignalRuntime::global();
        let id = runtime.create_signal(0i32);

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let sub_id = runtime
            .subscribe(id, move || {
                *call_count_clone.lock() += 1;
            })
            .expect("Failed to subscribe");

        runtime.set(id, 1);
        assert_eq!(*call_count.lock(), 1);

        // Unsubscribe
        runtime.unsubscribe(id, sub_id);

        runtime.set(id, 2);
        // Should not be called
        assert_eq!(*call_count.lock(), 1);
    }

    #[test]
    #[should_panic(expected = "Signal type mismatch")]
    fn test_type_safety() {
        let runtime = SignalRuntime::global();
        let id = runtime.create_signal(42i32);
        // Try to get as wrong type
        let _: String = runtime.get(id); // Should panic
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let runtime = SignalRuntime::global();
        let id = runtime.create_signal(0i32);

        // Spawn multiple threads that read/write the same signal
        let handles: Vec<_> = (0..10)
            .map(|i| {
                thread::spawn(move || {
                    for _ in 0..100 {
                        runtime.update(id, |n: i32| n + 1);
                    }
                    i
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have incremented 10 * 100 = 1000 times
        assert_eq!(runtime.get::<i32>(id), 1000);
    }
}
