//! `ensure_visual_update` phase-gate tests (issue #1157).
//!
//! Split out of `scheduler.rs`'s inline `tests` module for the same reason
//! `lock_discipline_tests.rs` was: `scheduler.rs` is already large, and this
//! family -- every test built on the `ensure_visual_update` phase `match`
//! -- is one cohesive slice nothing else in the crate uses. Declared as a
//! child module (`#[cfg(test)] mod visual_update_tests;` in `scheduler.rs`),
//! so it still sees every private field and type `scheduler.rs` defines,
//! exactly as the inline module did.

use std::sync::OnceLock;

use super::*;

// =========================================================================
// `ensure_visual_update` phase gate (Flutter parity: `ensureVisualUpdate`,
// `scheduler/binding.dart`) -- `Idle`/`PostFrameCallbacks` request a frame.
// The three mid-frame phases no-op ONLY for the thread already driving the
// frame (`frame_thread`); a caller on any other thread always requests,
// since a lost cross-thread wake is the worst failure class this crate
// names. See `ensure_visual_update`'s own doc for the full contract,
// including what it does NOT promise even for the driving thread.
// =========================================================================

/// RED pin: before `ensure_visual_update` gained its phase `match`, it
/// called `schedule_frame_if_enabled()` unconditionally regardless of
/// phase -- reverting `ensure_visual_update`'s body back to that single
/// unconditional call reddens this.
#[test]
fn ensure_visual_update_is_a_noop_during_transient_callbacks() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let ran = Arc::new(AtomicBool::new(false));
    let ran_for_callback = Arc::clone(&ran);
    scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
        assert_eq!(probe.phase(), SchedulerPhase::TransientCallbacks);
        probe.ensure_visual_update();
        assert!(
            !probe.is_frame_scheduled(),
            "must already read as not-scheduled from inside the very \
             callback that called ensure_visual_update, not just after \
             the frame closes"
        );
        ran_for_callback.store(true, Ordering::Release);
    }));

    scheduler.execute_frame();

    assert!(
        ran.load(Ordering::Acquire),
        "the callback must actually have run for this pin to mean anything"
    );
    assert!(
        !scheduler.is_frame_scheduled(),
        "ensure_visual_update called from TransientCallbacks must not \
         schedule a frame"
    );
}

/// Same pin, one phase later: a microtask scheduled from a transient
/// callback runs during `MidFrameMicrotasks` (`flush_microtasks`, called
/// right after the transient loop in `handle_begin_frame`), so this
/// reaches the same `match` arm from the other mid-frame phase. Reddens
/// under the same revert as the transient-phase pin above.
#[test]
fn ensure_visual_update_is_a_noop_during_mid_frame_microtasks() {
    let scheduler = UpdateScheduler::new();
    let observed_phase: Arc<OnceLock<SchedulerPhase>> = Arc::new(OnceLock::new());
    let ran = Arc::new(AtomicBool::new(false));

    let scheduler_for_transient = scheduler.clone();
    let probe = scheduler.clone();
    let observed_for_task = Arc::clone(&observed_phase);
    let ran_for_task = Arc::clone(&ran);
    scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
        scheduler_for_transient.schedule_microtask(Box::new(move || {
            let _ = observed_for_task.set(probe.phase());
            probe.ensure_visual_update();
            assert!(
                !probe.is_frame_scheduled(),
                "must already read as not-scheduled from inside the very \
                 callback that called ensure_visual_update"
            );
            ran_for_task.store(true, Ordering::Release);
        }));
    }));

    scheduler.execute_frame();

    assert_eq!(
        observed_phase.get().copied(),
        Some(SchedulerPhase::MidFrameMicrotasks),
        "precondition: the microtask must actually run during \
         MidFrameMicrotasks for this pin to exercise the right phase"
    );
    assert!(
        ran.load(Ordering::Acquire),
        "the microtask must actually have run for this pin to mean anything"
    );
    assert!(
        !scheduler.is_frame_scheduled(),
        "ensure_visual_update called from MidFrameMicrotasks must not \
         schedule a frame"
    );
}

