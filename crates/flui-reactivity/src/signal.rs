//! Signal hook implementation for reactive state.
//!
//! Provides `use_signal` hook that creates reactive state similar to React's useState.
//! When a signal changes, all components that depend on it are automatically re-rendered.
//!
//! # New in 0.7.0: Copy-Based Signals
//!
//! `Signal<T>` is now Copy! This eliminates the need for explicit `.clone()` calls.
//! Signals are just lightweight IDs that reference data in a thread-local runtime.

use super::context::HookContext;
use super::traits::{DependencyId, Hook};
use std::marker::PhantomData;

/// Unique identifier for a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(u64);

/// Unique identifier for a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    /// Create a new subscription ID.
    ///
    /// # Panics
    ///
    /// Panics if u64::MAX subscriptions have been created (practically impossible).
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);

        // Use fetch_update to check overflow BEFORE increment
        let id = COUNTER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current >= u64::MAX - 1 {
                    None // Causes fetch_update to fail
                } else {
                    Some(current + 1)
                }
            })
            .expect("SubscriptionId counter overflow! Cannot create more subscriptions.");

        Self(id)
    }
}

impl Default for SubscriptionId {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalId {
    /// Create a new signal ID.
    ///
    /// # Panics
    ///
    /// Panics if u64::MAX signals have been created (practically impossible).
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);

        // Use fetch_update to check overflow BEFORE increment
        let id = COUNTER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current >= u64::MAX - 1 {
                    None
                } else {
                    Some(current + 1)
                }
            })
            .expect("SignalId counter overflow! Cannot create more signals.");

        Self(id)
    }
}

impl Default for SignalId {
    fn default() -> Self {
        Self::new()
    }
}

// SignalInner removed - signal data now stored in SignalRuntime

/// RAII guard for a signal subscription.
///
/// Automatically unsubscribes when dropped.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(0);
/// let subscription = count.subscribe_scoped(|| {
///     println!("Count changed!");
/// });
/// // Subscription automatically cleaned up when it goes out of scope
/// ```
pub struct Subscription<T> {
    signal: Signal<T>,
    id: SubscriptionId,
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        self.signal.unsubscribe(self.id);
    }
}

impl<T> std::fmt::Debug for Subscription<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("signal_id", &self.signal.id())
            .field("subscription_id", &self.id)
            .finish()
    }
}

/// A reactive signal that can be read and updated.
///
/// # New in 0.7.0: Copy-Based Signals
///
/// `Signal<T>` is now **Copy**! This means:
/// - No need to `.clone()` before moving into closures
/// - Copying is implicit and free (just an 8-byte ID)
/// - All copies refer to the same underlying value
///
/// # Migration
///
/// Old code with `.clone()` still works but is unnecessary:
///
/// ```rust,ignore
/// // Old (still works, but .clone() is unnecessary)
/// let count = use_signal(ctx, 0);
/// let count_clone = count.clone();  // ← No-op with Copy
///
/// // New (idiomatic)
/// let count = use_signal(ctx, 0);
/// // Just use count directly - Copy happens implicitly!
/// Button::new("Click").on_tap(move || count.update(|n| n + 1))
/// ```
///
/// # Notification Guarantees
///
/// **All mutation methods guarantee subscriber notification:**
/// - [`set()`](Signal::set) - Always notifies subscribers
/// - [`update()`](Signal::update) - Always notifies subscribers
/// - [`update_mut()`](Signal::update_mut) - Always notifies subscribers
///
/// **Notification is synchronous and happens before the method returns.**
/// This ensures subscribers see a consistent state.
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(ctx, 0);
///
/// // No clone needed! Signal is Copy
/// Button::new("Increment")
///     .on_tap(move || count.update(|n| n + 1))
/// ```
///
/// # Thread Safety
///
/// Signal is **thread-safe** (implements `Send` and `Sync`). It's designed for
/// multi-threaded UI applications where updates can happen on different threads.
#[derive(Debug, Clone, Copy)]
pub struct Signal<T> {
    id: SignalId,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> Signal<T> {
    /// Create a new signal with an initial value (standalone usage).
    ///
    /// For use without a BuildContext. Creates a signal in the global runtime.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let count = Signal::new(0);
    /// println!("Count: {}", count.get());
    /// count.set(42);
    /// ```
    pub fn new(initial: T) -> Self
    where
        T: Clone + Send + 'static,
    {
        let runtime = super::SignalRuntime::global();
        let id = runtime.create_signal(initial);
        Self {
            id,
            _phantom: PhantomData,
        }
    }

    /// Create a signal from an existing SignalId (internal use only)
    fn from_id(id: SignalId) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }

