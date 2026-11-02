//! The host's background lanes (ADR-0047), as code below the host sees them.
//!
//! The host owns the pools: `flui-runtime`'s `ExecutionServices` bounds their
//! admission and cancels their work at shutdown. A capability handle reaches
//! them through an [`Executor`], so a widget runs background work inside
//! those bounds instead of starting threads of its own.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A pure background compute job: a boxed closure, run once on a worker
/// thread (or inline on wasm32).
pub type ComputeJob = Box<dyn FnOnce() + Send + 'static>;

/// A background IO future. Result delivery is the caller's business (send it
/// through a channel the caller owns); the executor only drives the future.
pub type IoFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// A wait for room on a lane ([`Executor::room`]). Resolves `Ok(())` once
/// the lane has room, which is not reserved: a spawn that follows may find
/// it taken, and waits again. Resolves `Err` when waiting cannot help: the
/// lane is shutting down, or its workers could not start. Dropping it ends
/// the wait and lets go of its waker.
#[must_use = "a wait for room does nothing unless it is awaited"]
pub struct RoomFuture(Pin<Box<dyn Future<Output = Result<(), SpawnError>> + Send + 'static>>);

impl RoomFuture {
    /// A wait that resolves as `wait` does.
    pub fn new(wait: impl Future<Output = Result<(), SpawnError>> + Send + 'static) -> Self {
        Self(Box::pin(wait))
    }

    /// A wait already over: room now, or `Err` when waiting cannot help.
    pub fn ready(answer: Result<(), SpawnError>) -> Self {
        Self::new(std::future::ready(answer))
    }
}

impl Future for RoomFuture {
    type Output = Result<(), SpawnError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}

impl fmt::Debug for RoomFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoomFuture").finish_non_exhaustive()
    }
}

/// Why a spawn was refused.
///
/// The job or future passed to the failing spawn call is dropped (its
/// destructors run); a caller that wants to retry must rebuild it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SpawnError {
    /// The lane's bounded admission window is full. Backpressure, not a bug:
    /// the caller may retry later or shed the work.
    #[error("the executor lane's bounded admission window is full")]
    Saturated,
    /// Execution services are shutting down (or already shut down); no new
    /// work is admitted.
    #[error("execution services are shutting down; new work is refused")]
    ShuttingDown,
    /// The lane's worker pool could not be started — the OS refused thread
    /// creation (resource exhaustion). An environment failure, not a bug
    /// (see `docs/PANIC-POLICY.md`): the pool slot stays unstarted, so a
    /// later spawn retries once resources free up.
    #[error(
        "the executor lane's worker pool could not be started (OS thread/resource \
         exhaustion); a later spawn may succeed"
    )]
    Unavailable,
}

/// One of the host's background lanes.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// CPU jobs ([`Executor::spawn_compute`]).
    Compute,
    /// Await-heavy futures ([`Executor::spawn_io`]).
    Io,
}

/// The host's two background lanes: compute for CPU jobs, IO for
/// await-heavy futures.
///
/// Implementations run each job and drive each future at most once, on any
/// thread, and refuse with a [`SpawnError`] rather than queue without bound.
/// A caller refused with [`SpawnError::Saturated`] waits for room with
/// [`room`](Self::room) and spawns again.
pub trait Executor: Send + Sync + 'static {
    /// Run `job` on the compute lane.
    ///
    /// # Errors
    ///
    /// The [`SpawnError`] the lane refused `job` with; `job` is dropped.
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError>;

    /// Drive `future` on the IO lane.
    ///
    /// # Errors
    ///
    /// The [`SpawnError`] the lane refused `future` with; `future` is
    /// dropped.
    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError>;

    /// Waits for `lane` to have room for one more unit of work.
    ///
    /// A lane this executor does not run resolves
    /// `Err(SpawnError::Unavailable)`.
    fn room(&self, lane: Lane) -> RoomFuture;
}
