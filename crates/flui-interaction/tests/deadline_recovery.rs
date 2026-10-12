//! Owner deadline recovery uses the same binding and open members as consumers.

use std::{
    cell::{Cell, RefCell},
    panic::AssertUnwindSafe,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::{
    MonotonicClock as _,
    geometry::Offset,
    panic::{FailureClass, PanicRecovery},
};
use flui_interaction::{
    GestureBinding, GestureDeadlineError, GestureDeadlineUpdate, ManualClock, PointerId,
    arena::GestureArenaMember,
};
use web_time::Instant;

struct DeadlineMember {
    due: Cell<Option<Instant>>,
    fail_query: Cell<bool>,
    fail_poll: Cell<bool>,
    queries: Cell<usize>,
    effects: Cell<usize>,
    query_publication: RefCell<Option<(GestureDeadlineUpdate, Option<Instant>)>>,
    poll_publication: RefCell<Option<(GestureDeadlineUpdate, Option<Instant>)>>,
    release_owner: RefCell<Option<Rc<RefCell<Option<Rc<Self>>>>>>,
    drops: Rc<Cell<usize>>,
}

impl DeadlineMember {
    fn new(due: Instant) -> Rc<Self> {
        Rc::new(Self {
            due: Cell::new(Some(due)),
            fail_query: Cell::new(false),
            fail_poll: Cell::new(false),
            queries: Cell::new(0),
            effects: Cell::new(0),
            query_publication: RefCell::new(None),
            poll_publication: RefCell::new(None),
            release_owner: RefCell::new(None),
            drops: Rc::new(Cell::new(0)),
        })
    }
}

impl GestureArenaMember for DeadlineMember {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {}
    fn deadline(&self) -> Option<Instant> {
        self.queries.set(self.queries.get() + 1);
        let publication = self.query_publication.borrow_mut().take();
        if let Some((update, due)) = publication {
            update
                .publish(due)
                .expect("reentrant query update is accepted");
        }
        let release = self.release_owner.borrow_mut().take();
        if let Some(owner) = release {
            drop(owner.borrow_mut().take());
        }
        assert!(!self.fail_query.get(), "deadline query failure");
        self.due.get()
    }
    fn poll_deadline(&self, _: Instant) {
        self.effects.set(self.effects.get() + 1);
        let publication = self.poll_publication.borrow_mut().take();
        if let Some((update, due)) = publication {
            update
                .publish(due)
                .expect("reentrant poll update is accepted");
        }
        assert!(
            !self.fail_poll.get(),
            "deadline poll failure after an effect"
        );
        self.due.set(None);
    }
}

impl Drop for DeadlineMember {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

fn pointer(value: u64) -> PointerId {
    PointerId::try_from(value).expect("fixture pointer is nonzero")
}

fn sample(binding: &GestureBinding, now: Instant) -> bool {
    let mut recovery = PanicRecovery::new();
    flui_interaction::__runtime::tick_deadlines_with_recovery(binding, now, &mut recovery.scope());
    recovery.has_failure()
}

fn query_failure_has_one_bounded_probe_and_inert_reads() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(clock.now());
    member.fail_query.set(true);
    let _entry = binding.arena().add(pointer(1), &member);
    let healthy = DeadlineMember::new(clock.now() + Duration::from_millis(50));
    let _healthy_entry = binding.arena().add(pointer(2), &healthy);
    assert!(sample(&binding, clock.now()));
    assert_eq!(member.queries.get(), 1);
    let deadline = std::panic::catch_unwind(AssertUnwindSafe(|| {
        flui_interaction::__runtime::next_deadline_snapshot(&binding)
    }));
    assert!(
        deadline.is_ok(),
        "a host snapshot read must not invoke a failed query"
    );
    assert_eq!(
        deadline.expect("inert read"),
        clock.now().checked_add(Duration::from_millis(50))
    );
    clock.advance(Duration::from_millis(99));
    assert!(
        !sample(&binding, clock.now()),
        "the same obligation is reported once"
    );
    assert_eq!(
        member.queries.get(),
        1,
        "an earlier healthy turn cannot probe early"
    );
    assert_eq!(
        healthy.effects.get(),
        1,
        "an earlier healthy deadline progresses before the failed query retry"
    );
    clock.advance(Duration::from_millis(1));
    assert!(
        !sample(&binding, clock.now()),
        "the re-probe cannot repeat the report"
    );
    assert_eq!(member.queries.get(), 2, "one automatic re-probe is due");
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None
    );
    clock.advance(Duration::from_secs(5));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(
        member.queries.get(),
        2,
        "repeated query failure parks discovery debt"
    );
}