    /// Create a signal from an existing SignalId
    ///
    /// **Note:** This is intended for testing only. In production code, use `use_signal()`.
    #[doc(hidden)]
    pub fn new_from_id(id: SignalId) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }

    /// Get the global runtime
    fn runtime(&self) -> &'static super::SignalRuntime {
        super::SignalRuntime::global()
    }

    /// Get the current value of the signal (standalone usage).
    ///
    /// For use without a HookContext. Simply returns the current value.
    /// Automatically tracks the signal access for dependency tracking in Computed signals.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let count = Signal::new(0);
    /// let value = count.get();
    /// println!("Value: {}", value);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The signal has been removed from the runtime (indicates a framework bug)
    /// - There's a type mismatch between the signal's actual type and T (indicates a framework bug)
    ///
    /// These panics should never happen in safe code and indicate serious framework bugs.
    pub fn get(&self) -> T
    where
        T: Clone + Send + 'static,
    {
        // Track for computed dependencies
        crate::computed::track_signal_access(self.id);
        self.runtime().get(self.id)
    }

    /// Get the current value and track as a dependency (for FLUI integration).
    ///
    /// This tracks the signal as a dependency in the given context,
    /// enabling automatic rebuilds when the signal changes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let value = count.get_tracked(ctx);
    /// ```
    pub fn get_tracked(&self, ctx: &mut HookContext) -> T
    where
        T: Clone + Send + 'static,
    {
        // Track dependency
        ctx.track_dependency(DependencyId::new(self.id.0));
        self.runtime().get(self.id)
    }

    /// Get a reference to the current value without cloning.
    ///
    /// This tracks the signal as a dependency in the given context.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// count.with(ctx, |n| println!("Count: {}", n));
    /// ```
    pub fn with<R>(&self, ctx: &mut HookContext, f: impl FnOnce(&T) -> R) -> R
    where
        T: Send + 'static,
    {
        // Track dependency
        ctx.track_dependency(DependencyId::new(self.id.0));
        self.runtime().with(self.id, f)
    }

    /// Set the signal to a new value.
    ///
    /// This will trigger re-renders of dependent components and notify all subscribers.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// count.set(42);
    /// ```
    pub fn set(&self, value: T)
    where
        T: Send + 'static,
    {
        self.runtime().set(self.id, value);
    }

    /// Update the signal using a function.
    ///
    /// This is useful for updates that depend on the current value.
    /// All subscribers are notified after the update.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// count.update(|n| n + 1);
    /// ```
    pub fn update(&self, f: impl FnOnce(T) -> T)
    where
        T: Clone + Send + 'static,
    {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "[SIGNAL] update() called for signal {:?} from thread {:?}",
            self.id,
            std::thread::current().id()
        );

        self.runtime().update(self.id, f);
    }

    /// Update the signal by mutating it in place.
    ///
    /// All subscribers are notified after the update.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// count.update_mut(|n| *n += 1);
    /// ```
    pub fn update_mut(&self, f: impl FnOnce(&mut T))
    where
        T: Send + 'static,
    {
        self.runtime().update_mut(self.id, f);
    }

    /// Get the signal ID.
    pub fn id(&self) -> SignalId {
        self.id
    }

    /// Subscribe to changes with a callback.
    ///
    /// Returns a subscription ID that **must be manually unsubscribed** using
    /// [`unsubscribe()`](Signal::unsubscribe).
    ///
    /// # ⚠️ Memory Leak Warning
    ///
    /// **Forgetting to call `unsubscribe()` causes memory leaks!**
    ///
    /// ```rust,ignore
    /// // ❌ MEMORY LEAK: Never unsubscribed!
    /// signal.subscribe(|| println!("Changed"));
    ///
    /// // ✅ CORRECT: Manual cleanup
    /// let id = signal.subscribe(|| println!("Changed"));
    /// // ... later ...
    /// signal.unsubscribe(id);
    ///
    /// // ✅ BETTER: Use subscribe_scoped() for automatic cleanup
    /// let _subscription = signal.subscribe_scoped(|| println!("Changed"));
    /// // Automatically unsubscribes when _subscription drops
    /// ```
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let id = signal.subscribe(|| {
    ///     println!("Signal changed!");
    /// });
    /// // Later...
    /// signal.unsubscribe(id);
    /// ```
    #[must_use = "Subscription ID must be stored and unsubscribed, or use subscribe_scoped() instead"]
    pub fn subscribe<F>(&self, callback: F) -> Result<SubscriptionId, crate::error::SignalError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.runtime().subscribe(self.id, callback)
    }

    /// Subscribe to changes with automatic cleanup.
    ///
    /// Returns a `Subscription` guard that automatically unsubscribes when dropped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// {
    ///     let _subscription = signal.subscribe_scoped(|| {
    ///         println!("Signal changed!");
    ///     });
    ///     signal.set(42); // Callback is called
    /// } // Subscription dropped, callback unsubscribed
    /// signal.set(43); // Callback is NOT called
    /// ```
    pub fn subscribe_scoped<F>(
        self,
        callback: F,
    ) -> Result<Subscription<T>, crate::error::SignalError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = self.subscribe(callback)?;
        Ok(Subscription { signal: self, id })
    }

    /// Unsubscribe from changes using a subscription ID.
    pub fn unsubscribe(&self, id: SubscriptionId) {
        self.runtime().unsubscribe(self.id, id);
    }

    /// Manually trigger notification to all subscribers without changing the value.
    ///
    /// This is used internally by Computed signals to propagate dirty flags.
    pub(crate) fn notify_subscribers(&self) {
        self.runtime().notify_subscribers(self.id);
    }
}

