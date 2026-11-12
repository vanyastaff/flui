//! Background lanes for the registry's image loads, and a poll loop to wait
//! for one.
//!
//! [`ThreadLanes`] are what a native host's default lanes are: IO futures on
//! a tokio runtime, compute jobs on threads of their own. [`QueuedLanes`]
//! run nothing until the test runs them, so a test decides what happens in
//! between.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use flui_assets::LoadServices;
use flui_foundation::testing::CountingWaker;
use flui_platform::executor::{ComputeJob, Executor, IoFuture, Lane, RoomFuture, SpawnError};
use parking_lot::Mutex;

/// How long [`block_on`] waits before it fails the test.
const BLOCK_ON_BUDGET: Duration = Duration::from_secs(10);

/// Polls `future` on this thread, parked between wakes, with no ambient
/// async runtime: an [`ImageLoad`](flui_assets::ImageLoad) needs none.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let thread = std::thread::current();
    let waker = CountingWaker::calling(move || thread.unpark())
        .shared()
        .waker();
    let mut ctx = Context::from_waker(&waker);
    let deadline = Instant::now() + BLOCK_ON_BUDGET;
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut ctx) {
            return output;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "the load did not complete within {BLOCK_ON_BUDGET:?}"
        );
        std::thread::park_timeout(remaining);
    }
}

/// A native host's default lanes: a one-worker tokio runtime for IO, a
/// thread per compute job.
pub struct ThreadLanes {
    io: Mutex<Option<tokio::runtime::Runtime>>,
}

impl ThreadLanes {
    pub fn new() -> Arc<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("the IO lane's runtime starts");
        Arc::new(Self {
            io: Mutex::new(Some(runtime)),
        })
    }

    /// The lanes as a load takes them.
    pub fn lanes(self: &Arc<Self>) -> Arc<dyn Executor> {
        Arc::clone(self) as Arc<dyn Executor>
    }

    /// The lanes as a load's services take them.
    pub fn services(self: &Arc<Self>) -> LoadServices {
        LoadServices::new().with_lanes(self.lanes())
    }

    /// The IO lane's runtime.
    pub fn io_handle(&self) -> tokio::runtime::Handle {
        self.io
            .lock()
            .as_ref()
            .expect("the IO lane is running")
            .handle()
            .clone()
    }
}

impl Executor for ThreadLanes {
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError> {
        std::thread::Builder::new()
            .name("test-compute".to_owned())
            .spawn(job)
            .map(drop)
            .map_err(|_| SpawnError::Unavailable)
    }

    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        let io = self.io.lock();
        let runtime = io.as_ref().ok_or(SpawnError::ShuttingDown)?;
        drop(runtime.spawn(future));
        Ok(())
    }

    fn room(&self, lane: Lane) -> RoomFuture {
        // A compute job gets a thread of its own; the IO runtime takes
        // every future until it is shut down.
        RoomFuture::ready(match lane {
            Lane::Io if self.io.lock().is_none() => Err(SpawnError::ShuttingDown),
            _ => Ok(()),
        })
    }
}

impl Drop for ThreadLanes {
    fn drop(&mut self) {
        if let Some(runtime) = self.io.get_mut().take() {
            runtime.shutdown_background();
        }
    }
}

/// Lanes that queue what they accept and run it only in
/// [`run_until_idle`](Self::run_until_idle), on the test's thread.
#[derive(Default)]
pub struct QueuedLanes {
    compute: Mutex<VecDeque<ComputeJob>>,
    io: Mutex<Vec<IoFuture>>,
    /// What each lane answers, and who waits for room on it.
    refusals: Arc<Refusals>,
    /// Compute jobs accepted so far.
    computes: AtomicUsize,
    /// Set by a spawn that queued a future not yet polled, or by room made
    /// for the queued futures waiting for it.
    fresh: AtomicBool,
}

/// What each lane of [`QueuedLanes`] answers while set, and the waits for
/// room on them.
#[derive(Default)]
struct Refusals {
    compute: Mutex<Option<SpawnError>>,
    io: Mutex<Option<SpawnError>>,
    /// Waiting for room while a lane refuses as saturated.
    room: Mutex<Vec<Waker>>,
}

impl Refusals {
    fn of(&self, lane: Lane) -> &Mutex<Option<SpawnError>> {
        match lane {
            Lane::Compute => &self.compute,
            _ => &self.io,
        }
    }
}

