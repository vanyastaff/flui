//! Background work for widgets (ADR-0047, ADR-0049): the
//! [`BackgroundCapability`] hands out a [`Background`] that runs a CPU job or
//! an IO future on the host's lanes and answers on the owner thread.
//!
//! The work runs inside the host's admission bounds and is cancelled at its
//! shutdown; the answer is a [`BackgroundTask`], which settles at an owner
//! turn boundary, never inline in the call that started it.
//!
//! ```rust,ignore
//! fn init_state(&mut self, cx: &dyn LifecycleContext) {
//!     let background = cx.capability::<BackgroundCapability>().expect("a host runs background work");
//!     let rebuild = cx.rebuild_handle();
//!     let decoded = Rc::clone(&self.decoded);
//!     background
//!         .compute(move || decode(&bytes))
//!         .on_complete(move |outcome| {
//!             *decoded.borrow_mut() = outcome.ok();
//!             rebuild.schedule(RebuildReason::StateChange);
//!         });
//! }
//! ```

use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

use flui_platform::capability::{Abandoned, Capability, Completion, CompletionSource, Resolver};
use flui_platform::executor::{ComputeJob, Executor, IoFuture, SpawnError};
use parking_lot::Mutex;

/// The background-work capability: `cx.capability::<BackgroundCapability>()`
/// hands out a [`Background`].
///
/// The framework provides it over the host's execution services; an
/// application replaces it, with an executor of its own say, through
/// `Application::capability::<BackgroundCapability>(provider)`.
#[derive(Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum BackgroundCapability {}

impl Capability for BackgroundCapability {
    type Handle = Background;
    const NAME: &'static str = "background";
}

/// The host's background lanes, as a widget holds them.
///
/// Cheap to clone; clones share the lanes. Owner-thread only: the tasks it
/// starts answer at the owner's turn boundaries.
#[derive(Clone)]
pub struct Background {
    executor: Arc<dyn Executor>,
    completions: CompletionSource,
}

impl Background {
    /// A handle that runs work on `executor` and answers through
    /// `completions`, the owner's.
    #[must_use]
    pub fn new(executor: Arc<dyn Executor>, completions: CompletionSource) -> Self {
        Self {
            executor,
            completions,
        }
    }

    /// The lanes themselves, for a crate below this one that runs its own
    /// work on them and answers through channels of its own, such as
    /// `flui-assets`' image loads. Work spawned here has none of `compute`'s
    /// and `io`'s answering at the owner's turn boundaries, cancellation or
    /// panic capture.
    #[must_use]
    pub fn executor(&self) -> &Arc<dyn Executor> {
        &self.executor
    }

    /// Run `job`, a CPU job (a decode, a large layout, a hash), on the
    /// compute lane.
    ///
    /// A job the task is cancelled before is skipped; one already running
    /// runs to its end, and its value is dropped.
    ///
    /// A panic in `job` is caught and answered as
    /// [`BackgroundError::Panicked`]. `job` is an `FnOnce` so it may consume
    /// its captures; like any Rust closure, one whose capture also panics as
    /// it drops during that unwind aborts the process.
    pub fn compute<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> BackgroundTask<T> {
        let (task, answer) = self.start::<T>();
        let refused = Arc::clone(&answer.resolver);
        let work: ComputeJob = Box::new(move || {
            if answer.cancel.is_cancelled() {
                answer.resolve(Err(BackgroundError::Cancelled));
                return;
            }
            let outcome = catch_unwind(AssertUnwindSafe(job)).map_err(panicked);
            answer.resolve_unless_cancelled(outcome);
        });
        if let Err(error) = self.executor.spawn_compute(work) {
            Answer::refuse(&refused, error);
        }
        task
    }

    /// Drive `future`, await-heavy work (a file, a request), on the IO lane.
    ///
    /// A cancelled task's future is dropped at its next await.
    pub fn io<T: Send + 'static>(
        &self,
        future: impl Future<Output = T> + Send + 'static,
    ) -> BackgroundTask<T> {
        let (task, answer) = self.start::<T>();
        let refused = Arc::clone(&answer.resolver);
        let work: IoFuture = Box::pin(Cancellable {
            future: Some(Box::pin(future)),
            answer: Some(answer),
        });
        if let Err(error) = self.executor.spawn_io(work) {
            Answer::refuse(&refused, error);
        }
        task
    }

    fn start<T: Send + 'static>(&self) -> (BackgroundTask<T>, Answer<T>) {
        let (completion, resolver) = self.completions.pending();
        let cancel = Arc::new(CancelState::default());
        let task = BackgroundTask {
            completion: Some(completion),
            cancel: Arc::clone(&cancel),
        };
        let answer = Answer {
            resolver: Arc::new(Mutex::new(Some(resolver))),
            cancel,
        };
        (task, answer)
    }
}

