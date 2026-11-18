//! Scheduler lock discipline: every callback family runs with no scheduler
//! storage borrowed or locked, so a callback can call back into the
//! scheduler, including the read-only `current_frame()` accessor.
//!
//! A child module of `scheduler`, so the oracle sees the private state
//! structs (`SchedulerInner`, `FrameState`, `CallbackState`, `BindingState`,
//! `WakeShared`) it destructures.

use super::*;

/// Probes every `RefCell` and `Mutex` a scheduler callback could observe from
/// inside its own invocation and asserts each one is free. Every callback
/// family drains, clones or snapshots its queue before it runs user code (see
/// `end_frame`, `dispatch_post_frame_callbacks`,
/// `handle_app_lifecycle_state_change` and `request_frame_impl`); this oracle
/// checks that at the call site.
///
/// Exhaustive by construction: the state structs are destructured with no
/// trailing `..`, so a field added to any of them fails to compile here until
/// it is classified: a `RefCell` or `Mutex` gets an assertion, anything else
/// (an atomic, an id mint) is bound `_`, so the decision is visible. The async
/// tasks and the post-frame entries live in the owner's `OwnerFrame`, not in
/// scheduler storage; the post-frame lane is probed through its own
/// `is_unlocked`, and `AsyncDriver::is_unlocked` serves the owner's own
/// re-entrancy tests.
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
        owner_frame_claimed: _,
        execution_release,
        execution_failure,
        execution_owner,
    } = &*scheduler.inner;
    assert!(
        execution_release.try_borrow_mut().is_ok(),
        "terminal release custody is not borrowed through user code"
    );
    assert!(
        execution_failure.try_borrow_mut().is_ok(),
        "execution failure receipt is not borrowed through user code"
    );
    assert!(
        execution_owner.try_borrow_mut().is_ok(),
        "execution owner metadata is not borrowed through user code"
    );

    let FrameState {
        current_frame,
        current_vsync_time,
        next_frame_id: _,
        clock: _,
        display_period: _,
        frame_count: _,
        completion_waiters,
    } = frame;
    let WakeShared {
        scheduler_phase: _,
        frames_enabled: _,
        owner_thread: _,
        frame_scheduled: _,
        wake_delivery,
        on_frame_scheduled,
        closed: _,
        late_visual_update: _,
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
        completion_waiters.try_borrow_mut().is_ok(),
        "completion_waiters is locked during a callback"
    );

    assert!(
        wake_delivery.is_unlocked(),
        "wake delivery is locked during a callback"
    );

    let CallbackState {
        id_mint: _,
        post_frame,
        // A `Notifier` lends no borrow of its list to a listener it runs.
        lifecycle_listeners: _,
    } = callbacks;
    assert!(
        post_frame.try_borrow_mut().is_ok(),
        "post_frame is locked during a callback"
    );
    if let Some(lane) = post_frame.borrow().lane() {
        assert!(
            lane.is_unlocked(),
            "post-frame lane state is borrowed during a callback"
        );
    }

    let BindingState {
        lifecycle_state: _,
        lifecycle_delivery: _,
    } = binding;
    assert!(
        on_frame_scheduled.try_lock().is_some(),
        "on_frame_scheduled is locked during a callback"
    );
}

/// Also pins that the callback receives THIS frame's own timing, not merely
/// that it runs unlocked: the id it is handed must match the one the
/// pipeline read from `current_frame()`.
fn post_frame_callback_runs_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let observed: Rc<Cell<Option<FrameId>>> = Rc::new(Cell::new(None));
    let observed_for_callback = Rc::clone(&observed);
    scheduler.add_post_frame_callback(Box::new(move |timing| {
        assert_no_scheduler_lock_held(&probe);
        observed_for_callback.set(Some(timing.id));
    }));

    let owner =
        crate::OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
    let now = Instant::now();
    let frame_id = owner
        .drive_frame(
            now,
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
        observed.get(),
        Some(frame_id),
        "the post-frame callback must receive THIS frame's own timing"
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
    use flui_foundation::testing::CountingWaker;

    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static PROBE: RefCell<Option<WeakUpdateScheduler>> = const { RefCell::new(None) };
    }
    let scheduler = UpdateScheduler::new();
    PROBE.with(|slot| *slot.borrow_mut() = Some(scheduler.downgrade()));
    let ran = Arc::new(AtomicBool::new(false));
    let mut future = scheduler.end_of_frame();
    let owner_thread = std::thread::current().id();
    let ran_in_wake = Arc::clone(&ran);
    let waker = CountingWaker::calling(move || {
        assert_eq!(owner_thread, std::thread::current().id());
        let probe = PROBE
            .with(|slot| {
                slot.borrow()
                    .as_ref()
                    .and_then(WeakUpdateScheduler::upgrade)
            })
            .expect("live owner probe");
        assert_no_scheduler_lock_held(&probe);
        ran_in_wake.store(true, Ordering::Release);
    })
    .shared()
    .waker();
    let mut ctx = Context::from_waker(&waker);
    assert!(Pin::new(&mut future).poll(&mut ctx).is_pending());

    crate::OwnerFrame::try_new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(crate::Instant::now(), || {}, || {})
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
                "post_frame_callback_runs_with_no_scheduler_lock_held",
                post_frame_callback_runs_with_no_scheduler_lock_held as fn(),
            ),
            (
                "completion_waker_runs_with_no_scheduler_lock_held",
                completion_waker_runs_with_no_scheduler_lock_held as fn(),
            ),
        ],
    );
}
