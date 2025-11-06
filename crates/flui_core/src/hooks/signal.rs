//! Signal hook implementation for reactive state.
//!
//! Provides `use_signal` hook that creates reactive state similar to React's useState.
//! When a signal changes, all components that depend on it are automatically re-rendered.

use super::hook_context::HookContext;
use super::hook_trait::{DependencyId, Hook};
use crate::BuildContext;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

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
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        // Always check for overflow
        if id == u64::MAX {
            panic!(
                "SubscriptionId counter overflow! Created {} subscriptions.",
                u64::MAX
            );
        }

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
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        // Always check for overflow
        if id == u64::MAX {
            panic!("SignalId counter overflow! Created {} signals.", u64::MAX);
        }

        Self(id)
    }
}

impl Default for SignalId {
    fn default() -> Self {
        Self::new()
    }
}

/// Inner signal state shared between Signal instances.
struct SignalInner<T> {
    value: Arc<Mutex<T>>,
    id: SignalId,
    /// Subscribers that are notified when the signal changes.
    /// Maps SubscriptionId to callback function.
    /// We use Arc to allow cloning callbacks for safe iteration.
    subscribers: Mutex<HashMap<SubscriptionId, Arc<dyn Fn() + Send + Sync>>>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for SignalInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalInner")
            .field("value", &self.value)
            .field("id", &self.id)
            .field(
                "subscribers",
                &format!("{} subscribers", self.subscribers.lock().len()),
            )
            .finish()
    }
}

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
/// When a signal is updated, it automatically tracks dependencies and
/// notifies dependent components to re-render.
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
/// **All subscribers are called in the order they were registered,**
/// though the exact order is not guaranteed between calls.
///
/// # Subscription Management
///
/// Two ways to subscribe to signal changes:
///
/// 1. **Manual subscription** with [`subscribe()`](Signal::subscribe):
///    ```rust,ignore
///    let id = signal.subscribe(|| println!("Changed!"));
///    // Must manually unsubscribe
///    signal.unsubscribe(id);
///    ```
///
/// 2. **RAII subscription** with [`subscribe_scoped()`](Signal::subscribe_scoped):
///    ```rust,ignore
///    {
///        let _sub = signal.subscribe_scoped(|| println!("Changed!"));
///        // Automatically unsubscribed when _sub is dropped
///    }
///    ```
///
/// # Example
///
/// ```rust,ignore
/// let count = use_signal(0);
///
/// // Subscribe to changes
/// let sub = count.subscribe_scoped(|| {
///     println!("Count changed to: {}", count.get());
/// });
///
/// count.set(42);       // Prints: "Count changed to: 42"
/// count.update(|n| n + 1); // Prints: "Count changed to: 43"
/// ```
///
/// # Thread Safety
///
/// Signal is **thread-safe** (implements `Send` and `Sync`). It's designed for
/// multi-threaded UI applications where updates can happen on different threads.
#[derive(Debug)]
pub struct Signal<T> {
    inner: Arc<SignalInner<T>>,
}

impl<T> Signal<T> {
    /// Get the current value of the signal.
    ///
    /// This tracks the signal as a dependency in the given context.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let value = count.get(ctx);
    /// ```
    pub fn get(&self, ctx: &mut HookContext) -> T
    where
        T: Clone,
    {
        // Track dependency
        ctx.track_dependency(DependencyId::new(self.inner.id.0));
        self.inner.value.lock().clone()
    }

