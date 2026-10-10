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
    scheduler.drive_frame(
        &flui_scheduler::OwnerFrame::new(&scheduler)
            .expect("the scheduler has no live owner frame"),
        Instant::now(),
        far_deadline(),
        || {
            log_pipe.push("pipeline");
        },
    );

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
    let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("one frame owner");
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {
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
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

/// The guard the previous test's `Idle` assertion protects: a frame left open at
/// `PersistentCallbacks` would make the next `handle_begin_frame` attempt an
/// illegal `PersistentCallbacks -> TransientCallbacks` transition.
fn a_frame_after_a_panicking_frame_starts_cleanly() {
    let scheduler = UpdateScheduler::new();
    let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("one frame owner");
    let _ = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || panic!("boom"));
    }));

    let ran = Arc::new(AtomicUsize::new(0));
    let ran_cb = Arc::clone(&ran);
    scheduler.add_post_frame_callback(Box::new(move |_| {
        ran_cb.fetch_add(1, Ordering::SeqCst);
    }));

    // Would `debug_assert!` on the illegal transition if the frame were still open.
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
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
    scheduler.add_persistent_frame_callback(std::rc::Rc::new(move |_| {
        log_persistent.push("persistent");
    }));

    let log_pipe = log.clone();
    let log_post = log.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        log_post.push("post_frame");
    }));
    scheduler.drive_frame(
        &flui_scheduler::OwnerFrame::new(&scheduler)
            .expect("the scheduler has no live owner frame"),
        Instant::now(),
        far_deadline(),
        || {
            log_pipe.push("pipeline");
        },
    );

    assert_eq!(
        log.get(),
        vec!["persistent", "pipeline", "post_frame"],
        "persistent callbacks run first; the pipeline follows them"
    );
}

fn retiring_owner_cancels_the_active_local_tail() {
    for fail_after_retire in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = std::rc::Rc::new(flui_scheduler::OwnerFrame::new(&scheduler).expect("owner"));
        let lane = owner.post_frame_handle();
        let head = owner.clone();
        lane.schedule(move |_| {
            assert!(head.retire().is_none());
            assert!(!fail_after_retire, "head failure");
        })
        .expect("head");
        let ran = std::rc::Rc::new(std::cell::Cell::new(false));
        let tail = ran.clone();
        lane.schedule(move |_| tail.set(true)).expect("tail");
        let result = catch_unwind(AssertUnwindSafe(|| {
            scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        }));
        assert_eq!(result.is_err(), fail_after_retire);
        if let Err(failure) = result {
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("head failure")
            );
        }
        assert!(
            !ran.get(),
            "retirement cancels callbacks already snapshotted for this frame"
        );
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert!(
            !ran.get(),
            "an abandoned frame cannot restore a retired local tail"
        );
    }
}

fn retiring_owner_preserves_registration_order_across_the_active_tail() {
    struct Capture {
        name: &'static str,
        log: Log,
        fail: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.log.push(self.name);
            if self.fail {
                std::panic::panic_any(self.name);
            }
        }
    }

    for failures in [[false, false], [true, false], [false, true], [true, true]] {
        let scheduler = UpdateScheduler::new();
        let owner = std::rc::Rc::new(flui_scheduler::OwnerFrame::new(&scheduler).expect("owner"));
        let lane = owner.post_frame_handle();
        let log = Log::default();
        let head = owner.clone();
        let later_lane = lane.clone();
        let later_log = log.clone();
        lane.schedule(move |_| {
            let newer = Capture {
                name: "newer",
                log: later_log,
                fail: failures[1],
            };
            later_lane
                .schedule(move |_| drop(newer))
                .expect("newer callback");
            let failure = head.retire();
            let expected = if failures[0] {
                Some("older")
            } else if failures[1] {
                Some("newer")
            } else {
                None
            };
            assert_eq!(
                failure
                    .as_ref()
                    .and_then(|payload| flui_foundation::panic::payload_text(payload.as_ref())),
                expected
            );
        })
        .expect("head");
        for (name, fail) in [("older", failures[0]), ("middle", false)] {
            let capture = Capture {
                name,
                log: log.clone(),
                fail,
            };
            lane.schedule(move |_| drop(capture)).expect("active tail");
        }
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert_eq!(log.get(), ["older", "middle", "newer"]);
        assert!(owner.retire().is_none());
        assert!(lane.schedule(|_| {}).is_err());
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert_eq!(log.get(), ["older", "middle", "newer"]);
    }
}

