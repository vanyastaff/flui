//! `GestureObserver` through the public arena and binding: decisions arrive
//! after the operation that made them, in batched passes, with each observer
//! call contained and registrations owned by their handles.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    PointerEvent, PointerKind, make_cancel_event_for_id, make_down_event_for_id,
};
use flui_interaction::gesture::{ContestCancel, GestureArenaEntry};
use flui_interaction::{
    CancelReason, GestureArena, GestureArenaMember, GestureBinding, GestureDisposition,
    GestureObservation, HitTestEntry, HitTestResult, IdExhausted, InteractionLane, ManualClock,
    ObserverHandle, PointerId, RenderId,
};
use flui_platform_api::EventTime;

fn run_rows(rows: &[(&str, fn())]) {
    for &(name, row) in rows {
        if let Err(payload) = std::panic::catch_unwind(row) {
            eprintln!("row `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

type Log = Rc<RefCell<Vec<String>>>;

fn pointer(id: u64) -> PointerId {
    PointerId::try_from(id).expect("nonzero pointer id")
}

/// A member that logs its verdicts, and may own a deadline whose poll claims
/// its contest.
struct Member {
    name: &'static str,
    log: Log,
    deadline: Cell<Option<EventTime>>,
    entry: RefCell<Option<GestureArenaEntry>>,
}

impl Member {
    fn new(name: &'static str, log: &Log) -> Rc<Self> {
        Rc::new(Self {
            name,
            log: Rc::clone(log),
            deadline: Cell::new(None),
            entry: RefCell::new(None),
        })
    }
}

impl GestureArenaMember for Member {
    fn accept_gesture(&self, _: PointerId) {
        self.log
            .borrow_mut()
            .push(format!("{} accepted", self.name));
    }

    fn reject_gesture(&self, _: PointerId) {
        self.log
            .borrow_mut()
            .push(format!("{} rejected", self.name));
    }

    fn deadline(&self) -> Option<EventTime> {
        self.deadline.get()
    }

    fn poll_deadline(&self, _: EventTime) {
        self.deadline.set(None);
        let entry = self.entry.borrow().clone();
        if let Some(entry) = entry {
            entry.resolve(GestureDisposition::Accepted);
        }
    }
}

/// Name an observation, naming members by identity among `members`.
fn describe(observation: &GestureObservation, members: &[&Rc<Member>]) -> String {
    let name = |member: &flui_interaction::gesture::ObservedMember| {
        members
            .iter()
            .find(|candidate| member.is(candidate))
            .map_or("?", |member| member.name)
    };
    match observation {
        GestureObservation::Joined { member, .. } => format!("joined {}", name(member)),
        GestureObservation::Closed { .. } => "closed".to_owned(),
        GestureObservation::Claimed { member, .. } => format!("claimed {}", name(member)),
        GestureObservation::Won { member, .. } => format!("won {}", name(member)),
        GestureObservation::Rejected { member, .. } => format!("rejected {}", name(member)),
        GestureObservation::Cancelled { cause, .. } => format!("cancelled {cause:?}"),
        GestureObservation::DeadlineElapsed { member, .. } => format!("deadline {}", name(member)),
        _ => "other".to_owned(),
    }
}

/// Register an observer that logs every observation it receives.
fn record_into(arena: &GestureArena, log: &Log, members: &[&Rc<Member>]) -> ObserverHandle {
    let log = Rc::clone(log);
    let members: Vec<Rc<Member>> = members.iter().map(|member| Rc::clone(member)).collect();
    arena
        .observe(move |observation: &GestureObservation| {
            let refs: Vec<&Rc<Member>> = members.iter().collect();
            log.borrow_mut()
                .push(format!("observed {}", describe(observation, &refs)));
        })
        .expect("observer registration")
}

fn logged(log: &Log) -> Vec<String> {
    log.borrow().clone()
}

#[test]
fn gesture_observer_delivery_contract() {
    run_rows(&[
        (
            "decisions_are_observed_after_the_operation_that_made_them",
            decisions_are_observed_after_the_operation_that_made_them,
        ),
        (
            "a_cancelled_contact_reports_its_reason_after_dispatch",
            a_cancelled_contact_reports_its_reason_after_dispatch,
        ),
        (
            "a_deadline_resolution_is_observed_before_its_verdicts",
            a_deadline_resolution_is_observed_before_its_verdicts,
        ),
        (
            "an_observer_may_cause_decisions_reentrantly",
            an_observer_may_cause_decisions_reentrantly,
        ),
        (
            "a_panicking_observer_is_contained_and_the_first_failure_kept",
            a_panicking_observer_is_contained_and_the_first_failure_kept,
        ),
        (
            "an_observer_replaced_mid_call_takes_effect_from_the_next_observation",
            an_observer_replaced_mid_call_takes_effect_from_the_next_observation,
        ),
        (
            "an_observer_that_installs_a_replacement_then_panics_retires_contained",
            an_observer_that_installs_a_replacement_then_panics_retires_contained,
        ),
        (
            "dropping_a_handle_removes_only_its_own_registration",
            dropping_a_handle_removes_only_its_own_registration,
        ),
        (
            "observer_registration_refuses_at_generation_exhaustion",
            observer_registration_refuses_at_generation_exhaustion,
        ),
        (
            "releasing_the_observer_during_its_call_finishes_cleanly",
            releasing_the_observer_during_its_call_finishes_cleanly,
        ),
    ]);
}

fn decisions_are_observed_after_the_operation_that_made_them() {
    let log = Log::default();
    let arena = GestureArena::new();
    let (a, b) = (Member::new("a", &log), Member::new("b", &log));
    let _observer = record_into(&arena, &log, &[&a, &b]);
    let p = pointer(1);
    let entry = arena.add(p, &a);
    let _b_entry = arena.add(p, &b);
    arena.close(p);
    entry.resolve(GestureDisposition::Accepted);
    assert_eq!(
        logged(&log),
        [
            "observed joined a",
            "observed joined b",
            "observed closed",
            // Members hear their verdicts inside the operation; the observer
            // hears the decision after it.
            "b rejected",
            "a accepted",
            "observed claimed a",
            "observed rejected b",
            "observed won a",
        ]
    );
}

fn a_cancelled_contact_reports_its_reason_after_dispatch() {
    let log = Log::default();
    let binding = GestureBinding::new();
    let a = Member::new("a", &log);
    let _observer = record_into(binding.arena(), &log, &[&a]);
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let p = pointer(4);
    lane.enter(|| {
        let (route_log, arena, member) = (Rc::clone(&log), binding.arena().clone(), Rc::clone(&a));
        let entries = RefCell::new(Vec::new());
        let target = handle
            .register_pointer(move |dispatch| match dispatch.global {
                PointerEvent::Down(_) => {
                    route_log.borrow_mut().push("route down".to_owned());
                    let entry = arena.add(p, &member);
                    // A claim made inside the dispatch is a whole arena
                    // operation, yet its observation waits for the dispatch.
                    entry.resolve(GestureDisposition::Accepted);
                    entries.borrow_mut().push(entry);
                    route_log
                        .borrow_mut()
                        .push("route down returned".to_owned());
                }
                PointerEvent::Cancel(_) => route_log.borrow_mut().push("route cancel".to_owned()),
                _ => {}
            })
            .expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        binding.handle_pointer_event(
            &make_down_event_for_id(p, Offset::ZERO, PointerKind::Touch).expect("down"),
            |_| path.clone(),
        );
        binding.handle_pointer_event(&make_cancel_event_for_id(p, PointerKind::Touch), |_| {
            panic!("a terminal is not hit-tested")
        });
    });
    assert_eq!(
        logged(&log),
        [
            "route down",
            "route down returned",
            "observed joined a",
            "observed claimed a",
            "observed closed",
            "route cancel",
            "a rejected",
            format!(
                "observed cancelled {:?}",
                ContestCancel::Pointer(CancelReason::Platform)
            )
            .as_str(),
            "observed rejected a",
        ]
    );
}

fn a_deadline_resolution_is_observed_before_its_verdicts() {
    let log = Log::default();
    let clock = Arc::new(ManualClock::new());
    let arena = GestureArena::with_clock(clock.clone());
    let (a, b) = (Member::new("a", &log), Member::new("b", &log));
    let p = pointer(2);
    *a.entry.borrow_mut() = Some(arena.add(p, &a));
    let _b_entry = arena.add(p, &b);
    arena.close(p);
    a.deadline
        .set(Some(arena.now() + Duration::from_millis(500)));
    let _observer = record_into(&arena, &log, &[&a, &b]);
    clock.advance(Duration::from_millis(600));
    arena.poll_deadlines();
    assert_eq!(
        logged(&log),
        [
            "b rejected",
            "a accepted",
            "observed deadline a",
            "observed claimed a",
            "observed rejected b",
            "observed won a",
        ]
    );
}

fn an_observer_may_cause_decisions_reentrantly() {
    let log = Log::default();
    let arena = GestureArena::new();
    let a = Member::new("a", &log);
    let delivered = Rc::new(Cell::new(0_usize));
    let spawned: Rc<RefCell<Vec<Rc<Member>>>> = Rc::default();
    let next_pointer = Rc::new(Cell::new(100_u64));
    let _observer = {
        let (arena_ref, delivered, spawned, log) = (
            arena.clone(),
            Rc::clone(&delivered),
            Rc::clone(&spawned),
            Rc::clone(&log),
        );
        arena
            .observe(move |_: &GestureObservation| {
                delivered.set(delivered.get() + 1);
                // Every observation opens and closes another contest, so an
                // unbatched delivery would never run out of work.
                if delivered.get() < 1_000 {
                    let member = Member::new("spawned", &log);
                    let p = pointer(next_pointer.get());
                    next_pointer.set(next_pointer.get() + 1);
                    let _entry = arena_ref.add(p, &member);
                    arena_ref.close(p);
                    spawned.borrow_mut().push(member);
                }
            })
            .expect("observer registration")
    };
    let p = pointer(1);
    let _entry = arena.add(p, &a);
    arena.close(p);
    assert_eq!(
        delivered.get(),
        2,
        "the pass delivers only what was queued when it started"
    );
    arena.drain_deferred_resolutions();
    assert_eq!(
        delivered.get(),
        // Two observations each opened and closed a contest (four), then the
        // drain awarded all three lone contests.
        2 + 4 + 3,
        "the next pass delivers what the first pass caused, and no more"
    );
}

/// An observer that panics with `message` on every observation it receives.
fn panicking(
    message: &'static str,
    calls: &Rc<Cell<usize>>,
) -> impl Fn(&GestureObservation) + 'static {
    let calls = Rc::clone(calls);
    move |_| {
        calls.set(calls.get() + 1);
        std::panic::panic_any(message);
    }
}

fn a_panicking_observer_is_contained_and_the_first_failure_kept() {
    let log = Log::default();
    let arena = GestureArena::new();
    let a = Member::new("a", &log);
    let (first_calls, second_calls) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let _first = arena
        .observe(panicking("first observer failure", &first_calls))
        .expect("first");
    let _second = arena
        .observe(panicking("second observer failure", &second_calls))
        .expect("second");
    let _recorder = record_into(&arena, &log, &[&a]);
    let p = pointer(1);
    let _entry = arena.add(p, &a);
    let ((), captured) = flui_testing::log_capture::capture(|| arena.close(p));
    assert_eq!(logged(&log), ["observed joined a", "observed closed"]);
    assert_eq!((first_calls.get(), second_calls.get()), (2, 2));
    let reports: Vec<_> = captured
        .records()
        .iter()
        .filter(|record| record.target == "flui_interaction::gesture::observer")
        .collect();
    assert_eq!(reports.len(), 1, "one report per pass:\n{captured}");
    assert_eq!(reports[0].field("panic"), Some("first observer failure"));
    assert_eq!(reports[0].field("later_failures"), Some("3"));
    assert!(!captured.contains("second observer failure"));

    // Both stay registered: the next decision reaches them again.
    arena.drain_deferred_resolutions();
    assert_eq!((first_calls.get(), second_calls.get()), (3, 3));
    assert_eq!(
        logged(&log)[2..],
        ["a accepted", "observed won a"].map(String::from)
    );
}

fn an_observer_replaced_mid_call_takes_effect_from_the_next_observation() {
    let log = Log::default();
    let arena = GestureArena::new();
    let a = Member::new("a", &log);
    let own: Rc<RefCell<Option<ObserverHandle>>> = Rc::default();
    let other: Rc<RefCell<Option<ObserverHandle>>> = Rc::default();
    let observer = {
        let (own, other, log) = (Rc::clone(&own), Rc::clone(&other), Rc::clone(&log));
        move |observation: &GestureObservation| {
            log.borrow_mut()
                .push(format!("outgoing {}", describe(observation, &[])));
            let replacement_log = Rc::clone(&log);
            if let Some(handle) = own.borrow().as_ref() {
                handle.replace(move |observation: &GestureObservation| {
                    replacement_log
                        .borrow_mut()
                        .push(format!("incoming {}", describe(observation, &[])));
                });
            }
            // Removing another observer mid-call also waits for the next
            // observation.
            drop(other.borrow_mut().take());
        }
    };
    *own.borrow_mut() = Some(arena.observe(observer).expect("own"));
    let removed_log = Rc::clone(&log);
    *other.borrow_mut() = Some(
        arena
            .observe(move |observation: &GestureObservation| {
                removed_log
                    .borrow_mut()
                    .push(format!("removed {}", describe(observation, &[])));
            })
            .expect("other"),
    );
    let p = pointer(1);
    let _entry = arena.add(p, &a);
    arena.close(p);
    assert_eq!(
        logged(&log),
        ["outgoing joined ?", "removed joined ?", "incoming closed"]
    );
}

/// Panics while it is destroyed.
struct PanicsOnDrop(Rc<Cell<bool>>);

impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        self.0.set(true);
        std::panic::panic_any("outgoing observer destructor failure");
    }
}

fn an_observer_that_installs_a_replacement_then_panics_retires_contained() {
    let log = Log::default();
    let arena = GestureArena::new();
    let a = Member::new("a", &log);
    let own: Rc<RefCell<Option<ObserverHandle>>> = Rc::default();
    let destroyed = Rc::new(Cell::new(false));
    let observer = {
        let (own, log) = (Rc::clone(&own), Rc::clone(&log));
        let captures = PanicsOnDrop(Rc::clone(&destroyed));
        move |_: &GestureObservation| {
            let _captured = &captures;
            let replacement_log = Rc::clone(&log);
            own.borrow().as_ref().expect("own handle").replace(
                move |observation: &GestureObservation| {
                    replacement_log
                        .borrow_mut()
                        .push(format!("incoming {}", describe(observation, &[])));
                },
            );
            std::panic::panic_any("installing observer failure");
        }
    };
    *own.borrow_mut() = Some(arena.observe(observer).expect("own"));
    let p = pointer(1);
    let _entry = arena.add(p, &a);
    let ((), captured) = flui_testing::log_capture::capture(|| arena.close(p));
    assert!(destroyed.get(), "the outgoing observer was destroyed");
    let report = captured
        .records()
        .iter()
        .find(|record| record.target == "flui_interaction::gesture::observer")
        .expect("the failure is reported");
    assert_eq!(report.field("panic"), Some("installing observer failure"));
    assert_eq!(report.field("failed_retirements"), Some("1"));
    assert_eq!(logged(&log), ["incoming closed"]);
    drop(own.borrow_mut().take());
}

fn dropping_a_handle_removes_only_its_own_registration() {
    let log = Log::default();
    let arena = GestureArena::new();
    let a = Member::new("a", &log);
    let first = record_into(&arena, &log, &[&a]);
    let kept_log = Rc::clone(&log);
    let _kept = arena
        .observe(move |observation: &GestureObservation| {
            kept_log
                .borrow_mut()
                .push(format!("kept {}", describe(observation, &[])));
        })
        .expect("kept");
    drop(first);
    let p = pointer(1);
    let _entry = arena.add(p, &a);
    arena.close(p);
    assert_eq!(logged(&log), ["kept joined ?", "kept closed"]);
}

fn observer_registration_refuses_at_generation_exhaustion() {
    let arena = GestureArena::new().with_observer_ids_after(u64::MAX - 1);
    let last = arena
        .observe(|_: &GestureObservation| {})
        .expect("the last identity is issued");
    assert!(matches!(
        arena.observe(|_: &GestureObservation| {}),
        Err(IdExhausted)
    ));
    drop(last);
    assert!(
        matches!(arena.observe(|_: &GestureObservation| {}), Err(IdExhausted)),
        "a released identity is never reissued"
    );
}

fn releasing_the_observer_during_its_call_finishes_cleanly() {
    let log = Log::default();
    let arena = GestureArena::new();
    let a = Member::new("a", &log);
    let own: Rc<RefCell<Option<ObserverHandle>>> = Rc::default();
    let owner: Rc<RefCell<Option<GestureArena>>> = Rc::new(RefCell::new(Some(arena.clone())));
    let calls = Rc::new(Cell::new(0));
    let observer = {
        let (own, owner, calls) = (Rc::clone(&own), Rc::clone(&owner), Rc::clone(&calls));
        move |_: &GestureObservation| {
            calls.set(calls.get() + 1);
            drop(owner.borrow_mut().take());
            drop(own.borrow_mut().take());
        }
    };
    *own.borrow_mut() = Some(arena.observe(observer).expect("own"));
    let p = pointer(1);
    let entry = arena.add(p, &a);
    arena.close(p);
    entry.resolve(GestureDisposition::Accepted);
    assert_eq!(calls.get(), 1, "the removed observer receives nothing more");
    assert!(own.borrow().is_none());
    assert_eq!(logged(&log), ["a accepted"]);
}
