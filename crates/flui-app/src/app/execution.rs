//! Unified background execution services (issue #557, first slice).
//!
//! # Work-class model
//!
//! FLUI classifies work by **deadline and behavior**, not by a user-supplied
//! priority (see the runtime architecture execution plan and ADR-0047):
//!
//! - **Frame-required compute** never leaves the owner thread. It is the
//!   frame pipeline itself plus `flui_scheduler::AsyncDriver`'s mid-frame
//!   poll — no thread pool is involved, so no amount of background work can
//!   starve it of its executor. This module deliberately provides **no**
//!   spawn API for that class.
//! - **Asynchronous compute** ([`ExecutionServices::spawn_compute`]) is a
//!   pure, possibly multi-frame CPU job (decode, tessellation, shaping). It
//!   runs on a bounded worker pool sized to leave headroom for the owner
//!   thread and the IO lane ([`default_compute_worker_count`]).
//! - **IO** ([`ExecutionServices::spawn_io`]) is an await-heavy future (file
//!   or network traffic). It runs on a small fixed-size async runtime
//!   ([`IO_WORKER_THREADS`] workers).
//! - **Durable services** are issue #558's lifecycle work and are not part
//!   of this module yet.
//!
//! # Ownership
//!
//! `AppRuntime` (the loop-scoped composition root) owns exactly one
//! [`ExecutionServices`] value. Nothing here is ambient: there is no global
//! accessor, and library crates cannot reach these pools. An embedded host
//! that already runs its own executors injects them through
//! [`HostExecutors`] (`AppConfig::with_executors`); the default pools are
//! then **never constructed**, so FLUI and the host cannot oversubscribe the
//! machine with two full-core pools.
//!
//! # Admission, cancellation, shutdown
//!
//! Both lanes have bounded admission ([`AdmissionLimits`]): a full lane
//! refuses new work with [`SpawnError::Saturated`] instead of queueing
//! without bound. Shutdown stops admission first (later spawns get
//! [`SpawnError::ShuttingDown`]), then cancels outstanding work (IO futures
//! are dropped at their next await point via a `CancellationToken`; queued
//! compute jobs that have not started are skipped), then joins running work
//! bounded by a deadline. On wasm32 the same API is sequential: compute jobs
//! run inline at the spawn site and IO futures go to the browser's
//! microtask queue via `wasm_bindgen_futures::spawn_local`.
//!
//! # Determinism for tests
//!
//! [`DeterministicExecutors`] is a single-threaded, FIFO implementation of
//! the same host-injection seam. Tests drive it with
//! [`DeterministicExecutors::run_until_idle`]; the admission/shutdown
//! conformance tests in this module run against both the default pools and
//! the deterministic implementation, so an injected executor observes the
//! same contract.

// The spawn/admission machinery has no shipped consumer yet by design: the
// background lanes' production callers are issue #558's task/worker/service
// lifecycles (and #559's raster lane), the next items on the same critical
// path. Until those land, this module's own tests are the executable
// coverage. `expect`, not `allow`: the moment #558 wires a production
// caller, the unfulfilled expectation errors and this attribute must go.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "background-lane spawn consumers arrive with the task/worker/service \
                  lifecycle work (issue #558); until then this machinery is exercised \
                  by this module's own tests"
    )
)]

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A pure background compute job: a boxed closure, run once on a worker
/// thread (or inline on wasm32).
pub type ComputeJob = Box<dyn FnOnce() + Send + 'static>;

/// A background IO future. Result delivery is the caller's business (send it
/// through a channel the caller owns); the executor only drives the future.
pub type IoFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

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
}

/// Host-provided worker pool for the **asynchronous compute** work class.
///
/// Implementations run each job exactly once, on any thread. FLUI wraps
/// every job it hands over with its own admission accounting and
/// shutdown-cancellation check, so an implementation only needs to execute.
///
/// Return [`SpawnError::Saturated`] to refuse a job when the host's own
/// queue is full — FLUI surfaces that to its caller unchanged.
pub trait HostComputePool: Send + Sync {
    /// Queue `job` for execution.
    fn spawn_job(&self, job: ComputeJob) -> Result<(), SpawnError>;
}

/// Host-provided executor for the **IO** work class.
///
/// Implementations drive each future to completion, on any thread. FLUI
/// wraps every future it hands over with its own admission accounting and
/// cancellation (on shutdown the wrapped future resolves early), so an
/// implementation only needs to poll.
pub trait HostIoPool: Send + Sync {
    /// Queue `future` for execution.
    fn spawn_future(&self, future: IoFuture) -> Result<(), SpawnError>;
}

/// The host-injection seam: an embedded host's own executors, passed via
/// `AppConfig::with_executors`.
///
/// When present, FLUI routes all background work through these pools and
/// **never constructs its default pools** — the mechanism by which FLUI
/// embedded in a game engine or editor avoids a second full-core thread
/// pool. FLUI's bounded-admission and shutdown contracts still apply on top
/// of whatever the host provides.
#[derive(Clone)]
pub struct HostExecutors {
    compute: Arc<dyn HostComputePool>,
    io: Arc<dyn HostIoPool>,
}