/// Neither `add_persistent_frame_callback` nor registering it requests a
/// frame on its own; `handle_begin_frame` stores `frame_scheduled = false`
/// at the very top of the frame, before any callback runs, so the
/// post-frame assertion below observes only what happens INSIDE this
/// frame -- an unconditional `schedule_frame_if_enabled()` call inside
/// `ensure_visual_update` is what would re-set it here, from inside the
/// persistent callback.
#[test]
fn ensure_visual_update_is_a_noop_during_persistent_callbacks() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let ran = Arc::new(AtomicBool::new(false));
    let ran_for_callback = Arc::clone(&ran);
    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        assert_eq!(probe.phase(), SchedulerPhase::PersistentCallbacks);
        probe.ensure_visual_update();
        assert!(
            !probe.is_frame_scheduled(),
            "must already read as not-scheduled from inside the very \
             callback that called ensure_visual_update"
        );
        ran_for_callback.store(true, Ordering::Release);
    }));

    scheduler.execute_frame();

    assert!(
        ran.load(Ordering::Acquire),
        "the callback must actually have run for this pin to mean anything"
    );
    assert!(
        !scheduler.is_frame_scheduled(),
        "ensure_visual_update called from PersistentCallbacks must not \
         schedule a frame"
    );
}

/// Characterization pin: `Idle` is one of the two phases
/// `ensure_visual_update` DOES request a frame from.
#[test]
fn ensure_visual_update_schedules_from_idle() {
    let scheduler = UpdateScheduler::new();
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);

    scheduler.ensure_visual_update();

    assert!(scheduler.is_frame_scheduled());
}

/// Characterization pin: `PostFrameCallbacks` is the other phase
/// `ensure_visual_update` requests a frame from.
#[test]
fn ensure_visual_update_schedules_from_post_frame_callbacks() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        assert_eq!(probe.phase(), SchedulerPhase::PostFrameCallbacks);
        probe.ensure_visual_update();
    }));

    scheduler.execute_frame();

    assert!(
        scheduler.is_frame_scheduled(),
        "ensure_visual_update called from PostFrameCallbacks must \
         schedule a frame"
    );
}

/// Frame demand from a thread OTHER than the one driving the current frame
/// must always be requested, never dropped as a same-thread no-op would be
/// -- a lost cross-thread wake is the worst failure class this crate names.
/// `UpdateScheduler` is `Send + Sync` and documented as reachable from any
/// thread (see `frame_thread`'s own doc): a mid-frame caller on a DIFFERENT
/// thread has no guarantee the frame-driving thread's own later phases will
/// observe whatever state prompted its call, unlike a same-thread mid-frame
/// caller (the no-op pins above).
///
/// RED pin: before the `frame_thread` guard, the mid-frame arms of
/// `ensure_visual_update`'s `match` were an unconditional no-op regardless
/// of which thread called -- reverting the guard back to that (dropping the
/// `frame_thread` comparison) reddens this with `!is_frame_scheduled()`.
#[test]
fn ensure_visual_update_from_another_thread_mid_frame_still_schedules() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let hook_calls = Arc::new(AtomicU32::new(0));
    let hook_calls_for_hook = Arc::clone(&hook_calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        hook_calls_for_hook.fetch_add(1, Ordering::SeqCst);
    })));

    let (mid_frame_tx, mid_frame_rx) = std::sync::mpsc::channel::<()>();
    let (caller_done_tx, caller_done_rx) = std::sync::mpsc::channel::<()>();
    // `RecurringFrameCallback` requires `Send + Sync`; a bare `Sender`/
    // `Receiver` is `Send` but not `Sync`, so the endpoints the PERSISTENT
    // callback captures are wrapped to make the closure as a whole `Sync`.
    // The endpoints the SPAWNED thread captures need no wrapping --
    // `thread::spawn` only requires `Send`.
    let mid_frame_tx = Mutex::new(mid_frame_tx);
    let caller_done_rx = Mutex::new(caller_done_rx);

    let other_thread_scheduler = scheduler.clone();
    let other_thread = std::thread::spawn(move || {
        mid_frame_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the frame-driving thread must signal it is mid-frame");
        other_thread_scheduler.ensure_visual_update();
        caller_done_tx
            .send(())
            .expect("the frame-driving thread must still be waiting on this");
    });

    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        assert_eq!(probe.phase(), SchedulerPhase::PersistentCallbacks);
        mid_frame_tx
            .lock()
            .send(())
            .expect("the other thread must still be listening");
        caller_done_rx
            .lock()
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the other thread's ensure_visual_update call must complete");
    }));

    scheduler.execute_frame();
    other_thread
        .join()
        .expect("the other thread must not panic");

    assert!(
        scheduler.is_frame_scheduled(),
        "ensure_visual_update called from another thread, even mid-frame, \
         must always request a frame"
    );
    assert_eq!(
        hook_calls.load(Ordering::SeqCst),
        1,
        "the false->true transition fires the wake hook exactly once for \
         this call -- handle_begin_frame's own clear at frame entry \
         precedes it"
    );
}
