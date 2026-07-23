//! Platform executor implementations
//!
//! Provides the background executor used for thread-safe asynchronous work.
//! It returns [`Task<T>`] handles for awaiting results.

use std::{future::Future, sync::Arc, time::Duration};

use tokio::runtime::Runtime;

use crate::{
    task::{Priority, Task},
    traits::PlatformExecutor,
};

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

impl std::fmt::Debug for BackgroundExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundExecutor").finish_non_exhaustive()
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

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
}
