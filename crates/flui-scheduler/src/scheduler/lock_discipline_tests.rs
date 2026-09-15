//! Scheduler lock-discipline, reentrant-registration, and lock-then-drop
//! tests (issue #1058).
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
/// anything else (an atomic, an id generator, a lock-free `DashMap`) is
/// bound `_` deliberately, so the classification decision is visible, not
/// silently skipped. `TaskQueue` and `AsyncDriver` hold their own mutexes
/// behind fields private to their own modules, so they are probed through
/// their own `#[cfg(test)] is_unlocked()` methods instead of a field
/// destructure here.
///
/// Left for a follow-up, not probed here: `callbacks.cancelled` (a
/// lock-free `DashMap` -- it cannot deadlock a reentrant caller the way a
/// `Mutex` can, so it is a different, lower-priority risk) and
/// `LocalPostFrameLane`'s owner-local queue (an `Rc<RefCell<_>>`, not a
/// `Mutex` -- a reentrant `RefCell` borrow panics rather than deadlocking,
/// a different failure mode this oracle does not yet cover).
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
        frame_count: _,
        janky_frame_count: _,
        warm_up_done: _,
        idle_deadline,
        completion_waiters,
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
        idle_deadline.try_lock().is_some(),
        "idle_deadline is locked during a callback"
    );
    assert!(
        completion_waiters.try_lock().is_some(),
        "completion_waiters is locked during a callback"
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
#[test]
fn transient_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let observed: Arc<Mutex<Option<FrameTiming>>> = Arc::new(Mutex::new(None));
    let observed_for_callback = Arc::clone(&observed);
    scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
        assert_no_scheduler_lock_held(&probe);
        *observed_for_callback.lock() = probe.current_frame();
    }));

    let frame_id = scheduler.handle_begin_frame(Instant::now());

    assert_eq!(
        observed.lock().map(|timing| timing.id),
        Some(frame_id),
        "current_frame() must observe THIS frame's own timing from inside a \
         transient callback, not merely be unlocked"
    );
}

/// Also pins that `current_frame()` observes THIS frame's own timing from
/// inside a persistent callback, comparing against the `&FrameTiming` the
/// callback was directly handed.
#[test]
fn persistent_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.add_persistent_frame_callback(Arc::new(move |timing| {
        assert_no_scheduler_lock_held(&probe);
        assert_eq!(
            probe.current_frame().map(|t| t.id),
            Some(timing.id),
            "current_frame() must still observe this frame's own timing \
             from inside a persistent callback, not merely be unlocked"
        );
    }));
    scheduler.execute_frame();
}

/// Also pins `current_frame().is_none()` inside the callback — the
/// frame's `FrameTiming` is `take()`n before post-frame callbacks run
/// (see `end_frame_impl`), so a shared post-frame callback observes no
/// "current" frame at all, not merely an unlocked one.
#[test]
fn shared_post_frame_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        assert_no_scheduler_lock_held(&probe);
        assert!(
            probe.current_frame().is_none(),
            "the frame's timing is taken before post-frame callbacks run"
        );
    }));
    scheduler.execute_frame();
}

#[test]
fn local_post_frame_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let lane = scheduler.new_local_post_frame_lane();
    let probe = scheduler.clone();
    lane.local_handle()
        .schedule_local(move |_timing| {
            assert_no_scheduler_lock_held(&probe);
        })
        .expect("lane is alive");
    scheduler.execute_frame_with_lane(&lane);
}

#[test]
fn idle_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.schedule_idle_callback(move || {
        assert_no_scheduler_lock_held(&probe);
    });
    assert_eq!(scheduler.execute_idle_callbacks(), 1);
}

#[test]
fn microtask_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.schedule_microtask(Box::new(move || {
        assert_no_scheduler_lock_held(&probe);
    }));
    scheduler.handle_begin_frame(Instant::now());
}

#[test]
fn lifecycle_listener_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.add_lifecycle_state_listener(Arc::new(move |_state| {
        assert_no_scheduler_lock_held(&probe);
    }));
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
}

#[test]
fn timings_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let ran = Arc::new(AtomicBool::new(false));
    let ran_for_callback = Arc::clone(&ran);
    scheduler.add_timings_callback(Arc::new(move |_timings| {
        assert_no_scheduler_lock_held(&probe);
        ran_for_callback.store(true, Ordering::Release);
    }));

    scheduler.execute_frame();
    assert_eq!(scheduler.report_timings(), 1);
    assert!(ran.load(Ordering::Acquire));
}