// Clone is automatically derived from Copy
// Note: Copy types automatically implement Clone with memcpy semantics

/// Hook state for SignalHook.
#[derive(Debug, Clone)]
pub struct SignalState<T> {
    id: SignalId,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> Drop for SignalState<T> {
    fn drop(&mut self) {
        // NOTE: With the global registry approach, signals are NOT removed here.
        // Signals live as long as the SignalRuntime (PipelineOwner) they belong to.
        // This allows Signal handles to remain valid even after component rebuilds.
        //
        // Cleanup happens automatically when SignalRuntime is dropped (when PipelineOwner drops).

        #[cfg(debug_assertions)]
        {
            tracing::trace!(
                "[SIGNAL_STATE] Dropping signal state for {:?} (signal data persists in runtime)",
                self.id
            );

            // Expensive backtrace - only enabled via env var for deep debugging
            if std::env::var("FLUI_DEBUG_SIGNAL_DROP").is_ok() {
                tracing::trace!(
                    "[SIGNAL_STATE] Backtrace:\n{:?}",
                    std::backtrace::Backtrace::force_capture()
                );
            }
        }
    }
}

/// Signal hook implementation.
///
/// This hook creates a reactive signal that can be read and updated.
#[derive(Debug)]
pub struct SignalHook<T>(PhantomData<T>);

impl<T: Clone + Send + 'static> Hook for SignalHook<T> {
    type State = SignalState<T>;
    type Input = T;
    type Output = Signal<T>;

    fn create(input: Self::Input) -> Self::State {
        // Get global runtime and create signal
        let runtime = super::SignalRuntime::global();
        let id = runtime.create_signal(input);

        SignalState {
            id,
            _phantom: PhantomData,
        }
    }

    fn update(state: &mut Self::State, _input: Self::Input) -> Self::Output {
        // Return signal from ID
        Signal::from_id(state.id)
    }
}

