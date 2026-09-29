//! Task, worker, and service lifecycles over the unified execution services
//! (issue #558, ADR-0049).
//!
//! # Lifecycle taxonomy
//!
//! [`ExecutionServices`] (ADR-0047) answers *where* background work runs —
//! the bounded compute and IO lanes. This module answers *who owns it and
//! how it ends*. Work is classified by lifetime, and every class has an
//! explicit owner and an explicit shutdown story:
//!
//! | Class | Lifetime | Owner | Ends by |
//! |---|---|---|---|
//! | **Task** ([`TaskHandle`]) | one-shot | whoever holds the handle | completing, explicit [`TaskHandle::cancel`], or the handle dropping (cancel-on-drop) |
//! | **Worker** ([`WorkerHandle`]) | recurring, submission-driven | whoever holds the handle | the handle dropping (cancel-on-drop) |
//! | **Service** ([`ServiceDefinition`]) | application-lifetime | the loop-scoped `AppRuntime` ([`ServiceRegistry`]) | staged registry shutdown at loop exit: cancel → bounded join → evidence |
//! | **ProcessWorker** | process-external | — | **not implemented** — deliberately deferred until FLUI has process-spawning plumbing at all; nothing here pretends otherwise |
//!
//! **There is no fire-and-forget spawn.** Every unit of work has a named
//! owner: a task or worker is owned by its `#[must_use]` handle (dropping
//! the handle is itself a decision — it cancels), and a service is owned by
//! the runtime's registry, which cancels and joins it at loop exit. There
//! is deliberately no `detach()`: work that must outlive its spawn site is
//! a *service*, with a name and a registry entry, not an orphan.
//!
//! # Cancellation is cooperative, delivery is guaranteed at boundaries
//!
//! Cancellation never preempts running code. It is delivered:
//!
//! - **before start** — a cancelled task whose closure has not begun is
//!   skipped entirely;
//! - **at every await point** — an IO task's future is raced against its
//!   token and dropped (destructors run) at whatever await it had reached;
//! - **at explicit checks** — a compute closure observes
//!   [`TaskContext::is_cancelled`] at the granularity it chooses; a
//!   compute job that never checks runs to completion (and reports
//!   `Completed`).
//!
//! # Completion paths are dedicated, never shared queues
//!
//! Every task and every service reports its outcome through its own
//! capacity-one channel, created at spawn. Completion can therefore never
//! be starved, coalesced away, or blocked by a full general-purpose inbox —
//! and shutdown never waits on a queue that optional work might have
//! filled. Optional event streams ([`service_events`]) are the opposite
//! trade by design: bounded, latest-wins, lossy, and **pull-only** — a
//! publisher can never wake, mutate, or revive UI state, which is what
//! makes a late service result after realm teardown structurally inert
//! ([`PublishError::OwnerGone`]).
//!
//! # Not in this slice (stated, not silently assumed)
//!
//! - **ProcessWorker** — see the table above.
//! - **Journaled recoverable state** — issue #558's remaining criterion,
//!   a deliberately separate slice. The close-request veto that used to be
//!   named here alongside it has landed in
//!   [`close_request`](super::close_request); note that it answers a
//!   different question from [`ServiceLifetime::KeepsAppAlive`] and is
//!   consulted strictly earlier — see
//!   [`CloseRequestRouter::consult`](super::close_request::CloseRequestRouter::consult).
//! - **A widget-tier capability** (a `BuildContext`-acquired spawner per
//!   ADR-0018/0021's handle discipline) — today [`TaskSpawner`] reaches
//!   application code through [`ServiceContext`] only.
//! - **wasm32** — this module is native-only; the web runner's sequential
//!   execution model needs its own lifecycle slice.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TrySendError};
use std::sync::{Arc, Weak};
use std::task::Poll;

use tokio_util::sync::CancellationToken;
// The workspace's single monotonic-time source (root `Cargo.toml`'s TIME
// entry): `web_time::Duration` is std's `Duration` re-exported on every
// target, imported from here so this module's time vocabulary stays one
// crate even though the module itself is native-only today.
use web_time::{Duration, Instant};

use flui_runtime::execution::{ExecutionServices, SpawnError};

// ============================================================================
// Cancellation
// ============================================================================

/// A cooperative cancellation signal, scoped to one task, worker, or
/// service.
///
/// Wraps the runtime's internal cancellation machinery so the public
/// lifecycle surface stays runtime-neutral (the same reason
/// `HostExecutors` carries trait objects rather than executor types).
/// Signals form a tree: every lifecycle signal is a child of the
/// execution services' root, so full loop-exit shutdown reaches every
/// outstanding unit of work without a registry walk.
///
/// Cancellation is **level, not edge**: once cancelled, a signal stays
/// cancelled forever, and observing it late is always safe.
#[derive(Clone, Debug)]
pub struct CancellationSignal {
    token: CancellationToken,
}

impl CancellationSignal {
    fn child_of(parent: &CancellationToken) -> Self {
        Self {
            token: parent.child_token(),
        }
    }

    /// Whether cancellation has been requested (by the owner's explicit
    /// cancel, the owner's drop, or runtime shutdown).
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Resolves when cancellation is requested. Resolves immediately if it
    /// already was — safe to await at any point, any number of times.
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + '_ {
        self.token.cancelled()
    }

    fn cancel(&self) {
        self.token.cancel();
    }
}

// ============================================================================
// Panic containment for spawned work
// ============================================================================

/// Polls the inner future inside `catch_unwind`, converting a panic into
/// `Err(())` instead of unwinding into the executor. The panic payload is
/// intentionally not carried — the lifecycle layer reports *that* work
/// panicked ([`TaskOutcome::Panicked`]); the panic message itself reaches
/// the log through the panic hook at the panic site.
///
/// `F: Unpin` keeps the pin projection safe-code-only; callers box the
/// future first (they hand it to a boxed-future executor lane anyway).
struct CatchUnwind<F> {
    inner: F,
}

impl<F: Future + Unpin> Future for CatchUnwind<F> {
    type Output = Result<F::Output, ()>;

    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let inner = Pin::new(&mut self.inner);
        match std::panic::catch_unwind(AssertUnwindSafe(|| inner.poll(context))) {
            Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(_panic) => Poll::Ready(Err(())),
        }
    }
}

// ============================================================================
// Task: one-shot background work with an owning handle
// ============================================================================