#[test]
fn frame_scheduled_hook_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        assert_no_scheduler_lock_held(&probe);
    })));

    scheduler.request_frame();
    assert!(scheduler.is_frame_scheduled());
}

// =========================================================================
// Reentrant registration (#1058) -- a callback that registers another
// callback of its own family from inside itself must have the new one
// deferred to the next frame, run exactly once (or every frame, for
// persistent), never lost and never run early.
// =========================================================================

#[test]
fn transient_callback_registering_another_transient_callback_defers_to_next_frame() {
    let scheduler = UpdateScheduler::new();
    let nested_ran = Arc::new(AtomicU32::new(0));

    let outer_scheduler = scheduler.clone();
    let nested_ran_for_outer = Arc::clone(&nested_ran);
    scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
        let nested_ran = Arc::clone(&nested_ran_for_outer);
        outer_scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
            nested_ran.fetch_add(1, Ordering::SeqCst);
        }));
    }));

    // A full frame cycle (not a bare `handle_begin_frame`) between the
    // two checks: `handle_begin_frame` alone leaves the phase machine at
    // `MidFrameMicrotasks`, and a second call from there is an illegal
    // `MidFrameMicrotasks -> TransientCallbacks` transition.
    scheduler.execute_frame();
    assert_eq!(
        nested_ran.load(Ordering::SeqCst),
        0,
        "re-registration must not run in the same handle_begin_frame call"
    );

    scheduler.execute_frame();
    assert_eq!(
        nested_ran.load(Ordering::SeqCst),
        1,
        "it must run exactly once, on the very next begin-frame"
    );

    scheduler.execute_frame();
    assert_eq!(
        nested_ran.load(Ordering::SeqCst),
        1,
        "a transient callback fires exactly once; a third frame must not \
         run it again"
    );
}

/// Persistent callbacks fire every frame for the lifetime of the
/// application (Flutter parity: `SchedulerBinding.addPersistentFrameCallback`'s
/// own doc, `scheduler/binding.dart` @ 3.44.0), so unlike the
/// transient/post-frame cases the freshly-registered callback below keeps
/// firing every frame after its first — this test pins the deferral of its
/// FIRST run, not a one-shot.
#[test]
fn persistent_callback_registering_another_persistent_callback_defers_to_next_frame() {
    let scheduler = UpdateScheduler::new();
    let outer_runs = Arc::new(AtomicU32::new(0));
    let nested_runs = Arc::new(AtomicU32::new(0));

    let outer_scheduler = scheduler.clone();
    let outer_runs_for_cb = Arc::clone(&outer_runs);
    let nested_runs_for_outer = Arc::clone(&nested_runs);
    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        // Register the nested callback on the first run only -- an
        // unconditional registration would add one more persistent
        // callback every single frame and the counts below would never
        // settle.
        if outer_runs_for_cb.fetch_add(1, Ordering::SeqCst) == 0 {
            let nested_runs = Arc::clone(&nested_runs_for_outer);
            outer_scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
                nested_runs.fetch_add(1, Ordering::SeqCst);
            }));
        }
    }));

    scheduler.execute_frame();
    assert_eq!(
        nested_runs.load(Ordering::SeqCst),
        0,
        "a persistent callback registered from inside a persistent callback \
         must not run in the frame that registered it"
    );

    scheduler.execute_frame();
    assert_eq!(
        nested_runs.load(Ordering::SeqCst),
        1,
        "it must run starting the very next frame"
    );

    scheduler.execute_frame();
    assert_eq!(
        nested_runs.load(Ordering::SeqCst),
        2,
        "and keep running every frame after that, like any persistent callback"
    );
}

/// `end_frame_impl`'s own comment documents this ordering ("a callback
/// registered *from* a post-frame callback runs on the next frame");
/// this is the direct test for it.
#[test]
fn shared_post_frame_callback_registering_another_defers_to_next_frame() {
    let scheduler = UpdateScheduler::new();
    let nested_ran = Arc::new(AtomicU32::new(0));

    let outer_scheduler = scheduler.clone();
    let nested_ran_for_outer = Arc::clone(&nested_ran);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        let nested_ran = Arc::clone(&nested_ran_for_outer);
        outer_scheduler.add_post_frame_callback(Box::new(move |_timing| {
            nested_ran.fetch_add(1, Ordering::SeqCst);
        }));
    }));

    scheduler.execute_frame();
    assert_eq!(
        nested_ran.load(Ordering::SeqCst),
        0,
        "a post-frame callback registered from inside a post-frame callback \
         must not run in the frame that registered it"
    );

    scheduler.execute_frame();
    assert_eq!(
        nested_ran.load(Ordering::SeqCst),
        1,
        "it must run exactly once, on the very next completed frame"
    );

    scheduler.execute_frame();
    assert_eq!(
        nested_ran.load(Ordering::SeqCst),
        1,
        "a post-frame callback fires exactly once; a third frame must not \
         run it again"
    );
}

