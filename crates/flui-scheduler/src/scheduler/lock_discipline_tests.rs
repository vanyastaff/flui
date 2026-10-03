//! Scheduler lock-discipline tests (issue #1058).
//!
//! Split out of `scheduler.rs`'s inline `tests` module: `scheduler.rs` is a
//! ~3.6k-line file with a ~1.4k-line inline test module, and this family --
//! the lock-discipline oracle (`assert_no_scheduler_lock_held`) plus every
//! test built on it -- is one cohesive slice nothing else in the crate
//! uses. Declared as a child module (`#[cfg(test)] mod
//! lock_discipline_tests;` in `scheduler.rs`), so it still sees every
//! private field and type `scheduler.rs` defines (`SchedulerInner`,
//! `FrameState`, `CallbackState`, `BindingState`, and
//! `UpdateScheduler::inner` itself) exactly as the inline module did.

use super::*;

// =========================================================================
// Lock discipline (#1058) -- every callback family must run with NO
// scheduler storage lock held, so a callback can safely call back into the
// scheduler, including the read-only `current_frame()` accessor the
// retired legacy `schedule_frame` callback loop could not tolerate (its
// `if let Some(timing) = self.inner.frame.current_frame.lock().as_ref()
// { callback(timing); }` kept the guard alive across the call).
// =========================================================================

/// Try-locks every mutex a scheduler callback could legally observe from
/// inside its own invocation and asserts each one is free. Every callback
/// family drains, clones, or snapshots its queue before invoking user code
/// -- see `handle_begin_frame`, `handle_draw_frame`, `end_frame_impl`,
/// `execute_idle_callbacks`, `flush_microtasks`,
/// `handle_app_lifecycle_state_change`, `report_timings`, and
/// `request_frame_impl`. This is the oracle proving that discipline holds
/// at the actual call site, not just in the source.
///
/// Exhaustive by construction: `FrameState`, `CallbackState`, and
/// `BindingState` are destructured below with no trailing `..`, so a field
/// added to any of them is a compile error here until this oracle
/// explicitly classifies it -- a `Mutex` gets a `try_lock` assertion;
/// anything else (an atomic, an id generator, a sharded-`RwLock`
/// `DashMap`) is bound `_` deliberately, so the classification decision is
/// visible, not silently skipped. `TaskQueue` and `AsyncDriver` hold their
/// own mutexes behind fields private to their own modules, so they are
/// probed through their own `#[cfg(test)] is_unlocked()` methods instead
/// of a field destructure here.
///
/// `callbacks.cancelled` and `LocalPostFrameLane`'s owner-local queue are
/// not probed here. Neither is a `Mutex`, but each is still a real
/// reentrancy hazard with its own different failure mode, not a lesser
/// one: `DashMap` (6.2.1) is a SHARDED `RwLock`, not lock-free --
/// `contains_key`/`get`/`get_mut` release their shard's lock immediately
/// when a key is absent, but a `Ref`/`RefMut`/`Entry` held alive across a
/// reentrant call into the SAME shard deadlocks exactly like the `Mutex`
/// family above (`entry()` holds its shard write-locked for its entire
/// life, vacant or occupied, regardless of whether the caller binds the
/// payload to a name); a reentrant `RefCell` borrow panics rather than
/// deadlocking, a third failure mode again. DashMap 6.2.1 has no
/// all-shards "is anything locked" API to assert here, and a lane is never
/// a field of `SchedulerInner` for this destructure to see in the first
/// place -- each `new_local_post_frame_lane()` call hands the caller its
/// own, held separately from the scheduler's own storage.
fn assert_no_scheduler_lock_held(scheduler: &UpdateScheduler) {
    // Destructured without `..` on purpose: a `Mutex` added directly to
    // `SchedulerInner` (beside the owned sub-objects) must fail to compile
    // here until it is classified below, exactly like a field added to any
    // of the three state structs.
    let SchedulerInner {
        frame,
        callbacks,
        binding,
        task_queue,
        async_driver,
    } = &*scheduler.inner;

    let FrameState {
        scheduler_phase: _,
        current_frame,
        current_vsync_time,
        budget,
        frame_scheduled: _,
        wake_delivery,
        frame_count: _,
        janky_frame_count: _,
        warm_up_done: _,
        idle_deadline,
        completion_waiters,
        frame_thread,
    } = frame;
    assert!(
        current_frame.try_lock().is_some(),
        "current_frame is locked during a callback"
    );
    assert!(
        current_vsync_time.try_lock().is_some(),
        "current_vsync_time is locked during a callback"
    );
    assert!(
        budget.try_lock().is_some(),
        "budget is locked during a callback"
    );
    assert!(
        frame_thread.try_lock().is_some(),
        "frame_thread is locked during a callback"
    );
    assert!(
        idle_deadline.try_lock().is_some(),
        "idle_deadline is locked during a callback"
    );
    assert!(
        completion_waiters.try_lock().is_some(),
        "completion_waiters is locked during a callback"
    );

    assert!(
        wake_delivery.is_unlocked(),
        "wake delivery is locked during a callback"
    );

    let CallbackState {
        post_frame_registration,
        transient,
        cancelled: _,
        id_gen: _,
        persistent,
        post_frame,
        microtasks,
        idle,
        lifecycle_listeners,
    } = callbacks;
    assert!(
        post_frame_registration.try_lock().is_some(),
        "post_frame_registration is locked during a callback"
    );
    assert!(
        transient.try_lock().is_some(),
        "transient is locked during a callback"
    );
    assert!(
        persistent.try_lock().is_some(),
        "persistent is locked during a callback"
    );
    assert!(
        post_frame.try_lock().is_some(),
        "post_frame is locked during a callback"
    );
    assert!(
        microtasks.try_lock().is_some(),
        "microtasks is locked during a callback"
    );
    assert!(
        idle.try_lock().is_some(),
        "idle is locked during a callback"
    );
    assert!(
        lifecycle_listeners.try_lock().is_some(),
        "lifecycle_listeners is locked during a callback"
    );

    let BindingState {
        frames_enabled: _,
        lifecycle_state: _,
        epoch_start,
        timings_callbacks,
        pending_timings,
        last_timings_report,
        performance_mode_requests: _,
        current_performance_mode,
        on_frame_scheduled,
    } = binding;
    assert!(
        epoch_start.try_lock().is_some(),
        "epoch_start is locked during a callback"
    );
    assert!(
        timings_callbacks.try_lock().is_some(),
        "timings_callbacks is locked during a callback"
    );
    assert!(
        pending_timings.try_lock().is_some(),
        "pending_timings is locked during a callback"
    );
    assert!(
        last_timings_report.try_lock().is_some(),
        "last_timings_report is locked during a callback"
    );
    assert!(
        current_performance_mode.try_lock().is_some(),
        "current_performance_mode is locked during a callback"
    );
    assert!(
        on_frame_scheduled.try_lock().is_some(),
        "on_frame_scheduled is locked during a callback"
    );

    assert!(
        task_queue.is_unlocked(),
        "TaskQueue's lock is locked during a callback"
    );
    assert!(
        async_driver.is_unlocked(),
        "AsyncDriver's lock(s) are locked during a callback"
    );
}

