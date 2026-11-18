//! The frame driver closes its own bookkeeping (phase, `frame_scheduled`,
//! frame count and frame-completion waiters) before it propagates a panic
//! from any step it owns, not merely from a panicking pipeline.
//!
//! The async tasks are polled before the pipeline slot opens, so a task whose
//! first poll panics aborts a frame that never reached its pipeline; the
//! tests below pin that, the post-frame callbacks of the clean frame after a
//! recovery, and a post-frame callback's own panic. A panicking pipeline is
//! covered by `tests/post_frame_callback_ordering.rs`.
//!
//! # Scope of the contract
//!
//! FLUI does not isolate a panic per callback: a panic here poisons and
//! propagates the whole frame. What these tests pin is that the scheduler's
//! own bookkeeping closes cleanly whichever step raised the panic, while the
//! panic itself still escapes to the caller.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use flui_foundation::testing::{CountingWaker, panicking_waker};
use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::{
    FrameCompletionFuture, FrameOutcome, Instant, OwnerFrame, SchedulerPhase, UpdateScheduler,
};

/// Registers `scheduler.end_of_frame()` and polls it once with a counting
/// waker so the waker is stored (the future itself stays `Pending`) —
/// letting a caller later assert exactly how many times the frame's own
/// completion notification woke it.
fn armed_completion_probe(
    scheduler: &UpdateScheduler,
) -> (FrameCompletionFuture, Arc<CountingWaker>) {
    let mut future = scheduler.end_of_frame();
    let counter = CountingWaker::new().shared();
    let waker = Waker::from(Arc::clone(&counter));
    let mut ctx = Context::from_waker(&waker);
    assert!(
        Pin::new(&mut future).poll(&mut ctx).is_pending(),
        "must not already be complete before any frame has run"
    );
    (future, counter)
}

/// The invariants every site below must satisfy once a panic from a step
/// `OwnerFrame::drive_frame` owns has been caught: the original panic
/// text (not a secondary one), phase and timestamp closed, the frame
/// counted exactly once, and the pre-registered completion waiter woken
/// exactly once by the abort. Whether another frame is scheduled depends on
/// the work the abort left behind, so each caller asserts it.
#[track_caller]
fn assert_recovered_from_panic(
    scheduler: &UpdateScheduler,
    payload: &(dyn std::any::Any + Send),
    expected_message: &str,
    frame_count_before: u64,
    mut completion_future: FrameCompletionFuture,
    completion_counter: &CountingWaker,
) {
    assert_eq!(
        flui_foundation::panic::payload_text(payload),
        Some(expected_message),
        "the original panic must propagate, not a secondary one"
    );
    assert_eq!(
        scheduler.phase(),
        SchedulerPhase::Idle,
        "the phase must be closed"
    );
    assert!(
        scheduler.current_frame().is_none(),
        "no frame timing must be left open"
    );
    assert_eq!(
        scheduler.frame_count(),
        frame_count_before + 1,
        "the aborted frame still counts as one attempted frame"
    );
    assert_eq!(
        completion_counter.count(),
        1,
        "the pre-registered end_of_frame waiter must be woken exactly once by the abort"
    );
    let waker = Waker::noop();
    let mut ctx = Context::from_waker(waker);
    let resolved = Pin::new(&mut completion_future).poll(&mut ctx);
    let Poll::Ready(outcome) = resolved else {
        panic!("an aborted frame still resolves end_of_frame -- it finished, badly");
    };
    assert!(
        matches!(outcome, Ok(FrameOutcome::Aborted { .. })),
        "abort_frame must resolve Aborted, not {outcome:?}"
    );
}

// ── Async future poll ───────────────────────────────────────────────────

struct PanicsOnPoll;

impl Future for PanicsOnPoll {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _ctx: &mut Context<'_>) -> Poll<()> {
        panic!("async poll probe");
    }
}

/// A future that records how many times it was polled and completes on its
/// second poll — a well-behaved sibling spawned alongside `PanicsOnPoll`,
/// to prove the zombie-slot fix does not corrupt the async driver's OTHER
/// tasks, only remove the panicking one's own slot.
struct CountedThenReady(Arc<AtomicUsize>);

