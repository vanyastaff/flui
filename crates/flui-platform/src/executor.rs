//! Platform executor implementations
//!
//! Provides async executors for background and foreground task execution.
//! Background executor uses Tokio runtime, foreground executor uses flume channels.
//! Both return [`Task<T>`] handles for awaiting results.

use crate::task::{Priority, Task};
use crate::traits::PlatformExecutor;
use parking_lot::Mutex;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

/// Background executor for multi-threaded async tasks
///
/// Spawns tasks on a multi-threaded tokio runtime. Returns [`Task<T>`] handles
/// that can be awaited for results or detached for fire-and-forget usage.
///
/// # Thread Pool Configuration
///
/// - **Worker threads**: `num_cpus::get()` (typically 4-16 on modern systems)
/// - **Thread names**: `flui-background-N` for easy identification in profilers
/// - **Runtime features**: All async features enabled (I/O, timers, etc.)
#[derive(Clone)]
pub struct BackgroundExecutor {
    runtime: Arc<Runtime>,
}

impl BackgroundExecutor {
    /// Create a new background executor with multi-threaded runtime
    ///
    /// # Panics
    ///
    /// Panics if the runtime cannot be created (extremely rare — would indicate
    /// system resource exhaustion or OS-level threading issues).
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

    /// Spawn an async task on the thread pool
    ///
    /// Returns a [`Task<R>`] that can be awaited for the result or detached.
    pub fn spawn<R: Send + 'static>(
        &self,
        future: impl Future<Output = R> + Send + 'static,
    ) -> Task<R> {
        let handle = self.runtime.spawn(future);
        Task::from_handle(handle)
    }

    /// Spawn an async task with explicit priority
    ///
    /// Priority is currently informational only — tokio uses fair scheduling.
    /// Priority-aware dispatching is planned for post-MVP.
    pub fn spawn_with_priority<R: Send + 'static>(
        &self,
        _priority: Priority,
        future: impl Future<Output = R> + Send + 'static,
    ) -> Task<R> {
        // TODO: route to priority-aware thread pools post-MVP
        self.spawn(future)
    }

    /// Create a timer that completes after the given duration
    pub fn timer(&self, duration: Duration) -> Task<()> {
        self.spawn(async move {
            tokio::time::sleep(duration).await;
        })
    }

    /// Block the current thread until the future completes
    ///
    /// Useful for bridging async code in synchronous contexts (e.g., tests).
    /// Do NOT call from within an async context — it will panic.
    pub fn block<R>(&self, future: impl Future<Output = R>) -> R {
        self.runtime.block_on(future)
    }

    /// Get a handle to the underlying async runtime
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
///        │ executor.spawn(future)
///        ▼
/// ┌─────────────────────┐
/// │ Sender (flume)      │
/// │ (thread-safe queue) │
/// └──────┬──────────────┘
///        │ send(task)
///        ▼
/// ┌─────────────────────┐
/// │ Receiver (flume)    │  <-- Platform message loop
/// │ (polled by UI)      │      calls drain_tasks()
/// └─────────────────────┘
/// ```
#[derive(Clone)]
pub struct ForegroundExecutor {
    sender: flume::Sender<Box<dyn FnOnce() + Send>>,
    #[allow(clippy::type_complexity)]
    receiver: Arc<Mutex<flume::Receiver<Box<dyn FnOnce() + Send>>>>,
}

impl ForegroundExecutor {
    /// Create a new foreground executor with task queue
    ///
    /// Returns an executor that queues tasks for UI thread execution.
    /// The platform should call `drain_tasks()` regularly in the message loop.
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();

        tracing::info!("Created ForegroundExecutor with unbounded flume channel");

        ForegroundExecutor {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Spawn a closure to run on the foreground (UI) thread
    ///
    /// Returns a [`Task<R>`] that resolves when the closure executes during
    /// the next `drain_tasks()` call.
    pub fn spawn<R: Send + 'static>(
        &self,
        future: impl Future<Output = R> + Send + 'static,
    ) -> Task<R> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let sender = self.sender.clone();

        // We need a runtime handle to drive the future on the foreground thread.
        // Wrap the future execution and result sending in a closure.
        if let Err(e) = sender.send(Box::new(move || {
            // Create a minimal runtime to poll the future to completion
            // on the foreground thread. For simple futures (e.g., async { 42 }),
            // this completes immediately.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create foreground task runtime");
            let result = rt.block_on(future);
            let _ = tx.send(result);
        })) {
            tracing::error!("Failed to send task to foreground executor: {:?}", e);
        }

        // Return a task that awaits the oneshot result
        Task::from_handle(tokio::task::spawn(async move {
            rx.await.expect("Foreground task sender dropped")
        }))
    }

    /// Drain and execute all pending tasks
    ///
    /// This method should be called regularly by the platform's message loop
    /// (e.g., in Windows' message pump, macOS' run loop, etc.).
    ///
    /// Tasks are executed in FIFO order. If a task spawns new tasks, they
    /// will be executed in the next drain cycle.
    pub fn drain_tasks(&self) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_background_executor_spawn() {
        let executor = BackgroundExecutor::new();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);

        executor
            .spawn(async move {
                flag_clone.store(true, Ordering::SeqCst);
            })
            .detach();

        // Give task time to execute
        std::thread::sleep(Duration::from_millis(100));
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_background_executor_spawn_await() {
        let executor = BackgroundExecutor::new();
        let result = executor.block(async {
            let task = executor.spawn(async { 1 + 1 });
            task.await
        });
        assert_eq!(result, 2);
    }

    #[test]
    fn test_background_executor_timer() {
        let executor = BackgroundExecutor::new();
        let start = std::time::Instant::now();
        executor.block(async {
            executor.timer(Duration::from_millis(50)).await;
        });
        assert!(start.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    fn test_background_executor_spawn_with_priority() {
        let executor = BackgroundExecutor::new();
        let result = executor.block(async {
            let task = executor.spawn_with_priority(Priority::High, async { 42 });
            task.await
        });
        assert_eq!(result, 42);
    }

    #[test]
    fn test_foreground_executor_spawn_and_drain() {
        let executor = ForegroundExecutor::new();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);

        // Use the PlatformExecutor trait method (Box<dyn FnOnce>)
        PlatformExecutor::spawn(
            &executor,
            Box::new(move || {
                flag_clone.store(true, Ordering::SeqCst);
            }),
        );

        assert!(!flag.load(Ordering::SeqCst));
        executor.drain_tasks();
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_foreground_executor_multiple_tasks() {
        let executor = ForegroundExecutor::new();
        let counter = Arc::new(Mutex::new(0));

        for i in 0..10 {
            let counter_clone = Arc::clone(&counter);
            PlatformExecutor::spawn(
                &executor,
                Box::new(move || {
                    let mut count = counter_clone.lock();
                    *count += i;
                }),
            );
        }

        executor.drain_tasks();
        assert_eq!(*counter.lock(), 45);
    }

    #[test]
    fn test_foreground_executor_clone() {
        let executor1 = ForegroundExecutor::new();
        let executor2 = executor1.clone();

        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);

        PlatformExecutor::spawn(
            &executor2,
            Box::new(move || {
                flag_clone.store(true, Ordering::SeqCst);
            }),
        );

        executor1.drain_tasks();
        assert!(flag.load(Ordering::SeqCst));
    }
}
