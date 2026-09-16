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

use std::sync::OnceLock;

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
/// covered by their own standalone tests instead of being folded into this
/// destructure -- neither is a `Mutex`, but each is still a real
/// reentrancy hazard with its own different failure mode, not a lesser
/// one: `DashMap` (6.2.1) is a SHARDED `RwLock`, not lock-free --
/// `contains_key`/`get`/`get_mut` release their shard's lock immediately
/// when a key is absent, but a `Ref`/`RefMut`/`Entry` held alive across a
/// reentrant call into the SAME shard deadlocks exactly like the `Mutex`
/// family above (`entry()` holds its shard write-locked for its entire
/// life, vacant or occupied, regardless of whether the caller binds the
/// payload to a name); a reentrant `RefCell` borrow panics rather than
/// deadlocking, a third failure mode again.
/// [`cancelled_dashmap_not_locked_on_reentrant_cancel_of_a_settled_id`]
/// probes the running callback's own key with `try_get_mut` (a `try_write`
/// attempt, so it fails on ANY existing holder, reader or writer) BEFORE
/// making any other call into the map -- DashMap 6.2.1 has no all-shards
/// "is anything locked" API, so this is a completeness pin on the one key
/// a reentrant dispatch actually touches, not an exhaustive per-shard
/// sweep. `LocalPostFrameLane::is_unlocked` is asserted directly inside
/// `post_frame.rs`'s `local_then_local_nested_registration_defers`, since
/// a lane is never a field of `SchedulerInner` for this destructure to see
/// in the first place -- each `new_local_post_frame_lane()` call hands the
/// caller its own, held separately from the scheduler's own storage.
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

/// `end_of_frame()` reaches the platform wake hook, so the completion
/// registry's guard must be released before the demand is issued.
///
/// Asserting the hook RAN is half the oracle: `end_of_frame` used to issue
/// no demand at all, and without that assertion this test passes against
/// that version by never firing the hook it is probing.
#[test]
fn end_of_frame_demand_runs_the_frame_scheduled_hook_with_no_scheduler_lock_held() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();
    let ran = Arc::new(AtomicBool::new(false));
    let ran_for_hook = Arc::clone(&ran);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        assert_no_scheduler_lock_held(&probe);
        ran_for_hook.store(true, Ordering::Release);
    })));

    let _waiter = scheduler.end_of_frame();

    assert!(
        ran.load(Ordering::Acquire),
        "the hook must actually have fired -- an unfired hook asserts nothing"
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
#[test]
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

/// Completeness pin for the `callbacks.cancelled` DashMap named in
/// [`assert_no_scheduler_lock_held`]'s own doc. DashMap 6.2.1 is a sharded
/// `RwLock`, not lock-free: a `Ref`/`RefMut`/`Entry` held alive across a
/// reentrant call into the SAME shard deadlocks, exactly like the `Mutex`
/// family the rest of this oracle covers (empirically confirmed:
/// `entry()` holds its shard's write-lock for its ENTIRE lifetime, vacant
/// or occupied, even matched into `Entry::Vacant(_)`/`Entry::Occupied(_)`
/// with the payload left unbound; `get()`/`get_mut()` release their
/// shard's lock immediately when the key is ABSENT, but hold it as long as
/// a bound `Some(..)` result stays alive when the key is PRESENT).
///
/// This probes the transient dispatch loop's own "skip if cancelled"
/// check (`contains_key`, in `handle_begin_frame`): it captures the
/// CURRENTLY DISPATCHING callback's own id (`own_id`, via an
/// `Arc<OnceLock<CallbackId>>` set right after `schedule_frame_callback`
/// returns, since the id is not known until then) and, as the very first
/// thing the callback does, asserts `!cancelled.try_get_mut(&own_id).is_locked()`
/// -- proving the dispatch loop released whatever guard its own lookup
/// produced BEFORE invoking this callback, not merely that some later,
/// unrelated call succeeds. Reddens if the dispatch loop is ever
/// "improved" into
/// `match self.inner.callbacks.cancelled.entry(cancellable.id) { Occupied(_)
/// => continue, Vacant(_) => (cancellable.callback)(vsync_time) }`: an
/// unbound `Vacant(_)` arm still holds `own_id`'s shard write-locked for
/// the whole match body, exactly where this probe trips.
///
/// The second half keeps the original test's purpose:
/// `cancel_frame_callback` (scheduler.rs) takes `position`+`remove` for a
/// still-queued id and never touches `cancelled`; `.insert(id, ())` fires
/// only on the not-found branch (already-fired or never-registered). From
/// inside the same callback, cancel an id that has already fired
/// (`settled_id`), assert its shard is not locked afterward (a per-key/
/// shard pin -- DashMap 6.2.1 has no all-shards "is anything locked" API),
/// and additionally assert `try_get(&settled_id)` is `Present` afterward,
/// so the insert branch is known to have actually run rather than this
/// assertion passing vacuously against the OTHER (`position`+`remove`)
/// branch.
#[test]
fn cancelled_dashmap_not_locked_on_reentrant_cancel_of_a_settled_id() {
    let scheduler = UpdateScheduler::new();
    let probe = scheduler.clone();

    // Registered first, so the one-shot transient queue pops and runs this
    // before the probing callback below -- "settled" by the time it is
    // cancelled.
    let settled_id = scheduler.schedule_frame_callback(Box::new(|_| {}));

    let own_id: Arc<OnceLock<CallbackId>> = Arc::new(OnceLock::new());
    let own_id_for_callback = Arc::clone(&own_id);
    let registered_id = scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
        let own_id = own_id_for_callback
            .get()
            .copied()
            .expect("set immediately after schedule_frame_callback returns, below");

        assert!(
            !probe
                .inner
                .callbacks
                .cancelled
                .try_get_mut(&own_id)
                .is_locked(),
            "the dispatch loop's own cancellation check must release its \
             guard on this callback's id BEFORE invoking it"
        );

        assert!(
            !probe.cancel_frame_callback(settled_id),
            "the earlier callback must already have run (and been removed \
             from the queue) by the time this one does"
        );
        let result = probe.inner.callbacks.cancelled.try_get(&settled_id);
        assert!(
            !result.is_locked(),
            "cancel_frame_callback's DashMap insert on a settled id must \
             not leave that id's shard locked for a reentrant caller"
        );
        assert!(
            result.is_present(),
            "the not-found branch's insert must have actually run -- \
             otherwise the assertion above passes vacuously against the \
             OTHER branch"
        );
    }));
    own_id
        .set(registered_id)
        .expect("set exactly once, before the callback can possibly run");

    scheduler.execute_frame();
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

