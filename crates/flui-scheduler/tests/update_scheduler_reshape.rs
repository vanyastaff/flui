//! Pinned end-state invariants for issue #556's `UpdateScheduler` reshape:
//! hard rename off `Scheduler`, `drive_frame(now, deadline, ..)`,
//! `budget()` guard retired, `VsyncScheduler` deleted.
//!
//! These are mutant-first exploits: each one is written to *fail* against
//! the pre-reshape shape, not merely to pass against the current one.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use flui_scheduler::{IdleDeadline, Instant, MAX_BUILD_REENTRY_PASSES, Priority, UpdateScheduler};

/// Tiny-deadline exploit: a `deadline` that has already passed defers
/// `Priority::Idle` work, but `Priority::Animation` and `Priority::Build`
/// tasks still run to completion. Kills "deadline starves logical work" —
/// before this reshape there was no separate `deadline` at all, and a
/// regression that re-couples Build to the deadline (or drops Idle's own
/// gate) fails this test.
#[test]
fn tiny_deadline_defers_idle_but_never_defers_build_or_animation() {
    let scheduler = UpdateScheduler::new();

    let animation_ran = Arc::new(AtomicBool::new(false));
    let build_ran = Arc::new(AtomicBool::new(false));
    let idle_ran = Arc::new(AtomicBool::new(false));

    {
        let flag = Arc::clone(&animation_ran);
        scheduler.add_task(Priority::Animation, move || {
            flag.store(true, Ordering::SeqCst);
        });
    }
    {
        let flag = Arc::clone(&build_ran);
        scheduler.add_task(Priority::Build, move || flag.store(true, Ordering::SeqCst));
    }
    {
        let flag = Arc::clone(&idle_ran);
        scheduler.add_task(Priority::Idle, move || flag.store(true, Ordering::SeqCst));
    }

    let now = Instant::now();
    // A deadline that has already passed by the time `handle_draw_frame`
    // checks it — the tightest possible Idle-slice.
    let already_passed_deadline = IdleDeadline(now);
    scheduler.drive_frame(now, already_passed_deadline, || {});

    assert!(
        animation_ran.load(Ordering::SeqCst),
        "Priority::Animation must run regardless of the deadline"
    );
    assert!(
        build_ran.load(Ordering::SeqCst),
        "Priority::Build must run regardless of the deadline — a deadline \
         bounds Idle-priority work only, never Build or Animation"
    );
    assert!(
        !idle_ran.load(Ordering::SeqCst),
        "Priority::Idle must be deferred once its deadline has passed"
    );
}

/// Reentrancy exploit: `TaskQueue::execute_until` bounds each call to an id
/// watermark captured once, under its first lock acquisition (see its own
/// doc in `task.rs`) — a task enqueued reentrantly during the call always
/// gets a strictly greater id and is left queued for the NEXT call, not this
/// one. A Priority::Animation task that itself enqueues Priority::Build work
/// during that same pass is therefore invisible to the SAME
/// `execute_until(Build)` call it ran inside of — but `handle_draw_frame`'s
/// own reentrant-pass loop calls `execute_until` again immediately, within
/// the same `handle_draw_frame` invocation, so the freshly-queued Build task
/// still runs in THIS frame, one pass later, not deferred a whole frame.
/// Kills a regression in `handle_draw_frame` that collapses the
/// Animation/Build drain into one non-reentrant pass — reentrant Build work
/// must still run THIS frame, not next, and an already-passed Idle deadline
/// must not matter to that (it only ever bounds Idle work).
#[test]
fn build_work_enqueued_reentrantly_by_an_animation_task_runs_this_frame() {
    let scheduler = UpdateScheduler::new();
    let build_ran = Arc::new(AtomicBool::new(false));

    {
        let reentrant_scheduler = scheduler.clone();
        let flag = Arc::clone(&build_ran);
        scheduler.add_task(Priority::Animation, move || {
            reentrant_scheduler.add_task(Priority::Build, move || {
                flag.store(true, Ordering::SeqCst);
            });
        });
    }

    let now = Instant::now();
    // An already-passed Idle deadline is part of the exploit: it proves the
    // reentrant Build work ran because of the Animation/Build drain, not
    // because a generous deadline let a *later* Idle-priority pass pick it
    // up by coincidence.
    scheduler.drive_frame(now, IdleDeadline(now), || {});

    assert!(
        build_ran.load(Ordering::SeqCst),
        "Build work enqueued by an Animation task during the same \
         handle_draw_frame pass must run in THIS frame, not be silently \
         deferred a whole frame"
    );
}