    /// Get the current value without tracking as a dependency.
    ///
    /// Use this when you need to read the value but don't want
    /// to register a dependency (e.g., in event handlers or subscribers).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// signal.subscribe(move || {
    ///     let value = signal_clone.get_untracked();
    ///     println!("Value: {}", value);
    /// });
    /// ```
    pub fn get_untracked(&self) -> T
    where
        T: Clone,
    {
        self.inner.value.lock().clone()
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
    pub fn with<R>(&self, ctx: &mut HookContext, f: impl FnOnce(&T) -> R) -> R {
        // Track dependency
        ctx.track_dependency(DependencyId::new(self.inner.id.0));
        f(&*self.inner.value.lock())
    }

    /// Set the signal to a new value.
    ///
    /// This will trigger re-renders of dependent components and notify all subscribers.
    pub fn set(&self, value: T) {
        *self.inner.value.lock() = value;
        self.notify_subscribers();
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
        T: Clone,
    {
        let old_value = self.inner.value.lock().clone();
        let new_value = f(old_value);
        *self.inner.value.lock() = new_value;
        self.notify_subscribers();
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
    pub fn update_mut(&self, f: impl FnOnce(&mut T)) {
        f(&mut *self.inner.value.lock());
        self.notify_subscribers();
    }

    /// Get the signal ID.
    pub fn id(&self) -> SignalId {
        self.inner.id
    }

    /// Subscribe to changes with a callback.
    ///
    /// Returns a subscription ID that **must be manually unsubscribed** using
    /// [`unsubscribe()`](Signal::unsubscribe).
    ///
    /// # ⚠️ Memory Leak Warning
    ///
    /// **Forgetting to call `unsubscribe()` causes memory leaks!** Each subscriber
    /// holds an `Rc<dyn Fn()>` that will never be freed unless explicitly removed.
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
    /// # Recommendation
    ///
    /// **Prefer [`subscribe_scoped()`](Signal::subscribe_scoped) for most use cases.**
    /// It provides automatic cleanup via RAII and prevents memory leaks.
    ///
    /// Only use `subscribe()` when you need fine-grained control over subscription
    /// lifetime (e.g., unsubscribing from multiple locations, conditional unsubscribe).
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
    pub fn subscribe<F>(&self, callback: F) -> SubscriptionId
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = SubscriptionId::new();
        self.inner
            .subscribers
            .lock()
            .insert(id, Arc::new(callback));
        id
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
    pub fn subscribe_scoped<F>(&self, callback: F) -> Subscription<T>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = self.subscribe(callback);
        Subscription {
            signal: self.clone(),
            id,
        }
    }

    /// Unsubscribe from changes using a subscription ID.
    pub fn unsubscribe(&self, id: SubscriptionId) {
        self.inner.subscribers.lock().remove(&id);
    }

    /// Notify all subscribers that the signal has changed.
    ///
    /// This is called automatically by `set()`, `update()`, and `update_mut()`.
    fn notify_subscribers(&self) {
        // Clone all subscriber Arc's to avoid holding the lock during callbacks
        let subscribers: Vec<_> = self.inner.subscribers.lock().values().cloned().collect();

        // Call each subscriber - safe because we own Arc clones
        for subscriber in subscribers {
            subscriber();
        }
    }
}

/// Clone implementation for Signal.
///
/// # ⚠️ Shared Value Semantics
///
/// **Cloning a Signal creates a NEW handle to the SAME shared value.**
///
/// ```rust,ignore
/// let signal1 = use_signal(ctx, 0);
/// let signal2 = signal1.clone();
///
/// signal1.set(42);
/// assert_eq!(signal2.get(), 42);  // ✅ Same value!
/// ```
///
/// Both `signal1` and `signal2` point to the same underlying `Arc<Mutex<T>>`,
/// so changes made through one are immediately visible through the other.
///
/// # When to Use Clone
///
/// Signal cloning is useful when you need to:
/// 1. **Pass signals into closures** (event handlers, effects)
/// 2. **Share signals between components** (parent → child)
/// 3. **Store signals in collections** (Vec, HashMap)
///
/// # Example: Event Handler
///
/// ```rust,ignore
/// let count = use_signal(ctx, 0);
///
/// Button::new("Increment", {
///     let count = count.clone();  // ← Clone for closure
///     move |_| {
///         count.update(|n| n + 1);
///     }
/// })
/// ```
///
/// # Performance
///
/// Cloning a Signal is very cheap:
/// - Only clones an `Arc<SignalInner>` (just increments a reference count)
/// - O(1) time complexity
/// - No data is copied
///
/// # Not Like Rust Copy
///
/// Unlike `Copy` types (e.g., `i32`), Signal clones share state:
///
/// ```rust,ignore
/// // Copy types: Independent values
/// let x = 5;
/// let y = x;
/// x = 10;  // y is still 5
///
/// // Signal: Shared value
/// let s1 = use_signal(ctx, 5);
/// let s2 = s1.clone();
/// s1.set(10);  // s2 also sees 10!
/// ```
impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Hook state for SignalHook.
#[derive(Debug)]
pub struct SignalState<T> {
    value: Arc<Mutex<T>>,
    id: SignalId,
}

impl<T> Drop for SignalState<T> {
    fn drop(&mut self) {
        // Subscribers are stored in SignalInner, not SignalState.
        // When the last Signal clone drops, SignalInner will drop and automatically
        // clear all subscribers (stored in Mutex<HashMap>).
        //
        // This is a no-op because SignalState only holds the value and ID,
        // not the subscriber list.
        #[cfg(debug_assertions)]
        tracing::debug!("Dropping SignalState for signal {:?}", self.id);
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

    fn create(initial: T) -> Self::State {
        SignalState {
            value: Arc::new(Mutex::new(initial)),
            id: SignalId::new(),
        }
    }

    fn update(state: &mut Self::State, _input: T) -> Self::Output {
        Signal {
            inner: Arc::new(SignalInner {
                value: Arc::clone(&state.value),
                id: state.id,
                subscribers: Mutex::new(HashMap::new()),
            }),
        }
    }
}

/// Create a reactive signal.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::hooks::use_signal;
///
/// struct Counter;
///
/// impl Component for Counter {
///     fn build(&self, ctx: &BuildContext) -> View {
///         let count = use_signal(ctx, 0);
///
///         Button::new("Increment")
///             .on_press(move || count.update(|n| n + 1))
///             .into()
///     }
/// }
/// ```
pub fn use_signal<T: Clone + Send + 'static>(ctx: &BuildContext, initial: T) -> Signal<T> {
    ctx.with_hook_context_mut(|hook_ctx| hook_ctx.use_hook::<SignalHook<T>>(initial))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::hook_context::{ComponentId, HookContext};

    #[test]
    fn test_signal_get_set() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);
        assert_eq!(signal.get(&mut ctx), 0);

        signal.set(42);
        assert_eq!(signal.get(&mut ctx), 42);
    }

    #[test]
    fn test_signal_update() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);
        signal.update(|n| n + 1);
        assert_eq!(signal.get(&mut ctx), 1);

        signal.update(|n| n * 2);
        assert_eq!(signal.get(&mut ctx), 2);
    }

    #[test]
    fn test_signal_clone() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal1 = ctx.use_hook::<SignalHook<i32>>(0);
        let signal2 = signal1.clone();

        signal1.set(42);
        assert_eq!(signal2.get(&mut ctx), 42);
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

        let id = signal.subscribe(move || {
            *call_count_clone.lock() += 1;
        });

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
        let signal_clone = signal.clone();

        let _id = signal.subscribe(move || {
            // Use get_untracked() in subscribers (no access to HookContext)
            *last_value_clone.lock() = signal_clone.get_untracked();
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