// Note: use_signal() has been removed in favor of Signal::new() for standalone usage.
// FLUI framework integration will provide hooks separately in flui-core.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ComponentId, HookContext};
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn test_signal_get_set() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);
        assert_eq!(signal.get_tracked(&mut ctx), 0);

        signal.set(42);
        assert_eq!(signal.get_tracked(&mut ctx), 42);
    }

    #[test]
    fn test_signal_update() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);
        signal.update(|n| n + 1);
        assert_eq!(signal.get_tracked(&mut ctx), 1);

        signal.update(|n| n * 2);
        assert_eq!(signal.get_tracked(&mut ctx), 2);
    }

    #[test]
    fn test_signal_clone() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal1 = ctx.use_hook::<SignalHook<i32>>(0);
        let signal2 = signal1; // Signal is Copy

        signal1.set(42);
        assert_eq!(signal2.get_tracked(&mut ctx), 42);
    }

    // =========================================================================
    // Subscriber Notification Tests
    // =========================================================================

    #[test]
    fn test_subscribe_notifies_on_set() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let _id = signal.subscribe(move || {
            *call_count_clone.lock() += 1;
        });

        assert_eq!(*call_count.lock(), 0);

        signal.set(1);
        assert_eq!(*call_count.lock(), 1);

        signal.set(2);
        assert_eq!(*call_count.lock(), 2);
    }

    #[test]
    fn test_subscribe_notifies_on_update() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let _id = signal.subscribe(move || {
            *call_count_clone.lock() += 1;
        });

        signal.update(|n| n + 1);
        assert_eq!(*call_count.lock(), 1);

        signal.update(|n| n * 2);
        assert_eq!(*call_count.lock(), 2);
    }

    #[test]
    fn test_subscribe_notifies_on_update_mut() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let _id = signal.subscribe(move || {
            *call_count_clone.lock() += 1;
        });

        signal.update_mut(|n| *n += 1);
        assert_eq!(*call_count.lock(), 1);

        signal.update_mut(|n| *n *= 2);
        assert_eq!(*call_count.lock(), 2);
    }

    #[test]
    fn test_multiple_subscribers() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count1 = Arc::new(Mutex::new(0));
        let call_count2 = Arc::new(Mutex::new(0));
        let call_count3 = Arc::new(Mutex::new(0));

        let cc1 = Arc::clone(&call_count1);
        let cc2 = Arc::clone(&call_count2);
        let cc3 = Arc::clone(&call_count3);

        let _id1 = signal.subscribe(move || {
            *cc1.lock() += 1;
        });
        let _id2 = signal.subscribe(move || {
            *cc2.lock() += 1;
        });
        let _id3 = signal.subscribe(move || {
            *cc3.lock() += 1;
        });

        signal.set(42);

        assert_eq!(*call_count1.lock(), 1);
        assert_eq!(*call_count2.lock(), 1);
        assert_eq!(*call_count3.lock(), 1);
    }

    #[test]
    fn test_unsubscribe() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let id = signal
            .subscribe(move || {
                *call_count_clone.lock() += 1;
            })
            .expect("Failed to subscribe");

        signal.set(1);
        assert_eq!(*call_count.lock(), 1);

        // Unsubscribe
        signal.unsubscribe(id);

        // Should not be called anymore
        signal.set(2);
        assert_eq!(*call_count.lock(), 1);
    }

    #[test]
    fn test_subscribe_scoped_auto_unsubscribe() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = Arc::clone(&call_count);

        {
            let _subscription = signal.subscribe_scoped(move || {
                *call_count_clone.lock() += 1;
            });

            signal.set(1);
            assert_eq!(*call_count.lock(), 1);
        } // Subscription dropped here

        // Should not be called anymore
        signal.set(2);
        assert_eq!(*call_count.lock(), 1);
    }

    #[test]
    fn test_subscriber_can_read_signal_value() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let last_value = Arc::new(Mutex::new(0));
        let last_value_clone = Arc::clone(&last_value);
        let signal_clone = signal; // Signal is Copy

        let _id = signal.subscribe(move || {
            // Use get() in subscribers (standalone API)
            *last_value_clone.lock() = signal_clone.get();
        });

        signal.set(42);
        assert_eq!(*last_value.lock(), 42);

        signal.set(100);
        assert_eq!(*last_value.lock(), 100);
    }

    #[test]
    fn test_subscriber_notification_order() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let log = Arc::new(Mutex::new(Vec::new()));

        let log1 = Arc::clone(&log);
        let _id1 = signal.subscribe(move || {
            log1.lock().push(1);
        });

        let log2 = Arc::clone(&log);
        let _id2 = signal.subscribe(move || {
            log2.lock().push(2);
        });

        let log3 = Arc::clone(&log);
        let _id3 = signal.subscribe(move || {
            log3.lock().push(3);
        });

        signal.set(42);

        // All subscribers should be called
        assert_eq!(log.lock().len(), 3);
        assert!(log.lock().contains(&1));
        assert!(log.lock().contains(&2));
        assert!(log.lock().contains(&3));
    }
}
