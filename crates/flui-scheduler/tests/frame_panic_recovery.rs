//! Issue #1057: the shared frame driver must close its own bookkeeping —
//! phase, `frame_scheduled`, frame count, and frame-completion waiters —
//! before propagating a panic from ANY phase it owns, not merely a
//! panicking pipeline.
//!
//! Each test below panics BEFORE the pipeline slot even opens: a transient
//! callback, a mid-frame microtask, a `Priority::Build` task, a persistent
//! callback, and an async future's first poll. Two more prove the fix is
//! not pipeline-`drive_frame`-specific: one drives the identical scenario
//! through `drive_frame_with_lane` (the owner-local-lane entry point), and
//! one through `execute_frame` (the no-pipeline convenience path, which now
//! shares `drive_frame`'s own recovery boundary rather than hand-rolling a
//! second, unguarded sequence). A last test confirms recovery does not
//! starve `Priority::Idle` work or a clean frame's post-frame callbacks.
//!
//! `crates/flui-scheduler/tests/post_frame_callback_ordering.rs` already
//! covers a panicking PIPELINE — that path was fixed before this issue and
//! stays green, unmodified, alongside these.
//!
//! # Divergence from Flutter, named rather than assumed
//!
//! `.flutter/packages/flutter/lib/src/scheduler/binding.dart` @ 3.44.0 never
//! reaches the state this file is guarding against: `handleBeginFrame`'s
//! `finally` resets `_schedulerPhase` to `SchedulerPhase.midFrameMicrotasks`
//! around its own transient-callback loop, and `handleDrawFrame`'s `finally`
//! resets it to `SchedulerPhase.idle` — but `_invokeFrameCallback` wraps
//! EVERY individual transient/persistent/post-frame callback in its own
//! `FlutterError`-reporting boundary, so a throwing callback never unwinds
//! Dart's call stack at all. Those `finally` blocks exist for symmetry with
//! `handleDrawFrame`'s pipeline-exception handling, not because a callback
//! panic ever reaches them. FLUI does not isolate per callback — a panic
//! here poisons and propagates the whole frame, unchanged by this issue —
//! so what these tests pin is narrower than Flutter's contract: the
//! scheduler's OWN bookkeeping closes cleanly no matter which phase raised
//! the panic, while the panic itself still escapes to the caller.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use flui_scheduler::{
    FrameCompletionFuture, IdleDeadline, Instant, Priority, SchedulerPhase, UpdateScheduler,
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
    assert!(
        Pin::new(&mut completion_future).poll(&mut cx).is_ready(),
        "an aborted frame still resolves end_of_frame -- it finished, badly"
    );
}

// ── Transient callback ──────────────────────────────────────────────────

#[test]
fn transient_callback_panic_closes_the_frame_and_preserves_its_sibling() {
    let scheduler = UpdateScheduler::new();
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
        scheduler.drive_frame(Instant::now(), far_deadline(), || {});
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

    scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        1,
        "the preserved sibling survives to the next frame, exactly once"
    );
}

// ── Mid-frame microtask ─────────────────────────────────────────────────

#[test]
fn mid_frame_microtask_panic_closes_the_frame_and_preserves_its_sibling() {
    let scheduler = UpdateScheduler::new();
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);

    let sibling_ran = Arc::new(AtomicUsize::new(0));
    let sibling_ran_task = Arc::clone(&sibling_ran);
    scheduler.schedule_microtask(Box::new(|| panic!("microtask probe")));
    scheduler.schedule_microtask(Box::new(move || {
        sibling_ran_task.fetch_add(1, Ordering::SeqCst);
    }));

    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    }))
    .expect_err("the panic must propagate");

    assert_recovered_from_panic(
        &scheduler,
        &*payload,
        "microtask probe",
        frame_count_before,
        completion_future,
        &completion_counter,
    );
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        0,
        "not yet reached this frame"
    );

    scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        1,
        "the preserved microtask survives to the next frame, exactly once"
    );
}

// ── Build-priority task ─────────────────────────────────────────────────

#[test]
fn build_priority_task_panic_closes_the_frame_and_preserves_its_sibling() {
    let scheduler = UpdateScheduler::new();
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);

    let sibling_ran = Arc::new(AtomicUsize::new(0));
    let sibling_ran_task = Arc::clone(&sibling_ran);
    let queue_len_before = scheduler.task_queue().len();
    scheduler.add_task(Priority::Build, || panic!("build task probe"));
    scheduler.add_task(Priority::Build, move || {
        sibling_ran_task.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(scheduler.task_queue().len(), queue_len_before + 2);

    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    }))
    .expect_err("the panic must propagate");

    assert_recovered_from_panic(
        &scheduler,
        &*payload,
        "build task probe",
        frame_count_before,
        completion_future,
        &completion_counter,
    );
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        0,
        "not yet reached this frame"
    );
    assert_eq!(
        scheduler.task_queue().len(),
        queue_len_before + 1,
        "the panicking task is gone; the sibling is still queued -- no underflow, no resurrection"
    );

    scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        1,
        "the preserved task survives to the next frame, exactly once"
    );
    assert_eq!(scheduler.task_queue().len(), queue_len_before);
}

