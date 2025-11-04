//! Signal primitive - Copy-able reactive value
//!
//! Signals are the core reactive primitive in FLUI. They are cheap to copy
//! (just 8 bytes) and automatically track dependencies when accessed.

use std::marker::PhantomData;
use crate::runtime::{with_runtime, SubscriberCallback};
use crate::scope::with_scope;

/// Unique identifier for a signal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(pub(crate) usize);

impl SignalId {
    /// Create a new SignalId from a usize
    #[inline]
    pub const fn new(id: usize) -> Self {
        Self(id)
    }

    /// Get the raw usize value
    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0
    }
}

/// A reactive value that automatically tracks dependencies
///
/// Signal<T> is a Copy type (just 8 bytes) that acts as a handle to a value
/// stored in the SignalRuntime. When you read a signal with `.get()`, it
/// automatically registers a dependency in the current reactive scope.
///
/// # Example
///
/// ```rust,ignore
/// let count = Signal::new(0);
///
/// // Copy the signal (cheap!)
/// let count_copy = count;
///
/// // Read the value (registers dependency)
/// println!("Count: {}", count.get());
///
/// // Update the value (notifies dependents)
/// count.set(10);
/// count.update(|v| *v += 1);
/// ```
pub struct Signal<T> {
    id: SignalId,
    _phantom: PhantomData<T>,
}

impl<T: 'static> Signal<T> {
    /// Create a new signal with an initial value
    ///
    /// The value is stored in the thread-local SignalRuntime.
    pub fn new(value: T) -> Self {
        let id = with_runtime(|runtime| runtime.create_signal(value));
        Self {
            id,
            _phantom: PhantomData,
        }
    }

    /// Get the signal's ID
    #[inline]
    pub const fn id(&self) -> SignalId {
        self.id
    }

    /// Read the signal's value
    ///
    /// This automatically registers a dependency in the current reactive scope
    /// (if any), so that when this signal changes, the scope will be notified.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        // Register dependency in current scope
        with_scope(|scope| {
            if let Some(scope) = scope {
                scope.track_signal(self.id);
            }
        });

        // Get value from runtime
        with_runtime(|runtime| runtime.get_signal(self.id))
    }

    /// Read the signal's value with a closure (avoids cloning)
    ///
    /// This is more efficient than `.get()` for non-Copy types.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        // Register dependency in current scope
        with_scope(|scope| {
            if let Some(scope) = scope {
                scope.track_signal(self.id);
            }
        });

        // Access value from runtime
        with_runtime(|runtime| runtime.with_signal(self.id, f))
    }

    /// Set the signal's value
    ///
    /// This notifies all subscribers that the signal has changed.
    pub fn set(&self, value: T) {
        with_runtime(|runtime| {
            runtime.set_signal(self.id, value);
            runtime.notify_subscribers(self.id);
        });
    }

    /// Update the signal's value using a closure
    ///
    /// This is more efficient than `get()` + `set()` as it avoids cloning.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        with_runtime(|runtime| {
            runtime.update_signal(self.id, f);
            runtime.notify_subscribers(self.id);
        });
    }

    /// Subscribe to changes to this signal
    ///
    /// Returns a subscription ID that can be used to unsubscribe.
    pub fn subscribe(&self, callback: SubscriberCallback) -> usize {
        with_runtime(|runtime| runtime.subscribe(self.id, callback))
    }

    /// Unsubscribe from changes
    pub fn unsubscribe(&self, subscription_id: usize) {
        with_runtime(|runtime| runtime.unsubscribe(self.id, subscription_id));
    }
}

// Signal is Copy (just an 8-byte index)
impl<T> Copy for Signal<T> {}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signal")
            .field("id", &self.id)
            .finish()
    }
}

// Convenient trait for incrementing numeric signals
impl<T> Signal<T>
where
    T: std::ops::AddAssign<T> + From<u8> + 'static,
{
    /// Increment the signal by 1
    pub fn increment(&self) {
        self.update(|v| *v += T::from(1));
    }
}

// Convenient trait for decrementing numeric signals
impl<T> Signal<T>
where
    T: std::ops::SubAssign<T> + From<u8> + 'static,
{
    /// Decrement the signal by 1
    pub fn decrement(&self) {
        self.update(|v| *v -= T::from(1));
    }
}