fn unknown_poll_is_not_replayed_and_healthy_tail_progresses() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let failed = DeadlineMember::new(clock.now());
    failed.fail_poll.set(true);
    let healthy = DeadlineMember::new(clock.now());
    let _failed_entry = binding.arena().add(pointer(1), &failed);
    let _healthy_entry = binding.arena().add(pointer(2), &healthy);
    assert!(sample(&binding, clock.now()));
    assert_eq!(
        healthy.effects.get(),
        1,
        "a failed open poll cannot erase the healthy tail"
    );
    assert_eq!(failed.effects.get(), 1);
    clock.advance(Duration::from_secs(1));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(
        failed.effects.get(),
        1,
        "an unknown side effect cannot replay for the same revision"
    );
}

fn a_reused_pointer_admits_a_new_deadline_identity() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(clock.now());
    member.fail_poll.set(true);
    let _old = binding.arena().add(pointer(1), &member);
    assert!(sample(&binding, clock.now()));
    binding.arena().sweep(pointer(1));
    let _new = binding.arena().add(pointer(1), &member);
    assert!(
        sample(&binding, clock.now()),
        "new contact identity can attempt its new obligation"
    );
    assert_eq!(member.effects.get(), 2);
}

fn explicit_same_time_rearm_is_independent_and_stale_authority_refuses() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(clock.now());
    member.fail_poll.set(true);
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = binding
        .arena()
        .register_deadline_member(pointer(1), &erased)
        .expect("one member-wide source");
    let update = registration.update_handle();
    update.publish(Some(clock.now())).expect("first revision");
    assert!(sample(&binding, clock.now()));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(member.effects.get(), 1);
    update
        .publish(Some(clock.now()))
        .expect("same numerical deadline is a new revision");
    assert!(sample(&binding, clock.now()));
    assert_eq!(member.effects.get(), 2);
    drop(registration);
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None
    );
    let replacement = binding
        .arena()
        .register_deadline_member(pointer(1), &erased)
        .expect("replacement source");
    replacement
        .update_handle()
        .publish(Some(clock.now() + Duration::from_secs(1)))
        .expect("replacement deadline");
    assert_eq!(
        update.publish(Some(clock.now())),
        Err(GestureDeadlineError::Retired)
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        Some(clock.now() + Duration::from_secs(1))
    );
    flui_interaction::__runtime::close_gestures(
        &binding,
        flui_interaction::__runtime::CloseMode::Ordinary,
    );
    assert_eq!(
        replacement.update_handle().refresh(),
        Err(GestureDeadlineError::Closed)
    );
}

fn a_member_wide_poll_cannot_escape_through_another_captured_slot() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(clock.now());
    member.fail_poll.set(true);
    let _one = binding.arena().add(pointer(1), &member);
    let _two = binding.arena().add(pointer(2), &member);
    assert!(sample(&binding, clock.now()));
    binding.arena().sweep(pointer(1));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(
        member.effects.get(),
        1,
        "another captured slot is the same member-wide obligation"
    );
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = binding
        .arena()
        .register_deadline_member(pointer(2), &erased)
        .expect("explicit new source");
    assert_eq!(
        binding
            .arena()
            .register_deadline_member(pointer(1), &erased)
            .err(),
        Some(GestureDeadlineError::AlreadyRegistered)
    );
    registration
        .update_handle()
        .publish(Some(clock.now()))
        .expect("explicit new revision");
    assert!(sample(&binding, clock.now()));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(member.effects.get(), 2);
}

fn reentrant_query_and_poll_publications_survive_the_old_failure() {
    for during_query in [true, false] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let member = DeadlineMember::new(clock.now());
        let erased: Rc<dyn GestureArenaMember> = member.clone();
        let registration = binding
            .arena()
            .register_deadline_member(pointer(1), &erased)
            .expect("source");
        let update = registration.update_handle();
        let replacement_due = clock.now() + Duration::from_secs(2);
        if during_query {
            member.fail_query.set(true);
            *member.query_publication.borrow_mut() = Some((update.clone(), Some(replacement_due)));
            update.refresh().expect("query revision");
        } else {
            member.fail_poll.set(true);
            *member.poll_publication.borrow_mut() = Some((update.clone(), Some(replacement_due)));
            update
                .publish(Some(clock.now()))
                .expect("due poll revision");
        }
        assert!(sample(&binding, clock.now()));
        assert_eq!(
            flui_interaction::__runtime::next_deadline_snapshot(&binding),
            Some(replacement_due),
            "old failure cannot quarantine a reentrant replacement"
        );
        assert!(!sample(&binding, clock.now()));
    }
}