/// A Build task that unconditionally re-enqueues itself must not hang the
/// frame: `TaskQueue::execute_until`'s count budget (the number of tasks
/// already queued when THAT call started, read once under its first lock
/// acquisition) bounds each call to at most that many pops, so a
/// self-re-enqueuing chain runs once per call, not without limit inside a
/// single call — `handle_draw_frame`'s own reentrant-pass loop still gets to
/// count passes and give up at [`MAX_BUILD_REENTRY_PASSES`]. Before this
/// bound existed, a live re-peek of the heap absorbed the whole chain
/// inside ONE `execute_until` call and never returned, so the outer loop's
/// pass counter never advanced past 1 and this cap became unreachable dead
/// code.
///
/// The observed run count is `MAX_BUILD_REENTRY_PASSES + 1`, not the cap
/// itself: the reentry loop's 32nd pass warns and breaks with the
/// 33rd-re-enqueued task still queued (its own call's budget already spent)
/// -- but `handle_draw_frame` falls through, right after, to an
/// unconditional `execute_until(Priority::Idle)` sweep (skipped only once
/// the Idle deadline has passed, which a `far_future` deadline never does).
/// That threshold accepts ANY priority, so it picks up exactly that one
/// leftover Build task and runs it too, re-enqueuing a 34th that stays
/// queued for a genuinely next frame. This is not new: the same two-drain
/// shape existed before this bound and would have caught the same leftover
/// task the same way; the bound only changes how the FIRST 32 executions
/// are contained, not this trailing sweep.
#[test]
fn a_self_reenqueuing_build_task_is_bounded_by_the_reentry_cap_not_hung_forever() {
    let scheduler = UpdateScheduler::new();
    let runs = Arc::new(AtomicUsize::new(0));

    // A named fn, not a closure capturing itself: each execution re-enqueues
    // one more instance of itself, unconditionally, under its own scheduler
    // handle and shared counter.
    fn requeue(scheduler: UpdateScheduler, runs: Arc<AtomicUsize>) {
        let next_scheduler = scheduler.clone();
        let next_runs = Arc::clone(&runs);
        scheduler.add_task(Priority::Build, move || {
            next_runs.fetch_add(1, Ordering::SeqCst);
            requeue(next_scheduler, next_runs);
        });
    }
    requeue(scheduler.clone(), Arc::clone(&runs));

    // Terminates at all -- the primary regression this test guards -- and
    // the warning fires exactly once. `execute_frame` runs a full
    // `handle_draw_frame` reentrant-pass loop identically to `drive_frame`.
    let (_frame_id, log) = flui_testing::log_capture::capture(|| scheduler.execute_frame());

    assert_eq!(
        runs.load(Ordering::SeqCst),
        MAX_BUILD_REENTRY_PASSES + 1,
        "the reentry cap bounds the Build/Animation reentrant-pass loop to \
         MAX_BUILD_REENTRY_PASSES executions; the trailing, unconditional \
         execute_until(Priority::Idle) sweep right after picks up the ONE \
         task the cap left queued (its threshold accepts any priority) -- \
         see this test's own doc for why that +1 is not the count budget's \
         doing"
    );
    assert_eq!(
        log.count_containing("reentrant drain"),
        1,
        "the reentry-cap warning must fire exactly once: {log}"
    );
}

/// A caller driving the phase machine by hand (`handle_begin_frame` +
/// `handle_draw_frame`, skipping `drive_frame` entirely — the path
/// `HeadlessBinding` and direct unit tests use) has no deadline in play at
/// all, and Idle work must never be deferred for it either.
#[test]
fn no_deadline_set_means_idle_work_is_never_deferred() {
    let scheduler = UpdateScheduler::new();
    let idle_ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&idle_ran);
    scheduler.add_task(Priority::Idle, move || flag.store(true, Ordering::SeqCst));

    scheduler.handle_begin_frame(Instant::now());
    scheduler.handle_draw_frame();

    assert!(
        idle_ran.load(Ordering::SeqCst),
        "driving the phase machine directly (no `drive_frame`, no deadline) \
         must never defer Idle work"
    );
}

/// Panic-leak exploit: a task that panics inside `drive_frame` must not
/// leave the Idle-slice deadline permanently set. Without
/// `IdleDeadlineGuard`'s `Drop`, an already-passed deadline set at the top
/// of `drive_frame` survives an unwind through `handle_draw_frame` (nothing
/// sequential after the panicking call ever runs to clear it), and every
/// *later* frame — including one driven by hand, skipping `drive_frame`
/// entirely, as `HeadlessBinding` does — would see a deadline that has
/// always already passed and defer `Priority::Idle` forever.
#[test]
fn a_panicking_task_never_leaks_a_stale_idle_deadline() {
    let scheduler = UpdateScheduler::new();

    // First frame: an already-passed deadline (so the leak, if present,
    // would be visible immediately) plus a Build-priority task that panics
    // partway through `handle_draw_frame`, well before the point that would
    // ordinarily clear the deadline.
    let now = Instant::now();
    scheduler.add_task(Priority::Build, || panic!("task exploded"));
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(now, IdleDeadline(now), || {});
    }));
    assert!(
        unwound.is_err(),
        "the panicking task must actually unwind through drive_frame for \
         this exploit to mean anything"
    );
    // `drive_frame` only wraps `pipeline` in its own `catch_unwind`; a task
    // panicking inside `handle_begin_frame`/`handle_draw_frame` propagates
    // straight out, leaving the phase machine open exactly as it would for
    // a hand-driven caller — `abort_frame`'s own doc names this as the
    // caller's responsibility in that case. This is orthogonal to the
    // exploit itself (the Idle-deadline leak), so recover the phase machine
    // the documented way before driving a second frame.
    scheduler.abort_frame();

    // Second frame, driven by hand (no `drive_frame`, no deadline of its
    // own) — a leaked deadline from the first frame would defer Idle work
    // here too.
    let idle_ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&idle_ran);
    scheduler.add_task(Priority::Idle, move || flag.store(true, Ordering::SeqCst));
    scheduler.handle_begin_frame(Instant::now());
    scheduler.handle_draw_frame();

    assert!(
        idle_ran.load(Ordering::SeqCst),
        "a panicking task in an earlier frame must not leak its Idle-slice \
         deadline into later frames"
    );
}