impl Future for CountedThenReady {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _ctx: &mut Context<'_>) -> Poll<()> {
        let polls = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        if polls >= 2 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

fn async_future_poll_panic_closes_the_frame() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);
    let pending_before = owner.async_driver().pending_task_count();

    let sibling_polls = Arc::new(AtomicUsize::new(0));
    let _panicking_token = owner.async_driver().spawn_local(Box::pin(PanicsOnPoll));
    let _sibling_token = owner
        .async_driver()
        .spawn_local(Box::pin(CountedThenReady(Arc::clone(&sibling_polls))));
    assert_eq!(
        owner.async_driver().pending_task_count(),
        pending_before + 2
    );

    let payload = catch_unwind(AssertUnwindSafe(|| {
        owner
            .drive_frame(Instant::now(), || {}, || {})
            .expect("live owner frame");
    }))
    .expect_err("the panic must propagate");

    assert_recovered_from_panic(
        &scheduler,
        &*payload,
        "async poll probe",
        frame_count_before,
        completion_future,
        &completion_counter,
    );
    // The panicking future's own slot is gone; the well-behaved sibling's
    // is not -- only one of the two tasks the driver held is a zombie.
    assert_eq!(
        owner.async_driver().pending_task_count(),
        pending_before + 1,
        "the panicking future's slot must not be left as a zombie, but the \
         sibling task must still be tracked"
    );
    assert_eq!(
        sibling_polls.load(Ordering::SeqCst),
        0,
        "the sibling is polled in ascending task-id order, after the panicking one -- \
         it must not have been reached in the frame that aborted"
    );
    assert!(
        scheduler.is_frame_scheduled(),
        "the ready sibling the abort left unpolled asks for the next frame"
    );

    // A later frame's async-driver step does not touch the removed slot,
    // and it polls the sibling normally -- exactly once, since one poll
    // (of the two `CountedThenReady` needs) happens per frame.
    owner
        .drive_frame(Instant::now(), || {}, || {})
        .expect("live owner frame");
    assert_eq!(
        owner.async_driver().pending_task_count(),
        pending_before + 1
    );
    assert_eq!(
        sibling_polls.load(Ordering::SeqCst),
        1,
        "the sibling must be polled on the very next frame, exactly once"
    );
    assert_eq!(
        completion_counter.count(),
        1,
        "the completion waiter's notifier was drained by the abort; a later, unrelated \
         clean frame must not wake it a second time"
    );
}

/// A task that panics alone leaves nothing ready: the abort requests no frame
/// for its own failure, so a deterministic panic does not loop.
fn a_lone_panicking_task_requests_no_frame_after_the_abort() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
    let _panicking_token = owner.async_driver().spawn_local(Box::pin(PanicsOnPoll));

    let _ = catch_unwind(AssertUnwindSafe(|| {
        owner
            .drive_frame(Instant::now(), || {}, || {})
            .expect("live owner frame");
    }))
    .expect_err("the panic must propagate");

    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
    assert!(
        !scheduler.is_frame_scheduled(),
        "the aborted frame does not request another for its own failure"
    );
}

// ── Post-frame callbacks are not starved by recovery ───────────────────

fn post_frame_callbacks_run_in_the_clean_frame_after_a_panic() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");

    let _ = catch_unwind(AssertUnwindSafe(|| {
        owner
            .drive_frame(Instant::now(), || {}, || panic!("recovery probe"))
            .expect("live owner frame");
    }));

    let post_frame_ran = Arc::new(AtomicUsize::new(0));
    let post_frame_ran_cb = Arc::clone(&post_frame_ran);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        post_frame_ran_cb.fetch_add(1, Ordering::SeqCst);
    }));

    owner
        .drive_frame(Instant::now(), || {}, || {})
        .expect("live owner frame");

    assert_eq!(
        post_frame_ran.load(Ordering::SeqCst),
        1,
        "a clean frame after recovery still runs its post-frame callbacks"
    );
}

// ── A post-frame callback's own panic is not an aborted frame ──────────

/// A post-frame callback's panic runs through `end_frame_impl`, never
/// `abort_frame`: the pipeline already committed layout and paint before
/// this callback ran, and only the callback itself failed. The completion
/// future must resolve `Completed`, never `Aborted`, even though the panic
/// still propagates to the caller.
fn a_post_frame_callback_panic_still_resolves_completed_not_aborted() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
    let (mut completion_future, completion_counter) = armed_completion_probe(&scheduler);

    scheduler.add_post_frame_callback(Box::new(|_timing| panic!("post-frame probe")));

    let payload = catch_unwind(AssertUnwindSafe(|| {
        owner
            .drive_frame(Instant::now(), || {}, || {})
            .expect("live owner frame");
    }))
    .expect_err("the post-frame callback's panic must still propagate");

    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("post-frame probe"),
        "the original panic must propagate"
    );
    assert_eq!(
        completion_counter.count(),
        1,
        "the pre-registered waiter must still be woken"
    );

    let waker = Waker::noop();
    let mut ctx = Context::from_waker(waker);
    let Poll::Ready(outcome) = Pin::new(&mut completion_future).poll(&mut ctx) else {
        panic!("a post-frame callback's own panic must still resolve the completion future");
    };
    assert!(
        matches!(outcome, Ok(FrameOutcome::Completed { .. })),
        "the pipeline committed; only the post-frame callback failed -- this must not read \
         as Aborted, got {outcome:?}"
    );
}