fn a_successful_reprobe_and_explicit_refresh_restore_discovery() {
    for recover_on_reprobe in [true, false] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let due = clock.now() + Duration::from_secs(2);
        let member = DeadlineMember::new(due);
        member.fail_query.set(true);
        let erased: Rc<dyn GestureArenaMember> = member.clone();
        let registration = binding
            .arena()
            .register_deadline_member(pointer(1), &erased)
            .expect("source");
        let update = registration.update_handle();
        assert!(sample(&binding, clock.now()));
        clock.advance(Duration::from_millis(100));
        member.fail_query.set(!recover_on_reprobe);
        assert!(!sample(&binding, clock.now()));
        if !recover_on_reprobe {
            assert_eq!(
                flui_interaction::__runtime::next_deadline_snapshot(&binding),
                None
            );
            member.fail_query.set(false);
            update
                .refresh()
                .expect("explicit source update after parked retry");
            assert!(!sample(&binding, clock.now()));
        }
        assert_eq!(
            flui_interaction::__runtime::next_deadline_snapshot(&binding),
            Some(due)
        );
    }
}

fn a_shared_traversal_clone_does_not_keep_the_member_alive_after_failure() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(clock.now());
    member.fail_poll.set(true);
    let weak = Rc::downgrade(&member);
    let _entry = binding.arena().add(pointer(1), &member);
    assert!(sample(&binding, clock.now()));
    drop(member);
    assert!(
        weak.upgrade().is_none(),
        "the surviving external owner retains sole member custody"
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None
    );
}

fn built_in_deadline_notifications_have_a_committed_delivery_postcondition() {
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::routing::PointerDispatch;
    use flui_interaction::{
        DoubleTapGestureRecognizer, GestureRecognizer as _, LongPressGestureRecognizer,
        MultiTapGestureRecognizer,
    };
    for kind in ["long press", "double tap", "multi tap"] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let notices = Rc::new(Cell::new(0));
        let captured = Rc::clone(&notices);
        let down = make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
            .expect("down fixture");
        let recognizer: Rc<dyn flui_interaction::GestureRecognizer> = match kind {
            "long press" => {
                let recognizer = LongPressGestureRecognizer::builder(binding.arena().clone())
                    .on_long_press(move || {
                        captured.set(captured.get() + 1);
                        panic!("committed long-press notice");
                    })
                    .build();
                recognizer.add_pointer(PointerDispatch::at_root(&down));
                binding.arena().close(pointer(1));
                binding.arena().drain_deferred_resolutions();
                recognizer
            }
            "double tap" => {
                let recognizer = DoubleTapGestureRecognizer::builder(binding.arena().clone())
                    .on_double_tap_cancel(move |_| {
                        captured.set(captured.get() + 1);
                        panic!("committed double-tap cancellation");
                    })
                    .build();
                recognizer.add_pointer(PointerDispatch::at_root(&down));
                let up = make_up_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
                    .expect("up fixture");
                recognizer.handle_event(PointerDispatch::at_root(&up));
                recognizer
            }
            _ => {
                let recognizer = MultiTapGestureRecognizer::builder(binding.arena().clone(), 2)
                    .on_multi_tap_cancel(move |_| {
                        captured.set(captured.get() + 1);
                        panic!("committed multi-tap cancellation");
                    })
                    .build();
                recognizer.add_pointer(PointerDispatch::at_root(&down));
                recognizer
            }
        };
        clock.advance(Duration::from_secs(1));
        let healthy = DeadlineMember::new(clock.now());
        let _entry = binding.arena().add(pointer(2), &healthy);
        let mut recovery = PanicRecovery::new();
        flui_interaction::__runtime::tick_deadlines_with_recovery(
            &binding,
            clock.now(),
            &mut recovery.scope(),
        );
        assert_eq!(
            recovery.first_failure_class(),
            Some(FailureClass::Delivery),
            "{kind} commits before notification"
        );
        assert_eq!(
            recovery.blocking_failure_class(),
            None,
            "{kind} retains valid state"
        );
        assert_eq!(healthy.effects.get(), 1);
        assert_eq!(notices.get(), 1);
        assert!(
            !sample(&binding, clock.now()),
            "{kind} cannot repeat its committed notice"
        );
        assert_eq!(notices.get(), 1);
        drop(recognizer);
    }
}

