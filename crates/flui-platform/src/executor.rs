//! Platform executor implementations
//!
//! Provides async executors for background and foreground task execution.
//! Background executor uses Tokio runtime, foreground executor uses flume channels.

use crate::traits::PlatformExecutor;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Background executor for multi-threaded async tasks
///
/// Spawns tasks on a multi-threaded runtime optimized for CPU-bound
/// and I/O-bound background work. The thread pool size is determined by
/// the number of available CPU cores.
///
/// # Thread Pool Configuration
///
/// - **Worker threads**: `num_cpus::get()` (typically 4-16 on modern systems)
/// - **Thread names**: `flui-background-N` for easy identification in profilers
/// - **Runtime features**: All async features enabled (I/O, timers, etc.)
///
/// # Use Cases
///
/// - File I/O operations
/// - Network requests
/// - Image/asset loading
/// - CPU-intensive computations
/// - Background data processing
///
/// # Examples
///
/// ```rust,ignore
/// use flui_platform::executor::BackgroundExecutor;
/// use flui_platform::PlatformExecutor;
///
/// let executor = BackgroundExecutor::new();
///
/// executor.spawn(Box::new(|| {
///     println!("Running on background thread");
/// }));
/// ```
pub struct BackgroundExecutor {
    runtime: Arc<Runtime>,
}

impl BackgroundExecutor {
    /// Create a new background executor with multi-threaded runtime
    ///
    /// # Panics
    ///
    /// Panics if the runtime cannot be created (extremely rare - would indicate
    /// system resource exhaustion or OS-level threading issues).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_platform::executor::BackgroundExecutor;
    ///
    /// let executor = BackgroundExecutor::new();
    /// ```
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(num_cpus::get())
            .thread_name("flui-background")
            .enable_all()
            .build()
            .expect("Failed to create background runtime");

        tracing::info!(
            "Created BackgroundExecutor with {} worker threads",
            num_cpus::get()
        );

        BackgroundExecutor {
            runtime: Arc::new(runtime),
        }
    }

    /// Get a handle to the underlying async runtime
    ///
    /// Useful for spawning native async tasks.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_platform::executor::BackgroundExecutor;
    ///
    /// let executor = BackgroundExecutor::new();
    /// let handle = executor.handle();
    ///
    /// handle.spawn(async {
    ///     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    /// });
    /// ```
    pub fn handle(&self) -> &tokio::runtime::Handle {
        self.runtime.handle()
    }
}

impl PlatformExecutor for BackgroundExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        self.runtime.spawn(async move {
            task();
        });
    }
}

impl Default for BackgroundExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Foreground executor for UI thread task execution
///
/// Executes tasks on the main UI thread using a message queue pattern.
/// Tasks are submitted via an unbounded flume channel and must be polled/drained
/// by the platform's message loop.
///
/// # Architecture
///
/// ```text
/// ┌─────────────┐
/// │  User Code  │
/// └──────┬──────┘
///        │ executor.spawn(task)
///        ▼
/// ┌─────────────────────┐
/// │ Sender (flume)      │
/// │ (thread-safe queue) │
/// └──────┬──────────────┘
///        │ send(task)
///        ▼
/// ┌─────────────────────┐
/// │ Receiver (flume)    │  ◄── Platform message loop
/// │ (polled by UI)      │      calls drain_tasks()
/// └─────────────────────┘
/// ```
///
/// # Performance
///
/// Uses `flume` channels which are faster than `tokio::sync::mpsc` for this use case:
/// - Lock-free in common case
/// - Better cache locality
/// - Optimized for single-consumer pattern
///
/// # Thread Safety
///
/// The sender is `Send + Sync` and can be cloned to share across threads.
/// All tasks are queued and executed serially on the UI thread.
///
/// # Use Cases
///
/// - Window updates
/// - Layout recalculation
/// - Event dispatch
/// - State mutations requiring UI thread
///
/// # Examples
///
/// ```rust,ignore
/// use flui_platform::executor::ForegroundExecutor;
/// use flui_platform::PlatformExecutor;
///
/// let executor = ForegroundExecutor::new();
///
/// // From any thread
/// executor.spawn(Box::new(|| {
///     println!("This runs on UI thread");
/// }));
///
/// // In message loop
/// executor.drain_tasks();
/// ```
pub struct ForegroundExecutor {
    sender: flume::Sender<Box<dyn FnOnce() + Send>>,
    receiver: Arc<Mutex<flume::Receiver<Box<dyn FnOnce() + Send>>>>,
}

