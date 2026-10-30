//! Background work as a widget meets it: a job's answer settles at the pump
//! after it runs, held work keeps its widget loading, a dropped task's work
//! is skipped or dropped at its next await, a panic answers `Panicked`, and
//! a refused spawn answers `Refused`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_platform::capability::{CapabilityRegistrar, ProviderContext};
use flui_platform::executor::{ComputeJob, Executor, IoFuture, SpawnError};
use flui_runtime::capability::CapabilityRegistry;
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_view::background::{Background, BackgroundCapability, BackgroundError};

use crate::capability_seam::acquire_capability;

fn window() -> HeadlessWindow {
    HeadlessWindow::new(10, 10)
}

fn background(host: &mut HeadlessHost) -> Background {
    acquire_capability::<BackgroundCapability>(host)
        .expect("the headless host runs background work")
}

pub(crate) fn a_compute_job_answers_at_the_pump_that_runs_it() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let mut task = background.compute(|| 6 * 7);
    assert_eq!(task.poll(), None, "never inside compute");
    assert_eq!(host.pending_background(), 1);
    assert_eq!(host.time_to_next_frame(), Some(Duration::ZERO));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), Some(Ok(42)));
    assert_eq!(host.pending_background(), 0);
}

pub(crate) fn on_complete_keeps_the_work_and_runs_once_with_its_answer() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let answers = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&answers);
    background
        .compute(|| "decoded")
        .on_complete(move |answer| sink.borrow_mut().push(answer));
    let _ = host.pump(Duration::ZERO);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*answers.borrow(), vec![Ok("decoded")]);
}

pub(crate) fn held_work_keeps_its_widget_loading_until_released() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    host.hold_background();
    let mut task = background.compute(|| 1);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), None, "held work does not run");
    assert_eq!(host.pending_background(), 1);
    assert!(
        host.pump_until_settled(Duration::ZERO, 3).is_err(),
        "held work keeps the host unsettled"
    );
    host.release_background();
    assert_eq!(
        host.pump_until_settled(Duration::ZERO, 3).map(|_| ()),
        Ok(())
    );
    assert_eq!(task.poll(), Some(Ok(1)));
}

pub(crate) fn run_background_runs_held_work_and_its_answer_settles_at_the_pump() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    host.hold_background();
    let mut task = background.compute(|| 2);
    assert_eq!(host.run_background(), 1);
    assert_eq!(task.poll(), None, "an answer settles at an owner turn");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), Some(Ok(2)));
}

pub(crate) fn a_dropped_task_skips_its_job() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let ran = Arc::new(AtomicBool::new(false));
    let job_ran = Arc::clone(&ran);
    drop(background.compute(move || job_ran.store(true, Ordering::SeqCst)));
    let _ = host.pump(Duration::ZERO);
    assert!(!ran.load(Ordering::SeqCst), "a cancelled job is skipped");
    assert_eq!(host.pending_background(), 0);
}

pub(crate) fn a_dropped_task_drops_its_parked_future_at_the_next_pump() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let task = background.io(std::future::pending::<()>());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(host.pending_background(), 1, "the future is parked");
    assert_eq!(
        host.time_to_next_frame(),
        None,
        "a parked future waits for its wake, not for a frame"
    );
    drop(task);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(host.pending_background(), 0, "the cancel woke and ended it");
}

pub(crate) fn a_cancelled_task_answers_cancelled() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let mut task = background.compute(|| 3);
    task.cancel();
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), Some(Err(BackgroundError::Cancelled)));
}

pub(crate) fn a_panicking_job_answers_panicked_on_the_owner_thread() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let mut task = background.compute(|| -> u8 { panic!("decode failed") });
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), Some(Err(BackgroundError::Panicked)));

    let mut io = background.io(async { panic!("read failed") });
    let _ = host.pump(Duration::ZERO);
    assert_eq!(io.poll(), Some(Err::<(), _>(BackgroundError::Panicked)));
}

/// Refuses every spawn, as a saturated lane does.
struct Saturated;

impl Executor for Saturated {
    fn spawn_compute(&self, _job: ComputeJob) -> Result<(), SpawnError> {
        Err(SpawnError::Saturated)
    }

