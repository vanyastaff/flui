//! Owner deadline recovery uses the same binding and open members as consumers.

use std::{cell::Cell, panic::AssertUnwindSafe, rc::Rc, sync::Arc, time::Duration};

use flui_foundation::{MonotonicClock as _, panic::PanicRecovery};
use flui_interaction::{GestureBinding, ManualClock, PointerId, arena::GestureArenaMember};
use web_time::Instant;

struct DeadlineMember {
    due: Cell<Option<Instant>>,
    fail_query: Cell<bool>,
    fail_poll: Cell<bool>,
    queries: Cell<usize>,
    effects: Cell<usize>,
}

impl DeadlineMember {
    fn new(due: Instant) -> Rc<Self> {
        Rc::new(Self {
            due: Cell::new(Some(due)),
            fail_query: Cell::new(false),
            fail_poll: Cell::new(false),
            queries: Cell::new(0),
            effects: Cell::new(0),
        })
    }
}

impl GestureArenaMember for DeadlineMember {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {}
    fn deadline(&self) -> Option<Instant> {
        self.queries.set(self.queries.get() + 1);
        assert!(!self.fail_query.get(), "deadline query failure");
        self.due.get()
    }
    fn poll_deadline(&self, _: Instant) {
        self.effects.set(self.effects.get() + 1);
        assert!(!self.fail_poll.get(), "deadline poll failure after an effect");
        self.due.set(None);
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
    assert!(sample(&binding, clock.now()));
    assert_eq!(member.queries.get(), 1);
    let deadline = std::panic::catch_unwind(AssertUnwindSafe(|| {
        flui_interaction::__runtime::next_deadline_snapshot(&binding)
    }));
    assert!(deadline.is_ok(), "a host snapshot read must not invoke a failed query");
    assert_eq!(deadline.expect("inert read"), clock.now().checked_add(Duration::from_millis(100)));
    clock.advance(Duration::from_millis(99));
    assert!(!sample(&binding, clock.now()), "the same obligation is reported once");
    assert_eq!(member.queries.get(), 1, "an earlier healthy turn cannot probe early");
    clock.advance(Duration::from_millis(1));
    assert!(!sample(&binding, clock.now()), "the re-probe cannot repeat the report");
    assert_eq!(member.queries.get(), 2, "one automatic re-probe is due");
    assert_eq!(flui_interaction::__runtime::next_deadline_snapshot(&binding), None);
    clock.advance(Duration::from_secs(5));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(member.queries.get(), 2, "repeated query failure parks discovery debt");
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
    assert_eq!(healthy.effects.get(), 1, "a failed open poll cannot erase the healthy tail");
    assert_eq!(failed.effects.get(), 1);
    clock.advance(Duration::from_secs(1));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(failed.effects.get(), 1, "an unknown side effect cannot replay for the same revision");
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
    assert!(sample(&binding, clock.now()), "new contact identity can attempt its new obligation");
    assert_eq!(member.effects.get(), 2);
}

#[test]
fn owner_deadline_recovery_matrix() {
    for (name, case) in [
        ("query_failure_has_one_bounded_probe_and_inert_reads", query_failure_has_one_bounded_probe_and_inert_reads as fn()),
        ("unknown_poll_is_not_replayed_and_healthy_tail_progresses", unknown_poll_is_not_replayed_and_healthy_tail_progresses as fn()),
        ("a_reused_pointer_admits_a_new_deadline_identity", a_reused_pointer_admits_a_new_deadline_identity as fn()),
    ] {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("deadline recovery case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}