impl ForegroundExecutor {
    /// Create a new foreground executor with task queue
    ///
    /// Returns an executor that queues tasks for UI thread execution.
    /// The platform should call `drain_tasks()` regularly in the message loop.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_platform::executor::ForegroundExecutor;
    ///
    /// let executor = ForegroundExecutor::new();
    /// ```
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();

        tracing::info!("Created ForegroundExecutor with unbounded flume channel");

        ForegroundExecutor {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Drain and execute all pending tasks
    ///
    /// This method should be called regularly by the platform's message loop
    /// (e.g., in Windows' message pump, macOS' run loop, etc.).
    ///
    /// # Execution
    ///
    /// Tasks are executed in FIFO order. If a task spawns new tasks, they
    /// will be executed in the next drain cycle.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // In Windows message loop:
    /// while GetMessageW(&mut msg, None, 0, 0).as_bool() {
    ///     foreground_executor.drain_tasks();
    ///     TranslateMessage(&msg);
    ///     DispatchMessageW(&msg);
    /// }
    /// ```
    pub fn drain_tasks(&self) {
        // Collect all pending tasks while holding the lock, then release
        // before executing. This prevents deadlocks if a task calls
        // drain_tasks() or pending_count() recursively.
        let tasks: Vec<_> = {
            let receiver = self.receiver.lock();
            std::iter::from_fn(|| receiver.try_recv().ok()).collect()
        };

        let count = tasks.len();
        for task in tasks {
            task();
        }

        if count > 0 {
            tracing::trace!("Drained {} foreground tasks", count);
        }
    }

    /// Get the number of pending tasks
    ///
    /// Returns the exact number of tasks currently in the queue.
    /// flume provides accurate queue length tracking.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let executor = ForegroundExecutor::new();
    /// executor.spawn(Box::new(|| {}));
    /// assert_eq!(executor.pending_count(), 1);
    /// ```
    pub fn pending_count(&self) -> usize {
        let receiver = self.receiver.lock();
        receiver.len()
    }
}

impl PlatformExecutor for ForegroundExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        if let Err(e) = self.sender.send(task) {
            tracing::error!("Failed to send task to foreground executor: {:?}", e);
        }
    }
}

impl Default for ForegroundExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ForegroundExecutor {
    fn clone(&self) -> Self {
        ForegroundExecutor {
            sender: self.sender.clone(),
            receiver: Arc::clone(&self.receiver),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_background_executor_spawn() {
        let executor = BackgroundExecutor::new();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);

        executor.spawn(Box::new(move || {
            flag_clone.store(true, Ordering::SeqCst);
        }));

        // Give task time to execute
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_foreground_executor_spawn_and_drain() {
        let executor = ForegroundExecutor::new();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);

        executor.spawn(Box::new(move || {
            flag_clone.store(true, Ordering::SeqCst);
        }));

        // Task should not execute until drained
        assert!(!flag.load(Ordering::SeqCst));

        executor.drain_tasks();

        // Now task should have executed
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_foreground_executor_multiple_tasks() {
        let executor = ForegroundExecutor::new();
        let counter = Arc::new(Mutex::new(0));

        for i in 0..10 {
            let counter_clone = Arc::clone(&counter);
            executor.spawn(Box::new(move || {
                let mut count = counter_clone.lock();
                *count += i;
            }));
        }

        executor.drain_tasks();

        assert_eq!(*counter.lock(), 45); // 0+1+2+...+9 = 45
    }

    #[test]
    fn test_foreground_executor_clone() {
        let executor1 = ForegroundExecutor::new();
        let executor2 = executor1.clone();

        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);

        executor2.spawn(Box::new(move || {
            flag_clone.store(true, Ordering::SeqCst);
        }));

        executor1.drain_tasks();
        assert!(flag.load(Ordering::SeqCst));
    }
}
