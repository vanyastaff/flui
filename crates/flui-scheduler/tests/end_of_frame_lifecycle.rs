//! Acceptance tests for `UpdateScheduler::end_of_frame`'s demand-driven
//! registration (issue #1055). Design-independent of the implementation
//! (weak-slot vs `event-listener` vs anything else) — they exercise only
//! the public API.
#![forbid(unsafe_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};

use flui_scheduler::UpdateScheduler;

// ─────────────────────────────────────────────────────────────────────────
// Acceptance criteria for the demand-driven, cancellation-safe registration
// (issue #1055).
//
// Two construction rules hold for every oracle in this file, and both exist
// because the obvious version of the test passes against the unfixed code:
//
//   * The `on_frame_scheduled` hook only ever COUNTS. Driving a frame from
//     inside it violates `set_on_frame_scheduled`'s own documented contract
//     ("must only touch wake machinery, never re-enter the scheduler"), and
//     a hook reached from a post-drain registration would re-enter
//     `handle_begin_frame` at phase `PostFrameCallbacks`, which has no valid
//     transition.
//   * Demand behaviour is driven through a raw `impl Wake`, never
//     `spawn_local`. `AsyncDriver`'s own wake hook requests a frame
//     unconditionally and with no gate, so a task awaiting under the
//     in-crate driver is green against this bug and proves nothing.
// ─────────────────────────────────────────────────────────────────────────
use std::sync::Mutex;

use flui_scheduler::{FrameCompletionFuture, FrameOutcome};

/// Installs a wake hook that only counts `frame_scheduled` false→true edges.
fn counting_wake_hook(scheduler: &UpdateScheduler) -> Arc<AtomicUsize> {
    let edges = Arc::new(AtomicUsize::new(0));
    let sink = Arc::clone(&edges);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        sink.fetch_add(1, Ordering::SeqCst);
    })));
    edges
}

/// A raw `impl Wake` that registers a FRESH `end_of_frame()` from inside
/// `wake()` and keeps it alive, so the registration is observable after the
/// waking call returns.
struct RegisteringWaker {
    scheduler: UpdateScheduler,
    registered: Mutex<Vec<FrameCompletionFuture>>,
}

impl RegisteringWaker {
    fn new(scheduler: &UpdateScheduler) -> Arc<Self> {
        Arc::new(Self {
            scheduler: scheduler.clone(),
            registered: Mutex::new(Vec::new()),
        })
    }

    fn registration_count(&self) -> usize {
        self.registered.lock().expect("uncontended in a test").len()
    }
}

impl Wake for RegisteringWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let fresh = self.scheduler.end_of_frame();
        self.registered
            .lock()
            .expect("uncontended in a test")
            .push(fresh);
    }
}

/// An idle registration IS the demand, and the frame it buys is the one it
/// resolves with.
fn an_idle_registration_demands_one_frame_and_resolves_with_its_timing() {
    let scheduler = UpdateScheduler::new();
    let edges = counting_wake_hook(&scheduler);
    assert!(!scheduler.is_frame_scheduled());

    let mut waiter = scheduler.end_of_frame();

    assert_eq!(
        (scheduler.is_frame_scheduled(), edges.load(Ordering::SeqCst)),
        (true, 1),
        "registering IS the demand, and it fires the platform wake hook exactly once"
    );

    let frame_id = scheduler.execute_frame(
        &flui_scheduler::OwnerFrame::new(&scheduler)
            .expect("the scheduler has no live owner frame"),
    );
    let resolved = Pin::new(&mut waiter).poll(&mut Context::from_waker(Waker::noop()));
    let Poll::Ready(outcome) = resolved else {
        panic!("the frame the registration demanded must resolve it");
    };
    let Ok(FrameOutcome::Completed { timing, .. }) = outcome else {
        panic!("a clean execute_frame() must resolve Completed, not {outcome:?}");
    };
    assert_eq!(
        timing.id, frame_id,
        "it must resolve with THAT frame's timing, not merely with some timing"
    );
}

/// A registration made from inside an aborted frame's completion waker
/// demands EXACTLY one frame.
///
/// # This oracle fails by HANGING, not by asserting
///
/// The waker calls `end_of_frame()`, which takes a real
/// `completion_waiters` lock. Under the one regression that would hold that
/// registry guard across the wake loop, this blocks forever and nextest
/// reports it on the terminate-after timeout rather than in milliseconds.
/// That is tolerable because the demand genuinely is this test's assertion,
/// and because `completion_waker_runs_with_no_scheduler_lock_held` (in
/// `scheduler/lock_discipline_tests.rs`) catches that same regression in
/// its own process, fast, with a `try_lock` probe. Read a hang here as a
/// pointer to that sibling, never as the intended signal.
fn a_registration_from_inside_an_aborted_frames_waker_demands_exactly_one_frame() {
    let scheduler = UpdateScheduler::new();
    let edges = counting_wake_hook(&scheduler);
    let registrar = RegisteringWaker::new(&scheduler);

    let mut first = scheduler.end_of_frame();
    let waker = Waker::from(Arc::clone(&registrar));
    assert!(
        Pin::new(&mut first)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );

    scheduler.add_persistent_frame_callback(Arc::new(|_timing| {
        panic!("persistent callback fails this frame");
    }));

    let edges_before = edges.load(Ordering::SeqCst);
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scheduler.execute_frame(
            &flui_scheduler::OwnerFrame::new(&scheduler)
                .expect("the scheduler has no live owner frame"),
        );
    }));
    assert!(attempt.is_err(), "the frame must have aborted");

    assert_eq!(
        registrar.registration_count(),
        1,
        "an aborted frame still notifies its waiters -- it finished, badly"
    );
    assert_eq!(
        edges.load(Ordering::SeqCst) - edges_before,
        1,
        "the abort path must demand exactly one frame for the fresh registration"
    );
    assert!(scheduler.is_frame_scheduled());
}

#[test]
fn end_of_frame_demand_matrix() {
    if crate::frame_completion_recovery::selected_child() {
        return;
    }
    crate::run_table(
        "end_of_frame_demand_matrix",
        &[
            (
                "completion_wake_ownership_and_recovery",
                crate::frame_completion_recovery::completion_wake_ownership_and_recovery as fn(),
            ),
            (
                "an_idle_registration_demands_one_frame_and_resolves_with_its_timing",
                an_idle_registration_demands_one_frame_and_resolves_with_its_timing as fn(),
            ),
            (
                "a_registration_from_inside_an_aborted_frames_waker_demands_exactly_one_frame",
                a_registration_from_inside_an_aborted_frames_waker_demands_exactly_one_frame
                    as fn(),
            ),
        ],
    );
}
