//! Unified background execution services (ADR-0047).
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
//!   thread and the IO lane (`default_compute_worker_count`).
//! - **IO** ([`ExecutionServices::spawn_io`]) is an await-heavy future (file
//!   or network traffic). It runs on a small fixed-size async runtime
//!   (`IO_WORKER_THREADS` workers).
//! - **Durable services** (tasks, workers and services, ADR-0049) are the
//!   host's lifecycle layer, which spawns through these lanes; they are not
//!   part of this module.
//!
//! # Ownership
//!
//! The host's loop-scoped composition root (`flui-app`'s `AppRuntime`) owns
//! exactly one [`ExecutionServices`] value. Nothing here is ambient: there is
//! no global accessor, and library crates cannot reach these pools — this
//! crate's only allowed normal dependent is `flui-app`, checked by
//! `cargo xtask workspace`. An embedded host
//! that already runs its own executors injects them through
//! [`HostExecutors`] (`AppConfig::with_executors`); the default pools are
//! then **never constructed**, so FLUI and the host cannot oversubscribe the
//! machine with two full-core pools.
//!
//! # Admission, cancellation, shutdown
//!
//! Both lanes have bounded admission (generous defaults): a full lane
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

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

pub use flui_platform::executor::{ComputeJob, IoFuture, Lane, RoomFuture, SpawnError};

/// Host-provided worker pool for the **asynchronous compute** work class.
///
/// Implementations run each job exactly once, on any thread. FLUI wraps
/// every job it hands over with its own admission accounting and
/// shutdown-cancellation check, so an implementation only needs to execute.
///
/// Return [`SpawnError::Saturated`] to refuse a job when the host's own
/// queue is full — FLUI surfaces that to its caller unchanged — and resolve
/// [`room`](Self::room) only once it has room again.
pub trait HostComputePool: Send + Sync {
    /// Queue `job` for execution.
    fn spawn_job(&self, job: ComputeJob) -> Result<(), SpawnError>;

    /// Waits for the pool to take one more job, as [`RoomFuture`] says. A
    /// pool that never refuses resolves `Ok(())` at once.
    fn room(&self) -> RoomFuture;
}

/// Host-provided executor for the **IO** work class.
///
/// Implementations drive each future to completion, on any thread. FLUI
/// wraps every future it hands over with its own admission accounting and
/// cancellation (on shutdown the wrapped future resolves early), so an
/// implementation only needs to poll. A pool that refuses a future with
/// [`SpawnError::Saturated`] resolves [`room`](Self::room) only once it has
/// room again.
pub trait HostIoPool: Send + Sync {
    /// Queue `future` for execution.
    fn spawn_future(&self, future: IoFuture) -> Result<(), SpawnError>;

    /// Waits for the pool to take one more future, as
    /// [`HostComputePool::room`] does for a job.
    fn room(&self) -> RoomFuture;
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
///
/// Absent from a wasm32 non-test build, like its one production reader
/// ([`default_compute_worker_count`]): that target has no default pools.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) const IO_WORKER_THREADS: usize = 2;

/// Default compute-pool size for a machine with `available_parallelism`
/// hardware threads: everything except one hardware thread for the owner
/// (frame) thread and [`IO_WORKER_THREADS`] for the IO lane, never below 1.
///
/// This is the "background work cannot starve frame-required compute"
/// sizing half: even with every compute worker busy, the owner thread keeps
/// a hardware thread (the other half is structural — the frame lane never
/// runs on these pools at all, see the module doc).
///
/// Only the native default pools size themselves with it, so a wasm32
/// non-test build does not compile it.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn default_compute_worker_count(available_parallelism: usize) -> usize {
    available_parallelism
        .saturating_sub(IO_WORKER_THREADS + 1)
        .max(1)
}

/// Bounded admission windows, counted as work admitted but not yet finished
/// (queued + running).
///
/// Defaults are deliberately generous — they are overload backstops, not
/// throttles. Production always uses [`Default`]; tests inject small values
/// through `ExecutionServices::with_limits` (behind `testing`) to
/// exercise refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdmissionLimits {
    /// Maximum in-flight compute jobs.
    compute: usize,
    /// Maximum in-flight IO futures.
    io: usize,
}

impl Default for AdmissionLimits {
    fn default() -> Self {
        Self {
            compute: 256,
            io: 1024,
        }
    }
}

/// The wakers of the waits on one signal, each removed when its wait ends.
#[derive(Default)]
struct Waiters {
    inner: parking_lot::Mutex<WaitersInner>,
}

#[derive(Default)]
struct WaitersInner {
    /// The next registration's number; a `u64` counted up once per
    /// registration does not wrap.
    next: u64,
    wakers: Vec<(u64, Waker)>,
}