/// What a joined task reports — the join *evidence*. Exhaustive on
/// purpose: these three are a complete classification of how one-shot work
/// ends, and a caller should be forced to consider each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcome<T> {
    /// The task ran to completion and produced its value.
    ///
    /// A compute task that never checks its [`TaskContext`] completes even
    /// if it was cancelled mid-run — cancellation is cooperative.
    Completed(T),
    /// The task observed cancellation and exited early, was skipped before
    /// its body started, or was dropped unrun at executor shutdown.
    Cancelled,
    /// The task's body panicked. The panic is contained at the task
    /// boundary (it does not unwind into the executor); the payload is
    /// logged at the panic site via the panic hook.
    Panicked,
}

/// Read access to a task's own cancellation state, passed to compute-task
/// and worker closures.
#[derive(Clone, Debug)]
pub struct TaskContext {
    signal: CancellationSignal,
}

impl TaskContext {
    /// Whether this task has been asked to stop. Long-running compute
    /// checks this at its natural step boundaries and returns early; the
    /// task then reports [`TaskOutcome::Cancelled`] if it exits via the
    /// spawn wrapper's pre-start check, or whatever it returns if it
    /// finishes normally after observing the flag.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.signal.is_cancelled()
    }

    /// The underlying signal, for handing to helper code.
    #[must_use]
    pub fn cancellation(&self) -> &CancellationSignal {
        &self.signal
    }
}

/// The owning handle of one spawned task.
///
/// **Ownership contract (explicit): cancel-on-drop, no detach.** Dropping
/// the handle requests cancellation — cooperatively for compute (delivered
/// at the pre-start check and at [`TaskContext`] checks), and at the next
/// await point for IO (the future is dropped, destructors run). Storing
/// the handle in a struct field *is* ownership and is the intended way to
/// keep a task alive; note that `#[must_use]` only fires on an ignored
/// return value — a handle parked in a field is a deliberate owner, not an
/// accident the lint can see. Work that must outlive every natural owner
/// is a *service* ([`ServiceDefinition`]), never an orphaned task.
///
/// Join evidence is delivered through a dedicated capacity-one completion
/// channel created at spawn — it cannot be starved or displaced by other
/// traffic, and every join wait is deadline-bounded
/// ([`Self::join_within`]); there is no unbounded blocking join.
#[must_use = "dropping a TaskHandle cancels its task; hold it, join it, or cancel it explicitly"]
pub struct TaskHandle<T> {
    name: &'static str,
    signal: CancellationSignal,
    completion: mpsc::Receiver<TaskOutcome<T>>,
}

impl<T> TaskHandle<T> {
    /// The diagnostic name given at spawn.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Request cooperative cancellation without giving up the handle. The
    /// task reports [`TaskOutcome::Cancelled`] unless its body already ran
    /// to completion.
    pub fn cancel(&self) {
        self.signal.cancel();
    }

    /// The outcome, if the task has already finished; the handle back
    /// otherwise. Never blocks.
    pub fn try_join(self) -> Result<TaskOutcome<T>, Self> {
        match self.completion.try_recv() {
            Ok(outcome) => Ok(outcome),
            Err(mpsc::TryRecvError::Empty) => Err(self),
            // The spawn wrapper reports every outcome it can observe, so a
            // dropped-without-reporting sender means the work was discarded
            // unrun (executor shutdown) — cancellation by another name.
            Err(mpsc::TryRecvError::Disconnected) => Ok(TaskOutcome::Cancelled),
        }
    }

    /// Wait for the outcome, at most `deadline`. On timeout the handle
    /// comes back in [`JoinTimeout`] — keep waiting, cancel, or drop it
    /// (which cancels). The wait is always bounded; there is deliberately
    /// no unbounded `join()`.
    pub fn join_within(self, deadline: Duration) -> Result<TaskOutcome<T>, JoinTimeout<T>> {
        match self.completion.recv_timeout(deadline) {
            Ok(outcome) => Ok(outcome),
            Err(RecvTimeoutError::Timeout) => Err(JoinTimeout { handle: self }),
            // See `try_join` on why a vanished sender means "discarded
            // unrun".
            Err(RecvTimeoutError::Disconnected) => Ok(TaskOutcome::Cancelled),
        }
    }
}

impl<T> fmt::Debug for TaskHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskHandle")
            .field("name", &self.name)
            .field("cancelled", &self.signal.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl<T> Drop for TaskHandle<T> {
    fn drop(&mut self) {
        // Cancel-on-drop: the handle IS the ownership; no owner, no task.
        // Harmless after completion (the signal is level-triggered and the
        // work already reported), so joins need no special casing.
        self.signal.cancel();
    }
}

/// [`TaskHandle::join_within`] hit its deadline; the task is still
/// running (or still queued). The handle is returned for another wait, an
/// explicit cancel, or a cancel-by-drop.
#[must_use = "the timed-out handle still owns the task; dropping it cancels"]
pub struct JoinTimeout<T> {
    /// The still-owning handle.
    pub handle: TaskHandle<T>,
}

impl<T> fmt::Debug for JoinTimeout<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinTimeout")
            .field("handle", &self.handle)
            .finish()
    }
}

// ============================================================================
// TaskSpawner: the capability that mints tasks and workers
// ============================================================================

/// The capability to spawn tasks and workers onto the loop's execution
/// services.
///
/// Never ambient: there is no global spawner. This slice vends it to
/// application code through [`ServiceContext`] only (a widget-tier
/// capability handle is deferred work — see the module doc). Holds the
/// services weakly, so a spawner outliving its loop refuses with
/// [`SpawnError::ShuttingDown`] instead of keeping dead pools alive or
/// silently dropping work.
#[derive(Clone, Debug)]
pub struct TaskSpawner {
    services: Weak<ExecutionServices>,
}

impl TaskSpawner {
    pub(crate) fn new(services: &Arc<ExecutionServices>) -> Self {
        Self {
            services: Arc::downgrade(services),
        }
    }

    fn services(&self) -> Result<Arc<ExecutionServices>, SpawnError> {
        self.services.upgrade().ok_or(SpawnError::ShuttingDown)
    }