    fn spawn_io(&self, _future: IoFuture) -> Result<(), SpawnError> {
        Err(SpawnError::Saturated)
    }
}

pub(crate) fn a_refused_spawn_answers_refused_at_the_next_pump() {
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<BackgroundCapability>(|cx: &ProviderContext<'_>| {
        Ok(Background::new(
            Arc::new(Saturated),
            cx.completions().clone(),
        ))
    });
    let registry = CapabilityRegistry::builder()
        .overrides(registrar)
        .build()
        .expect("one provider");
    let mut host = HeadlessHost::with_capabilities(window(), registry);
    let background = background(&mut host);
    let mut task = background.compute(|| 4);
    assert_eq!(task.poll(), None, "never inside compute");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        task.poll(),
        Some(Err(BackgroundError::Refused(SpawnError::Saturated)))
    );
}

/// Hands each job and future to the test, so it chooses the thread and the
/// waker they run with.
#[derive(Clone, Default)]
struct Handed {
    jobs: Arc<std::sync::Mutex<Vec<ComputeJob>>>,
    futures: Arc<std::sync::Mutex<Vec<IoFuture>>>,
}

impl Executor for Handed {
    fn spawn_compute(&self, job: ComputeJob) -> Result<(), SpawnError> {
        self.jobs.lock().expect("BUG: test lock").push(job);
        Ok(())
    }

    fn spawn_io(&self, future: IoFuture) -> Result<(), SpawnError> {
        self.futures.lock().expect("BUG: test lock").push(future);
        Ok(())
    }
}

fn host_over(executor: &Handed) -> HeadlessHost {
    let executor = executor.clone();
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<BackgroundCapability>(move |cx: &ProviderContext<'_>| {
        Ok(Background::new(
            Arc::new(executor.clone()),
            cx.completions().clone(),
        ))
    });
    let registry = CapabilityRegistry::builder()
        .overrides(registrar)
        .build()
        .expect("one provider");
    HeadlessHost::with_capabilities(window(), registry)
}

pub(crate) fn a_task_cancelled_while_its_job_runs_answers_cancelled() {
    let executor = Handed::default();
    let mut host = host_over(&executor);
    let background = background(&mut host);
    let started = Arc::new(std::sync::Barrier::new(2));
    let go = Arc::new(std::sync::Barrier::new(2));
    let (job_started, job_go) = (Arc::clone(&started), Arc::clone(&go));
    let mut task = background.compute(move || {
        job_started.wait();
        job_go.wait();
        5
    });
    let job = executor
        .jobs
        .lock()
        .expect("BUG: test lock")
        .pop()
        .expect("the job was spawned");
    let worker = std::thread::spawn(job);
    started.wait();
    task.cancel();
    go.wait();
    worker.join().expect("the job runs to its end");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        task.poll(),
        Some(Err(BackgroundError::Cancelled)),
        "a cancel wins over the value of a job already running"
    );
}

std::thread_local! {
    /// The task a [`CancelsOnDrop`] waker cancels, as owner-local state a
    /// waker's destructor reaches.
    static CANCELLED_BY_WAKER: RefCell<Option<flui_view::background::BackgroundTask<()>>> =
        const { RefCell::new(None) };
}

/// A waker whose last drop cancels the task in [`CANCELLED_BY_WAKER`].
struct CancelsOnDrop;

#[expect(
    clippy::manual_noop_waker,
    reason = "the waker exists for its `Drop`, which `Waker::noop` has none of"
)]
impl std::task::Wake for CancelsOnDrop {
    fn wake(self: Arc<Self>) {}
}

impl Drop for CancelsOnDrop {
    fn drop(&mut self) {
        CANCELLED_BY_WAKER.with(|task| {
            if let Some(task) = task.borrow().as_ref() {
                task.cancel();
            }
        });
    }
}

