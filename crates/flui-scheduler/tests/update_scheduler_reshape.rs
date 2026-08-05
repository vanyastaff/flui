//! Pinned end-state invariants for issue #556's `UpdateScheduler` reshape:
//! hard rename off `Scheduler`, `drive_frame(now, deadline, ..)`,
//! `budget()` guard retired, `VsyncScheduler` deleted.
//!
//! These are mutant-first exploits: each one is written to *fail* against
//! the pre-reshape shape, not merely to pass against the current one.

use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use flui_scheduler::{IdleDeadline, Instant, Priority, UpdateScheduler};

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
    scheduler.drive_frame(now, IdleDeadline::far_future(now), || {});

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

/// A line legitimately allowed to spell a hardcoded 60fps reference, with
/// its reason recorded — a **shrink-only allowlist**, not a blanket
/// exclusion. Every entry here is a caller-facing *stats* default
/// (`FrameBudget`/`FrameTiming`/`FrameDuration`, none of which gate
/// anything) or an unrelated unit test/doctest verifying that type's own
/// arithmetic at fps=60. None of them is `UpdateScheduler` itself carrying
/// a frame-duration/target-fps field or accessor — that surface is deleted
/// outright (see the exact-zero patterns below) and MUST NOT reappear here.
///
/// New matches not listed here fail the sweep below; a stale entry (listed
/// here but no longer present in the file) also fails it, so this list
/// cannot silently drift out of sync with the source it describes.
const ALLOWED_SIXTY_FPS_LINES: &[(&str, &str, &str)] = &[
    (
        "src/scheduler.rs",
        "budget_target: FrameDuration::try_from_fps(60).expect(\"BUG: 60 fps is always valid\"),",
        "SchedulerBuilder::new()'s own default — the sole site in this crate \
         that declares the stats budget's fallback target; UpdateScheduler \
         itself reads it back through nothing but `budget_snapshot()`",
    ),
    (
        "src/budget.rs",
        "FrameDuration::try_from_fps(60).expect(\"BUG: 60 fps is always valid\")",
        "FrameBudgetBuilder::build()'s fallback when the caller configures \
         neither target_fps nor frame_duration — an ordinary builder \
         default, unrelated to UpdateScheduler's own (deleted) surface",
    ),
    (
        "src/frame.rs",
        "FrameDuration::try_from_fps(60).expect(\"BUG: 60 fps is always valid\")",
        "FrameTimingBuilder::build()'s identical fallback",
    ),
    (
        "src/duration.rs",
        "//! let budget = FrameDuration::try_from_fps(60).expect(\"60 fps valid\");",
        "module doctest demonstrating FrameDuration's own API, not a \
         scheduler default",
    ),
    (
        "src/duration.rs",
        "let budget = FrameDuration::try_from_fps(60).expect(\"fps > 0\");",
        "duration.rs's own unit test verifying try_from_fps/FrameDuration \
         arithmetic at a representative fps value",
    ),
    (
        "src/duration.rs",
        "//! assert!((budget.as_ms().value() - 1000.0 / 60.0).abs() < 0.001);",
        "same doctest as above, checking the computed millisecond value",
    ),
    (
        "src/frame.rs",
        "assert!((timing.target_duration_ms() - 1000.0 / 60.0).abs() < 0.01);",
        "frame.rs's own unit test verifying FrameTiming::new(60)'s target \
         duration arithmetic",
    ),
    (
        "src/budget.rs",
        "assert!((budget.target_duration_ms() - 1000.0 / 60.0).abs() < 0.01);",
        "budget.rs's own unit test verifying FrameBudget::new(60)'s target \
         duration arithmetic",
    ),
    (
        "src/lib.rs",
        "//! let budget = FrameDuration::try_from_fps(60).expect(\"fps > 0\"); // ~60fps budget",
        "crate-root doctest demonstrating FrameDuration's own API",
    ),
];

/// End-state registry sweep (mutant-first, red-exploit for this reshape):
/// `flui-scheduler`'s own source must contain none of —
///
/// - `FPS_60` — the deleted fixed 60fps default constant/constructor
///   assumption. Zero tolerance: no allowlist entry may use it.
/// - `16.67` — the associated magic-number literal this reshape retired
///   alongside the constant. Zero tolerance.
/// - `fn budget(` — the retired `MutexGuard`-returning accessor; replaced by
///   [`UpdateScheduler::budget_snapshot`], which returns an owned value.
///   Zero tolerance.
/// - `try_from_fps(60)` and `1000.0 / 60` — a hardcoded 60fps value.
///   Tolerated **only** on the exact lines named in
///   [`ALLOWED_SIXTY_FPS_LINES`], each with a stated, caller-facing-stats
///   reason; every other occurrence is a violation, catching the specific
///   mutant this exploit is for: a fresh `UpdateScheduler`-level 60fps
///   constant reintroduced under a different spelling than `FPS_60`.
///
/// Scoped to `src/` (the shipped crate surface the invariant is actually
/// about), not `tests/`/`examples/`.
#[test]
fn no_fixed_frame_rate_constant_or_guard_returning_budget_accessor_in_source() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest_dir);
    let src_dir = root.join("src");

    let zero_tolerance: [&str; 3] = ["FPS_60", "16.67", "fn budget("];
    let allowlisted: [&str; 2] = ["try_from_fps(60)", "1000.0 / 60"];

    let mut violations = Vec::new();
    let mut allowlist_seen = vec![false; ALLOWED_SIXTY_FPS_LINES.len()];

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
            let rel = path
                .strip_prefix(root)
                .expect("path under manifest root")
                .to_string_lossy()
                .replace('\\', "/");

            for pattern in zero_tolerance {
                if contents.contains(pattern) {
                    violations.push(format!("{rel}: contains {pattern:?} (zero-tolerance)"));
                }
            }

            for line in contents.lines() {
                let trimmed = line.trim();
                if !allowlisted.iter().any(|pattern| trimmed.contains(pattern)) {
                    continue;
                }
                match ALLOWED_SIXTY_FPS_LINES
                    .iter()
                    .position(|(file, text, _)| *file == rel && *text == trimmed)
                {
                    Some(index) => allowlist_seen[index] = true,
                    None => violations.push(format!(
                        "{rel}: un-allowlisted hardcoded 60fps line: {trimmed:?}"
                    )),
                }
            }
        }
    }

    for (index, (file, text, reason)) in ALLOWED_SIXTY_FPS_LINES.iter().enumerate() {
        if !allowlist_seen[index] {
            violations.push(format!(
                "stale allowlist entry no longer found in {file} ({reason}): {text:?}"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "flui-scheduler/src must not reintroduce a fixed frame-rate default, \
         a lock-guard-returning budget accessor, or an un-reviewed hardcoded \
         60fps line:\n{}",
        violations.join("\n"),
    );
}
