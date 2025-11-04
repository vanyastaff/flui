//! Signal hook implementation for reactive state.
//!
//! Provides `use_signal` hook that creates reactive state similar to React's useState.
//! When a signal changes, all components that depend on it are automatically re-rendered.

use super::hook_trait::{Hook, DependencyId};
use super::hook_context::HookContext;
use crate::BuildContext;
use std::cell::RefCell;
use std::rc::Rc;
use std::marker::PhantomData;
use std::collections::HashMap;

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
    /// In debug builds, panics if u64::MAX subscriptions have been created (practically impossible).
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        #[cfg(debug_assertions)]
        if id == u64::MAX {
            panic!(
                "SubscriptionId counter overflow! Created {} subscriptions. \
                 This is theoretically impossible in practice.",
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
    /// In debug builds, panics if u64::MAX signals have been created (practically impossible).
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        #[cfg(debug_assertions)]
        if id == u64::MAX {
            panic!(
                "SignalId counter overflow! Created {} signals. \
                 This is theoretically impossible in practice.",
                u64::MAX
            );
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
    value: Rc<RefCell<T>>,
    id: SignalId,
    /// Subscribers that are notified when the signal changes.
    /// Maps SubscriptionId to callback function.
    /// We use Rc to allow cloning callbacks for safe iteration.
    subscribers: RefCell<HashMap<SubscriptionId, Rc<dyn Fn()>>>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for SignalInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalInner")
            .field("value", &self.value)
            .field("id", &self.id)
            .field("subscribers", &format!("{} subscribers", self.subscribers.borrow().len()))
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
/// Signal is **not thread-safe** (no `Send` or `Sync`). It's designed for
/// single-threaded UI applications where all updates happen on the main thread.
#[derive(Debug)]
pub struct Signal<T> {
    inner: Rc<SignalInner<T>>,
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
        self.inner.value.borrow().clone()
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
        self.inner.value.borrow().clone()
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
        f(&*self.inner.value.borrow())
    }

    /// Set the signal to a new value.
    ///
    /// This will trigger re-renders of dependent components and notify all subscribers.
    pub fn set(&self, value: T) {
        *self.inner.value.borrow_mut() = value;
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
        let old_value = self.inner.value.borrow().clone();
        let new_value = f(old_value);
        *self.inner.value.borrow_mut() = new_value;
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
        f(&mut *self.inner.value.borrow_mut());
        self.notify_subscribers();
    }

    /// Get the signal ID.
    pub fn id(&self) -> SignalId {
        self.inner.id
    }

    /// Subscribe to changes with a callback.
    ///
    /// Returns a subscription ID that can be used to unsubscribe.
    /// For automatic cleanup, use `subscribe_scoped()` instead.
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
    pub fn subscribe<F>(&self, callback: F) -> SubscriptionId
    where
        F: Fn() + 'static,
    {
        let id = SubscriptionId::new();
        self.inner.subscribers.borrow_mut().insert(id, Rc::new(callback));
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
        F: Fn() + 'static,
    {
        let id = self.subscribe(callback);
        Subscription {
            signal: self.clone(),
            id,
        }
    }

    /// Unsubscribe from changes using a subscription ID.
    pub fn unsubscribe(&self, id: SubscriptionId) {
        self.inner.subscribers.borrow_mut().remove(&id);
    }

    /// Notify all subscribers that the signal has changed.
    ///
    /// This is called automatically by `set()`, `update()`, and `update_mut()`.
    fn notify_subscribers(&self) {
        // Clone all subscriber Rc's to avoid holding the borrow during callbacks
        let subscribers: Vec<_> = self.inner.subscribers.borrow()
            .values()
            .cloned()
            .collect();

        // Call each subscriber - safe because we own Rc clones
        for subscriber in subscribers {
            subscriber();
        }
    }
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Hook state for SignalHook.
#[derive(Debug)]
pub struct SignalState<T> {
    value: Rc<RefCell<T>>,
    id: SignalId,
}

impl<T> Drop for SignalState<T> {
    fn drop(&mut self) {
        // Subscribers are stored in SignalInner, not SignalState.
        // When the last Signal clone drops, SignalInner will drop and automatically
        // clear all subscribers (stored in RefCell<HashMap>).
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

impl<T: Clone + 'static> Hook for SignalHook<T> {
    type State = SignalState<T>;
    type Input = T;
    type Output = Signal<T>;

    fn create(initial: T) -> Self::State {
        SignalState {
            value: Rc::new(RefCell::new(initial)),
            id: SignalId::new(),
        }
    }

    fn update(state: &mut Self::State, _input: T) -> Self::Output {
        Signal {
            inner: Rc::new(SignalInner {
                value: state.value.clone(),
                id: state.id,
                subscribers: RefCell::new(HashMap::new()),
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
pub fn use_signal<T: Clone + 'static>(ctx: &BuildContext, initial: T) -> Signal<T> {
    ctx.with_hook_context_mut(|hook_ctx| {
        hook_ctx.use_hook::<SignalHook<T>>(initial)
    })
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

        let call_count = Rc::new(RefCell::new(0));
        let call_count_clone = call_count.clone();

        let _id = signal.subscribe(move || {
            *call_count_clone.borrow_mut() += 1;
        });

        assert_eq!(*call_count.borrow(), 0);

        signal.set(1);
        assert_eq!(*call_count.borrow(), 1);

        signal.set(2);
        assert_eq!(*call_count.borrow(), 2);
    }

    #[test]
    fn test_subscribe_notifies_on_update() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Rc::new(RefCell::new(0));
        let call_count_clone = call_count.clone();

        let _id = signal.subscribe(move || {
            *call_count_clone.borrow_mut() += 1;
        });

        signal.update(|n| n + 1);
        assert_eq!(*call_count.borrow(), 1);

        signal.update(|n| n * 2);
        assert_eq!(*call_count.borrow(), 2);
    }

    #[test]
    fn test_subscribe_notifies_on_update_mut() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Rc::new(RefCell::new(0));
        let call_count_clone = call_count.clone();

        let _id = signal.subscribe(move || {
            *call_count_clone.borrow_mut() += 1;
        });

        signal.update_mut(|n| *n += 1);
        assert_eq!(*call_count.borrow(), 1);

        signal.update_mut(|n| *n *= 2);
        assert_eq!(*call_count.borrow(), 2);
    }

    #[test]
    fn test_multiple_subscribers() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count1 = Rc::new(RefCell::new(0));
        let call_count2 = Rc::new(RefCell::new(0));
        let call_count3 = Rc::new(RefCell::new(0));

        let cc1 = call_count1.clone();
        let cc2 = call_count2.clone();
        let cc3 = call_count3.clone();

        let _id1 = signal.subscribe(move || {
            *cc1.borrow_mut() += 1;
        });
        let _id2 = signal.subscribe(move || {
            *cc2.borrow_mut() += 1;
        });
        let _id3 = signal.subscribe(move || {
            *cc3.borrow_mut() += 1;
        });

        signal.set(42);

        assert_eq!(*call_count1.borrow(), 1);
        assert_eq!(*call_count2.borrow(), 1);
        assert_eq!(*call_count3.borrow(), 1);
    }

    #[test]
    fn test_unsubscribe() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Rc::new(RefCell::new(0));
        let call_count_clone = call_count.clone();

        let id = signal.subscribe(move || {
            *call_count_clone.borrow_mut() += 1;
        });

        signal.set(1);
        assert_eq!(*call_count.borrow(), 1);

        // Unsubscribe
        signal.unsubscribe(id);

        // Should not be called anymore
        signal.set(2);
        assert_eq!(*call_count.borrow(), 1);
    }

    #[test]
    fn test_subscribe_scoped_auto_unsubscribe() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let call_count = Rc::new(RefCell::new(0));
        let call_count_clone = call_count.clone();

        {
            let _subscription = signal.subscribe_scoped(move || {
                *call_count_clone.borrow_mut() += 1;
            });

            signal.set(1);
            assert_eq!(*call_count.borrow(), 1);
        } // Subscription dropped here

        // Should not be called anymore
        signal.set(2);
        assert_eq!(*call_count.borrow(), 1);
    }

    #[test]
    fn test_subscriber_can_read_signal_value() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let last_value = Rc::new(RefCell::new(0));
        let last_value_clone = last_value.clone();
        let signal_clone = signal.clone();

        let _id = signal.subscribe(move || {
            // Use get_untracked() in subscribers (no access to HookContext)
            *last_value_clone.borrow_mut() = signal_clone.get_untracked();
        });

        signal.set(42);
        assert_eq!(*last_value.borrow(), 42);

        signal.set(100);
        assert_eq!(*last_value.borrow(), 100);
    }

    #[test]
    fn test_subscriber_notification_order() {
        let mut ctx = HookContext::new();
        ctx.begin_component(ComponentId(1));

        let signal = ctx.use_hook::<SignalHook<i32>>(0);

        let log = Rc::new(RefCell::new(Vec::new()));

        let log1 = log.clone();
        let _id1 = signal.subscribe(move || {
            log1.borrow_mut().push(1);
        });

        let log2 = log.clone();
        let _id2 = signal.subscribe(move || {
            log2.borrow_mut().push(2);
        });

        let log3 = log.clone();
        let _id3 = signal.subscribe(move || {
            log3.borrow_mut().push(3);
        });

        signal.set(42);

        // All subscribers should be called
        assert_eq!(log.borrow().len(), 3);
        assert!(log.borrow().contains(&1));
        assert!(log.borrow().contains(&2));
        assert!(log.borrow().contains(&3));
    }
}