pub(crate) fn a_replaced_waker_that_cancels_as_it_drops_does_not_deadlock() {
    use std::task::{Context, Poll, Waker};

    let executor = Handed::default();
    let mut host = host_over(&executor);
    let background = background(&mut host);
    let task = background.io(std::future::pending::<()>());
    CANCELLED_BY_WAKER.with(|slot| slot.replace(Some(task)));
    let mut future = executor
        .futures
        .lock()
        .expect("BUG: test lock")
        .pop()
        .expect("the future was spawned");

    let first = Waker::from(Arc::new(CancelsOnDrop));
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(&first)),
        Poll::Pending
    );
    drop(first);
    // The second poll's waker replaces the first, whose drop cancels.
    let second = Waker::noop();
    assert_eq!(
        future.as_mut().poll(&mut Context::from_waker(second)),
        Poll::Ready(()),
        "the cancel reached the future"
    );
    let mut task = CANCELLED_BY_WAKER
        .with(RefCell::take)
        .expect("the task is kept");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), Some(Err(BackgroundError::Cancelled)));
}

/// The variable under which `background_work_callback_keeps_its_captures`
/// runs its child case.
pub(crate) const CALLBACK_UNWIND_CHILD: &str = "FLUI_BACKGROUND_CALLBACK_UNWIND_CHILD";

/// Panics when dropped, as a capture with a faulty `Drop` does.
struct PanicOnDrop;

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        panic!("capture dropped");
    }
}

/// An `on_complete` closure that panics while it holds a capture whose
/// `Drop` panics too: the capture is kept through the unwind, so the
/// process does not abort, and its own panic comes at the teardown.
pub(crate) fn an_on_complete_panic_keeps_its_captures() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let capture = PanicOnDrop;
    background.compute(|| 1).on_complete(move |_| {
        let _ = &capture;
        panic!("callback failed");
    });
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = host.pump(Duration::ZERO);
        let _ = host.pump(Duration::ZERO);
    }));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(host)));
}

/// A future whose poll panics while it holds a capture that panics as it
/// drops.
struct PanicsWhilePolled {
    _capture: PanicOnDrop,
}

impl std::future::Future for PanicsWhilePolled {
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        panic!("read failed");
    }
}

pub(crate) fn a_future_whose_poll_panicked_is_kept_not_dropped() {
    use std::task::{Context, Poll, Waker};

    let executor = Handed::default();
    let mut host = host_over(&executor);
    let background = background(&mut host);
    let mut task = background.io(PanicsWhilePolled {
        _capture: PanicOnDrop,
    });
    let mut future = executor
        .futures
        .lock()
        .expect("BUG: test lock")
        .pop()
        .expect("the future was spawned");
    assert_eq!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    );
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(future)));
    assert!(
        dropped.is_ok(),
        "the failed future's captures are not dropped"
    );
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), Some(Err(BackgroundError::Panicked)));
}

pub(crate) fn work_handed_to_on_complete_is_cancelled_when_its_runtime_ends() {
    use std::task::{Context, Poll, Waker};

    let executor = Handed::default();
    let mut host = host_over(&executor);
    let background = background(&mut host);
    background
        .io(std::future::pending::<()>())
        .on_complete(|_| {});
    let mut future = executor
        .futures
        .lock()
        .expect("BUG: test lock")
        .pop()
        .expect("the future was spawned");
    assert_eq!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
    drop(background);
    drop(host);
    assert_eq!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(()),
        "the retired runtime's work is cancelled"
    );
}

/// A log subscriber that panics on every background-work event, as a faulty
/// one does.
struct PanicsOnEvent;

impl tracing::Subscriber for PanicsOnEvent {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if event
            .metadata()
            .target()
            .starts_with("flui_view::background")
        {
            panic!("log subscriber failed");
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

pub(crate) fn a_panicking_log_of_a_panicked_job_still_answers_panicked() {
    let mut host = HeadlessHost::new(window());
    let background = background(&mut host);
    let mut task = background.compute(|| -> u8 { panic!("decode failed") });
    let mut io = background.io(async { panic!("read failed") });
    tracing::subscriber::with_default(PanicsOnEvent, || {
        let _ = host.pump(Duration::ZERO);
    });
    let _ = host.pump(Duration::ZERO);
    assert_eq!(task.poll(), Some(Err(BackgroundError::Panicked)));
    assert_eq!(io.poll(), Some(Err::<(), _>(BackgroundError::Panicked)));
}
