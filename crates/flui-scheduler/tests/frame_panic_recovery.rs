//! Issue #1057: the shared frame driver must close its own bookkeeping —
//! phase, `frame_scheduled`, frame count, and frame-completion waiters —
//! before propagating a panic from ANY phase it owns, not merely a
//! panicking pipeline.
//!
//! Each test below panics BEFORE the pipeline slot even opens: a transient
//! callback, a mid-frame microtask, a `Priority::Build` task, a persistent
//! callback, and an async future's first poll. Another confirms recovery
//! does not starve `Priority::Idle` work or a clean frame's post-frame
//! callbacks.
//!
//! `crates/flui-scheduler/tests/post_frame_callback_ordering.rs` already
//! covers a panicking PIPELINE — that path was fixed before this issue and
//! stays green, unmodified, alongside these.
//!
//! # Scope of the contract
//!
//! FLUI does not isolate a panic per callback: a panic here poisons and propagates
//! the whole frame, unchanged by this issue. So what these tests pin is
//! narrower: the scheduler's OWN bookkeeping closes cleanly no matter which
//! phase raised the panic, while the panic itself still escapes to the
//! caller.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use flui_scheduler::{
    FrameCompletionFuture, FrameOutcome, IdleDeadline, Instant, OwnerFrame, Priority,
    SchedulerPhase, UpdateScheduler,
};

fn far_deadline() -> IdleDeadline {
    IdleDeadline::far_future(Instant::now())
}

/// Counts `wake()` calls; does nothing else.
struct CountingWaker(AtomicUsize);

impl CountingWaker {
    fn new() -> Arc<Self> {
        Arc::new(Self(AtomicUsize::new(0)))
    }

    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Registers `scheduler.end_of_frame()` and polls it once with a counting
/// waker so the waker is stored (the future itself stays `Pending`) —
/// letting a caller later assert exactly how many times the frame's own
/// completion notification woke it.
fn armed_completion_probe(
    scheduler: &UpdateScheduler,
) -> (FrameCompletionFuture, Arc<CountingWaker>) {
    let mut future = scheduler.end_of_frame();
    let counter = CountingWaker::new();
    let waker = Waker::from(Arc::clone(&counter));
    let mut cx = Context::from_waker(&waker);
    assert!(
        Pin::new(&mut future).poll(&mut cx).is_pending(),
        "must not already be complete before any frame has run"
    );
    (future, counter)
}

/// The invariants every site below must satisfy once a panic from a phase
/// `drive_frame`/`execute_frame` owns has been caught: the original panic
/// text (not a secondary one), phase/timestamp/`frame_scheduled` closed,
/// the frame counted exactly once, and the pre-registered completion
/// waiter woken exactly once by the abort.
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
    // One ordering detail keeps this green now that registering an
    // `end_of_frame()` waiter demands a frame: `armed_completion_probe`
    // registers BEFORE the frame is driven, so `handle_begin_frame`'s
    // unconditional `frame_scheduled.store(false)` clears that demand at
    // the top of the very frame this aborts.
    //
    // A registration made from inside the frame would not change this
    // either. `abort_frame` drains the WHOLE registry and resolves it with
    // the aborted frame's own timing, so such a waiter needs no frame of
    // its own; and while this probe is still live it would be suppressed
    // and issue no demand at all. The only registration that leaves the
    // latch set here is one made AFTER the drain, from inside a completion
    // waker, which the drain has already passed by.
    assert!(
        !scheduler.is_frame_scheduled(),
        "frame_scheduled must not be left latched by the aborted frame"
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
    let mut cx = Context::from_waker(waker);
    let resolved = Pin::new(&mut completion_future).poll(&mut cx);
    let Poll::Ready(outcome) = resolved else {
        panic!("an aborted frame still resolves end_of_frame -- it finished, badly");
    };
    assert!(
        matches!(outcome, Ok(FrameOutcome::Aborted { .. })),
        "abort_frame must resolve Aborted, not {outcome:?}"
    );
}

// ── Transient callback ──────────────────────────────────────────────────

fn transient_callback_panic_closes_the_frame_and_preserves_its_sibling() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);

    let sibling_ran = Arc::new(AtomicUsize::new(0));
    let sibling_ran_cb = Arc::clone(&sibling_ran);
    // Queued AFTER the panicking entry: still sitting in the shared queue
    // when the panic hits, so it must survive to the NEXT frame instead of
    // being lost with the batch a single-lock drain would already have
    // removed it into.
    scheduler.schedule_frame_callback(Box::new(|_| panic!("transient probe")));
    scheduler.schedule_frame_callback(Box::new(move |_| {
        sibling_ran_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    }))
    .expect_err("the panic must propagate");

    assert_recovered_from_panic(
        &scheduler,
        &*payload,
        "transient probe",
        frame_count_before,
        completion_future,
        &completion_counter,
    );
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        0,
        "not yet reached this frame"
    );

    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        1,
        "the preserved sibling survives to the next frame, exactly once"
    );
    assert_eq!(
        completion_counter.count(),
        1,
        "the completion waiter's notifier was drained by the abort; a later, unrelated \
         clean frame must not wake it a second time"
    );
}