fn incomplete_presentations_do_not_block_eligible_siblings() {
    let scheduler = UpdateScheduler::new();
    let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
    let a = owner.presentation_scope();
    let b = owner.presentation_scope();
    let a_handle = a.post_frame_handle();
    let log = Log::default();
    let initial = log.clone();
    a_handle
        .schedule(move |_| initial.push("a original"))
        .expect("idle registration");
    a.enter_segment().expect("a starts");
    a.suspend_segment().expect("a waits");
    b.enter_segment().expect("b starts");
    let sibling = log.clone();
    let retained_a = a_handle.clone();
    b.post_frame_handle()
        .schedule(move |_| {
            sibling.push("b");
            let later = sibling.clone();
            retained_a
                .schedule(move |_| later.push("a next from b"))
                .expect("retained a handle");
        })
        .expect("b callback");
    let runtime = log.clone();
    owner
        .post_frame_handle()
        .schedule(move |_| runtime.push("runtime"))
        .expect("runtime callback");
    b.complete_segment().expect("b coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(log.get(), ["b", "runtime"]);

    a.service_segment().expect("detached service");
    let service = log.clone();
    a_handle
        .schedule(move |_| service.push("a next from service"))
        .expect("service registration");
    a.resume_segment().expect("resume original epoch");
    let late = log.clone();
    let self_handle = a_handle.clone();
    a_handle
        .schedule(move |_| {
            late.push("a late layout");
            let next = late.clone();
            self_handle
                .schedule(move |_| next.push("a self next"))
                .expect("self registration");
        })
        .expect("resumed layout registration");
    a.complete_segment().expect("a coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(log.get(), ["b", "runtime", "a original", "a late layout"]);
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(
        log.get().len(),
        4,
        "a successor needs its own geometry completion"
    );
    a.enter_segment().expect("next a segment");
    a.complete_segment().expect("next a coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(
        log.get(),
        [
            "b",
            "runtime",
            "a original",
            "a late layout",
            "a next from b",
            "a next from service",
            "a self next"
        ]
    );
}

fn cancelled_geometry_transfers_completion_debt_to_replacement() {
    let scheduler = UpdateScheduler::new();
    let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
    let presentation = owner.presentation_scope();
    let log = Log::default();
    presentation.enter_segment().expect("start");
    let pending = log.clone();
    presentation
        .post_frame_handle()
        .schedule(move |_| pending.push("coherent replacement"))
        .expect("callback");
    presentation.cancel_segment().expect("withdraw geometry");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(log.get(), [] as [&str; 0]);
    assert!(
        presentation.enter_segment().is_err(),
        "debt cannot be replaced by a fresh epoch"
    );
    presentation
        .resume_segment()
        .expect("replacement retains promise");
    presentation
        .complete_segment()
        .expect("replacement coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(log.get(), ["coherent replacement"]);
}

fn eligible_panic_preserves_exact_epochs_and_healthy_recovery() {
    let scheduler = UpdateScheduler::new();
    let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
    let a = owner.presentation_scope();
    let b = owner.presentation_scope();
    let log = Log::default();
    a.enter_segment().expect("a starts");
    a.post_frame_handle()
        .schedule(|_| panic!("scoped head"))
        .expect("head");
    let tail = log.clone();
    a.post_frame_handle()
        .schedule(move |_| tail.push("a tail"))
        .expect("tail");
    b.enter_segment().expect("b starts");
    let blocked = log.clone();
    b.post_frame_handle()
        .schedule(move |_| blocked.push("b"))
        .expect("b");
    b.suspend_segment().expect("b waits");
    a.complete_segment().expect("a coherent");
    let result = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    }))
    .expect_err("head fails");
    assert_eq!(
        flui_foundation::panic::payload_text(result.as_ref()),
        Some("scoped head")
    );
    assert_eq!(log.get(), [] as [&str; 0]);
    a.enter_segment()
        .expect("new a segment after failed callback");
    a.suspend_segment().expect("new a geometry is incomplete");
    b.resume_segment().expect("b resumes");
    b.complete_segment().expect("b coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(
        log.get(),
        ["b"],
        "old completed tail cannot observe newer partial geometry"
    );
    a.resume_segment().expect("new a resumes");
    a.complete_segment().expect("new a coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(log.get(), ["b", "a tail"]);
}

fn completion_demand_coalesces_and_survives_hidden_presentation_gating() {
    use flui_scheduler::{DemandKind, FrameClock, PollDecision, SkipReason};

    let scheduler = UpdateScheduler::new();
    let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
    let presentation = owner
        .presentation_scope_factory()
        .create()
        .expect("live presentation");
    let clock = FrameClock::new();
    let log = Log::default();
    let completion = log.clone();
    presentation
        .post_frame_handle()
        .schedule(move |_| completion.push("coherent completion"))
        .expect("accepted completion");
    clock.mark_demand(DemandKind::Completion);
    clock.mark_demand(DemandKind::Completion);
    clock.mark_demand(DemandKind::Dirty);
    clock.clear_demand(DemandKind::Dirty);
    assert!(clock.try_arm_redraw_request());
    assert!(
        !clock.try_arm_redraw_request(),
        "one actuator edge for coalesced completion debt"
    );
    clock.set_hidden(true);
    assert_eq!(
        clock.poll(clock.now()),
        PollDecision::Skip(SkipReason::Hidden)
    );
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert!(log.get().is_empty(), "hidden completion remains accepted");

    clock.set_hidden(false);
    assert_eq!(clock.poll(clock.now()), PollDecision::Produce);
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {
        if presentation.has_completion_demand() {
            presentation.enter_segment().expect("eligible clean epoch");
            presentation.complete_segment().expect("coherent geometry");
        }
    });
    assert_eq!(log.get(), ["coherent completion"]);
    assert_eq!(
        clock.poll(clock.now()),
        PollDecision::Skip(SkipReason::NoDemand)
    );
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(log.get(), ["coherent completion"]);
}

