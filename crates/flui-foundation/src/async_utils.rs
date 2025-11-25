//! Async utilities for reactive programming
//!
//! This module provides async-compatible versions of foundation types
//! for use in async contexts and reactive programming patterns.

use crate::{ChangeNotifier, Listenable, ListenerCallback, ListenerId, ValueNotifier};
use std::sync::Arc;
use tokio::sync::{broadcast, watch, Notify};
use tokio::time::{timeout, Duration};

/// An async-compatible change notifier that can notify across async boundaries.
///
/// This is similar to `ChangeNotifier` but provides async notification capabilities
/// and integrates with tokio's async runtime.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::AsyncChangeNotifier;
/// use tokio::time::{sleep, Duration};
///
/// #[tokio::main]
/// async fn main() {
///     let notifier = AsyncChangeNotifier::new();
///     let mut receiver = notifier.subscribe();
///
///     // Spawn a task to notify after a delay
///     let notifier_clone = notifier.clone();
///     tokio::spawn(async move {
///         sleep(Duration::from_millis(100)).await;
///         notifier_clone.notify().await;
///     });
///
///     // Wait for notification
///     receiver.recv().await.unwrap();
///     println!("Received notification!");
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AsyncChangeNotifier {
    /// Broadcast sender for async notifications
    sender: broadcast::Sender<()>,
    /// Traditional synchronous notifier for compatibility
    sync_notifier: Arc<ChangeNotifier>,
}

impl AsyncChangeNotifier {
    /// Creates a new async change notifier.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self {
            sender,
            sync_notifier: Arc::new(ChangeNotifier::new()),
        }
    }

    /// Subscribes to notifications, returning a receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.sender.subscribe()
    }

    /// Notifies all subscribers asynchronously.
    ///
    /// This will notify both async subscribers and synchronous listeners.
    pub async fn notify(&self) {
        // Notify async subscribers (ignore if no receivers)
        let _ = self.sender.send(());

        // Also notify synchronous listeners for compatibility
        self.sync_notifier.notify();
    }

    /// Notifies all subscribers synchronously (for compatibility).
    pub fn notify_sync(&self) {
        let _ = self.sender.send(());
        self.sync_notifier.notify();
    }

    /// Adds a synchronous listener for compatibility with sync code.
    pub fn add_listener<F>(&self, callback: F) -> ListenerId
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.sync_notifier.add_listener(callback)
    }

    /// Removes a synchronous listener.
    pub fn remove_listener(&self, id: ListenerId) {
        self.sync_notifier.remove_listener(id);
    }

    /// Waits for the next notification with a timeout.
    pub async fn wait_for_change(
        &self,
        timeout_duration: Duration,
    ) -> Result<(), tokio::time::error::Elapsed> {
        let mut receiver = self.subscribe();
        timeout(timeout_duration, receiver.recv()).await?;
        Ok(())
    }

    /// Returns the number of active async subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for AsyncChangeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// An async-compatible value notifier that holds a value and notifies on changes.
///
/// This combines the functionality of `ValueNotifier` with async capabilities.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::AsyncValueNotifier;
///
/// #[tokio::main]
/// async fn main() {
///     let notifier = AsyncValueNotifier::new(0);
///     let mut receiver = notifier.subscribe();
///
///     // Update the value in another task
///     let notifier_clone = notifier.clone();
///     tokio::spawn(async move {
///         notifier_clone.set(42).await;
///     });
///
///     // Wait for the new value
///     let new_value = receiver.recv().await.unwrap();
///     assert_eq!(new_value, 42);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AsyncValueNotifier<T> {
    /// Watch sender for value updates
    sender: watch::Sender<T>,
    /// Traditional synchronous value notifier for compatibility
    sync_notifier: Arc<ValueNotifier<T>>,
}

