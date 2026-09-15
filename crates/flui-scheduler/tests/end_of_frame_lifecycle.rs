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
fn idle_frame_waiter_requests_a_frame() {
    let scheduler = UpdateScheduler::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&wakes);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    })));
    let mut waiter = scheduler.end_of_frame();
    assert!(
        Pin::new(&mut waiter)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert_eq!(
        (
            scheduler.has_scheduled_frame(),
            wakes.load(Ordering::Relaxed)
        ),
        (true, 1)
    );
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

#[test]
fn explicit_frame_completes_waiter_and_releases_waker() {
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
    scheduler.execute_frame();
    assert!(weak.upgrade().is_none());
    assert!(
        Pin::new(&mut waiter)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready()
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Acceptance criteria for the demand-driven, cancellation-safe registration
// (issue #1055). The three tests above are the issue's own reproducer and
// are RED EVIDENCE, not pins: `idle_frame_waiter_requests_a_frame` polls
// after registering, so it stays green whether the demand happens at
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

use flui_scheduler::{FrameCompletionFuture, IdleDeadline, Instant};

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
    let Poll::Ready(timing) = resolved else {
        panic!("the frame the registration demanded must resolve it");
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

/// Criterion 4 — coalescing. Registration being the demand must not mean a
/// frame per registration: `request_frame_impl`'s false→true swap edge is
/// what keeps N registrations inside one frame to at most one wake.
#[test]
fn many_registrations_inside_one_frame_demand_at_most_one_wake() {
    let scheduler = UpdateScheduler::new();
    let edges = counting_wake_hook(&scheduler);
    let held: Arc<Mutex<Vec<FrameCompletionFuture>>> = Arc::new(Mutex::new(Vec::new()));

    let registrar = scheduler.clone();
    let held_for_callback = Arc::clone(&held);
    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        let mut slot = held_for_callback.lock().expect("uncontended in a test");
        for _ in 0..8 {
            slot.push(registrar.end_of_frame());
        }
    }));

    let edges_before = edges.load(Ordering::SeqCst);
    scheduler.execute_frame();
    let fired = edges.load(Ordering::SeqCst) - edges_before;

    assert_eq!(
        held.lock().expect("uncontended in a test").len(),
        8,
        "all eight registrations must have happened inside the one frame"
    );
    assert!(
        fired <= 1,
        "eight registrations inside one frame fired the wake hook {fired} times; the \
         false->true swap edge must coalesce them to at most one"
    );
    assert!(
        scheduler.is_frame_scheduled(),
        "and they must still leave a frame demanded -- coalescing to ZERO would be the \
         original bug wearing the coalescing label"
    );
}

/// The suppressing branch of the demand predicate, which nothing else
/// covers: a registration made while another waiter is still live must
/// issue NO demand, because the frame that waiter already bought will drain
/// both.
///
/// # Why the mid-frame step is the whole test
///
/// `request_frame`'s own `frame_scheduled` false-to-true swap edge absorbs a
/// redundant demand, so an oracle that registers twice on an idle scheduler
/// counts one hook edge whether the predicate suppresses or not. Opening the
/// frame first defeats that: `handle_begin_frame` clears the latch at the
/// top of the frame and does not drain the registry, so the second
/// registration lands with the latch DOWN and the first waiter still live.
/// A demand issued there is visible.
///
/// The *suppression assertion alone* would be satisfied by code that never
/// demands at all, which is what makes the mid-frame latch clear above
/// load-bearing rather than scene-setting. The setup around it is not:
/// asserting `(true, 1)` after the first registration is exactly issue
/// #1055's red, so this test does not pass against the unfixed
/// `end_of_frame`.
///
/// What it uniquely pins is the suppressing branch. Replace the predicate
/// with a constant `false` and every other test in this crate stays green
/// while this one fails.
#[test]
fn a_registration_behind_a_live_waiter_issues_no_demand_of_its_own() {
    let scheduler = UpdateScheduler::new();
    let edges = counting_wake_hook(&scheduler);

    let mut first = scheduler.end_of_frame();
    assert_eq!(
        (scheduler.is_frame_scheduled(), edges.load(Ordering::SeqCst)),
        (true, 1),
        "the first registration demands"
    );

    scheduler.handle_begin_frame(Instant::now());
    assert!(
        !scheduler.is_frame_scheduled(),
        "handle_begin_frame clears the latch at the top of the frame; without that \
         the assertions below would be satisfied by the swap edge alone"
    );
    let edges_before = edges.load(Ordering::SeqCst);

    let mut second = scheduler.end_of_frame();

    assert_eq!(
        (
            scheduler.is_frame_scheduled(),
            edges.load(Ordering::SeqCst) - edges_before
        ),
        (false, 0),
        "a registration behind a live waiter must stay silent: the frame already in \
         flight drains the whole registry, so a second demand buys a surplus frame"
    );

    // Suppressed, not stranded. The in-flight frame must resolve both.
    scheduler.handle_draw_frame();
    scheduler.end_frame();
    let mut cx = Context::from_waker(Waker::noop());
    assert!(Pin::new(&mut first).poll(&mut cx).is_ready());
    assert!(
        Pin::new(&mut second).poll(&mut cx).is_ready(),
        "the silent registration must still be served by the frame it declined to \
         ask for a second time"
    );
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

/// Criterion 6 — cancellation releases the executor's waker immediately,
/// not at the next frame completion.
///
/// `strong_count`, not `Weak::upgrade`: the count distinguishes "released"
/// (1, the test's own handle) from "still parked in the registry" (2), which
/// is the state the unfixed code leaves behind.
#[test]
fn dropping_a_polled_waiter_releases_the_executor_waker_immediately() {
    let scheduler = UpdateScheduler::new();
    let mut waiter = scheduler.end_of_frame();

    let probe = Arc::new(WakeResource);
    {
        let waker = Waker::from(Arc::clone(&probe));
        assert!(
            Pin::new(&mut waiter)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
    }

    drop(waiter);

    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "dropping the future must release the stored waker with no frame and no lock; \
         a count of 2 means the registry still pins it"
    );
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

/// Guard — no sibling starvation. A waiter that registers behind a live one
/// issues no demand of its own; it must still be notified by the frame the
/// first one bought.
#[test]
fn a_waiter_registered_behind_a_live_one_is_still_notified() {
    let scheduler = UpdateScheduler::new();
    let mut first = scheduler.end_of_frame();
    let mut second = scheduler.end_of_frame();

    scheduler.execute_frame();

    let mut cx = Context::from_waker(Waker::noop());
    assert!(Pin::new(&mut first).poll(&mut cx).is_ready());
    assert!(
        Pin::new(&mut second).poll(&mut cx).is_ready(),
        "the silent registration must ride the demand the first one already issued"
    );
}

/// Guard — type pins. All four already hold against the unfixed code: Fix 1
/// moves the REGISTRY's handle to `Weak` and never changes the future's own
/// shape, so a green run here proves nothing about this fix.
#[test]
fn frame_completion_future_type_pins() {
    static_assertions::assert_impl_all!(FrameCompletionFuture: Send, Sync, Unpin);
    static_assertions::assert_not_impl_any!(FrameCompletionFuture: Clone);
}