fn successor_completion_demand_survives_wake_failure_and_host_clearing() {
    for failing_wake in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
        let factory = owner.presentation_scope_factory();
        let presentation = factory.create().expect("live assembly authority");
        let log = Log::default();
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempted = attempts.clone();
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            attempted.fetch_add(1, Ordering::SeqCst);
            assert!(!failing_wake, "completion wake failed");
        })));
        let unscoped = log.clone();
        owner
            .post_frame_handle()
            .schedule(move |_| unscoped.push("unscoped"))
            .expect("unscoped registration");
        assert_eq!(attempts.load(Ordering::SeqCst), 0);

        let first = log.clone();
        let successor = presentation.post_frame_handle();
        let admission = catch_unwind(AssertUnwindSafe(|| {
            presentation
                .post_frame_handle()
                .schedule(move |_| {
                    first.push("first completion");
                    let second = first.clone();
                    successor
                        .schedule(move |_| second.push("successor completion"))
                        .expect("completion registers successor");
                })
                .expect("scoped admission");
        }));
        assert_eq!(admission.is_err(), failing_wake);
        if let Err(failure) = admission {
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("completion wake failed")
            );
        }
        let healthy_wakes = Arc::new(AtomicUsize::new(0));
        let healthy = healthy_wakes.clone();
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            healthy.fetch_add(1, Ordering::SeqCst);
        })));

        // An unrelated host frame can clear its latch without admitting this
        // presentation. The scoped completion promise remains owned.
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert_eq!(log.get(), ["unscoped"]);
        let before_rearm = healthy_wakes.load(Ordering::SeqCst);
        presentation.rearm_completion_demand();
        assert!(healthy_wakes.load(Ordering::SeqCst) > before_rearm);
        for _ in 0..2 {
            scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {
                if presentation.has_completion_demand() {
                    presentation
                        .enter_segment()
                        .expect("completion-only admission");
                    presentation
                        .complete_segment()
                        .expect("coherent unchanged geometry");
                }
            });
        }
        assert_eq!(
            log.get(),
            ["unscoped", "first completion", "successor completion"]
        );
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert_eq!(log.get().len(), 3);
        drop(presentation);
        drop(owner);
        assert!(
            factory.create().is_err(),
            "assembly authority cannot follow a replacement owner"
        );
    }
}