fn retired_and_held_sources_cannot_resurrect_an_unknown_poll() {
    for explicit in [true, false] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let member = DeadlineMember::new(clock.now());
        member.fail_poll.set(true);
        let entry = binding.arena().add(pointer(1), &member);
        let erased: Rc<dyn GestureArenaMember> = member.clone();
        let registration = explicit.then(|| {
            binding
                .arena()
                .register_deadline_member(pointer(1), &erased)
                .expect("explicit source")
        });
        assert!(sample(&binding, clock.now()));
        if explicit {
            drop(registration);
        } else {
            entry.hold();
            binding.arena().sweep(pointer(1));
        }
        assert!(!sample(&binding, clock.now()));
        assert_eq!(
            member.effects.get(),
            1,
            "retirement or held transfer cannot authorize replay"
        );
        let _fresh = binding.arena().add(pointer(2), &member);
        assert!(
            sample(&binding, clock.now()),
            "a genuine new admission can own new deadline work"
        );
        assert_eq!(member.effects.get(), 2);
    }
}

fn retry_failure_preserves_retirement_even_when_reentry_supersedes_observation() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(clock.now());
    member.fail_query.set(true);
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = binding
        .arena()
        .register_deadline_member(pointer(1), &erased)
        .expect("source");
    drop(erased);
    let update = registration.update_handle();
    assert!(sample(&binding, clock.now()));
    let drops = Rc::clone(&member.drops);
    *member.query_publication.borrow_mut() =
        Some((update.clone(), Some(clock.now() + Duration::from_secs(2))));
    let owner = Rc::new(RefCell::new(Some(Rc::clone(&member))));
    *member.release_owner.borrow_mut() = Some(Rc::clone(&owner));
    drop(member);
    clock.advance(Duration::from_millis(100));
    assert!(
        !sample(&binding, clock.now()),
        "the retry fault is already reported"
    );
    assert!(owner.borrow().is_none());
    assert_eq!(
        drops.get(),
        0,
        "caught retry failure retains final opaque ownership"
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None,
        "retained ownership is not scheduling authority"
    );
    assert_eq!(update.refresh(), Err(GestureDeadlineError::Retired));
}

#[test]
fn owner_deadline_recovery_matrix() {
    for (name, case) in [
        (
            "query_failure_has_one_bounded_probe_and_inert_reads",
            query_failure_has_one_bounded_probe_and_inert_reads as fn(),
        ),
        (
            "unknown_poll_is_not_replayed_and_healthy_tail_progresses",
            unknown_poll_is_not_replayed_and_healthy_tail_progresses as fn(),
        ),
        (
            "a_reused_pointer_admits_a_new_deadline_identity",
            a_reused_pointer_admits_a_new_deadline_identity as fn(),
        ),
        (
            "explicit_same_time_rearm_is_independent_and_stale_authority_refuses",
            explicit_same_time_rearm_is_independent_and_stale_authority_refuses as fn(),
        ),
        (
            "a_member_wide_poll_cannot_escape_through_another_captured_slot",
            a_member_wide_poll_cannot_escape_through_another_captured_slot as fn(),
        ),
        (
            "reentrant_query_and_poll_publications_survive_the_old_failure",
            reentrant_query_and_poll_publications_survive_the_old_failure as fn(),
        ),
        (
            "a_successful_reprobe_and_explicit_refresh_restore_discovery",
            a_successful_reprobe_and_explicit_refresh_restore_discovery as fn(),
        ),
        (
            "a_shared_traversal_clone_does_not_keep_the_member_alive_after_failure",
            a_shared_traversal_clone_does_not_keep_the_member_alive_after_failure as fn(),
        ),
        (
            "built_in_deadline_notifications_have_a_committed_delivery_postcondition",
            built_in_deadline_notifications_have_a_committed_delivery_postcondition as fn(),
        ),
        (
            "retired_and_held_sources_cannot_resurrect_an_unknown_poll",
            retired_and_held_sources_cannot_resurrect_an_unknown_poll as fn(),
        ),
        (
            "retry_failure_preserves_retirement_even_when_reentry_supersedes_observation",
            retry_failure_preserves_retirement_even_when_reentry_supersedes_observation as fn(),
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("deadline recovery case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}
