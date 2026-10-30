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
