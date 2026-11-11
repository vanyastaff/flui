//! Commit-then-call: an owner's state is changed first and the user code
//! the change owes runs after the borrow is released (ADR-0287).

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::panic::{Obligation, PanicRecovery, commit, commit_into, payload_text};
use flui_foundation::testing::CountingWaker;
use flui_foundation::{IdExhausted, TicketSource};

/// Counts its drops; panics in its destructor when `fails`.
struct Destroyed {
    drops: Rc<Cell<usize>>,
    fails: Option<&'static str>,
}

impl Drop for Destroyed {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if let Some(message) = self.fails {
            panic!("{message}");
        }
    }
}

fn caught_text(payload: &(dyn std::any::Any + Send)) -> Option<String> {
    payload_text(payload).map(str::to_owned)
}

/// A queued call runs after the borrow ends and sees the committed state.
fn a_queued_call_sees_the_committed_state_unborrowed() {
    let cell = Rc::new(RefCell::new(0_u32));
    let seen = Rc::new(Cell::new(None));
    let (reader, observed) = (Rc::clone(&cell), Rc::clone(&seen));
    let answer = commit(&cell, move |state, out| {
        *state = 7;
        out.call(move || observed.set(Some(*reader.borrow())));
        "committed"
    });
    assert_eq!(answer, "committed");
    assert_eq!(seen.get(), Some(7), "the call read the committed state");
}