    /// Spawn a one-shot **compute** task (a pure, possibly multi-frame CPU
    /// job) onto the bounded compute lane.
    ///
    /// The closure receives a [`TaskContext`] to observe cancellation at
    /// its own step boundaries. A panic in the closure is contained at the
    /// task boundary and reported as [`TaskOutcome::Panicked`].
    ///
    /// # Errors
    ///
    /// [`SpawnError`] when the lane refuses admission (`Saturated`), the
    /// services are shutting down or gone (`ShuttingDown`), or the pool
    /// could not start (`Unavailable`). The closure is dropped unrun.
    pub fn spawn_compute<T, F>(
        &self,
        name: &'static str,
        job: F,
    ) -> Result<TaskHandle<T>, SpawnError>
    where
        T: Send + 'static,
        F: FnOnce(&TaskContext) -> T + Send + 'static,
    {
        let services = self.services()?;
        let signal = CancellationSignal::child_of(services.cancellation_root());
        let (report, completion) = mpsc::sync_channel(1);
        let task_signal = signal.clone();
        services.spawn_compute(Box::new(move || {
            // Delivery point 1: cancelled before the body started.
            if task_signal.is_cancelled() {
                let _ = report.try_send(TaskOutcome::Cancelled);
                return;
            }
            let context = TaskContext {
                signal: task_signal,
            };
            match std::panic::catch_unwind(AssertUnwindSafe(|| job(&context))) {
                Ok(value) => {
                    let _ = report.try_send(TaskOutcome::Completed(value));
                }
                Err(_panic) => {
                    tracing::error!(
                        task = name,
                        "background compute task panicked; reporting Panicked to its owner"
                    );
                    let _ = report.try_send(TaskOutcome::Panicked);
                }
            }
        }))?;
        Ok(TaskHandle {
            name,
            signal,
            completion,
        })
    }

    /// Spawn a one-shot **IO** task (an await-heavy future) onto the IO
    /// lane.
    ///
    /// `make` receives the task's own [`CancellationSignal`] and returns
    /// the future to run. The future is raced against that signal: on
    /// cancellation it is dropped at its current await point (destructors
    /// run) and the task reports [`TaskOutcome::Cancelled`]. A panic while
    /// polling is contained and reported as [`TaskOutcome::Panicked`].
    ///
    /// # Errors
    ///
    /// [`SpawnError`] as for [`Self::spawn_compute`]; the future is
    /// dropped unrun.
    pub fn spawn_io<T, F, Fut>(
        &self,
        name: &'static str,
        make: F,
    ) -> Result<TaskHandle<T>, SpawnError>
    where
        T: Send + 'static,
        F: FnOnce(CancellationSignal) -> Fut,
        Fut: Future<Output = T> + Send + 'static,
    {
        let services = self.services()?;
        let signal = CancellationSignal::child_of(services.cancellation_root());
        let (report, completion) = mpsc::sync_channel(1);
        // Boxed so `CatchUnwind`'s safe `Unpin` projection applies; the IO
        // lane takes a boxed future anyway.
        let future: Pin<Box<dyn Future<Output = T> + Send>> = Box::pin(make(signal.clone()));
        let task_signal = signal.clone();
        services.spawn_io(Box::pin(async move {
            let raced = task_signal
                .token
                .run_until_cancelled(CatchUnwind { inner: future })
                .await;
            let outcome = match raced {
                Some(Ok(value)) => TaskOutcome::Completed(value),
                Some(Err(())) => {
                    tracing::error!(
                        task = name,
                        "background IO task panicked; reporting Panicked to its owner"
                    );
                    TaskOutcome::Panicked
                }
                None => TaskOutcome::Cancelled,
            };
            let _ = report.try_send(outcome);
        }))?;
        Ok(TaskHandle {
            name,
            signal,
            completion,
        })
    }

    /// Spawn a recurring **worker**: a reusable compute job driven by
    /// [`WorkerHandle::submit`]. See [`WorkerHandle`] for the coalescing
    /// and result contract.
    ///
    /// Nothing runs until the first submission; creating a worker claims
    /// no pool capacity.
    ///
    /// # Errors
    ///
    /// [`SpawnError::ShuttingDown`] when the services are already gone.
    pub fn spawn_worker<I, O, F>(
        &self,
        name: &'static str,
        run: F,
    ) -> Result<WorkerHandle<I, O>, SpawnError>
    where
        I: Send + 'static,
        O: Send + 'static,
        F: FnMut(I, &TaskContext) -> O + Send + 'static,
    {
        let services = self.services()?;
        let signal = CancellationSignal::child_of(services.cancellation_root());
        Ok(WorkerHandle {
            name,
            signal,
            shared: Arc::new(WorkerShared {
                pending: parking_lot::Mutex::new(None),
                latest: parking_lot::Mutex::new(None),
                pump_active: AtomicBool::new(false),
                run: parking_lot::Mutex::new(Box::new(run)),
            }),
            spawner: self.clone(),
            next_generation: 0,
        })
    }
}

// ============================================================================
// Worker: recurring compute with generation-stamped, latest-wins delivery
// ============================================================================

/// The generation stamp of one worker submission. Strictly increasing per
/// worker, in submission order — compare stamps to recognize a stale
/// result after a newer submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkerGeneration(u64);

