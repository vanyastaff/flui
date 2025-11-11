//! Scheduler binding - frame callbacks
//!
//! SchedulerBinding manages frame callbacks for animations, rebuilds, and post-frame tasks.
//! It coordinates the timing of framework operations with the platform's vsync signal.

use super::BindingBase;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

/// Frame callback function type
///
/// Receives the elapsed time since app start.
/// Thread-safe to allow registration from any thread.
pub type FrameCallback = Arc<dyn Fn(Duration) + Send + Sync>;

/// Scheduler binding - manages frame callbacks
///
/// # Architecture
///
/// ```text
/// vsync → begin_frame → persistent callbacks → build/layout/paint → end_frame → post-frame callbacks
/// ```
///
/// # Callback Types
///
/// - **Persistent callbacks**: Called every frame (e.g., rebuild dirty widgets)
/// - **Post-frame callbacks**: Called once after frame completes (e.g., cleanup, one-time updates)
///
/// # Thread-Safety
///
/// Uses Arc<Mutex<>> for thread-safe callback registration and execution.
pub struct SchedulerBinding {
    /// Persistent callbacks (called every frame)
    ///
    /// These callbacks run at the start of each frame and are used for:
    /// - Rebuilding dirty widgets
    /// - Updating animations
    /// - Processing pending state changes
    persistent_callbacks: Arc<Mutex<Vec<FrameCallback>>>,

    /// One-time post-frame callbacks
    ///
    /// These callbacks run once after the frame completes and are used for:
    /// - Cleanup operations
    /// - Deferred state updates
    /// - Post-layout measurements
    post_frame_callbacks: Arc<Mutex<Vec<FrameCallback>>>,
}

impl SchedulerBinding {
    /// Create a new SchedulerBinding
    pub fn new() -> Self {
        Self {
            persistent_callbacks: Arc::new(Mutex::new(Vec::new())),
            post_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add persistent frame callback
    ///
    /// Persistent callbacks are called every frame at the start of the frame.
    /// They remain registered until the app exits.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// binding.add_persistent_frame_callback(Arc::new(|timestamp| {
    ///     println!("Frame at {:?}", timestamp);
    /// }));
    /// ```
    pub fn add_persistent_frame_callback(&self, callback: FrameCallback) {
        self.persistent_callbacks.lock().push(callback);
        tracing::trace!("Added persistent frame callback");
    }

    /// Add one-time post-frame callback
    ///
    /// Post-frame callbacks are called once after the current frame completes.
    /// They are automatically removed after execution.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// binding.add_post_frame_callback(Arc::new(|_| {
    ///     println!("Frame completed!");
    /// }));
    /// ```
    pub fn add_post_frame_callback(&self, callback: FrameCallback) {
        self.post_frame_callbacks.lock().push(callback);
        tracing::trace!("Added post-frame callback");
    }

    /// Called at start of frame
    ///
    /// Executes all persistent frame callbacks with the current timestamp.
    /// This is where widget rebuilds, animation updates, and state changes occur.
    ///
    /// # Parameters
    ///
    /// - `timestamp`: Time elapsed since app start
    pub fn handle_begin_frame(&self, timestamp: Duration) {
        let callbacks = self.persistent_callbacks.lock();

        tracing::trace!(
            callback_count = callbacks.len(),
            "Executing persistent frame callbacks"
        );

        for callback in callbacks.iter() {
            callback(timestamp);
        }
    }

    /// Called at end of frame
    ///
    /// Executes all post-frame callbacks and clears them (one-time execution).
    /// This is where cleanup, measurements, and deferred updates occur.
    pub fn handle_draw_frame(&self) {
        // Take all post-frame callbacks (consume and clear)
        let callbacks = std::mem::take(&mut *self.post_frame_callbacks.lock());

        if !callbacks.is_empty() {
            tracing::trace!(
                callback_count = callbacks.len(),
                "Executing post-frame callbacks"
            );

            for callback in callbacks {
                callback(Duration::ZERO);
            }
        }
    }

    /// Get number of persistent callbacks registered
    ///
    /// Useful for debugging and testing.
    #[must_use]
    pub fn persistent_callback_count(&self) -> usize {
        self.persistent_callbacks.lock().len()
    }

    /// Get number of pending post-frame callbacks
    ///
    /// Useful for debugging and testing.
    #[must_use]
    pub fn post_frame_callback_count(&self) -> usize {
        self.post_frame_callbacks.lock().len()
    }
}

impl Default for SchedulerBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingBase for SchedulerBinding {
    fn init(&mut self) {
        tracing::debug!("SchedulerBinding initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_scheduler_binding_creation() {
        let binding = SchedulerBinding::new();
        assert_eq!(binding.persistent_callback_count(), 0);
        assert_eq!(binding.post_frame_callback_count(), 0);
    }

    #[test]
    fn test_persistent_callback() {
        let binding = SchedulerBinding::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = counter.clone();
        binding.add_persistent_frame_callback(Arc::new(move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(binding.persistent_callback_count(), 1);

        // Execute twice - persistent callbacks stay registered
        binding.handle_begin_frame(Duration::from_secs(0));
        binding.handle_begin_frame(Duration::from_secs(1));

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(binding.persistent_callback_count(), 1); // Still registered
    }

    #[test]
    fn test_post_frame_callback() {
        let binding = SchedulerBinding::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = counter.clone();
        binding.add_post_frame_callback(Arc::new(move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(binding.post_frame_callback_count(), 1);

        // Execute - post-frame callbacks are one-time
        binding.handle_draw_frame();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(binding.post_frame_callback_count(), 0); // Cleared after execution

        // Execute again - no callbacks
        binding.handle_draw_frame();
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Still 1, not 2
    }

    #[test]
    fn test_multiple_callbacks() {
        let binding = SchedulerBinding::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // Add 3 persistent callbacks
        for _ in 0..3 {
            let counter_clone = counter.clone();
            binding.add_persistent_frame_callback(Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }));
        }

        assert_eq!(binding.persistent_callback_count(), 3);

        binding.handle_begin_frame(Duration::from_secs(0));
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }
}