impl QueuedLanes {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// The lanes as a load takes them.
    pub fn lanes(self: &Arc<Self>) -> Arc<dyn Executor> {
        Arc::clone(self) as Arc<dyn Executor>
    }

    /// The lanes as a load's services take them.
    pub fn services(self: &Arc<Self>) -> LoadServices {
        LoadServices::new().with_lanes(self.lanes())
    }

    /// Refuse every later spawn with `refusal`, on both lanes.
    pub fn refuse(&self, refusal: SpawnError) {
        self.refuse_lane(Lane::Compute, refusal);
        self.refuse_lane(Lane::Io, refusal);
    }

    /// Refuse every later spawn on `lane` with `refusal`.
    pub fn refuse_lane(&self, lane: Lane, refusal: SpawnError) {
        *self.refusals.of(lane).lock() = Some(refusal);
        // A refusal waiting cannot help ends the wait.
        if refusal != SpawnError::Saturated {
            self.wake_room();
        }
    }

    /// Take work again on both lanes, and wake whoever waits for room.
    pub fn make_room(&self) {
        *self.refusals.compute.lock() = None;
        *self.refusals.io.lock() = None;
        self.wake_room();
    }

    fn wake_room(&self) {
        let waiting = std::mem::take(&mut *self.refusals.room.lock());
        self.fresh.store(true, Ordering::SeqCst);
        for waker in waiting {
            waker.wake();
        }
    }

    /// Shut down as a host's lanes do: refuse every later spawn and drop
    /// what is queued unrun.
    pub fn shut_down(&self) {
        self.refuse(SpawnError::ShuttingDown);
        let io = std::mem::take(&mut *self.io.lock());
        let compute = std::mem::take(&mut *self.compute.lock());
        drop(io);
        drop(compute);
    }

    /// Retire the compute lane alone, as a host may before its IO lane:
    /// refuse every later spawn and drop the queued jobs unrun.
    pub fn drop_compute(&self) {
        self.refuse(SpawnError::ShuttingDown);
        drop(std::mem::take(&mut *self.compute.lock()));
    }

    /// Callers waiting for room now.
    pub fn waiting_for_room(&self) -> usize {
        self.refusals.room.lock().len()
    }

    /// Compute jobs accepted so far.
    pub fn computes(&self) -> usize {
        self.computes.load(Ordering::SeqCst)
    }

    /// IO futures queued and unfinished.
    pub fn pending_io(&self) -> usize {
        self.io.lock().len()
    }

    /// Runs queued jobs and polls queued futures until none is runnable.
    pub fn run_until_idle(&self) {
        self.run(true);
    }

    /// Polls queued futures until none is runnable, leaving compute jobs
    /// queued.
    pub fn run_io_until_idle(&self) {
        self.run(false);
    }

    fn run(&self, compute: bool) {
        // A queued future asked to be polled again when the count moved; the first pass
        // polls regardless.
        let wakes = CountingWaker::new().shared();
        let waker = wakes.waker();
        let mut seen = None;
        let mut ctx = Context::from_waker(&waker);
        loop {
            let mut ran = false;
            if compute {
                loop {
                    let job = self.compute.lock().pop_front();
                    let Some(job) = job else { break };
                    job();
                    ran = true;
                }
            }
            let count = wakes.count();
            let woken = seen != Some(count);
            seen = Some(count);
            if self.fresh.swap(false, Ordering::SeqCst) || woken {
                // Polled outside the queue's lock: a future may spawn.
                let mut futures = std::mem::take(&mut *self.io.lock());
                futures.retain_mut(|future| future.as_mut().poll(&mut ctx).is_pending());
                self.io.lock().extend(futures);
                ran = true;
            }
            if !ran {
                return;
            }
        }
    }
}

impl Executor for QueuedLanes {
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError> {
        let refusal = *self.refusals.compute.lock();
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        self.computes.fetch_add(1, Ordering::SeqCst);
        self.compute.lock().push_back(job);
        Ok(())
    }

    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        let refusal = *self.refusals.io.lock();
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        self.io.lock().push(future);
        self.fresh.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn room(&self, lane: Lane) -> RoomFuture {
        let refusals = Arc::clone(&self.refusals);
        RoomFuture::new(std::future::poll_fn(move |ctx| {
            let refusal = *refusals.of(lane).lock();
            match refusal {
                None => Poll::Ready(Ok(())),
                Some(SpawnError::Saturated) => {
                    refusals.room.lock().push(ctx.waker().clone());
                    Poll::Pending
                }
                Some(refusal) => Poll::Ready(Err(refusal)),
            }
        }))
    }
}