impl HostExecutors {
    /// Bundle a compute pool and an IO pool provided by the host.
    pub fn new(compute: Arc<dyn HostComputePool>, io: Arc<dyn HostIoPool>) -> Self {
        Self { compute, io }
    }
}

impl fmt::Debug for HostExecutors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostExecutors").finish_non_exhaustive()
    }
}

/// Worker threads in the default IO runtime.
///
/// IO futures are await-heavy, not CPU-heavy; two workers drive a large
/// number of concurrent file/network futures. Kept const so
/// [`default_compute_worker_count`] can reserve headroom for them.
pub(crate) const IO_WORKER_THREADS: usize = 2;

/// Default compute-pool size for a machine with `available_parallelism`
/// hardware threads: everything except one hardware thread for the owner
/// (frame) thread and [`IO_WORKER_THREADS`] for the IO lane, never below 1.
///
/// This is the "background work cannot starve frame-required compute"
/// sizing half: even with every compute worker busy, the owner thread keeps
/// a hardware thread (the other half is structural — the frame lane never
/// runs on these pools at all, see the module doc).
pub(crate) fn default_compute_worker_count(available_parallelism: usize) -> usize {
    available_parallelism
        .saturating_sub(IO_WORKER_THREADS + 1)
        .max(1)
}

/// Bounded admission windows, counted as work admitted but not yet finished
/// (queued + running).
///
/// Defaults are deliberately generous — they are overload backstops, not
/// throttles. Tests inject small values to exercise refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdmissionLimits {
    /// Maximum in-flight compute jobs.
    pub(crate) compute: usize,
    /// Maximum in-flight IO futures.
    pub(crate) io: usize,
}

impl Default for AdmissionLimits {
    fn default() -> Self {
        Self {
            compute: 256,
            io: 1024,
        }
    }
}

/// One lane's admission window: an in-flight counter with a cap.
struct Admission {
    in_flight: Arc<AtomicUsize>,
    limit: usize,
}

impl Admission {
    fn new(limit: usize) -> Self {
        Self {
            in_flight: Arc::new(AtomicUsize::new(0)),
            limit,
        }
    }

