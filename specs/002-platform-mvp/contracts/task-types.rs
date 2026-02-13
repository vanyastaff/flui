//! Task<T> and executor contract — async task abstraction with priority.
//!
//! Design contract for the implementation phase.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

// --- Priority ---

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Priority {
    High,
    #[default]
    Medium,
    Low,
}

// --- Task<T> ---

#[must_use]
pub struct Task<T>(TaskState<T>);

enum TaskState<T> {
    Ready(Option<T>),
    Spawned(tokio::task::JoinHandle<T>),
}

impl<T> Task<T> {
    /// Create an already-completed task.
    pub fn ready(val: T) -> Self;

    /// Detach the task to run in background without awaiting.
    pub fn detach(self);
}

impl<T: Send + 'static> Future for Task<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}

// --- BackgroundExecutor ---

#[derive(Clone)]
pub struct BackgroundExecutor {
    runtime: Arc<tokio::runtime::Runtime>,
}

impl BackgroundExecutor {
    pub fn new() -> Self;

    /// Spawn an async task on the thread pool.
    pub fn spawn<R: Send + 'static>(
        &self,
        future: impl Future<Output = R> + Send + 'static,
    ) -> Task<R>;

    /// Spawn with explicit priority (metadata only for now, tokio fair-schedules).
    pub fn spawn_with_priority<R: Send + 'static>(
        &self,
        priority: Priority,
        future: impl Future<Output = R> + Send + 'static,
    ) -> Task<R>;

    /// Create a timer that completes after duration.
    pub fn timer(&self, duration: std::time::Duration) -> Task<()>;

    /// Block the current thread until the future completes.
    pub fn block<R>(&self, future: impl Future<Output = R>) -> R;

    /// Access the tokio runtime handle.
    pub fn handle(&self) -> &tokio::runtime::Handle;
}

// --- ForegroundExecutor ---

#[derive(Clone)]
pub struct ForegroundExecutor {
    sender: flume::Sender<Box<dyn FnOnce() + Send>>,
    receiver: Arc<parking_lot::Mutex<flume::Receiver<Box<dyn FnOnce() + Send>>>>,
}

impl ForegroundExecutor {
    pub fn new() -> Self;

    /// Spawn a task on the main thread (via channel).
    pub fn spawn<R: 'static>(&self, future: impl Future<Output = R> + 'static) -> Task<R>;

    /// Drain all pending tasks (called from event loop).
    pub fn drain_tasks(&self);

    /// Number of pending tasks.
    pub fn pending_count(&self) -> usize;
}