impl fmt::Debug for Background {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Background").finish_non_exhaustive()
    }
}

/// Why a [`BackgroundTask`] ended without a value.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BackgroundError {
    /// The lane refused the work; it never ran.
    #[error("the background lane refused the work: {0}")]
    Refused(SpawnError),
    /// The task was cancelled, or the host shut down, before the work
    /// finished.
    #[error("the background work was cancelled")]
    Cancelled,
    /// The work panicked. The panic is logged where it happened and not
    /// resumed on the owner thread.
    #[error("the background work panicked")]
    Panicked,
    /// The UI runtime ended before the answer arrived.
    #[error("the UI runtime ended before the background work answered")]
    OwnerRetired,
}

/// Background work started by a [`Background`]: its answer, once it settles
/// at an owner turn boundary.
///
/// Dropping it cancels the work: a job not yet started is skipped, and a
/// future is dropped at its next await. [`on_complete`](Self::on_complete)
/// hands the answer to a closure instead and keeps the work alive.
#[must_use = "dropping a BackgroundTask cancels its work"]
pub struct BackgroundTask<T: 'static> {
    completion: Option<Completion<Result<T, BackgroundError>>>,
    cancel: Arc<CancelState>,
}

impl<T: 'static> BackgroundTask<T> {
    /// Cancel the work; the task answers [`BackgroundError::Cancelled`]
    /// unless the work already finished.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Run `f` once with the answer, at an owner turn boundary. The work is
    /// no longer cancelled by dropping the task; the closure owns its
    /// answer, and a UI runtime that ends before the answer cancels it.
    ///
    /// `f` runs exactly once per answer: it is an `FnMut` only so that, as
    /// with `Completion::on_complete`, a panic in it keeps its captures
    /// rather than dropping them during the unwind, where a capture whose
    /// `Drop` also panics would abort the process. A UI runtime that ended
    /// before the answer settled has no turn to run it on.
    pub fn on_complete(mut self, mut f: impl FnMut(Result<T, BackgroundError>) + 'static) {
        let completion = self
            .completion
            .take()
            .expect("BUG: a BackgroundTask holds its completion until it is consumed");
        // The closure holds the work's cancellation: a UI runtime that
        // retires before the answer drops it unrun, which cancels the work.
        let cancel = CancelOnDrop(Arc::clone(&self.cancel));
        completion.on_complete(move |outcome| {
            let _cancel_if_dropped_unrun = &cancel;
            f(answer_of(outcome));
        });
    }

    /// Take the answer if it has settled: `None` while the work is pending,
    /// and after the answer was taken.
    pub fn poll(&mut self) -> Option<Result<T, BackgroundError>> {
        self.completion
            .as_mut()
            .and_then(Completion::poll)
            .map(answer_of)
    }

    /// The answer as a future, for a `FutureBuilder` or other code polled
    /// on the owner thread. Dropping the future cancels the work.
    pub fn into_future(mut self) -> impl Future<Output = Result<T, BackgroundError>> {
        let completion = self
            .completion
            .take()
            .expect("BUG: a BackgroundTask holds its completion until it is consumed");
        let cancel = CancelOnDrop(Arc::clone(&self.cancel));
        async move {
            let outcome = completion.into_future().await;
            drop(cancel);
            answer_of(outcome)
        }
    }
}

impl<T: 'static> Drop for BackgroundTask<T> {
    fn drop(&mut self) {
        // `on_complete` and `into_future` took the completion and own the
        // decision; a task dropped while still holding it is abandoned.
        if self.completion.is_some() {
            self.cancel.cancel();
        }
    }
}

