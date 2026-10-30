//! Pinned end-state invariants for issue #556's `UpdateScheduler` reshape:
//! hard rename off `Scheduler`, `drive_frame(now, deadline, ..)`,
//! `budget()` guard retired, `VsyncScheduler` deleted.
//!
//! These are mutant-first exploits: each one is written to *fail* against
//! the pre-reshape shape, not merely to pass against the current one.

use std::{
    cell::Cell,
    num::NonZeroU32,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use flui_foundation::{ManualClock, MonotonicClock as _};
use flui_scheduler::{
    ClockSource, DemandKind, FrameBudget, FrameClock, FrameId, FramePhase, FramePhaseDurations,
    FrameTimes, IdleDeadline, Instant, MAX_BUILD_REENTRY_PASSES, Milliseconds, PresentOutcome,
    PresentationAddress, PresentationId, Priority, SchedulerBuilder, UiRuntimeId, UpdateScheduler,
    frame_pace,
};

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
    // the warning fires exactly once during complete owner execution.
    let (_frame_id, log) = flui_testing::log_capture::capture(|| {
        flui_scheduler::OwnerFrame::new(&scheduler)
            .expect("the scheduler has no live owner frame")
            .drive_frame(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {},
                || {},
            )
            .expect("live owner frame");
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
        log.count_containing("drain passes"),
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
            (
                "a_fanning_out_task_runs_a_bounded_number_of_times_per_frame",
                a_fanning_out_task_runs_a_bounded_number_of_times_per_frame as fn(),
            ),
            (
                "a_fanning_out_microtask_runs_a_bounded_number_of_times_per_frame",
                a_fanning_out_microtask_runs_a_bounded_number_of_times_per_frame as fn(),
            ),
            (
                "work_queued_between_frames_requests_a_frame",
                work_queued_between_frames_requests_a_frame as fn(),
            ),
            (
                "idle_tasks_stop_at_the_deadline_one_task_at_a_time",
                idle_tasks_stop_at_the_deadline_one_task_at_a_time as fn(),
            ),
            (
                "jank_is_measured_on_the_schedulers_clock",
                jank_is_measured_on_the_schedulers_clock as fn(),
            ),
            (
                "frame_ids_count_per_scheduler",
                frame_ids_count_per_scheduler as fn(),
            ),
            (
                "a_frame_budget_refuses_non_finite_samples",
                a_frame_budget_refuses_non_finite_samples as fn(),
            ),
            (
                "a_late_frame_names_its_slowest_phase",
                a_late_frame_names_its_slowest_phase as fn(),
            ),
            (
                "frame_pace_ignores_idle_gaps",
                frame_pace_ignores_idle_gaps as fn(),
            ),
        ],
    );
}

/// Runs past this stop a fan-out instead of letting it exhaust memory, so a
/// regression fails fast rather than hanging the suite.
const FAN_OUT_GUARD: usize = 10_000;

/// A task that queues two more on every run doubles the queue each pass; one
/// frame runs a bounded number of them and requests the frame for the rest.
fn a_fanning_out_task_runs_a_bounded_number_of_times_per_frame() {
    fn fan_out(scheduler: UpdateScheduler, runs: Arc<AtomicUsize>) {
        let next = scheduler.clone();
        scheduler.add_task(Priority::Build, move || {
            let ran = runs.fetch_add(1, Ordering::SeqCst) + 1;
            assert!(
                ran < FAN_OUT_GUARD,
                "a fan-out ran {ran} times in one frame"
            );
            fan_out(next.clone(), Arc::clone(&runs));
            fan_out(next, runs);
        });
    }
    let scheduler = UpdateScheduler::new();
    let runs = Arc::new(AtomicUsize::new(0));
    fan_out(scheduler.clone(), Arc::clone(&runs));

    let now = Instant::now();
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
        .expect("live owner frame");

    let ran = runs.load(Ordering::SeqCst);
    assert!(
        (1..=256).contains(&ran),
        "one frame ran {ran} fanned-out tasks"
    );
    assert!(
        scheduler.is_frame_scheduled(),
        "the tasks left queued get the next frame"
    );
}

/// The microtask queue has the same bound: a fan-out flushed in one frame
/// stops at its run budget and requests the next frame for the rest.
fn a_fanning_out_microtask_runs_a_bounded_number_of_times_per_frame() {
    fn fan_out(scheduler: UpdateScheduler, runs: Arc<AtomicUsize>) {
        let next = scheduler.clone();
        scheduler.schedule_microtask(Box::new(move || {
            let ran = runs.fetch_add(1, Ordering::SeqCst) + 1;
            assert!(
                ran < FAN_OUT_GUARD,
                "a fan-out ran {ran} times in one frame"
            );
            fan_out(next.clone(), Arc::clone(&runs));
            fan_out(next, runs);
        }));
    }
    let scheduler = UpdateScheduler::new();
    let runs = Arc::new(AtomicUsize::new(0));
    {
        let scheduler_in_tick = scheduler.clone();
        let runs = Arc::clone(&runs);
        scheduler.schedule_frame_callback(Box::new(move |_| fan_out(scheduler_in_tick, runs)));
    }

    let now = Instant::now();
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
        .expect("live owner frame");

    let ran = runs.load(Ordering::SeqCst);
    assert!(
        (1..=256).contains(&ran),
        "one frame ran {ran} fanned-out microtasks"
    );
    assert!(
        scheduler.is_frame_scheduled(),
        "the microtasks left queued get the next frame"
    );
}

/// Work queued between frames asks for the frame that runs it.
fn work_queued_between_frames_requests_a_frame() {
    let microtask = UpdateScheduler::new();
    microtask.schedule_microtask(Box::new(|| {}));
    assert!(
        microtask.is_frame_scheduled(),
        "a microtask requests its frame"
    );

    let task = UpdateScheduler::new();
    task.add_task(Priority::Idle, || {});
    assert!(task.is_frame_scheduled(), "a task requests its frame");
}