/// Also pins that `current_frame()` observes THIS frame's own timing from
/// inside a transient callback, not merely that it is unlocked --
/// `handle_begin_frame` sets `current_frame` before running the transient
/// loop, so the id it returns must match what the callback reads back.
fn transient_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let observed: Arc<Mutex<Option<FrameTiming>>> = Arc::new(Mutex::new(None));
    let observed_for_callback = Arc::clone(&observed);
    scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
        assert_no_scheduler_lock_held(&probe);
        let _prev = std::mem::replace(&mut *observed_for_callback.lock(), probe.current_frame());
    }));

    let frame_id = scheduler.handle_begin_frame(Instant::now());

    assert_eq!(
        observed.lock().map(|timing| timing.id),
        Some(frame_id),
        "current_frame() must observe THIS frame's own timing from inside a \
         transient callback, not merely be unlocked"
    );
}

/// The completion-waker family: `notify_frame_completion` calls each
/// waiter's `Waker::wake()`, and every scheduler lock must be free while it
/// does — including `completion_waiters` itself, which the drain holds
/// immediately before.
///
/// This family was the only callback family in this file with no
/// lock-discipline test, while `notify_frame_completion` is exactly where a
/// lock-across-`wake()` regression would land.
fn completion_waker_runs_with_no_scheduler_lock_held() {
    struct ProbingWaker {
        probe: UpdateScheduler,
        ran: Arc<AtomicBool>,
    }

    impl std::task::Wake for ProbingWaker {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            assert_no_scheduler_lock_held(&self.probe);
            self.ran.store(true, Ordering::Release);
        }
    }

    let scheduler = UpdateScheduler::new();
    let ran = Arc::new(AtomicBool::new(false));
    let mut future = scheduler.end_of_frame();
    let waker = Waker::from(Arc::new(ProbingWaker {
        probe: scheduler.clone(),
        ran: Arc::clone(&ran),
    }));
    let mut cx = Context::from_waker(&waker);
    assert!(Pin::new(&mut future).poll(&mut cx).is_pending());

    scheduler.execute_frame();

    assert!(
        ran.load(Ordering::Acquire),
        "the completion waker must actually have run"
    );
}

#[test]
fn callbacks_run_with_no_scheduler_lock_held() {
    crate::table_test::run_table(
        "callbacks_run_with_no_scheduler_lock_held",
        &[
            (
                "transient_callback_runs_with_no_scheduler_lock_held",
                transient_callback_runs_with_no_scheduler_lock_held as fn(),
            ),
            (
                "completion_waker_runs_with_no_scheduler_lock_held",
                completion_waker_runs_with_no_scheduler_lock_held as fn(),
            ),
        ],
    );
}