// ── Persistent callback ─────────────────────────────────────────────────

#[test]
fn persistent_callback_panic_closes_the_frame_before_the_pipeline_slot_ever_opens() {
    let scheduler = UpdateScheduler::new();
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
        scheduler.drive_frame(Instant::now(), far_deadline(), move || {
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
    scheduler.drive_frame(Instant::now(), far_deadline(), move || {
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
}

// ── Async future poll ───────────────────────────────────────────────────

struct PanicsOnPoll;

impl Future for PanicsOnPoll {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        panic!("async poll probe");
    }
}

#[test]
fn async_future_poll_panic_closes_the_frame() {
    let scheduler = UpdateScheduler::new();
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);
    let pending_before = scheduler.pending_task_count();

    let _token = scheduler.spawn_local(Box::pin(PanicsOnPoll));
    assert_eq!(scheduler.pending_task_count(), pending_before + 1);

    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), far_deadline(), || {});
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
    assert_eq!(
        scheduler.pending_task_count(),
        pending_before,
        "the panicking future's slot must not be left as a zombie (issue #1057)"
    );

    // A later frame's async-driver step does not touch the removed slot.
    scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    assert_eq!(scheduler.pending_task_count(), pending_before);
}

// ── Owner-local-lane entry point ────────────────────────────────────────

#[test]
fn transient_callback_panic_recovers_identically_through_drive_frame_with_lane() {
    let scheduler = UpdateScheduler::new();
    let lane = scheduler.new_local_post_frame_lane();
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);

    scheduler.schedule_frame_callback(Box::new(|_| panic!("lane transient probe")));

    let payload = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame_with_lane(Instant::now(), far_deadline(), || {}, &lane);
    }))
    .expect_err("the panic must propagate");

    assert_recovered_from_panic(
        &scheduler,
        &*payload,
        "lane transient probe",
        frame_count_before,
        completion_future,
        &completion_counter,
    );

    // The owner-local lane recovers too: a clean frame through the SAME
    // entry point still drains it.
    let local_ran = Arc::new(AtomicUsize::new(0));
    let local_ran_cb = Arc::clone(&local_ran);
    lane.local_handle()
        .schedule_local(move |_timing| {
            local_ran_cb.fetch_add(1, Ordering::SeqCst);
        })
        .expect("lane is alive after recovery");
    scheduler.drive_frame_with_lane(Instant::now(), far_deadline(), || {}, &lane);
    assert_eq!(local_ran.load(Ordering::SeqCst), 1);
}

// ── execute_frame (ALT-1: the no-pipeline convenience path) ────────────

#[test]
fn transient_callback_panic_recovers_identically_through_execute_frame() {
    let scheduler = UpdateScheduler::new();
    let frame_count_before = scheduler.frame_count();
    let (completion_future, completion_counter) = armed_completion_probe(&scheduler);

    scheduler.schedule_frame_callback(Box::new(|_| panic!("execute_frame transient probe")));

    let payload = catch_unwind(AssertUnwindSafe(|| scheduler.execute_frame()))
        .expect_err("the panic must propagate");

    assert_recovered_from_panic(
        &scheduler,
        &*payload,
        "execute_frame transient probe",
        frame_count_before,
        completion_future,
        &completion_counter,
    );

    // `execute_frame` keeps its clean-path contract too: a complete frame
    // whose post-frame callbacks run.
    let post_frame_ran = Arc::new(AtomicUsize::new(0));
    let post_frame_ran_cb = Arc::clone(&post_frame_ran);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        post_frame_ran_cb.fetch_add(1, Ordering::SeqCst);
    }));
    scheduler.execute_frame();
    assert_eq!(post_frame_ran.load(Ordering::SeqCst), 1);
}

// ── Idle work and post-frame callbacks are not starved by recovery ─────

#[test]
fn idle_priority_work_and_post_frame_callbacks_are_not_starved_after_a_panic_recovers() {
    let scheduler = UpdateScheduler::new();
    scheduler.schedule_frame_callback(Box::new(|_| panic!("idle-starvation probe")));

    let _ = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), far_deadline(), || {});
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

    scheduler.drive_frame(Instant::now(), far_deadline(), || {});

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