impl WorkerGeneration {
    /// The raw counter value.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A worker's closure, boxed for storage: owned input in, owned output
/// out, with the task context for cooperative cancellation checks.
type WorkerJob<I, O> = Box<dyn FnMut(I, &TaskContext) -> O + Send>;

struct WorkerShared<I, O> {
    /// The single-slot, latest-wins inbox: a submission REPLACES an
    /// unprocessed predecessor (inputs coalesce; submit never blocks and
    /// never queues without bound).
    pending: parking_lot::Mutex<Option<(WorkerGeneration, I)>>,
    /// The single-slot, latest-wins outbox: a newer result replaces an
    /// uncollected older one (stale results drop structurally).
    latest: parking_lot::Mutex<Option<(WorkerGeneration, O)>>,
    /// Whether a pump job is scheduled or running on the compute lane.
    pump_active: AtomicBool,
    /// The worker's closure. Locked only by the single active pump.
    run: parking_lot::Mutex<WorkerJob<I, O>>,
}

/// The owning handle of one recurring worker.
///
/// **Ownership contract (explicit): cancel-on-drop, no detach** — same as
/// [`TaskHandle`]. Dropping the handle cancels the worker; an unprocessed
/// pending input is dropped with the shared state.
///
/// # Coalescing and staleness
///
/// The worker owns a capacity-one input slot and a capacity-one result
/// slot, both latest-wins:
///
/// - [`Self::submit`] **never blocks and never queues**: a submission that
///   arrives before the previous input was picked up replaces it
///   (invalidations coalesce). Every submission gets a strictly
///   increasing [`WorkerGeneration`].
/// - [`Self::try_latest`] returns the newest available result with its
///   generation; older uncollected results were already replaced. A
///   result older than the caller's latest submission is recognizable by
///   its stamp.
///
/// Results are **pull-only**: nothing wakes the UI — the owner collects at
/// its own anchor (e.g. from a post-frame callback), which is what keeps
/// cross-thread results from committing mid-frame.
///
/// The worker occupies a compute-lane admission slot only while it is
/// actually processing (the pump job exits when the input slot is empty
/// and is re-spawned by the next submission), so an idle worker costs no
/// pool capacity.
#[must_use = "dropping a WorkerHandle cancels the worker; hold it to keep submitting"]
pub struct WorkerHandle<I, O> {
    name: &'static str,
    signal: CancellationSignal,
    shared: Arc<WorkerShared<I, O>>,
    spawner: TaskSpawner,
    next_generation: u64,
}

impl<I, O> WorkerHandle<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    /// The diagnostic name given at spawn.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Submit an input, replacing any not-yet-processed predecessor
    /// (latest-wins; never blocks, never queues without bound). Returns
    /// this submission's generation stamp.
    ///
    /// # Errors
    ///
    /// [`SpawnError::ShuttingDown`] if the worker was cancelled or the
    /// services are gone; other [`SpawnError`]s if the compute lane
    /// refuses the pump job. The input is dropped on error.
    pub fn submit(&mut self, input: I) -> Result<WorkerGeneration, SpawnError> {
        if self.signal.is_cancelled() {
            return Err(SpawnError::ShuttingDown);
        }
        self.next_generation += 1;
        let generation = WorkerGeneration(self.next_generation);
        let replaced = self.shared.pending.lock().replace((generation, input));
        if let Some((stale, _)) = replaced {
            tracing::trace!(
                worker = self.name,
                stale_generation = stale.get(),
                "worker input coalesced by a newer submission (latest-wins)"
            );
        }
        self.ensure_pump()?;
        Ok(generation)
    }

    /// The newest available result and its generation, if any. Taking it
    /// empties the slot; older results were already replaced (latest-wins).
    /// Never blocks.
    pub fn try_latest(&self) -> Option<(WorkerGeneration, O)> {
        self.shared.latest.lock().take()
    }

    /// Request cooperative cancellation without giving up the handle.
    /// Subsequent [`Self::submit`] calls refuse; an in-flight pump stops
    /// at its next between-items check.
    pub fn cancel(&self) {
        self.signal.cancel();
    }

    /// Schedule a pump job if none is active. The pump drains the input
    /// slot on the compute lane and exits when idle; the
    /// `pump_active` flag plus the post-clear re-check below close the
    /// race where a submission lands between "slot observed empty" and
    /// "flag cleared".
    fn ensure_pump(&self) -> Result<(), SpawnError> {
        if self.shared.pump_active.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let services = self
            .spawner
            .services()
            .inspect_err(|_| self.shared.pump_active.store(false, Ordering::Release))?;
        let shared = Arc::clone(&self.shared);
        let signal = self.signal.clone();
        let name = self.name;
        let spawned = services.spawn_compute(Box::new(move || {
            Self::pump(&shared, &signal, name);
        }));
        if spawned.is_err() {
            self.shared.pump_active.store(false, Ordering::Release);
        }
        spawned
    }

    fn pump(shared: &Arc<WorkerShared<I, O>>, signal: &CancellationSignal, name: &'static str) {
        loop {
            // Between-items cancellation point: a cancelled worker stops
            // before touching the next input.
            if signal.is_cancelled() {
                shared.pump_active.store(false, Ordering::Release);
                return;
            }
            let item = shared.pending.lock().take();
            let Some((generation, input)) = item else {
                shared.pump_active.store(false, Ordering::Release);
                // Re-check: a submit may have raced the clear above and
                // seen `pump_active` still true (so it did not spawn).
                if shared.pending.lock().is_some()
                    && !shared.pump_active.swap(true, Ordering::AcqRel)
                {
                    continue;
                }
                return;
            };
            let context = TaskContext {
                signal: signal.clone(),
            };
            let output =
                std::panic::catch_unwind(AssertUnwindSafe(|| (shared.run.lock())(input, &context)));
            match output {
                Ok(output) => {
                    let replaced = shared.latest.lock().replace((generation, output));
                    if let Some((stale, _)) = replaced {
                        tracing::trace!(
                            worker = name,
                            stale_generation = stale.get(),
                            "uncollected worker result replaced by a newer one (latest-wins)"
                        );
                    }
                }
                Err(_panic) => {
                    tracing::error!(
                        worker = name,
                        generation = generation.get(),
                        "worker job panicked; that submission's input is dropped, the worker \
                         stays usable"
                    );
                }
            }
        }
    }
}

impl<I, O> fmt::Debug for WorkerHandle<I, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerHandle")
            .field("name", &self.name)
            .field("cancelled", &self.signal.is_cancelled())
            .field("last_generation", &self.next_generation)
            .finish_non_exhaustive()
    }
}

impl<I, O> Drop for WorkerHandle<I, O> {
    fn drop(&mut self) {
        // Cancel-on-drop; the pump observes the signal before its next
        // item. A pending input drops with the shared state.
        self.signal.cancel();
    }
}

// ============================================================================
// Service events: typed, bounded, pull-only publication
// ============================================================================

/// Why a publish was refused.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PublishError {
    /// The [`ServiceEvents`] receiver was dropped — the owner is gone and
    /// the event has nowhere to go. This is the mechanism that makes a
    /// late service result after teardown inert: publishing into a dead
    /// owner is an `Err`, never a wake, a queue, or a revived realm.
    #[error("the event receiver was dropped; the owning side is gone")]
    OwnerGone,
}

struct EventQueue<E> {
    events: parking_lot::Mutex<VecDeque<E>>,
    capacity: usize,
    /// Cleared when the [`ServiceEvents`] receiver is dropped, **under the
    /// same mutex** that guards ring mutation. An in-flight `publish` may
    /// still hold an upgraded `Arc` after that; this flag is what makes the
    /// next ring mutation observe [`PublishError::OwnerGone`] instead of
    /// silently succeeding into a queue nobody will drain.
    receiver_alive: AtomicBool,
}

/// The publishing half of a [`service_events`] channel. `Send + Sync` and
/// cheap to clone — hand it to OS callbacks, service futures, or worker
/// threads.
pub struct ServicePublisher<E> {
    name: &'static str,
    queue: Weak<EventQueue<E>>,
}

