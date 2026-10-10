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

use crate::{task::Task, traits::PlatformExecutor};

/// Worker threads in the lazily-started runtime. Deliberately small: since
/// the unified execution services (issue #557) the loop-scoped `AppRuntime`
/// owns the process's real worker pools, and this executor remains only as
/// the `Platform::background_executor` compatibility surface (slated for
/// removal) for platform-internal marshaling and tests. It must never again
/// claim a full-core pool per platform instance.
const WORKER_THREADS: usize = 2;

/// Shared executor handles may release their final owner from an async task.
/// Runtime's default Drop waits for workers and therefore cannot run there.
struct RuntimeOwner(OnceLock<Runtime>);

impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

/// Background executor for multi-threaded async tasks
///
/// Spawns tasks on a multi-threaded tokio runtime. Returns [`Task<T>`] handles
/// that can be awaited for results or detached for fire-and-forget usage.
///
/// # Thread Pool Configuration
///
/// - **Lazy**: constructing the executor starts no threads; the runtime is
///   built on the first call that needs it (spawn/timer/block/handle). A
///   platform that constructs this executor but never uses it pays nothing.
///   Linux preference observation uses it for D-Bus IO.
/// - **Worker threads**: `WORKER_THREADS` (2 — small and fixed rather than
///   core-count sized; see that private const's doc for why)
/// - **Thread names**: `flui-platform-bg-N` for identification in profilers
/// - **Runtime features**: All async features enabled (I/O, timers, etc.)
/// - **Retirement**: the final shared owner requests shutdown without blocking
///   its caller, including on an async lane. Queued async work is cancelled as
///   with normal runtime shutdown; already executing blocking work finishes
///   independently. Keep an executor owner alive while its tasks must run.
#[derive(Clone)]
pub struct BackgroundExecutor {
    runtime: Arc<RuntimeOwner>,
}

impl BackgroundExecutor {
    /// Create a new background executor. Cheap: no threads are started
    /// until the first call that needs the runtime.
    pub fn new() -> Self {
        BackgroundExecutor {
            runtime: Arc::new(RuntimeOwner(OnceLock::new())),
        }
    }

    /// The lazily-started runtime.
    ///
    /// # Panics
    ///
    /// Panics if the runtime cannot be created (extremely rare — would
    /// indicate system resource exhaustion or OS-level threading issues).
    fn runtime(&self) -> &Runtime {
        self.runtime.0.get_or_init(|| {
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