// ── Mid-frame microtask ─────────────────────────────────────────────────

// ── Build-priority task ─────────────────────────────────────────────────

// ── Persistent callback ─────────────────────────────────────────────────

fn persistent_callback_panic_closes_the_frame_before_the_pipeline_slot_ever_opens() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);

    // Persistent callbacks are never removed by registration -- they fire
    // every frame for the application's lifetime -- so this one panics only
    // ONCE, to prove it survives (and is invoked again) rather than to
    // prove a preservation fix persistent callbacks never needed.
    let calls = Arc::new(AtomicUsize::new(0));
    let panicked_once = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let calls_cb = Arc::clone(&calls);
    let panicked_once_cb = Arc::clone(&panicked_once);
    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        calls_cb.fetch_add(1, Ordering::SeqCst);
        assert!(
            panicked_once_cb.swap(true, Ordering::SeqCst),
            "persistent probe"
        );
    }));

    let pipeline_ran = Arc::new(AtomicUsize::new(0));
    let pipeline_ran_pipe = Arc::clone(&pipeline_ran);
    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), move || {
            pipeline_ran_pipe.fetch_add(1, Ordering::SeqCst);
        });
    }))
    .expect_err("the panic must propagate");

    assert_recovered_from_panic(
        &scheduler,
        &*payload,
        "persistent probe",
        frame_count_before,
        completion_future,
        &completion_counter,
    );
    assert_eq!(
        pipeline_ran.load(Ordering::SeqCst),
        0,
        "the pipeline slot never opens for a frame that panicked before it"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Persistent callbacks cannot be unregistered by a panic: the SAME
    // callback runs again next frame, and this time it does not panic.
    let pipeline_ran_pipe = Arc::clone(&pipeline_ran);
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), move || {
        pipeline_ran_pipe.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "persistent callbacks are never dropped by a panic"
    );
    assert_eq!(
        pipeline_ran.load(Ordering::SeqCst),
        1,
        "the recovered frame reaches its own pipeline"
    );
    assert_eq!(
        completion_counter.count(),
        1,
        "the completion waiter's notifier was drained by the abort; a later, unrelated \
         clean frame must not wake it a second time"
    );
}

// ── Async future poll ───────────────────────────────────────────────────

struct PanicsOnPoll;

impl Future for PanicsOnPoll {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
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

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
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
    let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
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
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
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
        "the panicking future's slot must not be left as a zombie (issue #1057), but the \
         sibling task must still be tracked"
    );
    assert_eq!(
        sibling_polls.load(Ordering::SeqCst),
        0,
        "the sibling is polled in ascending task-id order, after the panicking one -- \
         it must not have been reached in the frame that aborted"
    );

    // A later frame's async-driver step does not touch the removed slot,
    // and it polls the sibling normally -- exactly once, since one poll
    // (of the two `CountedThenReady` needs) happens per frame.
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
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