impl<T: 'static> fmt::Debug for BackgroundTask<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackgroundTask")
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// The answer a settled completion carries.
fn answer_of<T>(
    outcome: Result<Result<T, BackgroundError>, Abandoned>,
) -> Result<T, BackgroundError> {
    match outcome {
        Ok(answer) => answer,
        Err(Abandoned::OwnerRetired) => Err(BackgroundError::OwnerRetired),
        // The worker's side dropped unanswered: the host discarded the work
        // unrun at shutdown.
        Err(_) => Err(BackgroundError::Cancelled),
    }
}

/// Logs a panic where the work ran and keeps its payload, whose destructor
/// may panic too, off the unwinding path. A panicking log is contained and
/// kept the same way, so reporting never stops the answer.
fn panicked(payload: Box<dyn std::any::Any + Send>) -> BackgroundError {
    let report = catch_unwind(AssertUnwindSafe(|| {
        tracing::error!(
            message = flui_foundation::panic::payload_text(payload.as_ref()).unwrap_or("<opaque>"),
            "background work panicked"
        );
    }));
    drop(payload);
    if let Err(diagnostic) = report {
        drop(diagnostic);
    }
    BackgroundError::Panicked
}

/// The cancellation a task shares with its work.
#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    /// The IO future's waker, so a cancel reaches a future parked on an
    /// await.
    waker: Mutex<Option<Waker>>,
}

impl CancelState {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let waker = self.waker.lock().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Cancels the work when the future awaiting its answer is dropped.
struct CancelOnDrop(Arc<CancelState>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Where the work's answer goes until one side takes it.
type ResolverSlot<T> = Mutex<Option<Resolver<Result<T, BackgroundError>>>>;

/// The work's side of a task: the resolver, shared with the spawn call so a
/// refusal answers it, and the cancellation.
struct Answer<T> {
    resolver: Arc<ResolverSlot<T>>,
    cancel: Arc<CancelState>,
}

impl<T: Send + 'static> Answer<T> {
    fn resolve(&self, answer: Result<T, BackgroundError>) {
        Self::settle(&self.resolver, answer);
    }

    /// Answer with what the work produced, unless the task was cancelled
    /// while it ran: a cancel wins over a value, and the value is dropped
    /// here, off the owner thread.
    fn resolve_unless_cancelled(&self, answer: Result<T, BackgroundError>) {
        if self.cancel.is_cancelled() {
            drop(answer);
            self.resolve(Err(BackgroundError::Cancelled));
        } else {
            self.resolve(answer);
        }
    }

    /// The lane dropped the work, and with it this answer's other half.
    fn refuse(resolver: &ResolverSlot<T>, error: SpawnError) {
        Self::settle(resolver, Err(BackgroundError::Refused(error)));
    }

    fn settle(resolver: &ResolverSlot<T>, answer: Result<T, BackgroundError>) {
        let resolver = resolver.lock().take();
        if let Some(resolver) = resolver {
            // An owner that retired first has nothing waiting.
            let _retired = resolver.resolve(answer);
        }
    }
}
/// An IO future that stops at its next poll once cancelled, contains its
/// panics, and answers its task.
struct Cancellable<T: 'static> {
    /// `None` once a poll panicked: the failed future is kept, never
    /// dropped, since its captures may panic as they drop.
    future: Option<Pin<Box<dyn Future<Output = T> + Send>>>,
    answer: Option<Answer<T>>,
}

impl<T: Send + 'static> Future for Cancellable<T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = &mut *self;
        let Some(answer) = this.answer.as_ref() else {
            return Poll::Ready(());
        };
        let waker = cx.waker().clone();
        // The waker this one replaces drops after the lock is released: its
        // destructor may reach a `cancel`, which takes the same lock.
        let replaced = answer.cancel.waker.lock().replace(waker);
        drop(replaced);
        let outcome = if answer.cancel.is_cancelled() {
            Err(BackgroundError::Cancelled)
        } else {
            let future = this
                .future
                .as_mut()
                .expect("BUG: a Cancellable keeps its future until it answers");
            match catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(cx))) {
                Ok(Poll::Pending) => return Poll::Pending,
                Ok(Poll::Ready(value)) => Ok(value),
                Err(payload) => {
                    std::mem::forget(this.future.take());
                    Err(panicked(payload))
                }
            }
        };
        if let Some(answer) = this.answer.take() {
            answer.resolve_unless_cancelled(outcome);
        }
        Poll::Ready(())
    }
}
