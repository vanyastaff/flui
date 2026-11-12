//! Async task abstraction
//!
//! Provides [`Task<T>`] — an awaitable handle to a spawned async operation.
//! Holds either a value or the receiving end of a channel that work running
//! elsewhere (a dialog's thread) answers through; no runtime is involved.
//!
//! # Design
//!
//! Based on GPUI's task pattern:
//! - `Task::ready(val)` — already-completed task (no spawn)
//! - `Task::detach()` — fire-and-forget (the work keeps running)
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
/// `Task<T>` holds either a value or the answer of work running elsewhere: a
/// host's file dialog runs on a thread of its own and answers through a
/// channel. It implements `Future` so it can be `.await`ed, and provides
/// `detach()` for fire-and-forget usage.
#[must_use = "await the task to observe its result, or use `.detach()` to discard it"]
pub struct Task<T>(TaskState<T>);

enum TaskState<T> {
    /// Task completed synchronously — value available immediately
    Ready(Option<T>),
    /// Work running elsewhere (a dialog's thread) answers through a channel;
    /// `gone` answers when the worker ended without sending.
    Answering {
        answer: tokio::sync::oneshot::Receiver<T>,
        gone: Option<T>,
    },
}

impl<T> Task<T> {
    /// Create an already-completed task
    ///
    /// Useful for returning pre-computed values from APIs that return
    /// `Task<T>`, or for testing without an async runtime.
    pub fn ready(val: T) -> Self {
        Task(TaskState::Ready(Some(val)))
    }

    /// A task answered through `answer`; `gone` is its result when the sender
    /// is dropped without sending, as when the worker thread panicked.
    #[cfg_attr(
        not(windows),
        allow(
            dead_code,
            reason = "only the Windows dialogs answer through a channel"
        )
    )]
    pub(crate) fn from_channel(answer: tokio::sync::oneshot::Receiver<T>, gone: T) -> Self {
        Task(TaskState::Answering {
            answer,
            gone: Some(gone),
        })
    }

    /// Detach the task to run in the background
    ///
    /// The work continues but its result is discarded.
    pub fn detach(self) {
        drop(self);
    }
}

// Results are moved out, never exposed through a pinned reference, and a
// `oneshot::Receiver` is `Unpin`.
impl<T> Unpin for Task<T> {}

impl<T> Future for Task<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.get_mut().0 {
            TaskState::Ready(val) => Poll::Ready(
                val.take()
                    .expect("BUG: Task::Ready polled after completion"),
            ),
            TaskState::Answering { answer, gone } => match Pin::new(answer).poll(ctx) {
                Poll::Ready(Ok(value)) => Poll::Ready(value),
                Poll::Ready(Err(_closed)) => Poll::Ready(
                    gone.take()
                        .expect("BUG: Task::Answering polled after completion"),
                ),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

impl<T> std::fmt::Debug for Task<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            TaskState::Ready(_) => f.debug_tuple("Task::Ready").finish(),
            TaskState::Answering { .. } => f.debug_tuple("Task::Answering").finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future as _,
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    use super::Task;

    fn poll<T>(task: &mut Task<T>) -> Poll<T> {
        Pin::new(task).poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn a_channel_task_answers_what_its_worker_sent_or_its_fallback() {
        let (answer, answered) = tokio::sync::oneshot::channel();
        let mut sent = Task::from_channel(answered, "gone");
        assert!(poll(&mut sent).is_pending(), "nothing sent yet");
        answer
            .send("sent")
            .expect("BUG: the task holds the receiver");
        assert_eq!(poll(&mut sent), Poll::Ready("sent"));

        let (answer, answered) = tokio::sync::oneshot::channel::<&str>();
        let mut abandoned = Task::from_channel(answered, "gone");
        std::thread::spawn(move || {
            let _answer = answer;
            panic!("a worker that panics before it answers");
        })
        .join()
        .expect_err("the worker panicked");
        assert_eq!(poll(&mut abandoned), Poll::Ready("gone"));
    }
}
