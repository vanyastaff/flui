//! Public recognizer cancellation, ownership, and containment contracts.

use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::{MonotonicClock, panic::payload_text};
use flui_interaction::{GestureArena, GestureArenaMember, ManualClock, PointerId};
use web_time::Instant;

struct Member {
    accepted: Cell<u32>,
    rejected: Cell<u32>,
    retired: Rc<Cell<u32>>,
    deadline: Cell<Option<Instant>>,
    queries: Cell<u32>,
    polls: Cell<u32>,
    query_failure: Cell<Option<&'static str>>,
}

impl Member {
    #[expect(
        clippy::arc_with_non_send_sync,
        reason = "the public arena member contract currently requires Arc"
    )]
    fn new(deadline: Option<Instant>) -> Arc<Self> {
        Arc::new(Self {
            accepted: Cell::new(0),
            rejected: Cell::new(0),
            retired: Rc::new(Cell::new(0)),
            deadline: Cell::new(deadline),
            queries: Cell::new(0),
            polls: Cell::new(0),
            query_failure: Cell::new(None),
        })
    }

    fn query_deadline(&self) -> Option<Instant> {
        self.queries.set(self.queries.get() + 1);
        if let Some(failure) = self.query_failure.get() {
            panic!("{failure}");
        }
        self.deadline.get()
    }
}

impl flui_interaction::sealed::arena_member::Sealed for Member {}

impl GestureArenaMember for Member {
    fn accept_gesture(&self, _: PointerId) {
        self.accepted.set(self.accepted.get() + 1);
    }

    fn reject_gesture(&self, _: PointerId) {
        self.rejected.set(self.rejected.get() + 1);
    }

    fn has_pending_deadline(&self) -> bool {
        self.query_deadline().is_some()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.query_deadline()
    }

    fn poll_deadline(&self) {
        self.polls.set(self.polls.get() + 1);
        self.deadline.set(None);
    }
}

impl Drop for Member {
    fn drop(&mut self) {
        self.retired.set(self.retired.get() + 1);
    }
}

fn dropped_member_without_cancel_leaves_the_arena() {
    let arena = GestureArena::new();
    let pointer = PointerId::PRIMARY;
    let departed = Member::new(None);
    let retired = Rc::clone(&departed.retired);
    let rival = Member::new(None);
    let _departed_entry = arena.add(pointer, departed.clone());
    let _rival_entry = arena.add(pointer, rival.clone());
    arena.close(pointer);
    drop(departed);

    arena.drain_deferred_resolutions();
    assert_eq!(rival.accepted.get(), 1, "the remaining live member wins");
    assert_eq!(rival.rejected.get(), 0);
    assert_eq!(
        retired.get(),
        1,
        "arena ownership cannot keep a member alive"
    );
    assert!(arena.is_empty());

    let next = Member::new(None);
    arena.add(pointer, next.clone());
    arena.close(pointer);
    arena.drain_deferred_resolutions();
    assert_eq!(
        next.accepted.get(),
        1,
        "the reused pointer starts a new contest"
    );
}

fn custom_member_owns_a_deadline() {
    let clock = ManualClock::new();
    let arena = GestureArena::with_clock(Arc::new(clock.clone()));
    let due = clock.now() + Duration::from_millis(20);
    let member = Member::new(Some(due));
    arena.add(PointerId::PRIMARY, member.clone());

    assert!(arena.has_pending_deadlines());
    assert_eq!(arena.next_deadline(), Some(due));
    arena.poll_deadlines();
    assert_eq!(member.polls.get(), 0, "only due deadlines may be polled");
    clock.advance(Duration::from_millis(20));
    arena.poll_deadlines();
    assert_eq!(member.polls.get(), 1);
    assert!(!arena.has_pending_deadlines());
    assert_eq!(arena.next_deadline(), None);
    arena.poll_deadlines();
    assert_eq!(
        member.polls.get(),
        1,
        "disarmed deadline is not polled again"
    );
}

fn panicking_deadline_query_does_not_hide_other_deadlines() {
    for competing in [false, true] {
        for pending_query in [false, true] {
            let clock = ManualClock::new();
            let arena = GestureArena::with_clock(Arc::new(clock.clone()));
            let due = clock.now() + Duration::from_millis(20);
            let first = Member::new(Some(due));
            let second = Member::new(Some(due));
            let healthy = Member::new(Some(due));
            first.query_failure.set(Some("first deadline query failed"));
            if competing {
                second
                    .query_failure
                    .set(Some("second deadline query failed"));
            }
            for member in [&first, &second, &healthy] {
                arena.add(PointerId::PRIMARY, member.clone());
            }

            let failure = catch_unwind(AssertUnwindSafe(|| {
                if pending_query {
                    let _ = arena.has_pending_deadlines();
                } else {
                    let _ = arena.next_deadline();
                }
            }))
            .expect_err("the first query failure resumes after all members are queried");
            assert_eq!(
                payload_text(failure.as_ref()),
                Some("first deadline query failed")
            );
            assert_eq!(second.queries.get(), 1, "later failing members are queried");
            assert_eq!(healthy.queries.get(), 1, "healthy deadline is not hidden");

            first.query_failure.set(None);
            second.query_failure.set(None);
            assert_eq!(arena.next_deadline(), Some(due));
            clock.advance(Duration::from_millis(20));
            arena.poll_deadlines();
            assert_eq!(
                healthy.polls.get(),
                1,
                "polling progresses after containment"
            );
            assert!(!arena.has_pending_deadlines());
        }
    }
}

#[test]
fn arena_member_lifecycle_contract() {
    let cases: &[(&str, fn())] = &[
        (
            "dropped_member_without_cancel_leaves_the_arena",
            dropped_member_without_cancel_leaves_the_arena,
        ),
        (
            "custom_member_owns_a_deadline",
            custom_member_owns_a_deadline,
        ),
        (
            "panicking_deadline_query_does_not_hide_other_deadlines",
            panicking_deadline_query_does_not_hide_other_deadlines,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(case) {
            failures.push(format!(
                "{name}: {}",
                payload_text(payload.as_ref()).unwrap_or("non-string failure")
            ));
        }
    }
    assert!(failures.is_empty(), "failed rows:\n{}", failures.join("\n"));
}
