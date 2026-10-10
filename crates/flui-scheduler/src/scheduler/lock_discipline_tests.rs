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
/// `flush_microtasks`,
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
/// visible, not silently skipped. `TaskQueue` holds its own mutex behind a
/// field private to its module, so it is probed through its own
/// `#[cfg(test)] is_unlocked()` instead of a field destructure here. The
/// async tasks are not scheduler storage: they live in the owner's
/// `OwnerFrame`, whose `AsyncDriver::is_unlocked` probe serves the owner's
/// own reentrancy tests.
///
/// `callbacks.cancelled` and the `OwnerFrame`'s owner-local queues are
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
/// place -- each `OwnerFrame::new` call hands the caller its
/// own, held separately from the scheduler's own storage.
fn assert_no_scheduler_lock_held(scheduler: &UpdateScheduler) {
    // Destructured without `..` on purpose: a `Mutex` added directly to
    // `SchedulerInner` (beside the owned sub-objects) must fail to compile
    // here until it is classified below, exactly like a field added to any
    // of the three state structs.
    let SchedulerInner {
        wake,
        frame,
        callbacks,
        binding,
        task_queue,
        owner_frame_claimed: _,
        execution_release,
    } = &*scheduler.inner;
    assert!(execution_release.try_borrow_mut().is_ok(), "terminal release custody is not borrowed through user code");

    let FrameState {
        current_frame,
        current_vsync_time,
        budget,
        frame_count: _,
        janky_frame_count: _,
        idle_deadline,
        completion_waiters,
        frame_thread,
    } = frame;
    let WakeShared {
        scheduler_phase: _,
        frames_enabled: _,
        frame_thread: _,
        frame_scheduled: _,
        wake_delivery,
        on_frame_scheduled,
        closed: _,
    } = &**wake;
    assert!(
        current_frame.try_borrow_mut().is_ok(),
        "current_frame is locked during a callback"
    );
    assert!(
        current_vsync_time.try_borrow_mut().is_ok(),
        "current_vsync_time is locked during a callback"
    );
    assert!(
        budget.try_borrow_mut().is_ok(),
        "budget is locked during a callback"
    );
    assert!(
        frame_thread.try_lock().is_some(),
        "frame_thread is locked during a callback"
    );
    assert!(
        idle_deadline.try_borrow_mut().is_ok(),
        "idle_deadline is locked during a callback"
    );
    assert!(
        completion_waiters.try_borrow_mut().is_ok(),
        "completion_waiters is locked during a callback"
    );

    assert!(
        wake_delivery.is_unlocked(),
        "wake delivery is locked during a callback"
    );

    let CallbackState {
        transient,
        cancelled: _,
        id_gen: _,
        persistent,
        post_frame,
        microtasks,
        lifecycle_listeners,
    } = callbacks;
    assert!(
        transient.try_borrow_mut().is_ok(),
        "transient is locked during a callback"
    );
    assert!(
        persistent.try_borrow_mut().is_ok(),
        "persistent is locked during a callback"
    );
    assert!(
        post_frame.try_borrow_mut().is_ok(),
        "post_frame is locked during a callback"
    );
    assert!(
        microtasks.try_borrow_mut().is_ok(),
        "microtasks is locked during a callback"
    );
    assert!(
        lifecycle_listeners.try_borrow_mut().is_ok(),
        "lifecycle_listeners is locked during a callback"
    );

    let BindingState {
        lifecycle_state: _,
        timings_callbacks,
        pending_timings,
        last_timings_report,
    } = binding;
    assert!(
        timings_callbacks.try_borrow_mut().is_ok(),
        "timings_callbacks is locked during a callback"
    );
    assert!(
        pending_timings.try_borrow_mut().is_ok(),
        "pending_timings is locked during a callback"
    );
    assert!(
        last_timings_report.try_borrow_mut().is_ok(),
        "last_timings_report is locked during a callback"
    );
    assert!(
        on_frame_scheduled.try_lock().is_some(),
        "on_frame_scheduled is locked during a callback"
    );

    assert!(
        task_queue.is_unlocked(),
        "TaskQueue's lock is locked during a callback"
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

    let owner = crate::OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let now = Instant::now();
    let frame_id = owner
        .drive_frame(
            now,
            super::IdleDeadline::far_future(now),
            || {},
            || {
                scheduler
                    .current_frame()
                    .expect("pipeline has an open frame")
                    .id
            },
        )
        .expect("live owner frame");

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
    thread_local! {
        static PROBE: RefCell<Option<WeakUpdateScheduler>> = const { RefCell::new(None) };
    }
    struct ProbingWaker {
        owner_thread: std::thread::ThreadId,
        ran: Arc<AtomicBool>,
    }

    impl std::task::Wake for ProbingWaker {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            assert_eq!(self.owner_thread, std::thread::current().id());
            let probe = PROBE
                .with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .and_then(WeakUpdateScheduler::upgrade)
                })
                .expect("live owner probe");
            assert_no_scheduler_lock_held(&probe);
            self.ran.store(true, Ordering::Release);
        }
    }

    let scheduler = UpdateScheduler::new();
    PROBE.with(|slot| *slot.borrow_mut() = Some(scheduler.downgrade()));
    let ran = Arc::new(AtomicBool::new(false));
    let mut future = scheduler.end_of_frame();
    let waker = Waker::from(Arc::new(ProbingWaker {
        owner_thread: std::thread::current().id(),
        ran: Arc::clone(&ran),
    }));
    let mut cx = Context::from_waker(&waker);
    assert!(Pin::new(&mut future).poll(&mut cx).is_pending());

    crate::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(
            crate::Instant::now(),
            crate::IdleDeadline::far_future(crate::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");

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
