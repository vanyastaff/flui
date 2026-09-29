//! Issue #1055 reproducer, verbatim from the issue body: RED evidence against
//! unfixed `UpdateScheduler::end_of_frame`. Design-independent of the fix
//! (weak-slot vs `event-listener` vs anything else) — it exercises only the
//! public API. Kept as the outer acceptance test once a fix lands.
#![forbid(unsafe_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};

use flui_scheduler::UpdateScheduler;

struct WakeResource;
#[expect(
    clippy::manual_noop_waker,
    reason = "Waker::noop() is a static with no allocation to observe; these tests \
               measure this Arc's strong count and liveness, which is exactly what \
               distinguishes a released waker from one the registry still pins"
)]
impl Wake for WakeResource {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn dropped_waiter_releases_its_executor_waker() {
    let scheduler = UpdateScheduler::new();
    let mut waiter = scheduler.end_of_frame();
    let weak = {
        let resource = Arc::new(WakeResource);
        let weak = Arc::downgrade(&resource);
        let waker = Waker::from(resource);
        assert!(
            Pin::new(&mut waiter)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        weak
    };
    drop(waiter);
    assert!(
        weak.upgrade().is_none(),
        "cancelled wait retains executor waker until a frame completes"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Acceptance criteria for the demand-driven, cancellation-safe registration
// (issue #1055). The three tests above are the issue's own reproducer, and
// TWO of them were red against the unfixed code: `explicit_frame_completes_
// waiter_and_releases_waker` passed there, since an explicit frame always
// did resolve and release. Even the two that failed are evidence rather
// than pins -- `idle_frame_waiter_requests_a_frame` polls after
// registering, so it stays green whether the demand happens at
// registration or at first poll. The tests below are the pins.
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

use flui_scheduler::{FrameCompletionFuture, FrameOutcome, IdleDeadline, Instant};

/// Installs a wake hook that only counts `frame_scheduled` false→true edges.
fn counting_wake_hook(scheduler: &UpdateScheduler) -> Arc<AtomicUsize> {
    let edges = Arc::new(AtomicUsize::new(0));
    let sink = Arc::clone(&edges);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        sink.fetch_add(1, Ordering::SeqCst);
    })));
    edges
}

fn far_deadline() -> IdleDeadline {
    IdleDeadline::far_future(Instant::now())
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

/// Criterion 1 — an idle registration IS the demand, and the frame it buys
/// is the one it resolves with.
#[test]
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

    let frame_id = scheduler.execute_frame();
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

/// Criterion 2 — the post-drain window. A registration made from inside a
/// completion waker (so: after the drain emptied the registry, while the
/// frame is still open at phase `PostFrameCallbacks`) must leave a frame
/// demanded once the frame returns.
///
/// Any demand gate that reads a scheduler phase, or a per-frame "a frame is
/// already coming" flag, goes silent in exactly this window and hangs the
/// fresh waiter forever. That is what this pins.
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
#[test]
fn a_registration_from_inside_a_completion_waker_demands_the_next_frame() {
    let scheduler = UpdateScheduler::new();
    let registrar = RegisteringWaker::new(&scheduler);

    let mut first = scheduler.end_of_frame();
    let waker = Waker::from(Arc::clone(&registrar));
    assert!(
        Pin::new(&mut first)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );

    scheduler.execute_frame();

    assert_eq!(
        registrar.registration_count(),
        1,
        "the completion waker must actually have run and registered"
    );
    assert!(
        scheduler.is_frame_scheduled(),
        "a registration made after the drain must demand a frame of its own; without \
         one it waits forever for a frame nobody asked for"
    );
}

/// Criterion 3 — the same, on the abort path, asserting EXACTLY one demand.
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
#[test]
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
        scheduler.execute_frame();
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

/// Criterion 5 — after every path that ends a frame, a fresh registration
/// demands again, firing the hook exactly once.
#[test]
fn a_registration_after_each_frame_ending_path_demands_exactly_one_frame() {
    #[track_caller]
    fn assert_demands_once_after(path: &str, end_the_frame: impl FnOnce(&UpdateScheduler)) {
        let scheduler = UpdateScheduler::new();
        let edges = counting_wake_hook(&scheduler);

        end_the_frame(&scheduler);
        assert!(
            !scheduler.is_frame_scheduled(),
            "{path}: the frame-ending path must leave no frame latched, or this oracle \
             cannot see the fresh demand's edge"
        );

        let edges_before = edges.load(Ordering::SeqCst);
        let _waiter = scheduler.end_of_frame();

        assert_eq!(
            edges.load(Ordering::SeqCst) - edges_before,
            1,
            "{path}: a registration after this frame-ending path must demand exactly one frame"
        );
        assert!(scheduler.is_frame_scheduled(), "{path}");
    }

    assert_demands_once_after("execute_frame", |scheduler| {
        scheduler.execute_frame();
    });
    assert_demands_once_after("drive_frame", |scheduler| {
        scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    });
    assert_demands_once_after("handle_begin_frame + handle_draw_frame + end_frame", |s| {
        s.handle_begin_frame(Instant::now());
        s.handle_draw_frame();
        s.end_frame();
    });
    assert_demands_once_after("handle_begin_frame + abort_frame", |scheduler| {
        scheduler.handle_begin_frame(Instant::now());
        scheduler.abort_frame();
    });
}

/// Criterion 9 — a pending completion waiter is real frame demand, so it
/// suppresses idle work until that frame runs. This is the intended meaning
/// of demand-driven, recorded as a consequence rather than discovered later.
#[test]
fn a_pending_completion_waiter_suppresses_idle_callbacks() {
    let scheduler = UpdateScheduler::new();
    scheduler.schedule_idle_callback(|| {});

    let _waiter = scheduler.end_of_frame();

    assert_eq!(
        scheduler.execute_idle_callbacks(),
        0,
        "a demanded frame must suppress idle work"
    );
    assert!(
        scheduler.has_idle_callbacks(),
        "suppressed, not consumed: the callback must still be there for the idle \
         window after the frame"
    );
}

/// Criterion 10 — the disabled→enabled edge re-demands.
///
/// The oracle is `is_frame_scheduled()` immediately after the edge, not
/// "the waiter resolves": driving a frame to check resolution resolves it
/// against the unfixed code too.
#[test]
fn re_enabling_frames_re_demands_on_the_edge() {
    let mut scheduler = UpdateScheduler::new();
    scheduler.set_frames_enabled(false);

    let _waiter = scheduler.end_of_frame();
    assert!(
        !scheduler.is_frame_scheduled(),
        "a registration made while frames are disabled must issue nothing"
    );

    scheduler.set_frames_enabled(true);

    assert!(
        scheduler.is_frame_scheduled(),
        "the disabled->enabled edge is the only thing that can re-issue a demand that \
         was never issued; without it this waiter waits forever"
    );
}

// ── Guards: green against the unfixed code too, so a green run proves
// nothing about this fix. They defend the shape against a future change.

/// Guard — enablement is respected. `schedule_frame_if_enabled`, not the
/// raw `request_frame`: a registration must not force frames on a scheduler
/// whose owner turned them off.
#[test]
fn a_registration_while_frames_are_disabled_demands_nothing() {
    let mut scheduler = UpdateScheduler::new();
    let edges = counting_wake_hook(&scheduler);
    scheduler.set_frames_enabled(false);

    let _waiter = scheduler.end_of_frame();

    assert_eq!(
        (scheduler.is_frame_scheduled(), edges.load(Ordering::SeqCst)),
        (false, 0),
        "a disabled scheduler must not be forced awake by a registration"
    );
}