impl Waiters {
    /// Holds `waker` for the wait `registration` names, registering it
    /// first when it is not registered (or was woken and let go since).
    fn register(&self, registration: &mut Option<u64>, waker: &Waker) {
        // Cloned, and the waker it replaces dropped, outside the lock: both
        // run the waker owner's code.
        let waker = waker.clone();
        let unused = {
            let mut inner = self.inner.lock();
            let held = registration.and_then(|id| {
                inner
                    .wakers
                    .iter_mut()
                    .find(|(held, _)| *held == id)
                    .map(|(_, held)| held)
            });
            match held {
                Some(held) if held.will_wake(&waker) => Some(waker),
                Some(held) => Some(std::mem::replace(held, waker)),
                None => {
                    let id = inner.next;
                    inner.next += 1;
                    inner.wakers.push((id, waker));
                    *registration = Some(id);
                    None
                }
            }
        };
        drop(unused);
    }

    /// Ends the wait `registration` names: its waker is let go of, outside
    /// the lock.
    fn remove(&self, registration: &mut Option<u64>) {
        let Some(id) = registration.take() else {
            return;
        };
        let removed = {
            let mut inner = self.inner.lock();
            inner
                .wakers
                .iter()
                .position(|(held, _)| *held == id)
                .map(|at| inner.wakers.swap_remove(at))
        };
        drop(removed);
    }

    /// Wakes, and lets go of, every waiting waker, outside the lock: a waker
    /// is its owner's code. A wait polled again registers again. A waker
    /// that panics leaves the rest woken; the first panic is raised once
    /// every one is (ADR-0281).
    fn wake_all(&self) {
        let waiting = std::mem::take(&mut self.inner.lock().wakers);
        let mut first = flui_foundation::panic::FirstPanic::new();
        for (_, waker) in waiting {
            first.contain(|| waker.wake());
        }
        first.resume();
    }

    /// Waits registered now.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().wakers.len()
    }
}

/// One lane's admission window: an in-flight counter with a cap, and the
/// waits for room in it.
struct Admission {
    window: Arc<Window>,
}

struct Window {
    in_flight: AtomicUsize,
    limit: usize,
    /// Woken, all of them, when a unit of work leaves the window.
    waiting: Waiters,
}

impl Window {
    fn has_room(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) < self.limit
    }
}

impl Admission {
    fn new(limit: usize) -> Self {
        Self {
            window: Arc::new(Window {
                in_flight: AtomicUsize::new(0),
                limit,
                waiting: Waiters::default(),
            }),
        }
    }

    /// Try to admit one unit of work; the returned guard releases the slot
    /// when dropped (job finished, future completed/cancelled, or the
    /// wrapper itself was dropped unrun at pool shutdown).
    fn try_admit(&self) -> Result<AdmissionGuard, SpawnError> {
        let window = &self.window;
        let mut current = window.in_flight.load(Ordering::Relaxed);
        loop {
            if current >= window.limit {
                return Err(SpawnError::Saturated);
            }
            match window.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Ok(AdmissionGuard {
                        window: Arc::clone(window),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }

    /// Test-only like [`ExecutionServices::in_flight`], its one caller.
    #[cfg(test)]
    fn in_flight(&self) -> usize {
        self.window.in_flight.load(Ordering::Acquire)
    }
}

/// A wait for room in one admission window; dropping it ends the wait.
struct WindowRoom {
    window: Arc<Window>,
    registration: Option<u64>,
}

impl WindowRoom {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.window.has_room() {
            self.window
                .waiting
                .register(&mut self.registration, cx.waker());
            // Checked again once registered: a unit that left before the
            // registration woke no one, and one that leaves after it wakes
            // this waker.
            if !self.window.has_room() {
                return Poll::Pending;
            }
        }
        self.window.waiting.remove(&mut self.registration);
        Poll::Ready(())
    }
}

impl Drop for WindowRoom {
    fn drop(&mut self) {
        self.window.waiting.remove(&mut self.registration);
    }
}

/// RAII release of one admission slot.
struct AdmissionGuard {
    window: Arc<Window>,
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        self.window.in_flight.fetch_sub(1, Ordering::AcqRel);
        self.window.waiting.wake_all();
    }
}

/// Whether the services still admit work, and the waits that hear when they
/// stop.
struct Lifecycle {
    /// `true` until shutdown begins.
    accepting: AtomicBool,
    /// Woken, all of them, when shutdown begins.
    stopping: Waiters,
}

impl Lifecycle {
    fn is_accepting(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
    }
}

/// The host's pool behind a window, asked for room once the window has some.
enum HostPool {
    None,
    Compute(Arc<dyn HostComputePool>),
    Io(Arc<dyn HostIoPool>),
}

/// A wait for room on one lane of [`ExecutionServices`]: its admission
/// window first, then the host's pool, and shutdown at any point.
struct ServicesRoom {
    lifecycle: Arc<Lifecycle>,
    registration: Option<u64>,
    window: WindowRoom,
    host: HostPool,
    /// The host pool's own wait, once asked.
    host_wait: Option<RoomFuture>,
}

impl Future for ServicesRoom {
    type Output = Result<(), SpawnError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        if !this.lifecycle.is_accepting() {
            return Poll::Ready(Err(SpawnError::ShuttingDown));
        }
        this.lifecycle
            .stopping
            .register(&mut this.registration, cx.waker());
        // Checked again once registered: shutdown stops admission before
        // it wakes the waiting, so this sees it or is woken.
        if !this.lifecycle.is_accepting() {
            return Poll::Ready(Err(SpawnError::ShuttingDown));
        }
        if this.window.poll(cx).is_pending() {
            return Poll::Pending;
        }
        if this.host_wait.is_none() {
            this.host_wait = match &this.host {
                HostPool::None => return Poll::Ready(Ok(())),
                HostPool::Compute(pool) => Some(pool.room()),
                HostPool::Io(pool) => Some(pool.room()),
            };
        }
        match &mut this.host_wait {
            Some(waiting) => Pin::new(waiting).poll(cx),
            None => Poll::Ready(Ok(())),
        }
    }
}

