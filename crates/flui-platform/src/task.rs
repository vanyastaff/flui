//! Async task abstraction
//!
//! Provides [`Task<T>`] — an awaitable handle to a spawned async operation.
//! Wraps `tokio::task::JoinHandle<T>` for background tasks and supports
//! pre-computed values via [`Task::ready`].
//!
//! # Design
//!
//! Based on GPUI's task pattern, adapted for tokio:
//! - `Task::ready(val)` — already-completed task (no spawn)
//! - `Task::detach()` — fire-and-forget (drops handle, task keeps running)
//! - `impl Future for Task<T>` — await the result
//!
//! There is deliberately no priority knob. One existed — a `Priority` enum
//! that `spawn_with_priority` took and discarded, because tokio exposes no
//! task-priority scheduling for it to route to — and an argument the callee
//! ignores is worse than no argument at all: it reads as a scheduling
//! guarantee the runtime never made. Priority-aware dispatch (Windows
//! ThreadPool, macOS GCD) can reintroduce the knob when it can honour it.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Debug/tracing label for a task
///
/// Provides human-readable identification for tasks in logs and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskLabel(pub &'static str);

impl std::fmt::Display for TaskLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// An awaitable handle to an async operation
///
/// `Task<T>` wraps either a pre-computed value or a spawned tokio task.
/// It implements `Future` so it can be `.await`ed, and provides `detach()`
/// for fire-and-forget usage.
///
/// # Examples
///
/// ```rust,ignore
/// // Pre-computed value
/// let task = Task::ready(42);
/// assert_eq!(task.await, 42);
///
/// // Spawned async work
/// let task = executor.spawn(async { expensive_computation() });
/// let result = task.await;
///
/// // Fire-and-forget
/// executor.spawn(async { log_analytics() }).detach();
/// ```
#[must_use = "await the task to observe its result, or use `.detach()` to discard it"]
pub struct Task<T>(TaskState<T>);

enum TaskState<T> {
    /// Task completed synchronously — value available immediately
    Ready(Option<T>),
    /// Task spawned on tokio runtime — awaiting JoinHandle
    #[cfg(not(target_arch = "wasm32"))]
    Spawned(tokio::task::JoinHandle<T>),
}

impl<T> Task<T> {
    /// Create an already-completed task
    ///
    /// Useful for returning pre-computed values from APIs that return
    /// `Task<T>`, or for testing without an async runtime.
    pub fn ready(val: T) -> Self {
        Task(TaskState::Ready(Some(val)))
    }

    /// Create a task from a tokio JoinHandle
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_handle(handle: tokio::task::JoinHandle<T>) -> Self {
        Task(TaskState::Spawned(handle))
    }

    /// Detach the task to run in the background
    ///
    /// The task continues executing but its result is discarded.
    /// This is the equivalent of "fire-and-forget".
    ///
    /// For `Ready` tasks, the value is simply dropped.
    /// For `Spawned` tasks, the JoinHandle is dropped but the underlying
    /// tokio task continues running to completion.
    pub fn detach(self) {
        // Dropping self drops the JoinHandle, but the tokio task keeps running.
        // For Ready tasks, the value is simply dropped.
        drop(self);
    }
}

// Results are moved out, never exposed through a pinned reference. The spawned
// variant contains only a JoinHandle, which is Unpin independently of T.
impl<T> Unpin for Task<T> {}

impl<T> Future for Task<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // `TaskState::Spawned` — the only arm that consults the waker — does not
        // exist on wasm32, where every Task is already `Ready`.
        #[cfg(target_arch = "wasm32")]
        let _ = cx;

        let this = self.get_mut();
        match &mut this.0 {
            TaskState::Ready(val) => {
                let val = val.take().expect("Task::Ready polled after completion");
                Poll::Ready(val)
            }
            #[cfg(not(target_arch = "wasm32"))]
            TaskState::Spawned(handle) => {
                // `tokio::task::JoinHandle<T>` carries an unconditional
                // `impl<T> Unpin for JoinHandle<T> {}` (tokio 1.53.0,
                // src/runtime/task/join.rs) — true regardless of `T`, since
                // the handle holds a `RawTask` plus `PhantomData<T>`, never
                // a borrow into `T` itself. `Pin::new` (safe) replaces what
                // used to be a justified `Pin::new_unchecked`.
                let handle = Pin::new(handle);
                match handle.poll(cx) {
                    Poll::Ready(Ok(val)) => Poll::Ready(val),
                    Poll::Ready(Err(join_error)) => {
                        // Task panicked or was cancelled
                        if join_error.is_panic() {
                            std::panic::resume_unwind(join_error.into_panic());
                        }
                        // Cancellation without an explicit `abort()` call (not exposed by
                        // this API) means the tokio runtime itself was dropped/shut down
                        // while this handle was still being polled — holding the
                        // `JoinHandle` does NOT prevent that. `Task<T>: Future<Output = T>`
                        // has no error channel to report it through, so this is a genuine
                        // invariant violation of "the runtime outlives tasks still being
                        // polled", not a scenario that can be handled by returning a value.
                        // Converting this to a typed error would mean `Output = Result<T, _>`
                        // for every `Task<T>` in the crate, touching every await site —
                        // a breaking change that needs its own dedicated review and test
                        // pass, not a drive-by edit alongside unrelated work.
                        panic!(
                            "BUG: Task was cancelled while its JoinHandle was still held — \
                             the tokio runtime was dropped/shut down while this task was \
                             still being polled"
                        );
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

impl<T> std::fmt::Debug for Task<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            TaskState::Ready(_) => f.debug_tuple("Task::Ready").finish(),
            #[cfg(not(target_arch = "wasm32"))]
            TaskState::Spawned(_) => f.debug_tuple("Task::Spawned").finish(),
        }
    }
}