impl<E> Clone for ServicePublisher<E> {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            queue: Weak::clone(&self.queue),
        }
    }
}

impl<E> fmt::Debug for ServicePublisher<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServicePublisher")
            .field("name", &self.name)
            .field("owner_alive", &(self.queue.strong_count() > 0))
            .finish()
    }
}

impl<E: Send> ServicePublisher<E> {
    /// Publish one event. **Never waits on consumer progress**: when the
    /// ring is full the oldest unconsumed event is retired (these are
    /// optional, latest-relevant events by contract — anything that must
    /// be delivered reliably belongs on a dedicated completion path, not
    /// here). Bounded capacity is a lossy policy, not a lock-free or
    /// wait-free guarantee: the publisher still takes a short mutex, and
    /// a payload `Drop` that re-enters [`Self::publish`] is supported only
    /// because retired events are destroyed *after* that mutex is
    /// released.
    ///
    /// Nested publish from an evicted event's destructor observes the
    /// newly enqueued event already in the ring, and may itself retire
    /// that newer event if capacity is still exhausted.
    ///
    /// # Errors
    ///
    /// [`PublishError::OwnerGone`] when the receiver was already dropped at
    /// upgrade time, **or** when it dies before this call commits a ring
    /// mutation (an outer `publish` can keep the queue `Arc` alive across
    /// that window). The refused event is discarded outside the mutex.
    pub fn publish(&self, event: E) -> Result<(), PublishError> {
        let Some(queue) = self.queue.upgrade() else {
            return Err(PublishError::OwnerGone);
        };
        // Keep retired / refused payloads out of the critical section: their
        // Drop is arbitrary user code and may publish again through a cloned
        // handle. Destroying them while `events` is locked deadlocks on the
        // non-reentrant mutex (issue #1071).
        let outcome = {
            let mut events = queue.events.lock();
            if queue.receiver_alive.load(Ordering::Acquire) {
                let evicted = if events.len() == queue.capacity {
                    events.pop_front()
                } else {
                    None
                };
                events.push_back(event);
                Ok(evicted)
            } else {
                Err(event)
            }
        };
        match outcome {
            Err(refused) => {
                drop(refused);
                Err(PublishError::OwnerGone)
            }
            Ok(evicted) => {
                if evicted.is_some() {
                    tracing::trace!(
                        service = self.name,
                        "event ring full; dropped the oldest unconsumed event"
                    );
                }
                drop(evicted);
                Ok(())
            }
        }
    }
}

/// The receiving half of a [`service_events`] channel — owned by the
/// consumer, **pull-only**: events sit in the bounded ring until drained
/// at an anchor of the owner's choosing; nothing here can wake or mutate
/// UI state. Dropping this receiver clears the queue's receiver-alive
/// flag so in-flight publishers that already upgraded their `Weak` still
/// observe [`PublishError::OwnerGone`] before committing a ring mutation.
pub struct ServiceEvents<E> {
    name: &'static str,
    queue: Arc<EventQueue<E>>,
}

impl<E> Drop for ServiceEvents<E> {
    fn drop(&mut self) {
        // Must serialize with `publish`'s ring mutation: clear liveness only
        // while holding the same mutex. Otherwise a publisher can load
        // `true`, lose the race to this Drop's store, then still `push_back`
        // and return `Ok(())` into a queue nobody will drain.
        let _guard = self.queue.events.lock();
        self.queue.receiver_alive.store(false, Ordering::Release);
    }
}

impl<E> fmt::Debug for ServiceEvents<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceEvents")
            .field("name", &self.name)
            .field("pending", &self.queue.events.lock().len())
            .finish()
    }
}

impl<E> ServiceEvents<E> {
    /// The oldest pending event, if any. Never blocks.
    ///
    /// The returned value is detached from the ring before this method
    /// returns, so dropping it (including a `Drop` that re-enters
    /// [`ServicePublisher::publish`]) cannot contend with the queue mutex.
    pub fn try_next(&self) -> Option<E> {
        let mut events = self.queue.events.lock();
        let event = events.pop_front();
        drop(events);
        event
    }

    /// Every pending event, oldest first. Never blocks.
    ///
    /// Same destructor-scope guarantee as [`Self::try_next`]: the drained
    /// values leave the mutex before their `Drop` can run.
    pub fn drain(&self) -> Vec<E> {
        let mut events = self.queue.events.lock();
        let drained: Vec<E> = events.drain(..).collect();
        drop(events);
        drained
    }
}

/// A typed, bounded, latest-relevant event channel for a service to
/// publish through — geolocation fixes, connectivity changes, download
/// progress. `capacity` is clamped to at least 1.
///
/// This is deliberately **not** a completion path: it is lossy under
/// pressure (oldest events drop) and inert after the receiver dies
/// ([`PublishError::OwnerGone`]). Completion evidence travels on each
/// task's and service's own dedicated channel instead.
#[must_use]
pub fn service_events<E: Send>(
    name: &'static str,
    capacity: usize,
) -> (ServicePublisher<E>, ServiceEvents<E>) {
    let queue = Arc::new(EventQueue {
        events: parking_lot::Mutex::new(VecDeque::new()),
        capacity: capacity.max(1),
        receiver_alive: AtomicBool::new(true),
    });
    (
        ServicePublisher {
            name,
            queue: Arc::downgrade(&queue),
        },
        ServiceEvents { name, queue },
    )
}

// ============================================================================
// Service: application-lifetime work owned by the runtime
// ============================================================================

/// Whether a service's lifetime is bound to the windows or to the
/// application.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceLifetime {
    /// Editor-like: the service does not hold the application open — when
    /// the last window closes under the default exit policy, the loop exits
    /// and staged teardown cancels and joins this service. ExplicitQuit keeps
    /// the loop and service alive until an explicit quit request.
    StopsWithLastWindow,
    /// Messenger-like: while this service is running, closing the last
    /// window does not exit the loop (`AppRuntime::should_exit` consults
    /// running keep-alive services). The application ends when the service
    /// completes or the embedder quits explicitly
    /// (`Platform::quit`), and the same staged teardown then applies.
    KeepsAppAlive,
}