// =========================================================================
// Lock-then-drop (#1058, sibling sites tracked with #1150) -- a removed
// callback/listener's captured state can be the LAST reference at removal
// time (these two are addressed by opaque `CallbackId`, not a live handle
// the caller must keep, so nothing else clones the stored value). Dropping
// it while the owning `Mutex` is still held deadlocks a capture whose own
// `Drop` re-enters the scheduler. Each test below runs the removal on a
// spawned thread and bounds the wait so a regression fails fast instead of
// hanging the run; the test thread holds no scheduler-locking value across
// the wait.
// =========================================================================

#[test]
fn cancel_frame_callback_drops_the_cancelled_callback_outside_the_lock() {
    let scheduler = UpdateScheduler::new();

    struct ReregistersOnDrop(UpdateScheduler);
    impl Drop for ReregistersOnDrop {
        fn drop(&mut self) {
            self.0.schedule_frame_callback(Box::new(|_| {}));
        }
    }

    let probe = ReregistersOnDrop(scheduler.clone());
    let id = scheduler.schedule_frame_callback(Box::new(move |_| {
        let _keep_alive = &probe;
    }));

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let cancelling_scheduler = scheduler.clone();
    std::thread::spawn(move || {
        cancelling_scheduler.cancel_frame_callback(id);
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect(
            "cancel_frame_callback must not deadlock when the cancelled \
             callback's Drop re-registers another one",
        );
}

#[test]
fn remove_lifecycle_state_listener_drops_the_removed_listener_outside_the_lock() {
    let scheduler = UpdateScheduler::new();

    struct ReregistersOnDrop(UpdateScheduler);
    impl Drop for ReregistersOnDrop {
        fn drop(&mut self) {
            self.0.add_lifecycle_state_listener(Arc::new(|_state| {}));
        }
    }

    let probe = Arc::new(ReregistersOnDrop(scheduler.clone()));
    let id = scheduler.add_lifecycle_state_listener(Arc::new(move |_state| {
        let _keep_alive = &probe;
    }));

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let removing_scheduler = scheduler.clone();
    std::thread::spawn(move || {
        removing_scheduler.remove_lifecycle_state_listener(id);
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect(
            "remove_lifecycle_state_listener must not deadlock when the removed \
             listener's Drop re-registers another one",
        );
}

/// `remove_timings_callback` was rewritten alongside the two deadlock
/// tests above, for the same defensive shape (locate the match under the
/// lock, drop it after the guard falls, never under it) — but its hazard
/// is not reachable through the public API the way the other two are:
/// `remove_timings_callback` takes `&TimingsCallback`, so the caller must
/// hold a live clone for the whole call, and the copy this function
/// removes from its own `Vec` can therefore never be the LAST strong
/// reference at removal time (the caller's borrowed clone always outlives
/// it). No deadlock reproduction is claimed for this site; this is a plain
/// removal-correctness regression test instead, closing this function's
/// previous lack of scheduler-level coverage (`report_timings` dispatch
/// only, in `tests/integration_tests.rs`, asserted no effect).
#[test]
fn remove_timings_callback_removes_only_the_matching_callback() {
    let scheduler = UpdateScheduler::new();
    let kept_calls = Arc::new(AtomicU32::new(0));
    let removed_calls = Arc::new(AtomicU32::new(0));

    let kept_calls_for_cb = Arc::clone(&kept_calls);
    let kept: TimingsCallback = Arc::new(move |_timings| {
        kept_calls_for_cb.fetch_add(1, Ordering::SeqCst);
    });
    let removed_calls_for_cb = Arc::clone(&removed_calls);
    let removed: TimingsCallback = Arc::new(move |_timings| {
        removed_calls_for_cb.fetch_add(1, Ordering::SeqCst);
    });

    scheduler.add_timings_callback(kept.clone());
    scheduler.add_timings_callback(removed.clone());
    scheduler.remove_timings_callback(&removed);

    scheduler.execute_frame();
    assert_eq!(scheduler.report_timings(), 1);

    assert_eq!(kept_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        removed_calls.load(Ordering::SeqCst),
        0,
        "the removed callback must not fire"
    );
}