/// Cancelling a not-yet-run sibling and registering a fresh one from inside
/// a transient callback must not let the fresh one run in the SAME frame.
///
/// `cancel_frame_callback` removes the cancelled entry from the live queue
/// directly when it is still queued (#1156's lock-then-drop fix) — so
/// cancelling B and registering C in the same callback leaves the queue's
/// LENGTH unchanged (minus one for B, plus one for C), even though B and C
/// are different entries with different ids. A bound expressed as "how many
/// entries were queued at entry" (issue #1057's own regression) is fooled by
/// this: the length-based budget still reaches the fresh entry C, because
/// the length looks the same as if B had simply run. An id watermark is
/// not: C's id always exceeds whatever was queued at the start of THIS
/// `handle_begin_frame` call, regardless of what got cancelled out from
/// under it in between.
#[test]
fn transient_callback_cancelling_a_later_sibling_and_registering_a_replacement_defers_the_replacement()
 {
    let scheduler = UpdateScheduler::new();
    let sibling_ran = Arc::new(AtomicU32::new(0));
    let replacement_ran = Arc::new(AtomicU32::new(0));
    let sibling_id_slot: Arc<Mutex<Option<CallbackId>>> = Arc::new(Mutex::new(None));

    // A: registered FIRST, so it pops and runs before B. Cancels B (still
    // queued, not yet invoked -- a "later" sibling) and registers C.
    let cancel_scheduler = scheduler.clone();
    let replacement_ran_for_a = Arc::clone(&replacement_ran);
    let sibling_id_slot_for_a = Arc::clone(&sibling_id_slot);
    scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
        let sibling_id: CallbackId = sibling_id_slot_for_a
            .lock()
            .expect("B must already be registered by the time A runs");
        assert!(
            cancel_scheduler.cancel_frame_callback(sibling_id),
            "B must still be queued (not yet invoked) when A cancels it"
        );
        let replacement_ran = Arc::clone(&replacement_ran_for_a);
        cancel_scheduler.schedule_frame_callback(Box::new(move |_| {
            replacement_ran.fetch_add(1, Ordering::SeqCst);
        }));
    }));

    // B: registered SECOND -- "later" than A -- and cancelled by A before
    // it ever runs.
    let sibling_ran_for_b = Arc::clone(&sibling_ran);
    let sibling_id = scheduler.schedule_frame_callback(Box::new(move |_| {
        sibling_ran_for_b.fetch_add(1, Ordering::SeqCst);
    }));
    let _prev = sibling_id_slot.lock().replace(sibling_id);

    scheduler.execute_frame();
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        0,
        "B was cancelled before it ever ran"
    );
    assert_eq!(
        replacement_ran.load(Ordering::SeqCst),
        0,
        "C must not run in the same handle_begin_frame call that registered it"
    );

    scheduler.execute_frame();
    assert_eq!(
        replacement_ran.load(Ordering::SeqCst),
        1,
        "C must run exactly once, on the very next begin-frame"
    );
    assert_eq!(
        sibling_ran.load(Ordering::SeqCst),
        0,
        "B, cancelled, never runs at all"
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