fn reentrant_partial_geometry_blocks_an_already_selected_tail() {
    let scheduler = UpdateScheduler::new();
    let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
    let a = std::rc::Rc::new(owner.presentation_scope());
    let b = owner.presentation_scope();
    let c = owner.presentation_scope();
    let log = Log::default();

    b.enter_segment().expect("b starts");
    let reentrant_a = a.clone();
    let earlier = log.clone();
    b.post_frame_handle()
        .schedule(move |_| {
            earlier.push("b replaces a geometry");
            reentrant_a.enter_segment().expect("a was idle");
            reentrant_a.suspend_segment().expect("new a is partial");
        })
        .expect("earliest callback");
    b.suspend_segment().expect("b initially incomplete");

    a.enter_segment().expect("a starts");
    a.post_frame_handle()
        .schedule(|_| panic!("initial a callback"))
        .expect("a head");
    let retained = log.clone();
    a.post_frame_handle()
        .schedule(move |_| retained.push("a coherent tail"))
        .expect("a tail");
    a.complete_segment().expect("a coherent");
    let failure = catch_unwind(AssertUnwindSafe(|| {
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    }))
    .expect_err("a head fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("initial a callback")
    );
    assert_eq!(log.get(), [] as [&str; 0]);

    b.resume_segment().expect("b resumes");
    b.complete_segment().expect("b coherent");
    c.enter_segment().expect("c starts");
    let healthy = log.clone();
    c.post_frame_handle()
        .schedule(move |_| healthy.push("c stays healthy"))
        .expect("c callback");
    c.complete_segment().expect("c coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(
        log.get(),
        ["b replaces a geometry", "c stays healthy"],
        "eligibility must survive reentrant changes after the batch snapshot"
    );

    a.resume_segment().expect("replacement a resumes");
    a.complete_segment().expect("replacement a coherent");
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
    assert_eq!(
        log.get(),
        [
            "b replaces a geometry",
            "c stays healthy",
            "a coherent tail"
        ]
    );
}

fn preserving_scope_close_retains_captures_and_keeps_siblings_deliverable() {
    struct Capture {
        log: Log,
        fail: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.log.push("capture dropped");
            assert!(!self.fail, "capture retirement must be retained");
        }
    }
    for capture_failure in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
        let a = owner.presentation_scope();
        let b = owner.presentation_scope();
        let log = Log::default();
        let capture = Capture {
            log: log.clone(),
            fail: capture_failure,
        };
        a.post_frame_handle()
            .schedule(move |_| drop(capture))
            .expect("a callback accepted");
        let first_failure = catch_unwind(AssertUnwindSafe(|| panic!("prior owner failure")))
            .expect_err("prior owner failed");
        // Caught failure makes thread::panicking false. Explicit preservation
        // must nevertheless keep opaque callback captures out of destruction.
        a.withdraw();
        a.close_preserving();
        assert!(a.post_frame_handle().schedule(|_| {}).is_err());
        assert!(a.close().is_none());
        drop(a);
        assert_eq!(log.get(), [] as [&str; 0]);
        b.enter_segment().expect("b starts");
        let sibling = log.clone();
        b.post_frame_handle()
            .schedule(move |_| sibling.push("healthy sibling"))
            .expect("b callback");
        b.complete_segment().expect("b coherent");
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert!(owner.retire().is_none());
        assert_eq!(log.get(), ["healthy sibling"]);
        assert_eq!(
            flui_foundation::panic::payload_text(first_failure.as_ref()),
            Some("prior owner failure")
        );
    }
}

fn withdrawal_defers_capture_retirement_until_other_authority_is_closed() {
    struct RetiringOwner {
        handle: flui_scheduler::PostFrameHandle,
        log: Log,
        name: &'static str,
        fail: bool,
    }
    impl Drop for RetiringOwner {
        fn drop(&mut self) {
            assert!(
                self.handle.schedule(|_| {}).is_err(),
                "withdrawn handle cannot reenter"
            );
            self.log.push(self.name);
            if self.fail {
                std::panic::panic_any(self.name);
            }
        }
    }
    for failures in [[false, false], [true, false], [false, true], [true, true]] {
        let scheduler = UpdateScheduler::new();
        let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
        let a = owner.presentation_scope();
        let b = owner.presentation_scope();
        let log = Log::default();
        for (name, fail) in [
            ("first capture", failures[0]),
            ("second capture", failures[1]),
        ] {
            let capture = RetiringOwner {
                handle: a.post_frame_handle(),
                log: log.clone(),
                name,
                fail,
            };
            a.post_frame_handle()
                .schedule(move |_| drop(capture))
                .expect("accepted capture");
        }
        a.withdraw();
        assert!(
            log.get().is_empty(),
            "withdrawal cannot run capture destruction"
        );
        drop(RetiringOwner {
            handle: a.post_frame_handle(),
            log: log.clone(),
            name: "other authority retired",
            fail: false,
        });
        assert_eq!(log.get(), ["other authority retired"]);
        let failure = a.close();
        assert_eq!(
            failure
                .as_ref()
                .and_then(|failure| flui_foundation::panic::payload_text(failure.as_ref())),
            if failures[0] {
                Some("first capture")
            } else if failures[1] {
                Some("second capture")
            } else {
                None
            }
        );
        assert!(
            a.close().is_none(),
            "retirement after withdrawal stays idempotent"
        );
        drop(a);
        b.enter_segment().expect("sibling starts");
        let sibling = log.clone();
        b.post_frame_handle()
            .schedule(move |_| sibling.push("sibling healthy"))
            .expect("sibling callback");
        b.complete_segment().expect("sibling coherent");
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert_eq!(
            log.get(),
            [
                "other authority retired",
                "first capture",
                "second capture",
                "sibling healthy"
            ]
        );
    }
}