impl Drop for ServicesRoom {
    fn drop(&mut self) {
        self.lifecycle.stopping.remove(&mut self.registration);
    }
}

/// The host-owned execution services: both background lanes, their
/// admission windows, and the shutdown protocol. One per host loop,
/// constructed only by the host's composition root. See the module doc for
/// the work-class model.
///
/// Construction is cheap; on the default backend the pools start lazily on
/// first spawn, so a run that never spawns background work never starts a
/// worker thread.
pub struct ExecutionServices {
    lifecycle: Arc<Lifecycle>,
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

    /// Handle to the IO runtime, starting it on first use.
    fn io_handle(&self) -> Result<tokio::runtime::Handle, SpawnError> {
        Self::handle_of(&self.io, || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(IO_WORKER_THREADS)
                .thread_name("flui-io")
                .enable_all()
                .build()
        })
    }

    /// Handle to the compute runtime, starting it on first use.
    fn compute_handle(&self) -> Result<tokio::runtime::Handle, SpawnError> {
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

    /// `Err(ShuttingDown)` once the slot is closed by shutdown;
    /// `Err(Unavailable)` when the OS refuses to start the pool — an
    /// environment failure, not an invariant, so no panic
    /// (`docs/PANIC-POLICY.md`): the slot stays `NotStarted` and a later
    /// spawn retries.
    fn handle_of(
        slot: &parking_lot::Mutex<PoolSlot>,
        build: impl FnOnce() -> std::io::Result<tokio::runtime::Runtime>,
    ) -> Result<tokio::runtime::Handle, SpawnError> {
        let mut slot = slot.lock();
        match &*slot {
            PoolSlot::Running(runtime) => Ok(runtime.handle().clone()),
            PoolSlot::Closed => Err(SpawnError::ShuttingDown),
            PoolSlot::NotStarted => match build() {
                Ok(runtime) => {
                    let handle = runtime.handle().clone();
                    *slot = PoolSlot::Running(runtime);
                    Ok(handle)
                }
                Err(error) => {
                    tracing::error!(
                        %error,
                        "failed to start a background worker pool; refusing the spawn \
                         (the slot stays unstarted, a later spawn retries)"
                    );
                    Err(SpawnError::Unavailable)
                }
            },
        }
    }

    /// Whether either default pool has actually started worker threads.
    #[cfg(any(test, feature = "testing"))]
    fn any_started(&self) -> bool {
        matches!(&*self.io.lock(), PoolSlot::Running(_))
            || matches!(&*self.compute.lock(), PoolSlot::Running(_))
    }
}

