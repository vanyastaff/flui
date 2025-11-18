//! Async support for flui-reactivity using tokio.
//!
//! This module provides asynchronous extensions for signals and computed values,
//! enabling integration with tokio-based async applications.
//!
//! # Features
//!
//! - **Async Signal Watching**: Wait for signal changes asynchronously
//! - **Async Computed Values**: Compute values using async functions
//! - **Channel Integration**: Bridge signals with tokio channels
//! - **Future-based Notifications**: Get notified via futures
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_reactivity::{Signal, async_support::SignalExt};
//!
//! #[tokio::main]
//! async fn main() {
//!     let signal = Signal::new(0);
//!
//!     // Spawn task that waits for signal change
//!     let watcher = tokio::spawn(async move {
//!         signal.wait_for_change().await;
//!         println!("Signal changed!");
//!     });
//!
//!     // Update signal from another task
//!     signal.set(42);
//!
//!     watcher.await.unwrap();
//! }
//! ```

use crate::signal::{Signal, SubscriptionId};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// RAII guard for signal-to-channel subscriptions.
///
/// Automatically unsubscribes when dropped, preventing memory leaks.
#[cfg(feature = "async")]
pub struct SignalChannelGuard<T> {
    receiver: T,
    _subscription: SubscriptionId,
}

#[cfg(feature = "async")]
impl<T> SignalChannelGuard<T> {
    fn new(receiver: T, subscription: SubscriptionId) -> Self {
        Self {
            receiver,
            _subscription: subscription,
        }
    }

    /// Get a reference to the inner receiver
    pub fn receiver(&self) -> &T {
        &self.receiver
    }

    /// Get a mutable reference to the inner receiver
    pub fn receiver_mut(&mut self) -> &mut T {
        &mut self.receiver
    }

    /// Consume the guard and return the receiver, **leaking the subscription**.
    ///
    /// # Memory Leak Warning
    ///
    /// ⚠️ **This method intentionally leaks memory!** The subscription will NEVER be cleaned up
    /// and will remain in memory until the `SignalRuntime` is dropped (typically program exit).
    ///
    /// Each leaked subscription:
    /// - Keeps an `Arc<dyn Fn() + Send + Sync>` alive permanently
    /// - Consumes one slot in the per-signal subscription limit (1000 max)
    /// - Will be notified on every signal change forever
    ///
    /// # When to Use
    ///
    /// Only use this if you need the subscription to persist for the entire program lifetime.
    /// In most cases, just use `SignalChannelGuard` directly and let Drop handle cleanup automatically.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // ❌ BAD: Leaks subscription
    /// let rx = signal.to_watch().into_receiver();
    ///
    /// // ✅ GOOD: Automatic cleanup
    /// let guard = signal.to_watch();
    /// let rx = guard.receiver();
    /// ```
    ///
    /// # Note on Safety
    ///
    /// This method is NOT marked `unsafe` because memory leaks are safe in Rust's memory model.
    /// `unsafe` is reserved for operations that can cause undefined behavior (data races,
    /// use-after-free, etc.), not for intentional resource leaks.
    #[must_use = "Subscription is leaked - ensure this is intentional"]
    pub fn into_receiver(self) -> T {
        std::mem::forget(self._subscription); // Intentionally leak subscription
        self.receiver
    }
}

#[cfg(feature = "async")]
impl<T> std::ops::Deref for SignalChannelGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

#[cfg(feature = "async")]
impl<T> std::ops::DerefMut for SignalChannelGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
}

