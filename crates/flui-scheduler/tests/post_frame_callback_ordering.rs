//! ADR-0021 U1.5: post-frame callbacks run **after** the pipeline, in the same frame.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/lib/src/scheduler/binding.dart:1338-1378`
//! (`handleDrawFrame`: persistent phase, then post-frame phase, inside a
//! `try { … } finally { _schedulerPhase = idle; }`);
//! `.../rendering/binding.dart:61`, `:557-558` (`drawFrame()` registered as the
//! first persistent callback). Expected values are read from the reference, not
//! from running this code.
//!
//! # The divergence this file documents, and does not claim away
//!
//! In Flutter the pipeline **is** a persistent callback. In FLUI the pipeline is
//! a closure passed to `Scheduler::drive_frame`, so a *registered* persistent
//! callback runs **before** it. Post-frame ordering — the only thing
//! `HeroController` needs — matches. Persistent-phase ordering does not, and
//! `persistent_callbacks_run_before_the_pipeline` pins that honestly.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_scheduler::{Instant, Scheduler, SchedulerPhase};
use parking_lot::Mutex;

/// Append-only log of the order things happened in.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<&'static str>>>);

impl Log {
    fn push(&self, s: &'static str) {
        self.0.lock().push(s);
    }
    fn get(&self) -> Vec<&'static str> {
        self.0.lock().clone()
    }
}

/// `handleDrawFrame`'s two phases, in order: persistent, then post-frame
/// (`scheduler/binding.dart:1343-1358`). The pipeline sits in the persistent
/// slot, so a post-frame callback must observe everything it did.
#[test]
fn drive_frame_runs_post_frame_callbacks_after_the_pipeline() {
    let scheduler = Scheduler::new();
    let log = Log::default();

    let log_cb = log.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        log_cb.push("post_frame");
    }));

    let log_pipe = log.clone();
    scheduler.drive_frame(Instant::now(), || {
        log_pipe.push("pipeline");
    });

    assert_eq!(log.get(), vec!["pipeline", "post_frame"]);
}

/// The negative half: the callback must not have run when the pipeline is
/// executing. Before U1.5 the production runner drained the queue *first*, so
/// this is the exact regression under guard.
#[test]
fn post_frame_callback_does_not_run_before_the_pipeline() {
    let scheduler = Scheduler::new();
    let fired = Arc::new(AtomicUsize::new(0));

    let fired_cb = Arc::clone(&fired);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let fired_pipe = Arc::clone(&fired);
    let seen_during_pipeline = scheduler.drive_frame(Instant::now(), || {
        assert_eq!(
            Scheduler::new().phase(),
            SchedulerPhase::Idle,
            "sanity: a fresh scheduler is idle"
        );
        fired_pipe.load(Ordering::SeqCst)
    });

    assert_eq!(
        seen_during_pipeline, 0,
        "the callback ran before the pipeline"
    );
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

/// The pipeline occupies the `PersistentCallbacks` slot — the same slot Flutter's
/// `drawFrame` occupies as a persistent callback.
#[test]
fn the_pipeline_runs_in_the_persistent_callbacks_phase() {
    let scheduler = Scheduler::new();
    let probe = scheduler.clone();
    let phase = scheduler.drive_frame(Instant::now(), || probe.phase());
    assert_eq!(phase, SchedulerPhase::PersistentCallbacks);
}

/// `_postFrameCallbacks` are "called exactly once" (`binding.dart:802`).
#[test]
fn post_frame_callback_runs_exactly_once_across_two_frames() {
    let scheduler = Scheduler::new();
    let calls = Arc::new(AtomicUsize::new(0));

    let calls_cb = Arc::clone(&calls);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        calls_cb.fetch_add(1, Ordering::SeqCst);
    }));

    scheduler.drive_frame(Instant::now(), || {});
    scheduler.drive_frame(Instant::now(), || {});

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// Flutter copies the list and clears it **before** invoking
/// (`scheduler/binding.dart:1350-1351`), so a callback registered from inside a
/// post-frame callback lands on the next frame, not this one.
#[test]
fn a_post_frame_callback_registered_from_a_post_frame_callback_defers_to_the_next_frame() {
    let scheduler = Scheduler::new();
    let log = Log::default();

    let inner_scheduler = scheduler.clone();
    let log_outer = log.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        log_outer.push("outer");
        let log_inner = log_outer.clone();
        inner_scheduler.add_post_frame_callback(Box::new(move |_| {
            log_inner.push("inner");
        }));
    }));

    scheduler.drive_frame(Instant::now(), || {});
    assert_eq!(log.get(), vec!["outer"], "the inner callback must defer");

    scheduler.drive_frame(Instant::now(), || {});
    assert_eq!(log.get(), vec!["outer", "inner"]);
}

/// The frame closes cleanly.
#[test]
fn phase_is_idle_after_a_successful_frame() {
    let scheduler = Scheduler::new();
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
    scheduler.drive_frame(Instant::now(), || {});
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
}

/// A pipeline that **returns an error value** is a completed frame: post-frame
/// callbacks fire, exactly once, and the phase settles.
#[test]
fn a_pipeline_returning_an_error_still_completes_the_frame() {
    let scheduler = Scheduler::new();
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let result: Result<(), &str> = scheduler.drive_frame(Instant::now(), || Err("render failed"));

    assert_eq!(result, Err("render failed"));
    assert_eq!(fired.load(Ordering::SeqCst), 1, "an error is not an abort");
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
}