/// A queued call may commit on the same cell; its own queue runs before the
/// rest of the outer one.
fn a_queued_call_commits_again_depth_first() {
    let cell = Rc::new(RefCell::new(Vec::<&'static str>::new()));
    let inner = Rc::clone(&cell);
    let (first, last) = (Rc::clone(&cell), Rc::clone(&cell));
    commit(&cell, move |log, out| {
        log.push("mutate");
        out.call(move || {
            let nested = Rc::clone(&inner);
            commit(&inner, move |log, out| {
                log.push("nested mutate");
                out.call(move || nested.borrow_mut().push("nested call"));
            });
            first.borrow_mut().push("first call");
        });
        out.call(move || last.borrow_mut().push("second call"));
    });
    assert_eq!(
        *cell.borrow(),
        [
            "mutate",
            "nested mutate",
            "nested call",
            "first call",
            "second call"
        ]
    );
}

/// Every queued item runs; the first panic is the one resumed, and a
/// released value's destructor is contained like a call.
fn the_first_failure_propagates_after_every_item_ran() {
    let cell = RefCell::new(());
    let drops = Rc::new(Cell::new(0));
    let ran = Rc::new(Cell::new(0));
    let (counted, released) = (Rc::clone(&ran), Rc::clone(&drops));
    let failure = catch_unwind(AssertUnwindSafe(|| {
        commit(&cell, move |(), out| {
            out.call(|| panic!("first failure"));
            out.release(Destroyed {
                drops: released,
                fails: Some("released value failed"),
            });
            out.call(move || counted.set(counted.get() + 1));
        });
    }))
    .expect_err("the first failure propagates");
    assert_eq!(caught_text(&*failure).as_deref(), Some("first failure"));
    assert_eq!(drops.get(), 1, "the released value was destroyed");
    assert_eq!(ran.get(), 1, "the call after both failures ran");
    assert!(cell.try_borrow_mut().is_ok(), "the cell is free again");
}

/// A call that panics keeps its captures until its panic is caught; they are
/// destroyed after it, contained, so a capture whose destructor panics too
/// does not abort, and the call's panic is the one resumed.
fn a_failing_calls_captures_are_destroyed_after_it() {
    let cell = RefCell::new(());
    let drops = Rc::new(Cell::new(0));
    let capture = Destroyed {
        drops: Rc::clone(&drops),
        fails: Some("capture failed"),
    };
    let failure = catch_unwind(AssertUnwindSafe(|| {
        commit(&cell, move |(), out| {
            out.call(move || {
                let _ = &capture;
                panic!("call failed");
            });
        });
    }))
    .expect_err("the call's panic propagates");
    assert_eq!(caught_text(&*failure).as_deref(), Some("call failed"));
    assert_eq!(drops.get(), 1, "the capture was destroyed");
}

/// Commits from `Drop` while the thread unwinds; its queued call panics.
struct CommitsWhileUnwinding {
    ran: Rc<Cell<usize>>,
}

impl Drop for CommitsWhileUnwinding {
    fn drop(&mut self) {
        let cell = RefCell::new(0_u32);
        let counted = Rc::clone(&self.ran);
        let answer = commit(&cell, move |state, out| {
            *state = 1;
            out.call(|| panic!("queued call failed during the unwind"));
            out.call(move || counted.set(counted.get() + 1));
            *state
        });
        assert_eq!(answer, 1, "the commit returns behind the unwind");
    }
}

/// While the thread already unwinds, a queued call's panic is discarded
/// behind that unwind: the rest of the queue runs, the commit returns, and
/// the unwinding failure is the one that arrives.
fn a_queued_failure_during_an_unwind_is_discarded() {
    let ran = Rc::new(Cell::new(0));
    let probe = CommitsWhileUnwinding {
        ran: Rc::clone(&ran),
    };
    let failure = catch_unwind(AssertUnwindSafe(move || {
        let _probe = probe;
        panic!("outer failure");
    }))
    .expect_err("the outer failure propagates");
    assert_eq!(caught_text(&*failure).as_deref(), Some("outer failure"));
    assert_eq!(ran.get(), 1, "the call after the failed one ran");
}

/// A mutation that panics runs none of its calls, destroys them and its
/// released values, still wakes its wakers, and its panic is the one
/// resumed.
fn a_failed_mutation_wakes_and_runs_no_call() {
    let cell = RefCell::new(0_u32);
    let wakes = CountingWaker::new().shared();
    let waker = wakes.waker();
    let ran = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let (counted, captured, released) = (Rc::clone(&ran), Rc::clone(&drops), Rc::clone(&drops));
    let failure = catch_unwind(AssertUnwindSafe(|| {
        commit(&cell, move |state, out| {
            *state = 1;
            let capture = Destroyed {
                drops: captured,
                fails: None,
            };
            out.call(move || {
                let _ = &capture;
                counted.set(counted.get() + 1);
            });
            out.wake(waker);
            out.release(Destroyed {
                drops: released,
                fails: None,
            });
            panic!("mutation failed");
        });
    }))
    .expect_err("the mutation's panic propagates");
    assert_eq!(caught_text(&*failure).as_deref(), Some("mutation failed"));
    assert_eq!(
        ran.get(),
        0,
        "no call announces a change that did not complete"
    );
    assert_eq!(
        drops.get(),
        2,
        "the call's capture and the released value were destroyed"
    );
    assert_eq!(wakes.count(), 1, "the waker was still woken");
    assert!(cell.try_borrow_mut().is_ok(), "the cell is free again");
}

/// A commit inside a mutation of the same cell is a framework defect: it
/// panics with `BUG:`, and the cell is usable afterwards.
fn a_reentrant_commit_panics_with_bug() {
    let cell = RefCell::new(0_u32);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        commit(&cell, |_, _| commit(&cell, |state, _| *state = 1));
    }))
    .expect_err("the nested commit is refused");
    assert!(
        caught_text(&*failure).is_some_and(|text| text.starts_with("BUG: ")),
        "{:?}",
        caught_text(&*failure)
    );
    assert_eq!(commit(&cell, |state, _| *state), 0, "the cell is unchanged");
}