fn closing_a_scope_retires_its_active_tail_without_losing_siblings() {
    struct Capture {
        handle: flui_scheduler::PostFrameHandle,
        log: Log,
        fail: bool,
        name: &'static str,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            assert!(
                self.handle.schedule(|_| {}).is_err(),
                "closed authority refuses reentry"
            );
            self.log.push(self.name);
            if self.fail {
                std::panic::panic_any(self.name);
            }
        }
    }
    for failures in [[false, false], [true, false], [false, true], [true, true]] {
        let scheduler = UpdateScheduler::new();
        let owner = flui_scheduler::OwnerFrame::new(&scheduler).expect("owner");
        let a = std::rc::Rc::new(owner.presentation_scope());
        let b = owner.presentation_scope();
        let log = Log::default();
        a.enter_segment().expect("a starts");
        let head = a.clone();
        a.post_frame_handle()
            .schedule(move |_| {
                if let Some(failure) = head.close() {
                    std::panic::resume_unwind(failure);
                }
            })
            .expect("head");
        for (name, fail) in [
            ("first retirement", failures[0]),
            ("second retirement", failures[1]),
        ] {
            let capture = Capture {
                handle: a.post_frame_handle(),
                log: log.clone(),
                fail,
                name,
            };
            a.post_frame_handle()
                .schedule(move |_| drop(capture))
                .expect("active a tail");
        }
        b.enter_segment().expect("b starts");
        let sibling = log.clone();
        b.post_frame_handle()
            .schedule(move |_| sibling.push("b survives"))
            .expect("b");
        a.complete_segment().expect("a coherent");
        b.complete_segment().expect("b coherent");
        let result = catch_unwind(AssertUnwindSafe(|| {
            scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        }));
        assert_eq!(result.is_err(), failures.contains(&true));
        if let Err(failure) = result {
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some(if failures[0] {
                    "first retirement"
                } else {
                    "second retirement"
                })
            );
            assert_eq!(log.get(), ["first retirement", "second retirement"]);
        }
        scheduler.drive_frame(&owner, Instant::now(), far_deadline(), || {});
        assert_eq!(
            log.get(),
            ["first retirement", "second retirement", "b survives"]
        );
        assert!(a.post_frame_handle().schedule(|_| {}).is_err());
    }
}

#[test]
fn post_frame_ordering_matrix() {
    crate::run_table(
        "post_frame_ordering_matrix",
        &[
            (
                "preserving_scope_close_retains_captures_and_keeps_siblings_deliverable",
                preserving_scope_close_retains_captures_and_keeps_siblings_deliverable as fn(),
            ),
            (
                "withdrawal_defers_capture_retirement_until_other_authority_is_closed",
                withdrawal_defers_capture_retirement_until_other_authority_is_closed as fn(),
            ),
            (
                "completion_demand_coalesces_and_survives_hidden_presentation_gating",
                completion_demand_coalesces_and_survives_hidden_presentation_gating as fn(),
            ),
            (
                "successor_completion_demand_survives_wake_failure_and_host_clearing",
                successor_completion_demand_survives_wake_failure_and_host_clearing as fn(),
            ),
            (
                "reentrant_partial_geometry_blocks_an_already_selected_tail",
                reentrant_partial_geometry_blocks_an_already_selected_tail as fn(),
            ),
            (
                "incomplete_presentations_do_not_block_eligible_siblings",
                incomplete_presentations_do_not_block_eligible_siblings as fn(),
            ),
            (
                "cancelled_geometry_transfers_completion_debt_to_replacement",
                cancelled_geometry_transfers_completion_debt_to_replacement as fn(),
            ),
            (
                "eligible_panic_preserves_exact_epochs_and_healthy_recovery",
                eligible_panic_preserves_exact_epochs_and_healthy_recovery as fn(),
            ),
            (
                "closing_a_scope_retires_its_active_tail_without_losing_siblings",
                closing_a_scope_retires_its_active_tail_without_losing_siblings as fn(),
            ),
            (
                "retiring_owner_preserves_registration_order_across_the_active_tail",
                retiring_owner_preserves_registration_order_across_the_active_tail as fn(),
            ),
            (
                "retiring_owner_cancels_the_active_local_tail",
                retiring_owner_cancels_the_active_local_tail as fn(),
            ),
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