/// Extension trait for async signal operations.
///
/// Provides async methods for waiting on signal changes using tokio primitives.
pub trait SignalExt<T: Clone + Send + Sync + 'static> {
    /// Wait for the next change to this signal.
    ///
    /// Returns a future that completes when the signal is updated via `set()` or `update()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let signal = Signal::new(0);
    ///
    /// tokio::spawn(async move {
    ///     signal.wait_for_change().await;
    ///     println!("Signal changed to: {}", signal.get());
    /// });
    ///
    /// signal.set(42); // Wakes up the waiting task
    /// ```
    fn wait_for_change(&self) -> WaitForChange<T>;

    /// Wait until the signal value satisfies a predicate.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let counter = Signal::new(0);
    ///
    /// tokio::spawn(async move {
    ///     counter.wait_until(|&value| value >= 10).await;
    ///     println!("Counter reached 10!");
    /// });
    /// ```
    fn wait_until<F>(&self, predicate: F) -> WaitUntil<T, F>
    where
        F: Fn(&T) -> bool + Send + 'static;

    /// Create a receiver that gets notified on every signal change.
    ///
    /// Returns a `tokio::sync::broadcast::Receiver` that receives the new value
    /// whenever the signal changes.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let signal = Signal::new(0);
    /// let mut rx = signal.to_broadcast(16); // Buffer size 16
    ///
    /// tokio::spawn(async move {
    ///     while let Ok(value) = rx.recv().await {
    ///         println!("Received: {}", value);
    ///     }
    /// });
    ///
    /// signal.set(1);
    /// signal.set(2);
    /// ```
    ///
    /// Returns a guard that automatically unsubscribes when dropped.
    #[cfg(feature = "async")]
    fn to_broadcast(
        &self,
        capacity: usize,
    ) -> SignalChannelGuard<tokio::sync::broadcast::Receiver<T>>;

    /// Create a watch channel from this signal.
    ///
    /// Returns a `tokio::sync::watch::Receiver` that can be used to
    /// asynchronously wait for and receive signal updates.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let signal = Signal::new(0);
    /// let mut watch_rx = signal.to_watch();
    ///
    /// tokio::spawn(async move {
    ///     while watch_rx.changed().await.is_ok() {
    ///         let value = *watch_rx.borrow();
    ///         println!("Value changed to: {}", value);
    ///     }
    /// });
    /// ```
    ///
    /// Returns a guard that automatically unsubscribes when dropped.
    #[cfg(feature = "async")]
    fn to_watch(&self) -> SignalChannelGuard<tokio::sync::watch::Receiver<T>>;
}

/// Future that completes when a signal changes.
///
/// Created by [`SignalExt::wait_for_change()`].
///
/// NOTE: This future consumes itself on completion.
pub struct WaitForChange<T: Clone + Send + Sync + 'static> {
    guard: Option<SignalChannelGuard<tokio::sync::watch::Receiver<T>>>,
}

impl<T: Clone + Send + Sync + 'static> WaitForChange<T> {
    /// Convert into an async function for easier usage
    pub async fn await_change(mut self) -> T {
        if let Some(guard) = &mut self.guard {
            // Wait for change
            if guard.receiver_mut().changed().await.is_ok() {
                // Return new value
                guard.receiver().borrow().clone()
            } else {
                // Signal dropped - return current value
                guard.receiver().borrow().clone()
            }
        } else {
            panic!("WaitForChange already consumed")
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Future for WaitForChange<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(guard) = self.guard.as_mut() {
            // Poll the changed() future
            let mut fut = Box::pin(guard.receiver_mut().changed());
            let poll_result = fut.as_mut().poll(cx);

            // Drop future to release mutable borrow
            drop(fut);

            match poll_result {
                Poll::Ready(Ok(())) => {
                    // Get value after releasing borrow
                    let value = guard.receiver().borrow().clone();
                    // Consume guard
                    self.guard = None;
                    Poll::Ready(value)
                }
                Poll::Ready(Err(_)) => Poll::Pending,
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Pending
        }
    }
}

/// Future that completes when a signal value satisfies a predicate.
///
/// Created by [`SignalExt::wait_until()`].
///
/// Uses Option for state management to avoid unsafe code.
/// Implements Unpin because all fields are Unpin.
pub struct WaitUntil<T: Clone + Send + Sync + 'static, F: Fn(&T) -> bool> {
    guard: Option<SignalChannelGuard<tokio::sync::watch::Receiver<T>>>,
    predicate: F,
}

// SAFETY: All fields are Unpin (Option<T>, F where F: Fn)
// SignalChannelGuard is Unpin because tokio::sync::watch::Receiver is Unpin
impl<T: Clone + Send + Sync + 'static, F: Fn(&T) -> bool> Unpin for WaitUntil<T, F> {}

impl<T: Clone + Send + Sync + 'static, F: Fn(&T) -> bool> Future for WaitUntil<T, F> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safe: WaitUntil implements Unpin, so Pin::get_mut is safe
        let this = self.get_mut();

        loop {
            if let Some(guard) = &mut this.guard {
                // Check current value
                let current_value = guard.receiver().borrow().clone();
                if (this.predicate)(&current_value) {
                    // Consume guard and return value
                    this.guard = None;
                    return Poll::Ready(current_value);
                }

                // Wait for next change
                let mut fut = Box::pin(guard.receiver_mut().changed());
                match fut.as_mut().poll(cx) {
                    Poll::Ready(Ok(())) => {
                        // Value changed, loop to check again
                        continue;
                    }
                    Poll::Ready(Err(_)) => {
                        // Signal dropped - never complete
                        return Poll::Pending;
                    }
                    Poll::Pending => {
                        // Waiting for change
                        return Poll::Pending;
                    }
                }
            } else {
                // Guard was consumed (future completed)
                return Poll::Pending;
            }
        }
    }
}

