//! Platform executor implementations
//!
//! Provides the background executor used for thread-safe asynchronous work.
//! It returns [`Task<T>`] handles for awaiting results.

use std::{
    future::Future,
    sync::{Arc, OnceLock},
    time::Duration,
};

use tokio::runtime::Runtime;

use crate::{
    task::{Priority, Task},
    traits::PlatformExecutor,
};

/// Worker threads in the lazily-started runtime. Deliberately small: since
/// the unified execution services (issue #557) the loop-scoped `AppRuntime`
/// owns the process's real worker pools, and this executor remains only as
/// the `Platform::background_executor` compatibility surface (a
/// removal-target — see `docs/runtime-contract.toml`) for platform-internal
/// marshaling and tests. It must never again claim a full-core pool per
/// platform instance.
const WORKER_THREADS: usize = 2;

/// Background executor for multi-threaded async tasks
///
/// Spawns tasks on a multi-threaded tokio runtime. Returns [`Task<T>`] handles
/// that can be awaited for results or detached for fire-and-forget usage.
///
/// # Thread Pool Configuration
///
/// - **Lazy**: constructing the executor starts no threads; the runtime is
///   built on the first call that needs it (spawn/timer/block/handle). A
///   platform that constructs this executor but never uses it — every
///   FLUI-managed run since the unified execution services (issue #557) —
///   pays nothing.
/// - **Worker threads**: `WORKER_THREADS` (2 — small and fixed rather than
///   core-count sized; see that private const's doc for why)
/// - **Thread names**: `flui-platform-bg-N` for identification in profilers
/// - **Runtime features**: All async features enabled (I/O, timers, etc.)
#[derive(Clone)]
pub struct BackgroundExecutor {
    runtime: Arc<OnceLock<Runtime>>,
}

impl BackgroundExecutor {
    /// Create a new background executor. Cheap: no threads are started
    /// until the first call that needs the runtime.
    pub fn new() -> Self {
        BackgroundExecutor {
            runtime: Arc::new(OnceLock::new()),
        }
    }

    /// The lazily-started runtime.
    ///
    /// # Panics
    ///
    /// Panics if the runtime cannot be created (extremely rare — would
    /// indicate system resource exhaustion or OS-level threading issues).
    fn runtime(&self) -> &Runtime {
        self.runtime.get_or_init(|| {
            tracing::info!(
                worker_threads = WORKER_THREADS,
                "starting the platform background runtime on first use"
            );
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(WORKER_THREADS)
                .thread_name("flui-platform-bg")
                .enable_all()
                .build()
                .expect(
                    "BUG: building a tokio runtime must succeed on any platform this \
                     crate targets short of OS thread exhaustion",
                )
        })
    }

    /// Whether the runtime has been started. Test seam for the laziness
    /// contract; production code has no reason to ask.
    #[cfg(test)]
    fn runtime_started(&self) -> bool {
        self.runtime.get().is_some()
    }

    /// Spawn an async task on the thread pool
    ///
    /// Returns a [`Task<R>`] that can be awaited for the result or detached.
    pub fn spawn<R: Send + 'static>(
        &self,
        future: impl Future<Output = R> + Send + 'static,
    ) -> Task<R> {
        let handle = self.runtime().spawn(future);
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
        self.runtime().block_on(future)
    }

    /// Get a handle to the underlying async runtime
    pub fn handle(&self) -> &tokio::runtime::Handle {
        self.runtime().handle()
    }
}

impl std::fmt::Debug for BackgroundExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundExecutor").finish_non_exhaustive()
    }
}

impl PlatformExecutor for BackgroundExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        self.runtime().spawn(async move {
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

    /// Constructing the executor must not start the runtime (every platform
    /// backend constructs one at platform init; FLUI-managed runs never use
    /// it, and must not pay threads for it); the first use starts it.
    #[test]
    fn test_background_executor_starts_lazily_on_first_use() {
        let executor = BackgroundExecutor::new();
        assert!(
            !executor.runtime_started(),
            "construction must not start worker threads"
        );
        executor.spawn(async {}).detach();
        assert!(executor.runtime_started());
    }

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
