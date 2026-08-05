//! Pinned end-state invariants for issue #556's `UpdateScheduler` reshape:
//! hard rename off `Scheduler`, `drive_frame(now, deadline, ..)`,
//! `budget()` guard retired, `VsyncScheduler` deleted.
//!
//! These are mutant-first exploits: each one is written to *fail* against
//! the pre-reshape shape, not merely to pass against the current one.

use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use flui_scheduler::{Instant, Priority, UpdateScheduler};

/// Phase-order canary: the async driver must still refuse to poll while the
/// scheduler is in `PersistentCallbacks` (build/layout/paint). This is the
/// same fence `drive_async_tasks` has always asserted — pinned again here,
/// under the renamed/reshaped type, so a future phase reorder that removes
/// or weakens the assert is caught by *this* slice's own test, not only by
/// `scheduler.rs`'s pre-existing unit test.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "BUG: the async driver must not poll during build/layout/paint")]
fn phase_order_canary_drive_async_tasks_still_refuses_the_persistent_phase() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        // Reordering phases so the async driver could be reached from here
        // must still trip this assert.
        probe.drive_async_tasks();
    }));

    scheduler.handle_begin_frame(Instant::now());
    scheduler.handle_draw_frame();
}

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
    let already_passed_deadline = now;
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

/// The converse: a deadline far in the future defers nothing — Idle work
/// runs too. Anti-vacuous pair for the exploit above (a scheduler that
/// *never* runs Idle work would also pass the first assertion above by
/// accident; this one catches that).
#[test]
fn generous_deadline_lets_idle_work_run_too() {
    let scheduler = UpdateScheduler::new();
    let idle_ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&idle_ran);
    scheduler.add_task(Priority::Idle, move || flag.store(true, Ordering::SeqCst));

    let now = Instant::now();
    scheduler.drive_frame(now, now + Duration::from_hours(1), || {});

    assert!(
        idle_ran.load(Ordering::SeqCst),
        "a deadline far in the future must not defer Idle work"
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

/// End-state registry sweep (mutant-first, red-exploit for this reshape):
/// `flui-scheduler`'s own source must contain none of —
///
/// - `FPS_60` — the deleted fixed 60fps default constant/constructor
///   assumption (`FrameDuration::try_from_fps(60)` is fine; the *named*
///   constant is gone).
/// - `16.67` — the associated magic-number literal this reshape retired
///   alongside the constant.
/// - `fn budget(` — the retired `MutexGuard`-returning accessor; replaced by
///   [`UpdateScheduler::budget_snapshot`], which returns an owned value.
///
/// Scoped to `src/` (the shipped crate surface the invariant is actually
/// about), not `tests/`/`examples/`, which may legitimately pick round
/// millisecond numbers as arbitrary test fixtures unrelated to this
/// constant (e.g. `FrameSkipPolicy::frames_to_skip`'s own algorithm tests).
#[test]
fn no_fixed_frame_rate_constant_or_guard_returning_budget_accessor_in_source() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src_dir = Path::new(manifest_dir).join("src");

    let forbidden: [&str; 3] = ["FPS_60", "16.67", "fn budget("];
    let mut violations = Vec::new();

    let mut stack = vec![src_dir];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("flui-scheduler/src must exist") {
            let entry = entry.expect("readable dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let contents = fs::read_to_string(&path).expect("valid utf-8 source file");
            for pattern in forbidden {
                if contents.contains(pattern) {
                    violations.push(format!("{}: contains {pattern:?}", path.display()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "flui-scheduler/src must not reintroduce a fixed frame-rate default \
         or a lock-guard-returning budget accessor:\n{}",
        violations.join("\n"),
    );
}