/// The future a service runs — its whole lifetime, cancellation observance
/// included.
pub type ServiceFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// What a service receives when started: its cancellation signal and the
/// capability to spawn tasks and workers.
///
/// The shutdown contract lives here: a service **must** observe
/// [`Self::cancellation`] and return within the registry's deadline —
/// cancel is the request, returning is the acknowledgment, and the flush
/// window in between is the service's chance to write final state. A
/// service that ignores cancellation is force-abandoned when the pools
/// close and is reported as having exceeded the deadline.
#[derive(Debug)]
pub struct ServiceContext {
    signal: CancellationSignal,
    spawner: TaskSpawner,
}

impl ServiceContext {
    /// This service's cancellation signal (clone it into the future).
    #[must_use]
    pub fn cancellation(&self) -> &CancellationSignal {
        &self.signal
    }

    /// The capability to spawn tasks and workers — the sanctioned,
    /// non-ambient route by which application code reaches the background
    /// lanes in this slice.
    #[must_use]
    pub fn spawner(&self) -> &TaskSpawner {
        &self.spawner
    }
}

/// The declaration of one application service: a name, a lifetime policy,
/// and a factory for its future.
///
/// Registered through `AppConfig::with_service` and started by the
/// bootstrap once the loop's execution services exist; owned thereafter by
/// the runtime's `ServiceRegistry`, which is the *named owner* every
/// unit of background work must have.
#[derive(Clone)]
pub struct ServiceDefinition {
    name: &'static str,
    lifetime: ServiceLifetime,
    run: Arc<dyn Fn(ServiceContext) -> ServiceFuture + Send + Sync>,
}

impl ServiceDefinition {
    /// Declare a service. `run` is called once, at start, with the
    /// service's [`ServiceContext`].
    pub fn new(
        name: &'static str,
        lifetime: ServiceLifetime,
        run: impl Fn(ServiceContext) -> ServiceFuture + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            lifetime,
            run: Arc::new(run),
        }
    }

    /// The service's diagnostic name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The declared lifetime policy.
    #[must_use]
    pub fn lifetime(&self) -> ServiceLifetime {
        self.lifetime
    }
}

impl fmt::Debug for ServiceDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceDefinition")
            .field("name", &self.name)
            .field("lifetime", &self.lifetime)
            .finish_non_exhaustive()
    }
}

/// Why a service could not be started.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ServiceStartError {
    /// The service's factory (the closure given to
    /// [`ServiceDefinition::new`]) panicked while constructing the
    /// service's future. Contained at the lifecycle boundary like every
    /// other panic here — the panic message reaches the log through the
    /// panic hook at the panic site; the service is not registered.
    #[error("the service's factory panicked while constructing its future")]
    FactoryPanicked,
    /// The execution lane refused the service's future.
    #[error(transparent)]
    Spawn(#[from] SpawnError),
}

/// How one service ended, as observed by the staged shutdown — the join
/// evidence the teardown log carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceShutdownOutcome {
    /// The service returned (either before shutdown or within the flush
    /// window after cancellation).
    Completed,
    /// The service's future panicked at some point before or during
    /// shutdown.
    Panicked,
    /// The service did not return within the shutdown deadline. It will be
    /// force-abandoned when the execution pools close right after.
    DeadlineExceeded,
    /// The executor discarded the service's future without it reporting —
    /// it never ran, or the pools closed under it. Seeing this in a
    /// correctly ordered teardown is a bug signal: services must be joined
    /// BEFORE the pools shut down.
    Abandoned,
}

/// One service's line in the [`ServiceShutdownReport`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct ServiceShutdownEntry {
    pub(crate) name: &'static str,
    pub(crate) outcome: ServiceShutdownOutcome,
}

/// The staged shutdown's evidence: one entry per registered service.
#[derive(Debug, Clone)]
pub(crate) struct ServiceShutdownReport {
    pub(crate) entries: Vec<ServiceShutdownEntry>,
}

impl ServiceShutdownReport {
    /// Whether every service completed (or had already completed).
    #[cfg(test)]
    pub(crate) fn all_completed(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.outcome == ServiceShutdownOutcome::Completed)
    }
}

/// How one service reported its own exit through its dedicated completion
/// channel.
enum ServiceExit {
    Completed,
    Panicked,
}

struct ServiceSlot {
    name: &'static str,
    lifetime: ServiceLifetime,
    signal: CancellationSignal,
    completion: mpsc::Receiver<ServiceExit>,
    /// Cached exit, once observed — the completion channel is read at most
    /// once per exit.
    finished: Option<ServiceShutdownOutcome>,
}

impl ServiceSlot {
    /// The service's exit if it has already reported one; caches the
    /// answer. Never blocks.
    fn poll_finished(&mut self) -> Option<ServiceShutdownOutcome> {
        if self.finished.is_none() {
            self.finished = match self.completion.try_recv() {
                Ok(ServiceExit::Completed) => Some(ServiceShutdownOutcome::Completed),
                Ok(ServiceExit::Panicked) => Some(ServiceShutdownOutcome::Panicked),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(ServiceShutdownOutcome::Abandoned),
            };
        }
        self.finished
    }
}

/// The runtime-owned registry of running services — the named owner of
/// every application-lifetime unit of work, and the seat of the staged
/// shutdown (stop admission → cancel all → deadline-bounded join →
/// evidence).
///
/// Loop-scoped like `ExecutionServices` itself: hot-restart reinstalls a
/// realm without touching running services; only full loop-exit teardown
/// shuts them down.
pub(crate) struct ServiceRegistry {
    accepting: bool,
    services: Vec<ServiceSlot>,
    /// Fired (from whatever worker thread the service ran on) when a
    /// `KeepsAppAlive` service reports its exit — the runner wires this to
    /// the platform's coalesced exit-policy re-evaluation request
    /// (`SharedPlatform::request_exit_policy_reevaluation`). Without it, a
    /// last-window close vetoed by a running keep-alive service would
    /// never be re-decided once that service completes: no window remains
    /// to produce the close event that consults the policy, so the veto
    /// would be permanent and the process would linger forever.
    ///
    /// A shared slot (not a value captured at `start`) so installation
    /// order cannot silently disarm the mechanism: a service started
    /// before the notifier is installed still fires it at completion.
    /// Read-and-clone at fire time, lock never held across the call.
    exit_notifier: ExitNotifierSlot,
}

/// The shared exit-notifier slot — see [`ServiceRegistry::exit_notifier`].
type ExitNotifierSlot = Arc<parking_lot::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

impl fmt::Debug for ServiceRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceRegistry")
            .field("accepting", &self.accepting)
            .field("services", &self.services.len())
            .field("exit_notifier", &self.exit_notifier.lock().is_some())
            .finish()
    }
}