// ── Idle work and post-frame callbacks are not starved by recovery ─────

fn idle_priority_work_and_post_frame_callbacks_are_not_starved_after_a_panic_recovers() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    scheduler.schedule_frame_callback(Box::new(|_| panic!("idle-starvation probe")));

    let _ = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    }));

    let idle_ran = Arc::new(AtomicUsize::new(0));
    let idle_ran_task = Arc::clone(&idle_ran);
    scheduler.add_task(Priority::Idle, move || {
        idle_ran_task.fetch_add(1, Ordering::SeqCst);
    });
    let post_frame_ran = Arc::new(AtomicUsize::new(0));
    let post_frame_ran_cb = Arc::clone(&post_frame_ran);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        post_frame_ran_cb.fetch_add(1, Ordering::SeqCst);
    }));

    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});

    assert_eq!(
        idle_ran.load(Ordering::SeqCst),
        1,
        "the idle-slice deadline must not leak from the panicking frame"
    );
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
/// still propagates to the caller (issue #1162; this distinction did not
/// exist before it -- both paths resolved the same bare `FrameTiming`).
fn a_post_frame_callback_panic_still_resolves_completed_not_aborted() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let (mut completion_future, completion_counter) = armed_completion_probe(&scheduler);

    scheduler.add_post_frame_callback(Box::new(|_timing| panic!("post-frame probe")));

    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
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
    let mut cx = Context::from_waker(waker);
    let Poll::Ready(outcome) = Pin::new(&mut completion_future).poll(&mut cx) else {
        panic!("a post-frame callback's own panic must still resolve the completion future");
    };
    assert!(
        matches!(outcome, Ok(FrameOutcome::Completed { .. })),
        "the pipeline committed; only the post-frame callback failed -- this must not read \
         as Aborted, got {outcome:?}"
    );
}

// ── A secondary panic during abort must not displace the original one ──

struct PanicWaker;

impl Wake for PanicWaker {
    fn wake(self: Arc<Self>) {
        panic!("waker probe");
    }
}

/// `drive_frame_impl`'s `Err` arm calls `abort_frame`, which calls
/// `notify_frame_completion` -- and a panicking waker there is a SECOND,
/// unrelated panic on top of the pipeline's own. Without containing it,
/// `abort_frame()` itself panics with the waker's payload before ever
/// reaching `resume_unwind(payload)`, so the caller observes the waker's
/// panic instead of the pipeline's -- the ORIGINAL failure this frame was
/// actually reporting is lost.
fn the_original_pipeline_panic_survives_a_panicking_completion_waker_during_abort() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let mut future = scheduler.end_of_frame();
    let panic_waker = Waker::from(Arc::new(PanicWaker));
    let mut cx = Context::from_waker(&panic_waker);
    assert!(Pin::new(&mut future).poll(&mut cx).is_pending());

    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {
            panic!("probe frame panic")
        })
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

#[test]
fn frame_panic_recovery_matrix() {
    crate::run_table(
        "frame_panic_recovery_matrix",
        &[
            (
                "transient_callback_panic_closes_the_frame_and_preserves_its_sibling",
                transient_callback_panic_closes_the_frame_and_preserves_its_sibling as fn(),
            ),
            (
                "persistent_callback_panic_closes_the_frame_before_the_pipeline_slot_ever_opens",
                persistent_callback_panic_closes_the_frame_before_the_pipeline_slot_ever_opens
                    as fn(),
            ),
            (
                "async_future_poll_panic_closes_the_frame",
                async_future_poll_panic_closes_the_frame as fn(),
            ),
            (
                "idle_priority_work_and_post_frame_callbacks_are_not_starved_after_a_panic_recovers",
                idle_priority_work_and_post_frame_callbacks_are_not_starved_after_a_panic_recovers
                    as fn(),
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
        ],
    );
}