impl<T> AsyncValueNotifier<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Creates a new async value notifier with an initial value.
    pub fn new(initial_value: T) -> Self {
        let (sender, _) = watch::channel(initial_value.clone());
        Self {
            sender,
            sync_notifier: Arc::new(ValueNotifier::new(initial_value)),
        }
    }

    /// Gets the current value.
    pub fn get(&self) -> T {
        self.sender.borrow().clone()
    }

    /// Sets a new value and notifies subscribers.
    pub async fn set(&self, value: T) {
        // Update the watch channel
        let _ = self.sender.send(value.clone());

        // Update the sync notifier for compatibility
        self.sync_notifier.set(value);
    }

    /// Sets a new value synchronously.
    pub fn set_sync(&self, value: T) {
        let _ = self.sender.send(value.clone());
        self.sync_notifier.set(value);
    }

    /// Updates the value using a function and notifies subscribers.
    pub async fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let mut value = self.get();
        f(&mut value);
        self.set(value).await;
    }

    /// Subscribes to value changes, returning a receiver.
    pub fn subscribe(&self) -> watch::Receiver<T> {
        self.sender.subscribe()
    }

    /// Adds a synchronous listener that receives the new value.
    pub fn add_listener<F>(&self, callback: F) -> ListenerId
    where
        F: Fn(&T) + Send + Sync + 'static,
    {
        self.sync_notifier.add_listener(callback)
    }

    /// Removes a synchronous listener.
    pub fn remove_listener(&self, id: ListenerId) {
        self.sync_notifier.remove_listener(id);
    }

    /// Waits for the value to change with a timeout.
    pub async fn wait_for_change(
        &self,
        timeout_duration: Duration,
    ) -> Result<T, tokio::time::error::Elapsed> {
        let mut receiver = self.subscribe();
        timeout(timeout_duration, receiver.changed()).await??;
        Ok(receiver.borrow().clone())
    }

    /// Returns the number of active async subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl<T> Default for AsyncValueNotifier<T>
where
    T: Default + Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// Async utilities for batching notifications.
///
/// This helps reduce notification noise when many changes happen quickly.
pub struct AsyncNotificationBatcher {
    notify: Arc<Notify>,
    is_pending: Arc<std::sync::atomic::AtomicBool>,
}

impl AsyncNotificationBatcher {
    /// Creates a new notification batcher.
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            is_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Schedules a notification to be sent after a delay.
    ///
    /// Multiple calls within the delay period will be batched into a single notification.
    pub async fn schedule_notification(&self, delay: Duration) {
        use std::sync::atomic::Ordering;

        // If a notification is already pending, don't schedule another
        if self
            .is_pending
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let notify = self.notify.clone();
        let is_pending = self.is_pending.clone();

        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            is_pending.store(false, Ordering::SeqCst);
            notify.notify_waiters();
        });
    }

    /// Waits for the next batched notification.
    pub async fn wait_for_notification(&self) {
        self.notify.notified().await;
    }
}

impl Default for AsyncNotificationBatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_async_change_notifier() {
        let notifier = AsyncChangeNotifier::new();
        let mut receiver = notifier.subscribe();

        // Notify in another task
        let notifier_clone = notifier.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            notifier_clone.notify().await;
        });

        // Should receive notification
        tokio::time::timeout(Duration::from_millis(100), receiver.recv())
            .await
            .expect("Should not timeout")
            .expect("Should receive notification");
    }

    #[tokio::test]
    async fn test_async_value_notifier() {
        let notifier = AsyncValueNotifier::new(0);
        let mut receiver = notifier.subscribe();

        // Set value in another task
        let notifier_clone = notifier.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            notifier_clone.set(42).await;
        });

        // Wait for change
        receiver.changed().await.expect("Should detect change");
        assert_eq!(*receiver.borrow(), 42);
    }

    #[tokio::test]
    async fn test_notification_batcher() {
        let batcher = AsyncNotificationBatcher::new();

        // Schedule multiple notifications quickly
        batcher
            .schedule_notification(Duration::from_millis(50))
            .await;
        batcher
            .schedule_notification(Duration::from_millis(50))
            .await; // Should be ignored
        batcher
            .schedule_notification(Duration::from_millis(50))
            .await; // Should be ignored

        // Should only get one notification
        tokio::time::timeout(Duration::from_millis(100), batcher.wait_for_notification())
            .await
            .expect("Should receive batched notification");
    }

    #[tokio::test]
    async fn test_timeout_functionality() {
        let notifier = AsyncChangeNotifier::new();

        // Should timeout since no notification is sent
        let result = notifier.wait_for_change(Duration::from_millis(10)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_functionality() {
        let notifier = AsyncValueNotifier::new(0);

        notifier.update(|value| *value += 10).await;
        assert_eq!(notifier.get(), 10);

        notifier.update(|value| *value *= 2).await;
        assert_eq!(notifier.get(), 20);
    }
}
