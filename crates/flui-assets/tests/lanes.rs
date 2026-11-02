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
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use flui_assets::LoadServices;
use flui_platform::executor::{ComputeJob, Executor, IoFuture, SpawnError};
use parking_lot::Mutex;

/// How long [`block_on`] waits before it fails the test.
const BLOCK_ON_BUDGET: Duration = Duration::from_secs(10);

/// Polls `future` on this thread, parked between wakes, with no ambient
/// async runtime: an [`ImageLoad`](flui_assets::ImageLoad) needs none.
pub fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let mut future = std::pin::pin!(future);
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let deadline = Instant::now() + BLOCK_ON_BUDGET;
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
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
    /// What every spawn answers while set.
    refusal: Mutex<Option<SpawnError>>,
    /// Compute jobs accepted so far.
    computes: AtomicUsize,
    /// Set by a spawn that queued a future not yet polled.
    fresh: AtomicBool,
}

/// Records that a queued future asked to be polled again.
struct Flag(AtomicBool);

impl Wake for Flag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
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

    /// Refuse every later spawn with `refusal`.
    pub fn refuse(&self, refusal: SpawnError) {
        *self.refusal.lock() = Some(refusal);
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
        let flag = Arc::new(Flag(AtomicBool::new(true)));
        let waker = Waker::from(Arc::clone(&flag));
        let mut cx = Context::from_waker(&waker);
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
            let woken = flag.0.swap(false, Ordering::SeqCst);
            if self.fresh.swap(false, Ordering::SeqCst) || woken {
                // Polled outside the queue's lock: a future may spawn.
                let mut futures = std::mem::take(&mut *self.io.lock());
                futures.retain_mut(|future| future.as_mut().poll(&mut cx).is_pending());
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
        let refusal = *self.refusal.lock();
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        self.computes.fetch_add(1, Ordering::SeqCst);
        self.compute.lock().push_back(job);
        Ok(())
    }

    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        let refusal = *self.refusal.lock();
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        self.io.lock().push(future);
        self.fresh.store(true, Ordering::SeqCst);
        Ok(())
    }
}