impl ExecutionServices {
    /// Services backed by FLUI's own default pools (lazily started).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::build(None, AdmissionLimits::default())
    }

    /// Services backed by host-injected pools; the default pools are never
    /// constructed.
    #[must_use]
    pub fn with_host(host: HostExecutors) -> Self {
        Self::build(Some(host), AdmissionLimits::default())
    }

    /// Test seam: custom admission windows (both backends), as the maximum
    /// in-flight compute jobs and IO futures.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn with_limits(host: Option<HostExecutors>, compute: usize, io: usize) -> Self {
        Self::build(host, AdmissionLimits { compute, io })
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
            lifecycle: Arc::new(Lifecycle {
                accepting: AtomicBool::new(true),
                stopping: Waiters::default(),
            }),
            compute_admission: Admission::new(limits.compute),
            io_admission: Admission::new(limits.io),
            backend,
            #[cfg(not(target_arch = "wasm32"))]
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    /// The root of this loop's cancellation tree. The host's lifecycle layer
    /// (`flui-app`'s task, worker and service handles, ADR-0049) derives
    /// every task's, worker's, and service's own signal as a child of this
    /// token, so [`Self::shutdown`]'s cancel stage reaches every outstanding
    /// unit of work without a registry walk.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn cancellation_root(&self) -> &tokio_util::sync::CancellationToken {
        &self.cancel
    }

    /// Whether this instance executes background work on FLUI's own backend
    /// (`true` — the lazily-started default pools on native targets,
    /// sequential in-place execution on wasm32) rather than routing it to
    /// host-injected pools (`false`).
    #[must_use]
    pub fn owns_default_pools(&self) -> bool {
        !matches!(self.backend, Backend::Host(_))
    }

    /// Spawn an **asynchronous compute** job (see the module doc's
    /// work-class model). Fire-and-forget: result delivery is the caller's
    /// business.
    ///
    /// # Errors
    ///
    /// [`SpawnError::ShuttingDown`] once [`Self::shutdown`] has begun,
    /// [`SpawnError::Saturated`] when the compute lane's admission window is
    /// full (or a host pool refuses the job), and
    /// [`SpawnError::Unavailable`] when the default compute pool cannot be
    /// started. The job is dropped unrun in every case.
    pub fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError> {
        if !self.lifecycle.is_accepting() {
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
                    // On `Err`, dropping `wrapped` drops the admission
                    // guard captured inside it — the slot is released.
                    let handle = pools.compute_handle()?;
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

    /// Whether new work is still admitted: `false` from the moment
    /// [`Self::shutdown`] begins. Work that was already running is not
    /// stopped by this; a long-running task checks it between units of work.
    #[must_use]
    pub fn is_accepting(&self) -> bool {
        self.lifecycle.is_accepting()
    }

    /// Spawn an **IO** future (see the module doc's work-class model).
    /// Fire-and-forget: result delivery is the caller's business. On
    /// shutdown the future is cancelled (dropped) at its next await point.
    ///
    /// # Errors
    ///
    /// [`SpawnError::ShuttingDown`] once [`Self::shutdown`] has begun,
    /// [`SpawnError::Saturated`] when the IO lane's admission window is full
    /// (or a host pool refuses the future), and [`SpawnError::Unavailable`]
    /// when the default IO runtime cannot be started. The future is dropped
    /// unpolled in every case.
    pub fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        if !self.lifecycle.is_accepting() {
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
                // pools, whose tasks FLUI cannot drop any other way.
                let _ = cancel.run_until_cancelled(future).await;
            });
            match &self.backend {
                Backend::Host(host) => host.io.spawn_future(wrapped),
                Backend::Default(pools) => {
                    // On `Err`, dropping `wrapped` drops the admission
                    // guard captured inside it — the slot is released.
                    let handle = pools.io_handle()?;
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

    /// Waits for `lane` to have room for one more unit of work: its
    /// admission window first, then the host's pool.
    ///
    /// The wait is woken when a unit of work leaves the window, when the
    /// host's pool has room again, and when [`Self::shutdown`] begins.
    /// Room is not reserved: a spawn that follows may find it taken and
    /// wait again. Dropping the wait lets go of its waker.
    ///
    /// # Errors
    ///
    /// Resolves `Err(SpawnError::ShuttingDown)` once [`Self::shutdown`] has
    /// begun, `Unavailable` for a lane these services do not run, and what
    /// a host pool answers when waiting cannot help.
    pub fn room(&self, lane: Lane) -> RoomFuture {
        let (admission, host) = match (lane, &self.backend) {
            (Lane::Compute, Backend::Host(host)) => (
                &self.compute_admission,
                HostPool::Compute(Arc::clone(&host.compute)),
            ),
            (Lane::Io, Backend::Host(host)) => {
                (&self.io_admission, HostPool::Io(Arc::clone(&host.io)))
            }
            // FLUI's own pools start at the spawn, which answers
            // `Unavailable` when the OS refuses their threads.
            (Lane::Compute, _) => (&self.compute_admission, HostPool::None),
            (Lane::Io, _) => (&self.io_admission, HostPool::None),
            // A lane these services do not run never has room.
            _ => return RoomFuture::ready(Err(SpawnError::Unavailable)),
        };
        RoomFuture::new(ServicesRoom {
            lifecycle: Arc::clone(&self.lifecycle),
            registration: None,
            window: WindowRoom {
                window: Arc::clone(&admission.window),
                registration: None,
            },
            host,
            host_wait: None,
        })
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
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn default_pools_started(&self) -> bool {
        match &self.backend {
            Backend::Host(_) => false,
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Default(pools) => pools.any_started(),
            #[cfg(target_arch = "wasm32")]
            Backend::Sequential => false,
        }
    }

    /// Shut down: stop admission, cancel outstanding work, then join running
    /// work until `deadline`, the IO pool first and the compute pool with
    /// what is left.
    ///
    /// After this returns every spawn refuses with
    /// [`SpawnError::ShuttingDown`]. Idempotent. With host-injected pools
    /// only the first two stages apply — FLUI cancels the work it wrapped,
    /// but joining the host's threads is the host's own lifecycle.
    ///
    /// `deadline` bounds the wait for **running** work (an IO future between
    /// await points, a compute closure mid-run); work that has not started
    /// is dropped unrun, which is the cancellation stage doing its job.
    pub fn shutdown(&self, deadline: std::time::Instant) {
        self.lifecycle.accepting.store(false, Ordering::Release);

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.cancel.cancel();
            if let Backend::Default(pools) = &self.backend {
                let io = pools.io.lock().take_runtime();
                let compute = pools.compute.lock().take_runtime();
                if let Some(runtime) = io {
                    runtime.shutdown_timeout(
                        deadline.saturating_duration_since(std::time::Instant::now()),
                    );
                }
                if let Some(runtime) = compute {
                    runtime.shutdown_timeout(
                        deadline.saturating_duration_since(std::time::Instant::now()),
                    );
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            // Sequential target: nothing to cancel or join — compute ran
            // inline and queued microtasks cannot be revoked. `deadline` is
            // deliberately unused.
            let _ = deadline;
        }
        // Last, so a waker that panics leaves the shutdown done: a caller
        // waiting for room hears it is shutting down.
        self.lifecycle.stopping.wake_all();
    }
}

/// The lanes as a capability handle reaches them, within these admission
/// windows and this shutdown.
impl flui_platform::executor::Executor for ExecutionServices {
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError> {
        Self::spawn_compute(self, job)
    }

    fn room(&self, lane: Lane) -> RoomFuture {
        Self::room(self, lane)
    }

    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        Self::spawn_io(self, future)
    }
}

impl fmt::Debug for ExecutionServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionServices")
            .field("accepting", &self.lifecycle.is_accepting())
            .field(
                "owns_default_pools",
                &!matches!(self.backend, Backend::Host(_)),
            )
            .finish_non_exhaustive()
    }
}

impl Drop for ExecutionServices {
    fn drop(&mut self) {
        // Last-resort teardown for the path that never ran the explicit
        // `shutdown` (a panic mid-teardown, a test dropping its host), on
        // every target: a wait for room outlives the services and must hear
        // that they stopped.
        self.lifecycle.accepting.store(false, Ordering::Release);
        // `shutdown_background` never blocks, so — unlike `Runtime`'s own
        // blocking `Drop` — this is safe even if the drop happens from
        // inside a task running on some other runtime.
        #[cfg(not(target_arch = "wasm32"))]
        {
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
        self.lifecycle.stopping.wake_all();
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
///
/// # Single-driver contract
///
/// Clones share one queue set: spawning from any thread — including from
/// inside driven work — is always fine. **Driving** is exclusive: at most
/// one [`run_until_idle`](Self::run_until_idle) may be in flight at a time,
/// and a second concurrent or reentrant call panics (`BUG:`) rather than
/// risk silently cancelling the first driver's in-flight task. Two
/// interleaved drivers would also not be *deterministic*, which is this
/// type's entire point.
#[derive(Clone, Default)]
pub struct DeterministicExecutors {
    inner: Arc<DeterministicShared>,
}

#[derive(Default)]
struct DeterministicShared {
    jobs: parking_lot::Mutex<std::collections::VecDeque<ComputeJob>>,
    tasks: parking_lot::Mutex<Vec<DeterministicTask>>,
    /// Exclusivity latch for [`DeterministicExecutors::run_until_idle`] —
    /// see the type's "Single-driver contract" doc section.
    driving: AtomicBool,
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
    ///
    /// # Panics
    ///
    /// Panics (`BUG:`) if a drive is already in flight — from another
    /// thread, or reentrantly from inside driven work. See the type's
    /// "Single-driver contract" doc section: a second driver could observe
    /// the first's checked-out task slot and silently cancel it, and
    /// interleaved drivers are not deterministic. Spawning during a drive
    /// is always fine; driving is what must be exclusive.
    pub fn run_until_idle(&self) -> usize {
        assert!(
            !self.inner.driving.swap(true, Ordering::AcqRel),
            "BUG: DeterministicExecutors::run_until_idle called while a drive is \
             already in flight -- this executor has a single-driver contract: one \
             drive at a time, never from inside driven work"
        );
        /// Clears the latch on every exit path, including an unwind out of
        /// a driven job's own panic.
        struct DriveGuard<'latch>(&'latch AtomicBool);
        impl Drop for DriveGuard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _driving = DriveGuard(&self.inner.driving);

        let mut steps = 0;
        loop {
            let mut progressed = false;

            // Compute jobs first, FIFO. The pop is its own statement so the
            // lock guard drops BEFORE the job runs — a `while let` scrutinee
            // temporary would live across the loop body, and a job that
            // spawns (same lock) would then deadlock on this non-reentrant
            // mutex.
            loop {
                let job = self.inner.jobs.lock().pop_front();
                let Some(job) = job else { break };
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
            self.compact_completed_tasks(|| {});

            if !progressed {
                return steps;
            }
        }
    }

    fn compact_completed_tasks(&self, before_lock: impl FnOnce()) {
        // The test seam admits work at the old snapshot/restore race boundary.
        // Production supplies a no-op. Keep admission and compaction atomic:
        // discarded slots contain no future and only our internal ready flag.
        before_lock();
        self.inner.tasks.lock().retain(|task| task.future.is_some());
    }

    /// Queued-but-unfinished work, `(jobs, futures)`.
    #[must_use]
    pub fn pending(&self) -> (usize, usize) {
        (self.inner.jobs.lock().len(), self.inner.tasks.lock().len())
    }

    /// Work [`run_until_idle`](Self::run_until_idle) would make progress on
    /// now: queued jobs plus woken futures. A future parked on a wake that
    /// has not come is pending but not runnable.
    #[must_use]
    pub fn runnable(&self) -> usize {
        let woken = self
            .inner
            .tasks
            .lock()
            .iter()
            .filter(|task| task.future.is_some() && task.ready.load(Ordering::Acquire))
            .count();
        self.inner.jobs.lock().len() + woken
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

    /// Its queue is unbounded: room always.
    fn room(&self) -> RoomFuture {
        RoomFuture::ready(Ok(()))
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

    /// Its queue is unbounded: room always.
    fn room(&self) -> RoomFuture {
        RoomFuture::ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    // ── conformance suite: the same contract for default and injected ──────
    //
    // Each check runs against BOTH backends: FLUI's default pools and the
    // injected `DeterministicExecutors` — the "an injected deterministic
    // executor passes the same tests" acceptance evidence. `drive` makes
    // queued deterministic work actually run; it is a no-op for the default
    // pools (their worker threads run the work themselves).

    fn conformance_admission_is_bounded_and_released() {
        // Deterministic backend: queued work does not run until driven, so
        // the admission window fills deterministically.
        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_limits(Some(deterministic.host_executors()), 2, 2);

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

    /// The frame lane needs no pool: with the compute admission window full
    /// and every worker parked, frame-thread work (an `AsyncDriver` poll on
    /// this thread) still completes immediately. This pins the structural
    /// half of "background work cannot starve frame-required compute"; the
    /// sizing half (`default_compute_worker_count`'s headroom) has no test.
    fn frame_lane_makes_progress_while_background_lanes_are_saturated() {
        let services = ExecutionServices::with_limits(None, 2, 2);
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

        // Frame-lane work: an owner-local task polled on THIS thread.
        let scheduler = flui_scheduler::UpdateScheduler::new();
        let owner_frame = flui_scheduler::OwnerFrame::try_new(&scheduler)
            .expect("the scheduler has no live owner frame");
        let done = Arc::new(AtomicBool::new(false));
        let done_for_task = Arc::clone(&done);
        let _token = owner_frame.async_driver().spawn_local(Box::pin(async move {
            done_for_task.store(true, Ordering::Release);
        }));
        assert_eq!(owner_frame.pump_background(|| {}).expect("owner turn"), 1);
        assert!(
            done.load(Ordering::Acquire),
            "the frame lane must complete without waiting for pool capacity"
        );

        gate.store(true, Ordering::Release);
        services.shutdown(std::time::Instant::now() + Duration::from_secs(5));
    }

    // ── deterministic executor semantics ────────────────────────────────────

    /// The latch clears on unwind: after a driven job panics, a later
    /// drive on the same executor works instead of tripping the guard.
    fn deterministic_drive_recovers_after_a_panicking_job() {
        let deterministic = DeterministicExecutors::new();
        deterministic
            .spawn_job(Box::new(|| panic!("job panic (expected by this test)")))
            .expect("deterministic spawn is unbounded");
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            deterministic.run_until_idle();
        }));
        assert!(unwound.is_err(), "the driven job's panic propagates");

        let ran = Arc::new(AtomicBool::new(false));
        let ran_for_job = Arc::clone(&ran);
        deterministic
            .spawn_job(Box::new(move || ran_for_job.store(true, Ordering::Release)))
            .expect("deterministic spawn is unbounded");
        deterministic.run_until_idle();
        assert!(
            ran.load(Ordering::Acquire),
            "the latch must clear on unwind"
        );
    }

    /// Pool-start failure is an environment error, not a panic
    /// (`docs/PANIC-POLICY.md`): the spawn is refused with `Unavailable`,
    /// the slot stays unstarted, and a later attempt (resources freed)
    /// succeeds. Driven through `handle_of`'s injected builder because a
    /// REAL tokio build failure needs OS thread/resource exhaustion, which
    /// no test can produce deterministically without destabilizing the
    /// process it runs in.
    // `DefaultPools`/`PoolSlot`/tokio are `cfg(not(wasm32))`: wasm32 uses
    // `Backend::Sequential` and builds no pools at all, so there is no
    // pool-start failure to simulate there.
    #[cfg(not(target_arch = "wasm32"))]
    fn pool_start_failure_refuses_the_spawn_and_recovers() {
        let slot = parking_lot::Mutex::new(PoolSlot::NotStarted);

        let result = DefaultPools::handle_of(&slot, || {
            Err(std::io::Error::other(
                "simulated: OS refused thread creation",
            ))
        });
        assert_eq!(result.err(), Some(SpawnError::Unavailable));
        assert!(
            matches!(&*slot.lock(), PoolSlot::NotStarted),
            "a failed start must leave the slot unstarted for a later retry"
        );

        let recovered = DefaultPools::handle_of(&slot, || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("flui-test-recovery")
                .build()
        });
        assert!(
            recovered.is_ok(),
            "once the environment recovers, the same slot must start normally"
        );
        // Tear the started runtime down non-blockingly.
        let runtime = slot.lock().take_runtime();
        if let Some(runtime) = runtime {
            runtime.shutdown_background();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn compaction_preserves_concurrently_admitted_future_and_next_work() {
        let executor = DeterministicExecutors::new();
        executor
            .spawn_future(Box::pin(std::future::pending()))
            .expect("parked future");
        let completed = Arc::new(AtomicUsize::new(0));
        let producer = executor.clone();
        let completed_in_producer = Arc::clone(&completed);
        executor.compact_completed_tasks(|| {
            // Joining puts an actual independent-handle admission exactly at
            // the compaction boundary without relying on timing or task count.
            std::thread::spawn(move || {
                producer
                    .spawn_future(Box::pin(async move {
                        completed_in_producer.fetch_add(1, Ordering::SeqCst);
                    }))
                    .expect("concurrent future admission");
            })
            .join()
            .expect("producer joined");
        });
        executor.run_until_idle();
        assert_eq!(
            completed.load(Ordering::SeqCst),
            1,
            "accepted work survives compaction"
        );
        let completed_in_next = Arc::clone(&completed);
        executor
            .spawn_future(Box::pin(async move {
                completed_in_next.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("next future admission");
        executor.run_until_idle();
        assert_eq!(
            completed.load(Ordering::SeqCst),
            2,
            "the next drive progresses"
        );
    }

    /// A waker that counts its wakes, and panics on the first when told to.
    fn counting_waker(panics: bool) -> (Arc<flui_foundation::testing::CountingWaker>, Waker) {
        let first = std::sync::atomic::AtomicBool::new(true);
        let count = flui_foundation::testing::CountingWaker::calling(move || {
            let first = first.swap(false, Ordering::SeqCst);
            assert!(!panics || !first, "a waiter's waker panicked");
        })
        .shared();
        let waker = count.waker();
        (count, waker)
    }

    fn poll(wait: &mut RoomFuture, waker: &Waker) -> Poll<Result<(), SpawnError>> {
        Pin::new(wait).poll(&mut Context::from_waker(waker))
    }

    /// A full lane keeps its waits pending and wakes each once work leaves
    /// it, past a waiter whose waker panics; a wait polled again holds one
    /// place; the other lane has room meanwhile.
    fn a_full_lane_wakes_its_waiters_when_work_leaves_it() {
        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_limits(Some(deterministic.host_executors()), 1, 1);
        services
            .spawn_compute(Box::new(|| {}))
            .expect("within the admission window");
        let (panicking, panicking_waker) = counting_waker(true);
        let (waiting, waiting_waker) = counting_waker(false);
        let mut first = services.room(Lane::Compute);
        let mut second = services.room(Lane::Compute);
        assert!(poll(&mut first, &panicking_waker).is_pending());
        assert!(poll(&mut second, &waiting_waker).is_pending());
        assert!(poll(&mut second, &waiting_waker).is_pending());
        assert_eq!(
            services.compute_admission.window.waiting.len(),
            2,
            "a wait polled again holds one place"
        );
        assert_eq!(
            poll(&mut services.room(Lane::Io), &waiting_waker),
            Poll::Ready(Ok(())),
            "the other lane has room"
        );

        let drove = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            deterministic.run_until_idle()
        }));
        assert!(drove.is_err(), "the first waker's panic is raised");
        assert_eq!(panicking.count(), 1);
        assert_eq!(waiting.count(), 1, "woken once past the panic");
        assert_eq!(poll(&mut second, &waiting_waker), Poll::Ready(Ok(())));
    }

    /// A wait dropped before the lane has room lets go of its waker at
    /// once, so waits started and abandoned while a lane stays full hold
    /// nothing.
    fn a_dropped_wait_lets_go_of_its_waker() {
        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_limits(Some(deterministic.host_executors()), 1, 1);
        services
            .spawn_compute(Box::new(|| {}))
            .expect("within the admission window");
        let (count, waker) = counting_waker(false);
        for _ in 0..3 {
            let mut wait = services.room(Lane::Compute);
            assert!(poll(&mut wait, &waker).is_pending());
        }
        assert_eq!(services.compute_admission.window.waiting.len(), 0);
        assert_eq!(services.lifecycle.stopping.len(), 0);
        assert_eq!(Arc::strong_count(&count), 2, "only the test's own waker");
    }

    /// Services dropped without a shutdown end a wait that outlives them,
    /// which then hears they stopped.
    fn dropped_services_end_a_wait_for_room() {
        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_limits(Some(deterministic.host_executors()), 1, 1);
        services
            .spawn_compute(Box::new(|| {}))
            .expect("within the admission window");
        let (waiting, waker) = counting_waker(false);
        let mut wait = services.room(Lane::Compute);
        assert!(poll(&mut wait, &waker).is_pending());

        drop(services);
        assert_eq!(waiting.count(), 1);
        assert_eq!(
            poll(&mut wait, &waker),
            Poll::Ready(Err(SpawnError::ShuttingDown))
        );
    }

    /// Shutdown wakes a wait for room, which then hears it is shutting
    /// down: one waiting on the admission window, and one waiting on a host
    /// pool with no room, whose lifecycle is the host's.
    fn shutdown_wakes_a_waiter_for_room() {
        struct Full;
        impl HostComputePool for Full {
            fn spawn_job(&self, _job: ComputeJob) -> Result<(), SpawnError> {
                Err(SpawnError::Saturated)
            }

            fn room(&self) -> RoomFuture {
                RoomFuture::new(std::future::pending())
            }
        }
        let deterministic = DeterministicExecutors::new();
        let services = ExecutionServices::with_limits(
            Some(HostExecutors::new(Arc::new(Full), Arc::new(deterministic))),
            1,
            1,
        );
        services
            .spawn_io(Box::pin(async {}))
            .expect("within the admission window");
        let (on_window, window_waker) = counting_waker(false);
        let (on_host, host_waker) = counting_waker(false);
        let mut window_wait = services.room(Lane::Io);
        let mut host_wait = services.room(Lane::Compute);
        assert!(poll(&mut window_wait, &window_waker).is_pending());
        assert!(
            poll(&mut host_wait, &host_waker).is_pending(),
            "the window has room, the host's pool none"
        );

        services.shutdown(std::time::Instant::now());
        assert_eq!(on_window.count(), 1);
        assert_eq!(on_host.count(), 1);
        assert_eq!(
            poll(&mut window_wait, &window_waker),
            Poll::Ready(Err(SpawnError::ShuttingDown))
        );
        assert_eq!(
            poll(&mut host_wait, &host_waker),
            Poll::Ready(Err(SpawnError::ShuttingDown))
        );
    }

    #[test]
    fn execution_lane_matrix() {
        crate::table_test::run_table(
            "execution_lane_matrix",
            &[
                #[cfg(not(target_arch = "wasm32"))]
                (
                    "compaction_preserves_concurrently_admitted_future_and_next_work",
                    compaction_preserves_concurrently_admitted_future_and_next_work as fn(),
                ),
                (
                    "conformance_admission_is_bounded_and_released",
                    conformance_admission_is_bounded_and_released as fn(),
                ),
                (
                    "a_full_lane_wakes_its_waiters_when_work_leaves_it",
                    a_full_lane_wakes_its_waiters_when_work_leaves_it as fn(),
                ),
                (
                    "shutdown_wakes_a_waiter_for_room",
                    shutdown_wakes_a_waiter_for_room as fn(),
                ),
                (
                    "dropped_services_end_a_wait_for_room",
                    dropped_services_end_a_wait_for_room as fn(),
                ),
                (
                    "a_dropped_wait_lets_go_of_its_waker",
                    a_dropped_wait_lets_go_of_its_waker as fn(),
                ),
                (
                    "frame_lane_makes_progress_while_background_lanes_are_saturated",
                    frame_lane_makes_progress_while_background_lanes_are_saturated as fn(),
                ),
                (
                    "deterministic_drive_recovers_after_a_panicking_job",
                    deterministic_drive_recovers_after_a_panicking_job as fn(),
                ),
                #[cfg(not(target_arch = "wasm32"))]
                (
                    "pool_start_failure_refuses_the_spawn_and_recovers",
                    pool_start_failure_refuses_the_spawn_and_recovers as fn(),
                ),
            ],
        );
    }
}

/// Assertions that only hold on wasm32, EXECUTED there.
///
/// These stay a lib test because they pin the private `Backend::Sequential` —
/// the whole wasm execution model — next to its definition, and the
/// `default_pools_started` probe they read exists only under `cfg(test)` or
/// the `testing` feature. That is why this crate's lib-test target builds
/// for wasm32.
///
/// Both assertions below are FALSE on native, which is the point: a wasm test
/// that would pass identically on a native target buys a wasm build and no
/// coverage. Run with `cargo xtask wasm-test`.
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_sequential_backend_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    /// With no host executors, wasm32 selects `Backend::Sequential`, whose
    /// entire content is that a compute job runs **inline at the spawn site** —
    /// there is no thread to hand it to.
    ///
    /// On native the same call hands the job to a pool and returns before it
    /// runs, so this assertion is false there. It is the divergence, not a
    /// property of the queue.
    #[wasm_bindgen_test]
    fn a_compute_job_has_already_run_when_spawn_compute_returns() {
        let services = ExecutionServices::with_defaults();
        let ran = Arc::new(AtomicUsize::new(0));

        let flag = Arc::clone(&ran);
        services
            .spawn_compute(Box::new(move || {
                flag.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("the sequential backend has no admission failure to report");

        assert_eq!(
            ran.load(Ordering::SeqCst),
            1,
            "Backend::Sequential must run the job inline before spawn_compute returns"
        );
    }

    /// The sequential backend builds no pools at all — there is no
    /// `DefaultPools` on this target — so nothing can report one as started,
    /// before or after work is spawned. On native the same sequence starts a
    /// tokio runtime and flips this to `true`.
    #[wasm_bindgen_test]
    fn the_sequential_backend_never_starts_a_default_pool() {
        let services = ExecutionServices::with_defaults();
        assert!(
            !services.default_pools_started(),
            "no pool exists to start on wasm32"
        );

        services
            .spawn_compute(Box::new(|| {}))
            .expect("the sequential backend has no admission failure to report");

        assert!(
            !services.default_pools_started(),
            "running work must not conjure a pool on the sequential backend"
        );
    }
}