impl ServiceRegistry {
    pub(crate) fn new() -> Self {
        Self {
            accepting: true,
            services: Vec::new(),
            exit_notifier: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    /// Install the keep-alive completion notifier — see the
    /// `exit_notifier` field's doc. Installed once by the bootstrap,
    /// alongside the exit-policy hook itself; a later install replaces the
    /// earlier one (the platform request it wraps is idempotent and
    /// coalesced, so which instance fires is immaterial).
    pub(crate) fn set_exit_notifier(&mut self, notifier: Arc<dyn Fn() + Send + Sync>) {
        let _prev = self.exit_notifier.lock().replace(notifier);
    }

    /// A clone of the installed notifier, for a caller that has its own
    /// reason to ask the platform to re-consult the exit policy.
    ///
    /// Cloned out rather than invoked here, because every caller must fire it
    /// with no lock and no `APP_RUNTIME` borrow held: the hook it triggers
    /// borrows `APP_RUNTIME` itself.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn exit_notifier(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.exit_notifier.lock().clone()
    }

    /// Start `definition`'s future on the IO lane and take ownership of
    /// its lifetime. The service's completion travels on its own dedicated
    /// capacity-one channel — never a shared queue — so the staged
    /// shutdown's join cannot be starved by other traffic.
    pub(crate) fn start(
        &mut self,
        definition: &ServiceDefinition,
        services: &Arc<ExecutionServices>,
    ) -> Result<(), ServiceStartError> {
        if !self.accepting {
            return Err(ServiceStartError::Spawn(SpawnError::ShuttingDown));
        }
        let signal = CancellationSignal::child_of(services.cancellation_root());
        let (report, completion) = mpsc::sync_channel(1);
        let context = ServiceContext {
            signal: signal.clone(),
            spawner: TaskSpawner::new(services),
        };
        // The factory is application code and gets the same panic
        // containment as every body this module runs: a panic here must
        // become a typed start failure, never an unwind through the
        // bootstrap (or through whatever non-bootstrap caller starts a
        // service later).
        let future = match std::panic::catch_unwind(AssertUnwindSafe(|| (definition.run)(context)))
        {
            Ok(future) => future,
            Err(_panic) => {
                tracing::error!(
                    service = definition.name,
                    "service factory panicked while constructing its future; \
                         the service is not registered"
                );
                return Err(ServiceStartError::FactoryPanicked);
            }
        };
        let name = definition.name;
        // Only a keep-alive service's exit can change the answer to "may
        // the zero-window loop end now?", so only those completions fire
        // the exit notifier — an editor-like service completing never
        // needs a re-check.
        let exit_notifier = matches!(definition.lifetime, ServiceLifetime::KeepsAppAlive)
            .then(|| Arc::clone(&self.exit_notifier));
        // Deliberately NOT raced against the service's own token here: the
        // service observes cancellation itself and uses the window between
        // cancel and the registry's deadline to flush. The hard stop is the
        // pools' own shutdown afterwards.
        services.spawn_io(Box::pin(async move {
            let exit = if (CatchUnwind { inner: future }).await.is_ok() {
                ServiceExit::Completed
            } else {
                tracing::error!(
                    service = name,
                    "service future panicked; reporting Panicked to the registry"
                );
                ServiceExit::Panicked
            };
            match report.try_send(exit) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                Err(TrySendError::Full(_)) => {
                    // Capacity one, exactly one send per service: full means
                    // a double-send, which this wrapper cannot produce.
                    tracing::error!(
                        service = name,
                        "BUG: service completion channel full on its single send"
                    );
                }
            }
            // AFTER the completion send above: the owner-thread re-check
            // this wakes reads the registry through `keeps_app_alive`,
            // which must be able to observe this very exit. Clone out of
            // the shared slot, lock released before the call.
            if let Some(notifier) = exit_notifier {
                let notify = notifier.lock().clone();
                if let Some(notify) = notify {
                    notify();
                }
            }
        }))?;
        tracing::debug!(
            service = definition.name,
            lifetime = ?definition.lifetime,
            "application service started"
        );
        self.services.push(ServiceSlot {
            name: definition.name,
            lifetime: definition.lifetime,
            signal,
            completion,
            finished: None,
        });
        Ok(())
    }

    /// Whether admission is open (no [`Self::shutdown`] since construction
    /// or the last [`Self::reopen`]). Checked before resolving execution
    /// services so a refused late start cannot resurrect a torn-down
    /// loop's pools as a side effect.
    pub(crate) fn is_accepting(&self) -> bool {
        self.accepting
    }

    /// Reopen admission after a prior loop's [`Self::shutdown`] closed it
    /// — called at realm install, the same known point that re-resolves
    /// the execution services for a second loop on this thread. A no-op
    /// when admission is already open; never touches running services.
    pub(crate) fn reopen(&mut self) {
        self.accepting = true;
    }

    /// Whether any still-running service declared
    /// [`ServiceLifetime::KeepsAppAlive`] — the messenger-like veto
    /// `AppRuntime::should_exit` consults after the last window closes.
    pub(crate) fn keeps_app_alive(&mut self) -> bool {
        self.services.iter_mut().any(|slot| {
            matches!(slot.lifetime, ServiceLifetime::KeepsAppAlive)
                && slot.poll_finished().is_none()
        })
    }

