//! Async task abstraction with priority support
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
//! Priority is stored as metadata. Tokio's fair scheduler handles all priorities
//! adequately for MVP. Priority-aware thread pools are deferred to post-MVP.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Task priority level
///
/// Stored as metadata on spawned tasks. Currently informational only —
/// tokio uses fair scheduling regardless of priority. Priority-aware
/// dispatching (e.g., Windows ThreadPool, macOS GCD) is planned for post-MVP.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Priority {
    /// High priority — UI-blocking operations, user input handling
    High,
    /// Medium priority — default for most operations
    #[default]
    Medium,
    /// Low priority — background maintenance, prefetching
    Low,
}

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
#[must_use = "tasks are cancelled when dropped; use `.detach()` to run in background"]
pub struct Task<T>(TaskState<T>);

enum TaskState<T> {
    /// Task completed synchronously — value available immediately
    Ready(Option<T>),
    /// Task spawned on tokio runtime — awaiting JoinHandle
    Spawned(tokio::task::JoinHandle<T>),
}

impl<T> Task<T> {
    /// Create an already-completed task
    ///
    /// Useful for returning pre-computed values from APIs that return `Task<T>`,
    /// or for testing without an async runtime.
    pub fn ready(val: T) -> Self {
        Task(TaskState::Ready(Some(val)))
    }

    /// Create a task from a tokio JoinHandle
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

impl<T: Send + 'static> Future for Task<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: We only access the inner state through the pin, and TaskState
        // variants are safe to move when accessed through Pin projection.
        let this = unsafe { self.get_unchecked_mut() };
        match &mut this.0 {
            TaskState::Ready(val) => {
                let val = val.take().expect("Task::Ready polled after completion");
                Poll::Ready(val)
            }
            TaskState::Spawned(handle) => {
                // SAFETY: JoinHandle is Unpin, so pinning is safe
                let handle = unsafe { Pin::new_unchecked(handle) };
                match handle.poll(cx) {
                    Poll::Ready(Ok(val)) => Poll::Ready(val),
                    Poll::Ready(Err(join_error)) => {
                        // Task panicked or was cancelled
                        if join_error.is_panic() {
                            std::panic::resume_unwind(join_error.into_panic());
                        }
                        // Task was cancelled — this shouldn't happen since we hold the handle
                        panic!("Task was unexpectedly cancelled");
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
            TaskState::Spawned(_) => f.debug_tuple("Task::Spawned").finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_ready() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let task = Task::ready(42);
            assert_eq!(task.await, 42);
        });
    }

    #[test]
    fn test_task_ready_string() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let task = Task::ready("hello".to_string());
            assert_eq!(task.await, "hello");
        });
    }

    #[test]
    fn test_task_spawned() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let handle = tokio::spawn(async { 1 + 1 });
            let task = Task::from_handle(handle);
            assert_eq!(task.await, 2);
        });
    }

    #[test]
    fn test_task_detach() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag_clone = flag.clone();

            let handle = tokio::spawn(async move {
                flag_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            Task::from_handle(handle).detach();

            // Give the detached task time to complete
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
        });
    }

    #[test]
    fn test_priority_default() {
        assert_eq!(Priority::default(), Priority::Medium);
    }

    #[test]
    fn test_task_label_display() {
        let label = TaskLabel("load-fonts");
        assert_eq!(format!("{label}"), "load-fonts");
    }

    #[test]
    fn test_task_debug() {
        let task: Task<i32> = Task::ready(0);
        let debug = format!("{task:?}");
        assert!(debug.contains("Ready"));
    }
}