    /// Try to admit one unit of work; the returned guard releases the slot
    /// when dropped (job finished, future completed/cancelled, or the
    /// wrapper itself was dropped unrun at pool shutdown).
    fn try_admit(&self) -> Result<AdmissionGuard, SpawnError> {
        let mut current = self.in_flight.load(Ordering::Relaxed);
        loop {
            if current >= self.limit {
                return Err(SpawnError::Saturated);
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Ok(AdmissionGuard {
                        in_flight: Arc::clone(&self.in_flight),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }
}

/// RAII release of one admission slot.
struct AdmissionGuard {
    in_flight: Arc<AtomicUsize>,
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The `AppRuntime`-owned execution services: both background lanes, their
/// admission windows, and the shutdown protocol. See the module doc for the
/// work-class model.
///
/// Construction is cheap; on the default backend the pools start lazily on
/// first spawn, so a run that never spawns background work never starts a
/// worker thread.
pub(crate) struct ExecutionServices {
    accepting: AtomicBool,
    compute_admission: Admission,
    io_admission: Admission,
    backend: Backend,
    #[cfg(not(target_arch = "wasm32"))]
    cancel: tokio_util::sync::CancellationToken,
}

enum Backend {
    /// Host-injected pools; the default pools are never constructed.
    Host(HostExecutors),
    /// FLUI's own default pools, lazily started.
    #[cfg(not(target_arch = "wasm32"))]
    Default(DefaultPools),
    /// wasm32 sequential execution: compute inline, IO via
    /// `wasm_bindgen_futures::spawn_local`.
    #[cfg(target_arch = "wasm32")]
    Sequential,
}

/// A default pool's lifecycle: not yet started, running, or shut down.
#[cfg(not(target_arch = "wasm32"))]
enum PoolSlot {
    NotStarted,
    Running(tokio::runtime::Runtime),
    Closed,
}

#[cfg(not(target_arch = "wasm32"))]
impl PoolSlot {
    fn take_runtime(&mut self) -> Option<tokio::runtime::Runtime> {
        match std::mem::replace(self, PoolSlot::Closed) {
            PoolSlot::Running(runtime) => Some(runtime),
            PoolSlot::NotStarted | PoolSlot::Closed => None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct DefaultPools {
    io: parking_lot::Mutex<PoolSlot>,
    compute: parking_lot::Mutex<PoolSlot>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DefaultPools {
    fn new() -> Self {
        Self {
            io: parking_lot::Mutex::new(PoolSlot::NotStarted),
            compute: parking_lot::Mutex::new(PoolSlot::NotStarted),
        }
    }

    /// Handle to the IO runtime, starting it on first use. `None` once the
    /// slot is closed by shutdown.
    fn io_handle(&self) -> Option<tokio::runtime::Handle> {
        Self::handle_of(&self.io, || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(IO_WORKER_THREADS)
                .thread_name("flui-io")
                .enable_all()
                .build()
        })
    }

    /// Handle to the compute runtime, starting it on first use. `None` once
    /// the slot is closed by shutdown.
    fn compute_handle(&self) -> Option<tokio::runtime::Handle> {
        Self::handle_of(&self.compute, || {
            let workers = default_compute_worker_count(
                std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get),
            );
            // No `enable_all`: compute jobs are synchronous closures and
            // need neither the IO nor the time driver.
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .thread_name("flui-compute")
                .build()
        })
    }

    fn handle_of(
        slot: &parking_lot::Mutex<PoolSlot>,
        build: impl FnOnce() -> std::io::Result<tokio::runtime::Runtime>,
    ) -> Option<tokio::runtime::Handle> {
        let mut slot = slot.lock();
        match &*slot {
            PoolSlot::Running(runtime) => Some(runtime.handle().clone()),
            PoolSlot::Closed => None,
            PoolSlot::NotStarted => {
                let runtime = build().expect(
                    "BUG: building a tokio runtime must succeed on any platform \
                     this crate targets short of OS thread exhaustion",
                );
                let handle = runtime.handle().clone();
                *slot = PoolSlot::Running(runtime);
                Some(handle)
            }
        }
    }

    /// Whether either default pool has actually started worker threads.
    #[cfg(test)]
    fn any_started(&self) -> bool {
        matches!(&*self.io.lock(), PoolSlot::Running(_))
            || matches!(&*self.compute.lock(), PoolSlot::Running(_))
    }
}

impl ExecutionServices {
    /// Services backed by FLUI's own default pools (lazily started).
    pub(crate) fn with_defaults() -> Self {
        Self::build(None, AdmissionLimits::default())
    }

    /// Services backed by host-injected pools; the default pools are never
    /// constructed.
    pub(crate) fn with_host(host: HostExecutors) -> Self {
        Self::build(Some(host), AdmissionLimits::default())
    }

    /// Test seam: custom admission windows (both backends).
    #[cfg(test)]
    pub(crate) fn with_limits(host: Option<HostExecutors>, limits: AdmissionLimits) -> Self {
        Self::build(host, limits)
    }

    fn build(host: Option<HostExecutors>, limits: AdmissionLimits) -> Self {
        let backend = match host {
            Some(host) => Backend::Host(host),
            #[cfg(not(target_arch = "wasm32"))]
            None => Backend::Default(DefaultPools::new()),
            #[cfg(target_arch = "wasm32")]
            None => Backend::Sequential,
        };
        Self {
            accepting: AtomicBool::new(true),
            compute_admission: Admission::new(limits.compute),
            io_admission: Admission::new(limits.io),
            backend,
            #[cfg(not(target_arch = "wasm32"))]
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    /// Whether this instance routes to host-injected pools (`false`) or owns
    /// the default pools (`true`).
    pub(crate) fn owns_default_pools(&self) -> bool {
        !matches!(self.backend, Backend::Host(_))
    }

    /// Spawn an **asynchronous compute** job (see the module doc's
    /// work-class model). Fire-and-forget: result delivery is the caller's
    /// business.
    pub(crate) fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(SpawnError::ShuttingDown);
        }
        let guard = self.compute_admission.try_admit()?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let cancel = self.cancel.clone();
            let wrapped: ComputeJob = Box::new(move || {
                let _slot = guard;
                // A job still queued when shutdown began is skipped; a job
                // already running is joined by the shutdown deadline instead.
                if cancel.is_cancelled() {
                    return;
                }
                job();
            });
            match &self.backend {
                Backend::Host(host) => host.compute.spawn_job(wrapped),
                Backend::Default(pools) => {
                    let Some(handle) = pools.compute_handle() else {
                        return Err(SpawnError::ShuttingDown);
                    };
                    handle.spawn(async move { wrapped() });
                    Ok(())
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let wrapped: ComputeJob = Box::new(move || {
                let _slot = guard;
                job();
            });
            match &self.backend {
                Backend::Host(host) => host.compute.spawn_job(wrapped),
                // Sequential execution: no threads exist on this target, so
                // the job runs inline at the spawn site.
                Backend::Sequential => {
                    wrapped();
                    Ok(())
                }
            }
        }
    }

    /// Spawn an **IO** future (see the module doc's work-class model).
    /// Fire-and-forget: result delivery is the caller's business. On
    /// shutdown the future is cancelled (dropped) at its next await point.
    pub(crate) fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(SpawnError::ShuttingDown);
        }
        let guard = self.io_admission.try_admit()?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let cancel = self.cancel.clone();
            let wrapped: IoFuture = Box::pin(async move {
                let _slot = guard;
                // Cancellation point: shutdown resolves this early, dropping
                // `future` (and running its destructors) at whatever await
                // point it had reached. Load-bearing especially for HOST
                // pools, whose tasks FLUI cannot drop any other way — see
                // `shutdown_cancels_work_handed_to_host_pools`.
                let _ = cancel.run_until_cancelled(future).await;
            });
            match &self.backend {
                Backend::Host(host) => host.io.spawn_future(wrapped),
                Backend::Default(pools) => {
                    let Some(handle) = pools.io_handle() else {
                        return Err(SpawnError::ShuttingDown);
                    };
                    handle.spawn(wrapped);
                    Ok(())
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let wrapped: IoFuture = Box::pin(async move {
                let _slot = guard;
                future.await;
            });
            match &self.backend {
                Backend::Host(host) => host.io.spawn_future(wrapped),
                // Browser microtask queue; single-threaded, uncancellable
                // once queued — shutdown on wasm stops admission only.
                Backend::Sequential => {
                    wasm_bindgen_futures::spawn_local(wrapped);
                    Ok(())
                }
            }
        }
    }

    /// Work admitted but not yet finished, per lane, `(compute, io)`.
    /// Diagnostics only — a snapshot, immediately stale.
    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> (usize, usize) {
        (
            self.compute_admission.in_flight(),
            self.io_admission.in_flight(),
        )
    }

    /// Whether either default pool has started worker threads. Always
    /// `false` with host-injected pools — the probe behind the
    /// "host injection avoids duplicate pools" evidence.
    #[cfg(test)]
    pub(crate) fn default_pools_started(&self) -> bool {
        match &self.backend {
            Backend::Host(_) => false,
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Default(pools) => pools.any_started(),
            #[cfg(target_arch = "wasm32")]
            Backend::Sequential => false,
        }
    }

    /// Shut down: stop admission, cancel outstanding work, then join running
    /// work, waiting at most `grace` per pool.
    ///
    /// After this returns every spawn refuses with
    /// [`SpawnError::ShuttingDown`]. Idempotent. With host-injected pools
    /// only the first two stages apply — FLUI cancels the work it wrapped,
    /// but joining the host's threads is the host's own lifecycle.
    ///
    /// `grace` bounds the wait for **running** work (an IO future between
    /// await points, a compute closure mid-run); work that has not started
    /// is dropped unrun, which is the cancellation stage doing its job.
    pub(crate) fn shutdown(&self, grace: std::time::Duration) {
        self.accepting.store(false, Ordering::Release);

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.cancel.cancel();
            if let Backend::Default(pools) = &self.backend {
                let io = pools.io.lock().take_runtime();
                let compute = pools.compute.lock().take_runtime();
                if let Some(runtime) = io {
                    runtime.shutdown_timeout(grace);
                }
                if let Some(runtime) = compute {
                    runtime.shutdown_timeout(grace);
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            // Sequential target: nothing to cancel or join — compute ran
            // inline and queued microtasks cannot be revoked. `grace` is
            // deliberately unused.
            let _ = grace;
        }
    }
}

impl fmt::Debug for ExecutionServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionServices")
            .field("accepting", &self.accepting.load(Ordering::Relaxed))
            .field("owns_default_pools", &self.owns_default_pools())
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for ExecutionServices {
    fn drop(&mut self) {
        // Last-resort teardown for the path that never ran the explicit
        // `shutdown` (a panic mid-teardown, a test dropping `AppRuntime`).
        // `shutdown_background` never blocks, so — unlike `Runtime`'s own
        // blocking `Drop` — this is safe even if the drop happens from
        // inside a task running on some other runtime.
        self.accepting.store(false, Ordering::Release);
        self.cancel.cancel();
        if let Backend::Default(pools) = &self.backend {
            let io = pools.io.lock().take_runtime();
            let compute = pools.compute.lock().take_runtime();
            if let Some(runtime) = io {
                runtime.shutdown_background();
            }
            if let Some(runtime) = compute {
                runtime.shutdown_background();
            }
        }
    }
}

// ============================================================================
// Deterministic executors (the injected reference implementation)
// ============================================================================

/// A deterministic, single-threaded implementation of the host-injection
/// seam, for tests and for hosts that need reproducible execution.
///
/// Work is queued at spawn and executed only inside
/// [`run_until_idle`](Self::run_until_idle), on the calling thread, in FIFO
/// spawn order (all queued compute jobs run before IO futures are polled in
/// each pass). Doubles as the reference implementation proving the
/// [`HostComputePool`]/[`HostIoPool`] contract is implementable outside
/// FLUI.
#[derive(Clone, Default)]
pub struct DeterministicExecutors {
    inner: Arc<DeterministicShared>,
}

#[derive(Default)]
struct DeterministicShared {
    jobs: parking_lot::Mutex<std::collections::VecDeque<ComputeJob>>,
    tasks: parking_lot::Mutex<Vec<DeterministicTask>>,
}

struct DeterministicTask {
    /// Absent while being polled (moved out so the lock is not held across
    /// user code — a polled future may spawn).
    future: Option<IoFuture>,
    /// Set by the waker; a task is polled only while this is `true`.
    ready: Arc<AtomicBool>,
}

/// Waker payload: flips the task's `ready` flag; `run_until_idle` picks the
/// task up on its next pass.
struct DeterministicWaker {
    ready: Arc<AtomicBool>,
}

impl std::task::Wake for DeterministicWaker {
    fn wake(self: Arc<Self>) {
        self.ready.store(true, Ordering::Release);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
    }
}

impl DeterministicExecutors {
    /// An empty deterministic executor pair.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The [`HostExecutors`] bundle routing both work classes to this
    /// executor — pass it to `AppConfig::with_executors`.
    #[must_use]
    pub fn host_executors(&self) -> HostExecutors {
        HostExecutors::new(Arc::new(self.clone()), Arc::new(self.clone()))
    }

    /// Run queued work on the calling thread until nothing is runnable:
    /// executes every queued compute job, then polls every woken IO future,
    /// repeating until a full pass makes no progress. Futures that returned
    /// `Pending` without an intervening wake stay parked (they are not spun
    /// on). Returns the number of jobs run plus polls made.
    pub fn run_until_idle(&self) -> usize {
        let mut steps = 0;
        loop {
            let mut progressed = false;

            // Compute jobs first, FIFO.
            while let Some(job) = self.inner.jobs.lock().pop_front() {
                job();
                steps += 1;
                progressed = true;
            }

            // Then one poll pass over every woken future, in spawn order.
            let woken: Vec<usize> = {
                let tasks = self.inner.tasks.lock();
                tasks
                    .iter()
                    .enumerate()
                    .filter(|(_, task)| task.ready.load(Ordering::Acquire))
                    .map(|(index, _)| index)
                    .collect()
            };
            for index in woken {
                let Some((mut future, ready)) = ({
                    let mut tasks = self.inner.tasks.lock();
                    tasks.get_mut(index).and_then(|task| {
                        // Clear before polling so a wake during the poll
                        // re-arms rather than being swallowed.
                        task.ready.store(false, Ordering::Release);
                        task.future
                            .take()
                            .map(|future| (future, Arc::clone(&task.ready)))
                    })
                }) else {
                    continue;
                };

                let waker = std::task::Waker::from(Arc::new(DeterministicWaker {
                    ready: Arc::clone(&ready),
                }));
                let mut context = std::task::Context::from_waker(&waker);
                let poll = future.as_mut().poll(&mut context);
                steps += 1;
                progressed = true;

                let mut tasks = self.inner.tasks.lock();
                match poll {
                    std::task::Poll::Ready(()) => {
                        if let Some(task) = tasks.get_mut(index) {
                            // Leave a completed slot in place (indices stay
                            // stable within the pass); it is compacted below.
                            task.future = None;
                            task.ready.store(false, Ordering::Release);
                        }
                    }
                    std::task::Poll::Pending => {
                        if let Some(task) = tasks.get_mut(index) {
                            task.future = Some(future);
                        }
                    }
                }
            }

            // Compact completed slots between passes (a slot with no future
            // and no pending re-queue is done).
            self.inner.tasks.lock().retain(|task| task.future.is_some());

            if !progressed {
                return steps;
            }
        }
    }

    /// Queued-but-unfinished work, `(jobs, futures)`.
    #[must_use]
    pub fn pending(&self) -> (usize, usize) {
        (self.inner.jobs.lock().len(), self.inner.tasks.lock().len())
    }
}

impl fmt::Debug for DeterministicExecutors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (jobs, futures) = self.pending();
        f.debug_struct("DeterministicExecutors")
            .field("pending_jobs", &jobs)
            .field("pending_futures", &futures)
            .finish()
    }
}

impl HostComputePool for DeterministicExecutors {
    fn spawn_job(&self, job: ComputeJob) -> Result<(), SpawnError> {
        self.inner.jobs.lock().push_back(job);
        Ok(())
    }
}

impl HostIoPool for DeterministicExecutors {
    fn spawn_future(&self, future: IoFuture) -> Result<(), SpawnError> {
        self.inner.tasks.lock().push(DeterministicTask {
            future: Some(future),
            // Starts ready: a fresh future needs its first poll.
            ready: Arc::new(AtomicBool::new(true)),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// Pool-sizing policy: at least one worker, and on any machine with
    /// enough hardware threads for the split (>= 4), the background lanes
    /// together leave the owner thread a hardware thread of its own.
    #[test]
    fn compute_pool_sizing_leaves_owner_thread_headroom() {
        assert_eq!(default_compute_worker_count(1), 1);
        assert_eq!(default_compute_worker_count(2), 1);
        assert_eq!(default_compute_worker_count(3), 1);
        assert_eq!(default_compute_worker_count(4), 1);
        assert_eq!(default_compute_worker_count(8), 5);
        assert_eq!(default_compute_worker_count(32), 29);
        for available in 4..=128 {
            let compute = default_compute_worker_count(available);
            assert!(
                compute + IO_WORKER_THREADS < available,
                "with {available} hardware threads, compute={compute} + io={IO_WORKER_THREADS} \
                 must leave the owner thread headroom"
            );
        }
    }

    // ── conformance suite: the same contract for default and injected ──────
    //
    // Each check runs against BOTH backends: FLUI's default pools and the
    // injected `DeterministicExecutors` — the "an injected deterministic
    // executor passes the same tests" acceptance evidence. `drive` makes
    // queued deterministic work actually run; it is a no-op for the default
    // pools (their worker threads run the work themselves).

    fn both_backends(check: impl Fn(&ExecutionServices, &dyn Fn())) {
        let default_services =
            ExecutionServices::with_limits(None, AdmissionLimits { compute: 4, io: 4 });
        check(&default_services, &|| {});
        default_services.shutdown(Duration::from_secs(5));

        let deterministic = DeterministicExecutors::new();
        let injected = ExecutionServices::with_limits(
            Some(deterministic.host_executors()),
            AdmissionLimits { compute: 4, io: 4 },
        );
        let driver = deterministic.clone();
        check(&injected, &move || {
            driver.run_until_idle();
        });
    }

    #[test]
    fn conformance_spawned_compute_job_runs() {
        both_backends(|services, drive| {
            let ran = Arc::new(AtomicBool::new(false));
            let ran_for_job = Arc::clone(&ran);
            services
                .spawn_compute(Box::new(move || {
                    ran_for_job.store(true, Ordering::Release);
                }))
                .expect("spawn must be admitted");
            drive();
            // Default pools run on worker threads; wait bounded.
            wait_until(|| ran.load(Ordering::Acquire));
            assert!(ran.load(Ordering::Acquire));
        });
    }

    #[test]
    fn conformance_spawned_io_future_completes() {
        both_backends(|services, drive| {
            let done = Arc::new(AtomicBool::new(false));
            let done_for_future = Arc::clone(&done);
            services
                .spawn_io(Box::pin(async move {
                    done_for_future.store(true, Ordering::Release);
                }))
                .expect("spawn must be admitted");
            drive();
            wait_until(|| done.load(Ordering::Acquire));
            assert!(done.load(Ordering::Acquire));
        });
    }

    #[test]
    fn conformance_no_admission_after_shutdown() {
        both_backends(|services, _drive| {
            services.shutdown(Duration::from_secs(5));
            let result = services.spawn_compute(Box::new(|| {}));
            assert_eq!(result, Err(SpawnError::ShuttingDown));
            let result = services.spawn_io(Box::pin(async {}));
            assert_eq!(result, Err(SpawnError::ShuttingDown));
        });
    }

    #[test]
    fn conformance_admission_is_bounded_and_released() {
        // Deterministic backend: queued work does not run until driven, so
        // the admission window fills deterministically.
        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_limits(
            Some(deterministic.host_executors()),
            AdmissionLimits { compute: 2, io: 2 },
        );

        for _ in 0..2 {
            services
                .spawn_compute(Box::new(|| {}))
                .expect("within the admission window");
        }
        assert_eq!(
            services.spawn_compute(Box::new(|| {})),
            Err(SpawnError::Saturated),
            "the third job must be refused, not queued without bound"
        );

        for _ in 0..2 {
            services
                .spawn_io(Box::pin(async {}))
                .expect("within the admission window");
        }
        assert_eq!(
            services.spawn_io(Box::pin(async {})),
            Err(SpawnError::Saturated)
        );

        // Running the queued work releases the slots: admission recovers.
        deterministic.run_until_idle();
        assert_eq!(services.in_flight(), (0, 0));
        services
            .spawn_compute(Box::new(|| {}))
            .expect("slots must be released after completion");
        services
            .spawn_io(Box::pin(async {}))
            .expect("slots must be released after completion");
    }

    /// Bounded admission on the DEFAULT pools too: park the compute workers
    /// on a gate, fill the window, and assert refusal while full.
    #[test]
    fn default_pool_admission_saturates_while_workers_are_parked() {
        let services = ExecutionServices::with_limits(None, AdmissionLimits { compute: 2, io: 2 });
        let gate = Arc::new(AtomicBool::new(false));
        for _ in 0..2 {
            let gate = Arc::clone(&gate);
            services
                .spawn_compute(Box::new(move || {
                    while !gate.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }))
                .expect("within the admission window");
        }
        assert_eq!(
            services.spawn_compute(Box::new(|| {})),
            Err(SpawnError::Saturated)
        );
        gate.store(true, Ordering::Release);
        wait_until(|| services.in_flight().0 == 0);
        services
            .spawn_compute(Box::new(|| {}))
            .expect("slots must be released after the gated jobs finish");
        services.shutdown(Duration::from_secs(5));
    }

    /// Shutdown joins a RUNNING compute job: the job observably finishes
    /// before `shutdown` returns.
    #[test]
    fn shutdown_joins_running_compute_work() {
        let services = ExecutionServices::with_defaults();
        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let started_for_job = Arc::clone(&started);
        let finished_for_job = Arc::clone(&finished);
        services
            .spawn_compute(Box::new(move || {
                started_for_job.store(true, Ordering::Release);
                std::thread::sleep(Duration::from_millis(100));
                finished_for_job.store(true, Ordering::Release);
            }))
            .expect("spawn must be admitted");
        wait_until(|| started.load(Ordering::Acquire));

        services.shutdown(Duration::from_secs(10));
        assert!(
            finished.load(Ordering::Acquire),
            "shutdown must join running work, not abandon it"
        );
    }

    /// Shutdown CANCELS a parked IO future promptly: a future that would
    /// otherwise never resolve is dropped at its await point (destructors
    /// run) and shutdown does not burn the whole grace deadline on it.
    #[test]
    fn shutdown_cancels_parked_io_future_without_burning_the_deadline() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let services = ExecutionServices::with_defaults();
        let dropped = Arc::new(AtomicBool::new(false));
        let polled = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(Arc::clone(&dropped));
        let polled_for_future = Arc::clone(&polled);
        services
            .spawn_io(Box::pin(async move {
                let _flag = flag;
                polled_for_future.store(true, Ordering::Release);
                std::future::pending::<()>().await;
            }))
            .expect("spawn must be admitted");
        wait_until(|| polled.load(Ordering::Acquire));

        let grace = Duration::from_secs(30);
        let before = std::time::Instant::now();
        services.shutdown(grace);
        let elapsed = before.elapsed();
        assert!(
            dropped.load(Ordering::Acquire),
            "cancellation must drop the parked future (destructors run)"
        );
        assert!(
            elapsed < Duration::from_secs(15),
            "a cancelled future must not burn the grace deadline; took {elapsed:?}"
        );
    }

    /// Cancellation must work on pools FLUI does not own: with host-injected
    /// executors, shutdown cannot drop the host's tasks — the wrapper's
    /// cancellation point is the only mechanism. After `shutdown`, the next
    /// drive of the host executor must resolve a parked future (its
    /// destructors run) instead of leaving it parked forever.
    #[test]
    fn shutdown_cancels_work_handed_to_host_pools() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_host(deterministic.host_executors());
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(Arc::clone(&dropped));
        services
            .spawn_io(Box::pin(async move {
                let _flag = flag;
                std::future::pending::<()>().await;
            }))
            .expect("spawn must be admitted");

        deterministic.run_until_idle();
        assert_eq!(
            deterministic.pending(),
            (0, 1),
            "the future is parked in the host pool"
        );
        assert!(!dropped.load(Ordering::Acquire));

        services.shutdown(Duration::from_secs(5));
        deterministic.run_until_idle();
        assert_eq!(
            deterministic.pending(),
            (0, 0),
            "cancellation must resolve the parked future in the host pool"
        );
        assert!(
            dropped.load(Ordering::Acquire),
            "cancellation must drop the parked future (destructors run)"
        );
        assert_eq!(
            services.in_flight(),
            (0, 0),
            "the admission slot is released"
        );
    }

    /// Host injection: work routes to the host pools and the default pools
    /// are never constructed — no second set of worker threads.
    #[test]
    fn host_injection_routes_work_and_never_starts_default_pools() {
        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_host(deterministic.host_executors());
        assert!(!services.owns_default_pools());

        let compute_ran = Arc::new(AtomicBool::new(false));
        let io_ran = Arc::new(AtomicBool::new(false));
        let compute_flag = Arc::clone(&compute_ran);
        let io_flag = Arc::clone(&io_ran);
        services
            .spawn_compute(Box::new(move || {
                compute_flag.store(true, Ordering::Release);
            }))
            .expect("spawn must be admitted");
        services
            .spawn_io(Box::pin(async move {
                io_flag.store(true, Ordering::Release);
            }))
            .expect("spawn must be admitted");

        assert!(
            !compute_ran.load(Ordering::Acquire) && !io_ran.load(Ordering::Acquire),
            "deterministic host work must not run before it is driven"
        );
        deterministic.run_until_idle();
        assert!(compute_ran.load(Ordering::Acquire));
        assert!(io_ran.load(Ordering::Acquire));
        assert!(
            !services.default_pools_started(),
            "host injection must never construct the default pools"
        );
    }

    /// Default pools are lazy: constructing the services starts no worker
    /// threads; the first spawn on a lane starts exactly that lane.
    #[test]
    fn default_pools_start_lazily_on_first_spawn() {
        let services = ExecutionServices::with_defaults();
        assert!(
            !services.default_pools_started(),
            "construction must not start worker threads"
        );
        services
            .spawn_compute(Box::new(|| {}))
            .expect("spawn must be admitted");
        assert!(services.default_pools_started());
        services.shutdown(Duration::from_secs(5));
    }

    /// The frame lane needs no pool: with the compute admission window full
    /// and every worker parked, frame-thread work (an `AsyncDriver` poll on
    /// this thread) still completes immediately. This pins the structural
    /// half of "background work cannot starve frame-required compute" — the
    /// sizing half is `compute_pool_sizing_leaves_owner_thread_headroom`.
    #[test]
    fn frame_lane_makes_progress_while_background_lanes_are_saturated() {
        let services = ExecutionServices::with_limits(None, AdmissionLimits { compute: 2, io: 2 });
        let gate = Arc::new(AtomicBool::new(false));
        for _ in 0..2 {
            let gate = Arc::clone(&gate);
            services
                .spawn_compute(Box::new(move || {
                    while !gate.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }))
                .expect("within the admission window");
        }
        assert_eq!(
            services.spawn_compute(Box::new(|| {})),
            Err(SpawnError::Saturated),
            "background lane is saturated"
        );

        // Frame-lane work: an AsyncDriver task polled on THIS thread.
        let driver = flui_scheduler::AsyncDriver::new();
        let done = Arc::new(AtomicBool::new(false));
        let done_for_task = Arc::clone(&done);
        let _token = driver.spawn_local(Box::pin(async move {
            done_for_task.store(true, Ordering::Release);
        }));
        assert_eq!(driver.poll_ready(), 1);
        assert!(
            done.load(Ordering::Acquire),
            "the frame lane must complete without waiting for pool capacity"
        );

        gate.store(true, Ordering::Release);
        services.shutdown(Duration::from_secs(5));
    }

    /// Shutdown is idempotent and a shutdown'd default backend refuses work
    /// even if a stale caller re-checks after the pools are closed.
    #[test]
    fn shutdown_is_idempotent() {
        let services = ExecutionServices::with_defaults();
        services
            .spawn_compute(Box::new(|| {}))
            .expect("spawn must be admitted");
        services.shutdown(Duration::from_secs(5));
        services.shutdown(Duration::from_secs(5));
        assert_eq!(
            services.spawn_io(Box::pin(async {})),
            Err(SpawnError::ShuttingDown)
        );
    }

    // ── deterministic executor semantics ────────────────────────────────────

    #[test]
    fn deterministic_runs_in_fifo_spawn_order() {
        let deterministic = DeterministicExecutors::new();
        let order = Arc::new(parking_lot::Mutex::new(Vec::new()));
        for index in 0..4 {
            let order = Arc::clone(&order);
            deterministic
                .spawn_job(Box::new(move || order.lock().push(index)))
                .expect("deterministic spawn is unbounded");
        }
        deterministic.run_until_idle();
        assert_eq!(*order.lock(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn deterministic_parks_pending_futures_until_woken() {
        let deterministic = DeterministicExecutors::new();
        let polls = Arc::new(AtomicUsize::new(0));
        let finish = Arc::new(AtomicBool::new(false));
        let waker_slot: Arc<parking_lot::Mutex<Option<std::task::Waker>>> =
            Arc::new(parking_lot::Mutex::new(None));

        let polls_for_future = Arc::clone(&polls);
        let finish_for_future = Arc::clone(&finish);
        let waker_for_future = Arc::clone(&waker_slot);
        deterministic
            .spawn_future(Box::pin(std::future::poll_fn(move |context| {
                polls_for_future.fetch_add(1, Ordering::Relaxed);
                if finish_for_future.load(Ordering::Acquire) {
                    std::task::Poll::Ready(())
                } else {
                    *waker_for_future.lock() = Some(context.waker().clone());
                    std::task::Poll::Pending
                }
            })))
            .expect("deterministic spawn is unbounded");

        deterministic.run_until_idle();
        assert_eq!(polls.load(Ordering::Relaxed), 1, "one poll, then parked");
        deterministic.run_until_idle();
        assert_eq!(polls.load(Ordering::Relaxed), 1, "no wake ⇒ no poll");

        finish.store(true, Ordering::Release);
        waker_slot
            .lock()
            .as_ref()
            .expect("waker stored")
            .wake_by_ref();
        deterministic.run_until_idle();
        assert_eq!(polls.load(Ordering::Relaxed), 2);
        assert_eq!(deterministic.pending(), (0, 0));
    }

    /// Bounded wait for cross-thread effects (default pools run on worker
    /// threads). Panics after 10s so a broken test fails loudly instead of
    /// hanging the suite.
    fn wait_until(condition: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(
                std::time::Instant::now() < deadline,
                "condition not reached within 10s"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