/// A **panicking** pipeline is an abandoned frame: `drive_frame` catches the
/// panic, calls `abort_frame` (phase → `Idle`, **no** post-frame callbacks), and
/// resumes the unwind. The queued callbacks survive to the next completed frame.
///
/// This mirrors Flutter: a throwing persistent callback skips the post-frame loop,
/// and `finally { _schedulerPhase = idle; }` still resets the phase
/// (`scheduler/binding.dart:1341-1374`).
///
/// `abort_frame` must not go through `set_scheduler_phase`: `PersistentCallbacks
/// -> Idle` is an illegal transition, so its `debug_assert!` would fire and — were
/// this a `Drop` guard running during unwind — double-panic into `abort`.
#[test]
fn a_panicking_pipeline_aborts_the_frame_and_runs_no_post_frame_callbacks() {
    let scheduler = Scheduler::new();
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), || panic!("pipeline exploded"));
    }))
    .is_err();
    assert!(panicked, "the panic must propagate, not be swallowed");

    assert_eq!(
        fired.load(Ordering::SeqCst),
        0,
        "post-frame callbacks must not run for a frame that never finished"
    );
    assert_eq!(
        scheduler.phase(),
        SchedulerPhase::Idle,
        "the frame must be reset, or the NEXT begin_frame trips the phase assert"
    );

    // The recovered scheduler drives a clean frame, and the queued callback runs.
    scheduler.drive_frame(Instant::now(), || {});
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

/// The guard the previous test's `Idle` assertion protects: a frame left open at
/// `PersistentCallbacks` would make the next `handle_begin_frame` attempt an
/// illegal `PersistentCallbacks -> TransientCallbacks` transition.
#[test]
fn a_frame_after_a_panicking_frame_starts_cleanly() {
    let scheduler = Scheduler::new();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), || panic!("boom"));
    }));

    let ran = Arc::new(AtomicUsize::new(0));
    let ran_cb = Arc::clone(&ran);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        ran_cb.fetch_add(1, Ordering::SeqCst);
    }));

    // Would `debug_assert!` on the illegal transition if the frame were still open.
    scheduler.drive_frame(Instant::now(), || {});
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
}

/// `abort_frame` is idempotent and a no-op outside a frame.
#[test]
fn abort_frame_is_a_no_op_when_no_frame_is_open() {
    let scheduler = Scheduler::new();
    scheduler.abort_frame();
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
    scheduler.drive_frame(Instant::now(), || {});
    scheduler.abort_frame();
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
}

/// `execute_frame` keeps its pre-U1.5 behavior: a complete frame whose post-frame
/// callbacks run. It is `begin → persistent → end` with no pipeline.
#[test]
fn execute_frame_is_begin_persistent_end_and_still_drains_post_frame() {
    let scheduler = Scheduler::new();
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    scheduler.execute_frame();

    assert_eq!(fired.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
}

/// `handle_draw_frame` alone no longer finishes the frame — it hands the
/// persistent slot to the caller. Characterizes the split.
#[test]
fn handle_draw_frame_alone_no_longer_drains_post_frame_callbacks() {
    let scheduler = Scheduler::new();
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    scheduler.handle_begin_frame(Instant::now());
    scheduler.handle_draw_frame();

    assert_eq!(fired.load(Ordering::SeqCst), 0);
    assert_eq!(scheduler.phase(), SchedulerPhase::PersistentCallbacks);

    scheduler.end_frame();
    assert_eq!(fired.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
}

/// **The remaining divergence, pinned rather than claimed away.**
///
/// Flutter registers `drawFrame()` as the first persistent callback
/// (`rendering/binding.dart:61`, `:557-558`), so a persistent callback added
/// later runs *after* the pipeline. FLUI's pipeline is a closure, so every
/// registered persistent callback runs *before* it. Nothing in the framework
/// registers one today.
#[test]
fn persistent_callbacks_run_before_the_pipeline_a_divergence_from_flutter() {
    let scheduler = Scheduler::new();
    let log = Log::default();

    let log_persistent = log.clone();
    scheduler.add_persistent_frame_callback(Arc::new(move |_| {
        log_persistent.push("persistent");
    }));

    let log_pipe = log.clone();
    let log_post = log.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        log_post.push("post_frame");
    }));
    scheduler.drive_frame(Instant::now(), || {
        log_pipe.push("pipeline");
    });

    assert_eq!(
        log.get(),
        vec!["persistent", "pipeline", "post_frame"],
        "in Flutter the pipeline IS the first persistent callback; here it follows them"
    );
}

/// Timing is recorded and completion waiters notified exactly once per frame.
#[test]
fn frame_timing_is_recorded_once_per_frame() {
    let scheduler = Scheduler::new();
    let timings = Arc::new(AtomicUsize::new(0));
    let timings_cb = Arc::clone(&timings);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        timings_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let before = scheduler.frame_count();
    scheduler.drive_frame(Instant::now(), || {});
    assert_eq!(scheduler.frame_count(), before + 1);
    assert_eq!(timings.load(Ordering::SeqCst), 1);
}
