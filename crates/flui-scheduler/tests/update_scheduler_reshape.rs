//! Pinned end-state invariants for issue #556's `UpdateScheduler` reshape:
//! hard rename off `Scheduler`, `drive_frame(now, deadline, ..)`,
//! `budget()` guard retired, `VsyncScheduler` deleted.
//!
//! These are mutant-first exploits: each one is written to *fail* against
//! the pre-reshape shape, not merely to pass against the current one.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_scheduler::{IdleDeadline, Instant, MAX_BUILD_REENTRY_PASSES, Priority, UpdateScheduler};

/// Tiny-deadline exploit: a `deadline` that has already passed defers
/// `Priority::Idle` work, but `Priority::Animation` and `Priority::Build`
/// tasks still run to completion. Kills "deadline starves logical work" —
/// before this reshape there was no separate `deadline` at all, and a
/// regression that re-couples Build to the deadline (or drops Idle's own
/// gate) fails this test.
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
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(now, already_passed_deadline, || {}, || {})
        .expect("live owner frame");

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
    let (_frame_id, log) = flui_testing::log_capture::capture(|| {
        flui_scheduler::OwnerFrame::new(&scheduler)
            .expect("the scheduler has no live owner frame")
            .drive_frame(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {},
                || {},
            )
            .expect("live owner frame")
    });

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

fn exhausted_id_generator_keeps_refusing_after_panics() {
    let generator = flui_scheduler::IdGenerator::<flui_scheduler::markers::Frame>::starting_from(
        usize::MAX - 1,
    );
    assert_eq!(generator.next().get(), usize::MAX - 1);
    for _ in 0..3 {
        assert!(std::panic::catch_unwind(|| generator.next()).is_err());
    }
    generator.reset();
    assert_eq!(generator.next().get(), 1);
    assert_eq!(generator.next().get(), 2);
}

fn concurrent_id_exhaustion_admits_only_the_remaining_identity() {
    let generator = flui_scheduler::IdGenerator::<flui_scheduler::markers::Frame>::starting_from(
        usize::MAX - 1,
    );
    let admitted = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| std::panic::catch_unwind(|| generator.next())))
            .collect();
        workers
            .into_iter()
            .filter_map(|worker| worker.join().expect("worker contains generator panic").ok())
            .collect::<Vec<_>>()
    });
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].get(), usize::MAX - 1);
    assert!(std::panic::catch_unwind(|| generator.next()).is_err());
}

#[test]
fn update_scheduler_bounds_matrix() {
    crate::run_table(
        "update_scheduler_bounds_matrix",
        &[
            (
                "exhausted_id_generator_keeps_refusing_after_panics",
                exhausted_id_generator_keeps_refusing_after_panics,
            ),
            (
                "concurrent_id_exhaustion_admits_only_the_remaining_identity",
                concurrent_id_exhaustion_admits_only_the_remaining_identity,
            ),
            (
                "tiny_deadline_defers_idle_but_never_defers_build_or_animation",
                tiny_deadline_defers_idle_but_never_defers_build_or_animation as fn(),
            ),
            (
                "a_self_reenqueuing_build_task_is_bounded_by_the_reentry_cap_not_hung_forever",
                a_self_reenqueuing_build_task_is_bounded_by_the_reentry_cap_not_hung_forever
                    as fn(),
            ),
        ],
    );
}