/// Inside an operation's recovery, an earlier failure stays first; a failed
/// mutation answers `None`.
fn commit_into_keeps_an_earlier_failure_first() {
    let cell = RefCell::new(0_u32);
    let ran = Rc::new(Cell::new(0));
    let mut recovery = PanicRecovery::new();
    recovery.run(|| panic!("earlier failure"));
    let counted = Rc::clone(&ran);
    let answer = commit_into(&cell, &mut recovery, move |state, out| {
        *state = 3;
        out.call(|| panic!("later failure"));
        out.call(move || counted.set(counted.get() + 1));
        *state
    });
    assert_eq!(answer, Some(3));
    assert_eq!(ran.get(), 1, "delivery continued after the later failure");
    let refused = commit_into(&cell, &mut recovery, |_, _| -> u32 {
        panic!("failed mutation")
    });
    assert_eq!(refused, None);
    let resumed = catch_unwind(AssertUnwindSafe(|| recovery.finish()))
        .expect_err("the earlier failure resumes");
    assert_eq!(caught_text(&*resumed).as_deref(), Some("earlier failure"));
}

/// An obligation dropped without `discharge` is owed again; one raised
/// during its delivery survives `discharge`.
fn an_obligation_is_owed_until_discharged() {
    let owed = Cell::new(false);
    assert!(Obligation::take(&owed).is_none(), "nothing is owed");

    owed.set(true);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        let _delivery = Obligation::take(&owed).expect("owed");
        assert!(!owed.get(), "the flag is cleared while the delivery runs");
        panic!("delivery failed");
    }))
    .expect_err("the delivery fails");
    assert_eq!(caught_text(&*failure).as_deref(), Some("delivery failed"));
    assert!(owed.get(), "a failed delivery is owed again");

    Obligation::take(&owed).expect("owed").discharge();
    assert!(!owed.get(), "a completed delivery is no longer owed");

    owed.set(true);
    let delivery = Obligation::take(&owed).expect("owed");
    owed.set(true);
    delivery.discharge();
    assert!(
        owed.get(),
        "an obligation raised during the delivery stays owed"
    );
}

/// Which request an answer belongs to.
struct Read;

/// An answer is accepted only for the current ticket, once.
fn a_ticket_matches_one_answer_to_its_request() {
    let source = TicketSource::<Read>::new();
    let older = source.issue().expect("a fresh source issues");
    let newer = source.issue().expect("a fresh source issues");
    assert!(newer > older, "a later ticket orders after an earlier one");
    assert!(
        !source.is_current(older),
        "an older request's answer is stale"
    );
    assert!(!source.redeem(older));
    assert!(source.is_current(newer));
    assert!(source.redeem(newer), "the current answer is accepted");
    assert!(
        !source.redeem(newer),
        "the same answer is not accepted twice"
    );

    let pending = source.issue().expect("a fresh source issues");
    source.invalidate();
    assert!(!source.redeem(pending), "an answer after a write is stale");
}

/// An exhausted source refuses permanently and keeps the current ticket.
fn an_exhausted_ticket_source_refuses() {
    let source = TicketSource::<Read>::starting_after(u64::MAX - 1);
    let last = source.issue().expect("the last ticket is issued");
    assert_eq!(source.issue(), Err(IdExhausted));
    assert_eq!(source.issue(), Err(IdExhausted), "the refusal is permanent");
    assert!(
        source.is_current(last),
        "a refusal keeps the current ticket"
    );
}

#[test]
fn commit_then_call_contract() {
    crate::run_table(&[
        (
            "a queued call sees the committed state",
            a_queued_call_sees_the_committed_state_unborrowed,
        ),
        (
            "a queued call commits again",
            a_queued_call_commits_again_depth_first,
        ),
        (
            "first failure after every item",
            the_first_failure_propagates_after_every_item_ran,
        ),
        (
            "a failing call's captures",
            a_failing_calls_captures_are_destroyed_after_it,
        ),
        (
            "queued failure during an unwind",
            a_queued_failure_during_an_unwind_is_discarded,
        ),
        ("failed mutation", a_failed_mutation_wakes_and_runs_no_call),
        ("re-entrant commit", a_reentrant_commit_panics_with_bug),
        (
            "commit into a recovery",
            commit_into_keeps_an_earlier_failure_first,
        ),
        ("obligation", an_obligation_is_owed_until_discharged),
        ("ticket", a_ticket_matches_one_answer_to_its_request),
        (
            "exhausted ticket source",
            an_exhausted_ticket_source_refuses,
        ),
    ]);
}