    /// The staged shutdown: stop admission, cancel **every** service
    /// first (so all flush windows overlap instead of serializing), then
    /// join each against one shared deadline. Every wait is bounded; a
    /// service that ignores cancellation costs at most the remaining
    /// deadline and is reported, never waited on unboundedly.
    ///
    /// Runs BEFORE `ExecutionServices::shutdown` at loop-exit teardown —
    /// the pools' own cancellation hard-drops any future still running, so
    /// this cooperative pass must come first or no service ever gets its
    /// flush window.
    pub(crate) fn shutdown(&mut self, deadline: Duration) -> ServiceShutdownReport {
        self.accepting = false;
        for slot in &self.services {
            slot.signal.cancel();
        }
        let join_by = Instant::now() + deadline;
        let mut entries = Vec::with_capacity(self.services.len());
        for mut slot in self.services.drain(..) {
            let outcome = if let Some(outcome) = slot.poll_finished() {
                outcome
            } else {
                let remaining = join_by.saturating_duration_since(Instant::now());
                match slot.completion.recv_timeout(remaining) {
                    Ok(ServiceExit::Completed) => ServiceShutdownOutcome::Completed,
                    Ok(ServiceExit::Panicked) => ServiceShutdownOutcome::Panicked,
                    Err(RecvTimeoutError::Timeout) => ServiceShutdownOutcome::DeadlineExceeded,
                    Err(RecvTimeoutError::Disconnected) => ServiceShutdownOutcome::Abandoned,
                }
            };
            match outcome {
                ServiceShutdownOutcome::Completed => {
                    tracing::debug!(service = slot.name, "service completed during shutdown");
                }
                ServiceShutdownOutcome::Panicked => {
                    tracing::warn!(service = slot.name, "service had panicked before shutdown");
                }
                ServiceShutdownOutcome::DeadlineExceeded => {
                    tracing::warn!(
                        service = slot.name,
                        ?deadline,
                        "service ignored cancellation past the shutdown deadline; it will be \
                         force-abandoned when the execution pools close"
                    );
                }
                ServiceShutdownOutcome::Abandoned => {
                    tracing::warn!(
                        service = slot.name,
                        "service future was discarded without reporting (executor closed \
                         under it?)"
                    );
                }
            }
            entries.push(ServiceShutdownEntry {
                name: slot.name,
                outcome,
            });
        }
        ServiceShutdownReport { entries }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use flui_runtime::execution::DeterministicExecutors;

    /// Deterministic fixture: services routed to an injected
    /// [`DeterministicExecutors`], so spawned work runs only when the test
    /// drives it.
    fn deterministic_services() -> (Arc<ExecutionServices>, DeterministicExecutors) {
        let deterministic = DeterministicExecutors::new();
        let services = Arc::new(ExecutionServices::with_limits(
            Some(deterministic.host_executors()),
            8,
            8,
        ));
        (services, deterministic)
    }

    // ── Task: completion, join evidence, deadlines ──────────────────────────

    // ── Task: cancel-on-drop and explicit cancel ────────────────────────────

    // ── Task: panic containment and shutdown interaction ────────────────────

    /// A panicking task reports `Panicked` instead of unwinding into the
    /// executor: the deterministic drive survives, and the owner gets
    /// evidence rather than silence.
    fn task_panic_is_contained_and_reported() {
        let (services, deterministic) = deterministic_services();
        let spawner = TaskSpawner::new(&services);
        let handle = spawner
            .spawn_compute("panics", |_context| -> u32 {
                panic!("task panic (expected by this test)")
            })
            .expect("spawn must be admitted");
        deterministic.run_until_idle();
        assert_eq!(
            handle.join_within(Duration::from_secs(1)).ok(),
            Some(TaskOutcome::Panicked)
        );
    }

    // ── Worker: coalescing, generations, latest-wins delivery ───────────────

    /// A panicking worker job drops that submission but leaves the worker
    /// usable for the next one.
    fn worker_survives_a_panicking_job() {
        let (services, deterministic) = deterministic_services();
        let spawner = TaskSpawner::new(&services);
        let mut worker = spawner
            .spawn_worker("flaky", |input: u32, _context| {
                assert!(input != 13, "worker panic (expected by this test)");
                input
            })
            .expect("worker must be created");
        worker.submit(13).expect("submit");
        deterministic.run_until_idle();
        assert_eq!(
            worker.try_latest(),
            None,
            "the panicked submission has no result"
        );
        let generation = worker.submit(7).expect("the worker must remain usable");
        deterministic.run_until_idle();
        assert_eq!(worker.try_latest(), Some((generation, 7)));
    }

    // ── Service events: bounded, pull-only, inert after owner death ─────────

    fn service_events_ring_drops_oldest_and_never_blocks() {
        let (publisher, events) = service_events::<u32>("positions", 3);
        for value in 0..5 {
            publisher.publish(value).expect("receiver alive");
        }
        assert_eq!(
            events.drain(),
            vec![2, 3, 4],
            "the ring must keep the newest events and drop the oldest"
        );
        assert_eq!(events.try_next(), None);
    }

    // ── Service registry: lifetime policy, staged shutdown, deadlines ───────

    fn until_cancelled_service(
        name: &'static str,
        lifetime: ServiceLifetime,
        flushed: Arc<AtomicBool>,
    ) -> ServiceDefinition {
        ServiceDefinition::new(name, lifetime, move |context| {
            let signal = context.cancellation().clone();
            let flushed = Arc::clone(&flushed);
            Box::pin(async move {
                signal.cancelled().await;
                // The flush window: state written AFTER cancellation was
                // observed but BEFORE the future returns.
                flushed.store(true, Ordering::Release);
            })
        })
    }

    /// The staged shutdown delivers cancellation and then joins: a service
    /// that waits for its signal gets its flush window and reports
    /// `Completed`. Fails if the cancel stage is skipped (the join would
    /// time out) or if the join is skipped (the flush would be unobserved).
    fn shutdown_cancels_then_joins_and_the_flush_window_is_real() {
        // Real pools: the service future must run CONCURRENTLY with the
        // registry's blocking join.
        let services = Arc::new(ExecutionServices::with_defaults());
        let mut registry = ServiceRegistry::new();
        let flushed = Arc::new(AtomicBool::new(false));
        registry
            .start(
                &until_cancelled_service(
                    "flushing",
                    ServiceLifetime::StopsWithLastWindow,
                    Arc::clone(&flushed),
                ),
                &services,
            )
            .expect("service must start");

        let report = registry.shutdown(Duration::from_secs(10));
        assert!(report.all_completed(), "report: {report:?}");
        assert!(
            flushed.load(Ordering::Acquire),
            "the service must have used its flush window before shutdown returned"
        );
        services.shutdown(Duration::from_secs(5));
    }

    #[test]
    fn service_lifecycle_matrix() {
        crate::table_test::run_table(
            "service_lifecycle_matrix",
            &[
                (
                    "task_panic_is_contained_and_reported",
                    task_panic_is_contained_and_reported as fn(),
                ),
                (
                    "worker_survives_a_panicking_job",
                    worker_survives_a_panicking_job as fn(),
                ),
                (
                    "service_events_ring_drops_oldest_and_never_blocks",
                    service_events_ring_drops_oldest_and_never_blocks as fn(),
                ),
                (
                    "shutdown_cancels_then_joins_and_the_flush_window_is_real",
                    shutdown_cancels_then_joins_and_the_flush_window_is_real as fn(),
                ),
            ],
        );
    }
}