/// The Idle deadline is checked before every Idle task, on the scheduler's
/// clock: the task that overruns it stops the next one, which waits for a
/// frame the scheduler requests.
fn idle_tasks_stop_at_the_deadline_one_task_at_a_time() {
    let clock = ManualClock::new();
    let scheduler = SchedulerBuilder::new()
        .clock(ClockSource::Manual(clock.clone()))
        .build();
    let ran = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let clock = clock.clone();
        let ran = Arc::clone(&ran);
        scheduler.add_task(Priority::Idle, move || {
            ran.fetch_add(1, Ordering::SeqCst);
            clock.advance(Duration::from_millis(10));
        });
    }

    let now = clock.now();
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(
            now,
            IdleDeadline(now + Duration::from_millis(5)),
            || {},
            || {},
        )
        .expect("live owner frame");

    assert_eq!(ran.load(Ordering::SeqCst), 1, "the second task waits");
    assert!(
        scheduler.is_frame_scheduled(),
        "the deferred task gets a frame"
    );
}

/// Jank is measured on the scheduler's clock: a frame whose pipeline
/// advances a virtual clock 30 ms is janky at 60 Hz, one that advances 5 ms
/// is not, however long either took on the wall clock.
fn jank_is_measured_on_the_schedulers_clock() {
    let clock = ManualClock::new();
    let scheduler = SchedulerBuilder::new()
        .clock(ClockSource::Manual(clock.clone()))
        .build();
    let owner =
        flui_scheduler::OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    for (work, janky_after) in [(30, 1), (5, 1)] {
        let now = clock.now();
        owner
            .drive_frame(
                now,
                IdleDeadline::far_future(now),
                || {},
                || clock.advance(Duration::from_millis(work)),
            )
            .expect("live owner frame");
        assert_eq!(
            scheduler.janky_frame_count(),
            janky_after,
            "after a frame of {work} ms"
        );
    }
}

/// Frame numbers belong to their scheduler: every scheduler's first frame is
/// frame 1, however many frames other schedulers ran.
fn frame_ids_count_per_scheduler() {
    for _ in 0..2 {
        let scheduler = UpdateScheduler::new();
        let observed = Rc::new(Cell::new(None));
        let now = Instant::now();
        let scheduler_in_frame = scheduler.clone();
        let observed_in_frame = Rc::clone(&observed);
        flui_scheduler::OwnerFrame::new(&scheduler)
            .expect("the scheduler has no live owner frame")
            .drive_frame(
                now,
                IdleDeadline::far_future(now),
                || {},
                move || {
                    observed_in_frame.set(scheduler_in_frame.current_frame().map(|frame| frame.id));
                },
            )
            .expect("live owner frame");
        assert_eq!(observed.get(), Some(FrameId::FIRST));
    }
}

/// A non-finite or negative sample is refused and cannot poison the rolling
/// average.
fn a_frame_budget_refuses_non_finite_samples() {
    let mut budget = FrameBudget::new(NonZeroU32::new(60).expect("60 is non-zero"));
    assert!(budget.record_frame_duration(Milliseconds::new(10.0)));
    for sample in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(!budget.record_frame_duration(Milliseconds::new(sample)));
    }
    assert_eq!(budget.frame_count(), 1);
    assert_eq!(budget.avg_frame_time(), Milliseconds::new(10.0));
}

/// A snapshot names the phase that made a late frame late, including the
/// submit the segment's end leaves to the backend.
fn a_late_frame_names_its_slowest_phase() {
    let clock = FrameClock::new();
    clock.mark_demand(DemandKind::Dirty);
    let start = Instant::now();
    assert!(clock.poll(start).should_run_segment());
    let ms = Duration::from_millis;
    let snapshot = clock.record_frame(
        address(),
        FrameTimes {
            clock_timestamp: start,
            segment_start: start,
            segment_end: start + ms(9),
            submit_at: start + ms(30),
            phases: FramePhaseDurations::new(ms(2), ms(3), ms(4)),
        },
        PresentOutcome::Presented,
    );
    assert_eq!(snapshot.phase_duration(FramePhase::Paint), ms(4));
    assert_eq!(snapshot.phase_duration(FramePhase::Composite), ms(21));
    assert_eq!(
        snapshot.over_budget_phase(ms(16)),
        Some(FramePhase::Composite)
    );
    assert_eq!(snapshot.over_budget_phase(ms(40)), None);
}

/// The one presentation these frames belong to.
fn address() -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(1),
    }
}

/// Idle time between bursts of frames is neither a slow frame nor a low
/// frame rate.
fn frame_pace_ignores_idle_gaps() {
    let clock = FrameClock::new();
    let start = Instant::now();
    let ms = Duration::from_millis;
    let snapshots: Vec<_> = [0, 16, 5_016, 5_032]
        .into_iter()
        .map(|at| {
            clock.mark_demand(DemandKind::Dirty);
            let begun = start + ms(at);
            assert!(clock.poll(begun).should_run_segment());
            clock.record_frame(
                address(),
                FrameTimes {
                    clock_timestamp: begun,
                    segment_start: begun,
                    segment_end: begun + ms(3),
                    submit_at: begun + ms(4),
                    phases: FramePhaseDurations::default(),
                },
                PresentOutcome::Presented,
            )
        })
        .collect();

    let pace = frame_pace(&snapshots);
    assert!((pace.frames_per_second - 62.5).abs() < 1e-6, "{pace:?}");
    assert_eq!(pace.frame_work, ms(4));
}