impl<T: Clone + Send + Sync + 'static> SignalExt<T> for Signal<T> {
    fn wait_for_change(&self) -> WaitForChange<T> {
        WaitForChange {
            guard: Some(self.to_watch()),
        }
    }

    fn wait_until<F>(&self, predicate: F) -> WaitUntil<T, F>
    where
        F: Fn(&T) -> bool + Send + 'static,
    {
        WaitUntil {
            guard: Some(self.to_watch()),
            predicate,
        }
    }

    #[cfg(feature = "async")]
    fn to_broadcast(
        &self,
        capacity: usize,
    ) -> SignalChannelGuard<tokio::sync::broadcast::Receiver<T>> {
        let (tx, rx) = tokio::sync::broadcast::channel(capacity);

        let signal = self.clone();
        let subscription = self
            .subscribe(move || {
                let value = signal.get();
                let _ = tx.send(value); // Ignore send errors (no receivers)
            })
            .expect("Failed to subscribe for broadcast");

        SignalChannelGuard::new(rx, subscription)
    }

    #[cfg(feature = "async")]
    fn to_watch(&self) -> SignalChannelGuard<tokio::sync::watch::Receiver<T>> {
        let initial_value = self.get();
        let (tx, rx) = tokio::sync::watch::channel(initial_value);

        let signal = self.clone();
        let subscription = self
            .subscribe(move || {
                let value = signal.get();
                let _ = tx.send(value); // Ignore send errors
            })
            .expect("Failed to subscribe for watch");

        SignalChannelGuard::new(rx, subscription)
    }
}

/// Async computed value that can be awaited.
///
/// Unlike regular `Computed`, this allows using async functions
/// to compute values, enabling integration with async I/O, timers, etc.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_reactivity::async_support::AsyncComputed;
///
/// let base = Signal::new(5);
///
/// let async_doubled = AsyncComputed::new(async move {
///     let value = base.get();
///     // Simulate async work
///     tokio::time::sleep(Duration::from_millis(10)).await;
///     value * 2
/// });
///
/// let result = async_doubled.await;
/// assert_eq!(result, 10);
/// ```
#[cfg(feature = "async")]
pub struct AsyncComputed<T> {
    future: Pin<Box<dyn Future<Output = T> + Send>>,
}

#[cfg(feature = "async")]
impl<T> AsyncComputed<T> {
    /// Create a new async computed value from an async block.
    ///
    /// The provided future will be polled when the `AsyncComputed` is awaited.
    pub fn new<F>(future: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        Self {
            future: Box::pin(future),
        }
    }
}

#[cfg(feature = "async")]
impl<T> Future for AsyncComputed<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.future.as_mut().poll(cx)
    }
}

/// Helper to spawn a task that reacts to signal changes.
///
/// Creates a background task that executes a callback whenever the signal changes.
/// The task runs until the returned handle is dropped.
///
/// # Examples
///
/// ```rust,ignore
/// let signal = Signal::new(0);
///
/// let _watcher = spawn_signal_watcher(signal, |value| async move {
///     println!("Signal changed to: {}", value);
///
///     // Perform async operations
///     tokio::time::sleep(Duration::from_millis(10)).await;
/// });
///
/// signal.set(42); // Triggers the watcher
/// ```
#[cfg(feature = "async")]
pub fn spawn_signal_watcher<T, F, Fut>(
    signal: Signal<T>,
    callback: F,
) -> tokio::task::JoinHandle<()>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(T) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        // Use watch channel for efficient notification
        let mut watch_rx = signal.to_watch();

        // Process initial value (clone before await to avoid holding guard)
        {
            let value = watch_rx.borrow().clone();
            callback(value).await;
        }

        // Wait for changes
        while watch_rx.changed().await.is_ok() {
            // Clone value before callback to avoid holding guard across await
            let value = watch_rx.borrow().clone();
            callback(value).await;
        }
    })
}

#[cfg(all(test, feature = "async"))]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_broadcast_integration() {
        let signal = Signal::new(0);
        let mut rx = signal.to_broadcast(10);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            signal.set(42);
        });

        let value = rx.recv().await.unwrap();
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn test_watch_integration() {
        let signal = Signal::new(0);
        let mut watch_rx = signal.to_watch();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            signal.set(100);
        });

        watch_rx.changed().await.unwrap();
        assert_eq!(*watch_rx.borrow(), 100);
    }

    #[tokio::test]
    async fn test_async_computed() {
        let base = Signal::new(5);

        let result = AsyncComputed::new(async move {
            let value = base.get();
            tokio::time::sleep(Duration::from_millis(5)).await;
            value * 2
        })
        .await;

        assert_eq!(result, 10);
    }
}