// ── A secondary panic during abort must not displace the original one ──
/// The complete owner's recovery path aborts the frame, which calls
/// `notify_frame_completion` -- and a panicking waker there is a SECOND,
/// unrelated panic on top of the pipeline's own. Without containing it,
/// `abort_frame()` itself panics with the waker's payload before ever
/// reaching `resume_unwind(payload)`, so the caller observes the waker's
/// panic instead of the pipeline's -- the ORIGINAL failure this frame was
/// actually reporting is lost.
fn the_original_pipeline_panic_survives_a_panicking_completion_waker_during_abort() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
    let mut future = scheduler.end_of_frame();
    let panic_waker = panicking_waker("waker probe");
    let mut ctx = Context::from_waker(&panic_waker);
    assert!(Pin::new(&mut future).poll(&mut ctx).is_pending());

    let payload = catch_unwind(AssertUnwindSafe(|| {
        owner
            .drive_frame(Instant::now(), || {}, || panic!("probe frame panic"))
            .expect("live owner frame")
    }))
    .expect_err("a panic must still escape drive_frame");

    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("probe frame panic"),
        "the ORIGINAL pipeline panic must survive a panicking waker inside \
         abort_frame's own notify_frame_completion, not be displaced by it"
    );
    assert_eq!(
        scheduler.phase(),
        SchedulerPhase::Idle,
        "the phase reset inside abort_frame happens before notify_frame_completion \
         runs, so it must hold regardless of the waker's own panic"
    );
}

/// The post-frame callbacks queued behind a failing one were accepted for a
/// frame: the frame that failed requests the one that runs them.
fn a_post_frame_panic_requests_the_frame_for_the_callbacks_behind_it() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
    let tail_ran = Arc::new(AtomicUsize::new(0));
    let tail = Arc::clone(&tail_ran);
    scheduler.add_post_frame_callback(Box::new(|_| panic!("post-frame probe")));
    scheduler.add_post_frame_callback(Box::new(move |_| {
        tail.fetch_add(1, Ordering::SeqCst);
    }));

    let _ = catch_unwind(AssertUnwindSafe(|| {
        owner
            .drive_frame(Instant::now(), || {}, || {})
            .expect("live owner frame");
    }))
    .expect_err("the panic must propagate");
    assert_eq!(tail_ran.load(Ordering::SeqCst), 0, "the tail waits");
    assert!(
        scheduler.is_frame_scheduled(),
        "the failed frame requests the frame its tail runs in"
    );

    owner
        .drive_frame(Instant::now(), || {}, || {})
        .expect("live owner frame");
    assert_eq!(tail_ran.load(Ordering::SeqCst), 1);
}

#[test]
fn frame_panic_recovery_matrix() {
    crate::run_table(
        "frame_panic_recovery_matrix",
        &[
            (
                "async_future_poll_panic_closes_the_frame",
                async_future_poll_panic_closes_the_frame as fn(),
            ),
            (
                "a_lone_panicking_task_requests_no_frame_after_the_abort",
                a_lone_panicking_task_requests_no_frame_after_the_abort as fn(),
            ),
            (
                "post_frame_callbacks_run_in_the_clean_frame_after_a_panic",
                post_frame_callbacks_run_in_the_clean_frame_after_a_panic as fn(),
            ),
            (
                "a_post_frame_callback_panic_still_resolves_completed_not_aborted",
                a_post_frame_callback_panic_still_resolves_completed_not_aborted as fn(),
            ),
            (
                "the_original_pipeline_panic_survives_a_panicking_completion_waker_during_abort",
                the_original_pipeline_panic_survives_a_panicking_completion_waker_during_abort
                    as fn(),
            ),
            (
                "a_post_frame_panic_requests_the_frame_for_the_callbacks_behind_it",
                a_post_frame_panic_requests_the_frame_for_the_callbacks_behind_it as fn(),
            ),
        ],
    );
}
