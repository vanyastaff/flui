//! Post-frame callbacks run **after** the pipeline, in the same frame.
//!
//! # Persistent callbacks and the pipeline
//!
//! The pipeline is not itself a persistent callback: it is
//! a closure passed to `UpdateScheduler::drive_frame`, so a *registered* persistent
//! callback runs **before** it. Post-frame ordering — the only thing a
//! geometry-measuring post-frame callback needs — is unaffected.
//! `persistent_callbacks_run_before_the_pipeline` pins the persistent-phase
//! ordering.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_scheduler::{IdleDeadline, Instant, SchedulerPhase, UpdateScheduler};
use parking_lot::Mutex;

/// An Idle-slice deadline far enough in the future that it never passes
/// during a test — these tests exercise post-frame ordering, not the
/// deadline gate itself, so Idle-priority work must never be deferred here.
fn far_deadline() -> IdleDeadline {
    IdleDeadline::far_future(Instant::now())
}

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

/// The draw-frame phases run in order: persistent, then post-frame. The pipeline sits in the persistent
/// slot, so a post-frame callback must observe everything it did.
fn drive_frame_runs_post_frame_callbacks_after_the_pipeline() {
    let scheduler = UpdateScheduler::new();
    let log = Log::default();

    let log_cb = log.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        log_cb.push("post_frame");
    }));

    let log_pipe = log.clone();
    scheduler.drive_frame(Instant::now(), far_deadline(), || {
        log_pipe.push("pipeline");
    });

    assert_eq!(log.get(), vec!["pipeline", "post_frame"]);
}

/// A **panicking** pipeline is an abandoned frame: `drive_frame` catches the
/// panic, calls `abort_frame` (phase → `Idle`, **no** post-frame callbacks), and
/// resumes the unwind. The queued callbacks survive to the next completed frame.
///
/// A panicking persistent callback skips the post-frame loop, and the phase is
/// still reset.
///
/// `abort_frame` must not go through `set_scheduler_phase`: `PersistentCallbacks
/// -> Idle` is an illegal transition, so its `debug_assert!` would fire and — were
/// this a `Drop` guard running during unwind — double-panic into `abort`.
fn a_panicking_pipeline_aborts_the_frame_and_runs_no_post_frame_callbacks() {
    let scheduler = UpdateScheduler::new();
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), far_deadline(), || {
            panic!("pipeline exploded")
        });
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
    scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

/// The guard the previous test's `Idle` assertion protects: a frame left open at
/// `PersistentCallbacks` would make the next `handle_begin_frame` attempt an
/// illegal `PersistentCallbacks -> TransientCallbacks` transition.
fn a_frame_after_a_panicking_frame_starts_cleanly() {
    let scheduler = UpdateScheduler::new();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(Instant::now(), far_deadline(), || panic!("boom"));
    }));

    let ran = Arc::new(AtomicUsize::new(0));
    let ran_cb = Arc::clone(&ran);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        ran_cb.fetch_add(1, Ordering::SeqCst);
    }));

    // Would `debug_assert!` on the illegal transition if the frame were still open.
    scheduler.drive_frame(Instant::now(), far_deadline(), || {});
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
}

/// **Persistent callbacks run before the pipeline.**
///
/// The pipeline is a closure, not a registered persistent callback, so every
/// registered persistent callback runs *before* it. Nothing in the framework
/// registers one today.
fn persistent_callbacks_run_before_the_pipeline() {
    let scheduler = UpdateScheduler::new();
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
    scheduler.drive_frame(Instant::now(), far_deadline(), || {
        log_pipe.push("pipeline");
    });

    assert_eq!(
        log.get(),
        vec!["persistent", "pipeline", "post_frame"],
        "persistent callbacks run first; the pipeline follows them"
    );
}

#[test]
fn post_frame_ordering_matrix() {
    crate::run_table(
        "post_frame_ordering_matrix",
        &[
            (
                "drive_frame_runs_post_frame_callbacks_after_the_pipeline",
                drive_frame_runs_post_frame_callbacks_after_the_pipeline as fn(),
            ),
            (
                "a_panicking_pipeline_aborts_the_frame_and_runs_no_post_frame_callbacks",
                a_panicking_pipeline_aborts_the_frame_and_runs_no_post_frame_callbacks as fn(),
            ),
            (
                "a_frame_after_a_panicking_frame_starts_cleanly",
                a_frame_after_a_panicking_frame_starts_cleanly as fn(),
            ),
            (
                "persistent_callbacks_run_before_the_pipeline",
                persistent_callbacks_run_before_the_pipeline as fn(),
            ),
        ],
    );
}
