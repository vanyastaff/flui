//! `GestureObserver` through the public arena and binding: decisions arrive
//! after the operation that made them, in batched passes, with each observer
//! call contained and registrations owned by their handles.

use flui_interaction::gesture::GestureContest;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    PointerEvent, PointerKind, make_cancel_event_for_id, make_down_event_for_id,
    make_up_event_for_id,
};
use flui_interaction::gesture::{
    CompositionBranch, CompositionKind, ContestCancel, GestureArenaEntry, GestureComposition,
    ObservedComposition, RejectionReason,
};
use flui_interaction::{
    CancelReason, GestureArena, GestureArenaMember, GestureBinding, GestureDisposition,
    GestureObservation, HitTestEntry, HitTestResult, IdExhausted, InputSurface, InteractionLane,
    ManualClock, ObserverHandle, PointerId, RenderId,
};
use flui_platform::EventTime;

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
    fn accept_gesture(&self, _: GestureContest) {
        self.log
            .borrow_mut()
            .push(format!("{} accepted", self.name));
    }

    fn reject_gesture(&self, _: GestureContest) {
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
        (
            "a_reused_pointer_contests_a_new_sequence",
            a_reused_pointer_contests_a_new_sequence,
        ),
        (
            "an_elapsed_deadline_names_its_contest_and_sequence",
            an_elapsed_deadline_names_its_contest_and_sequence,
        ),
        ("each_rejection_names_why", each_rejection_names_why),
        (
            "a_join_reports_its_whole_composition_path",
            a_join_reports_its_whole_composition_path,
        ),
        (
            "a_failed_operation_still_reaches_observers",
            a_failed_operation_still_reaches_observers,
        ),
        (
            "a_failing_clock_loses_no_committed_verdict",
            a_failing_clock_loses_no_committed_verdict,
        ),
        (
            "a_clock_failure_before_a_member_failure_is_the_one_raised",
            a_clock_failure_before_a_member_failure_is_the_one_raised,
        ),
    ]);
}

/// Every rejection says why: the member withdrew, another won, the contest
/// was cancelled, or the hold a successor depended on failed.
fn each_rejection_names_why() {
    let log = Log::default();
    let arena = GestureArena::new();
    let reasons = Rc::new(RefCell::new(Vec::new()));
    let (a, b, c, d) = (
        Member::new("a", &log),
        Member::new("b", &log),
        Member::new("c", &log),
        Member::new("d", &log),
    );
    let names = [Rc::clone(&a), Rc::clone(&b), Rc::clone(&c), Rc::clone(&d)];
    let sink = Rc::clone(&reasons);
    let _observer = arena
        .observe(move |observation: &GestureObservation| {
            if let GestureObservation::Rejected { member, reason, .. } = observation {
                let name = names
                    .iter()
                    .find(|candidate| member.is(candidate))
                    .map_or("?", |member| member.name);
                sink.borrow_mut().push((name, *reason));
            }
        })
        .expect("observer registration");

    let decided = pointer(1);
    let a_entry = arena.add(decided, &a);
    let b_entry = arena.add(decided, &b);
    let _c_entry = arena.add(decided, &c);
    arena.close(decided);
    a_entry.resolve(GestureDisposition::Rejected);
    b_entry.resolve(GestureDisposition::Accepted);

    let abandoned = pointer(2);
    let _d_entry = arena.add(abandoned, &d);
    arena.abandon(abandoned);

    let branch = arena.sequenced().expect("sequenced composition");
    let (hold, successor, rival) = (
        Member::new("hold", &log),
        Member::new("successor", &log),
        Member::new("rival", &log),
    );
    let held = pointer(3);
    let hold_entry = branch.hold().add(held, &hold);
    let _successor_entry = branch.successor().add(held, &successor);
    let _rival_entry = arena.add(held, &rival);
    arena.close(held);
    let before = reasons.borrow().len();
    hold_entry.resolve(GestureDisposition::Rejected);
    let orphaned = reasons.borrow()[before..]
        .iter()
        .map(|(_, reason)| *reason)
        .collect::<Vec<_>>();

    assert_eq!(
        reasons.borrow()[..before],
        [
            ("a", RejectionReason::Withdrew),
            ("c", RejectionReason::Defeated),
            ("d", RejectionReason::Cancelled),
        ]
    );
    assert_eq!(
        orphaned,
        [RejectionReason::Withdrew, RejectionReason::HoldFailed],
        "the hold withdrew and its successor failed with it"
    );
}

/// A join names every composition its member sits under, innermost first,
/// with the branch it took and a node identity two members can share.
fn a_join_reports_its_whole_composition_path() {
    let arena = GestureArena::new();
    let (preferred, fallback) = arena
        .compose(GestureComposition::RequireFailure)
        .expect("root composition")
        .into_branches();
    let (nested, sibling) = fallback
        .compose(GestureComposition::Exclusive)
        .expect("a branch composes again")
        .into_branches();
    let joined = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&joined);
    let _observer = arena
        .observe(move |observation: &GestureObservation| {
            if let GestureObservation::Joined { composition, .. } = observation {
                sink.borrow_mut().push(composition.clone());
            }
        })
        .expect("observer registration");
    let log = Log::default();
    let p = pointer(1);
    let (first, inner, other, plain) = (
        Member::new("preferred", &log),
        Member::new("nested", &log),
        Member::new("sibling", &log),
        Member::new("plain", &log),
    );
    let _entries = [
        preferred.add(p, &first),
        nested.add(p, &inner),
        sibling.add(p, &other),
        arena.add(p, &plain),
    ];
    arena.close(p);
    let joined = joined.borrow();
    let shape = |path: &[ObservedComposition]| {
        path.iter()
            .map(|step| (step.kind(), step.branch()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shape(&joined[0]),
        [(CompositionKind::RequireFailure, CompositionBranch::First)]
    );
    assert_eq!(
        shape(&joined[1]),
        [
            (CompositionKind::Exclusive, CompositionBranch::First),
            (CompositionKind::RequireFailure, CompositionBranch::Second),
        ]
    );
    assert_eq!(
        shape(&joined[2]),
        [
            (CompositionKind::Exclusive, CompositionBranch::Second),
            (CompositionKind::RequireFailure, CompositionBranch::Second),
        ]
    );
    assert!(
        joined[3].is_empty(),
        "an uncomposed member joins under none"
    );
    assert!(
        joined[1][0].is_same_node(&joined[2][0]),
        "nested and sibling share their node"
    );
    assert!(
        joined[1][1].is_same_node(&joined[0][0]),
        "and sit under the preferred's root"
    );
    assert!(!joined[1][0].is_same_node(&joined[1][1]));
}

/// A deadline the arena polls is reported for the contest its member joined,
/// with the sequence the binding started at that contact's Down.
fn an_elapsed_deadline_names_its_contest_and_sequence() {
    let binding = GestureBinding::new();
    let elapsed = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&elapsed);
    let _observer = binding
        .arena()
        .observe(move |observation: &GestureObservation| {
            if let GestureObservation::DeadlineElapsed { contest, .. } = observation {
                sink.borrow_mut().push(*contest);
            }
        })
        .expect("observer registration");
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let log = Log::default();
    let member = Member::new("a", &log);
    let p = pointer(5);
    let mut dispatched = None;
    lane.enter(|| {
        let (arena, member) = (binding.arena().clone(), Rc::clone(&member));
        let target = handle
            .register_pointer(move |dispatch| {
                if let PointerEvent::Down(_) = dispatch.global {
                    *member.entry.borrow_mut() = Some(arena.add(p, &member));
                    member.deadline.set(Some(EventTime::from_nanos(0)));
                }
            })
            .expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        let down = make_down_event_for_id(p, Offset::ZERO, PointerKind::Touch).expect("down");
        dispatched = binding
            .dispatch_pointer_on(InputSurface::Window, &down, |_| path.clone())
            .sequence();
    });
    binding.arena().poll_deadlines();
    let elapsed = elapsed.borrow().clone();
    assert_eq!(elapsed.len(), 1, "{elapsed:?}");
    let contest = elapsed[0].expect("the member joined a contest");
    assert_eq!(contest.pointer(), p);
    assert!(dispatched.is_some());
    assert_eq!(contest.sequence(), dispatched);
}

/// Two contacts reusing one pointer ID open two contests an observer tells
/// apart by the sequence each Down started, the one dispatch reports.
fn a_reused_pointer_contests_a_new_sequence() {
    let binding = GestureBinding::new();
    let joined = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&joined);
    let _observer = binding
        .arena()
        .observe(move |observation: &GestureObservation| {
            if let GestureObservation::Joined { contest, .. } = observation {
                sink.borrow_mut()
                    .push((contest.pointer(), contest.sequence()));
            }
        })
        .expect("observer registration");
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let log = Log::default();
    let member = Member::new("a", &log);
    let p = pointer(9);
    let mut dispatched = Vec::new();
    lane.enter(|| {
        let (arena, member) = (binding.arena().clone(), Rc::clone(&member));
        let entries = RefCell::new(Vec::new());
        let target = handle
            .register_pointer(move |dispatch| {
                if let PointerEvent::Down(_) = dispatch.global {
                    entries.borrow_mut().push(arena.add(p, &member));
                }
            })
            .expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        for _ in 0..2 {
            let down = make_down_event_for_id(p, Offset::ZERO, PointerKind::Touch).expect("down");
            let outcome =
                binding.dispatch_pointer_on(InputSurface::Window, &down, |_| path.clone());
            dispatched.push(outcome.sequence());
            let up = make_up_event_for_id(p, Offset::ZERO, PointerKind::Touch).expect("up");
            let _ = binding.dispatch_pointer_on(InputSurface::Window, &up, |_| path.clone());
        }
    });
    let joined = joined.borrow().clone();
    assert_eq!(joined.len(), 2, "each Down opens a contest");
    assert!(joined.iter().all(|(pointer, _)| *pointer == p));
    assert!(dispatched.iter().all(Option::is_some));
    assert_eq!(
        joined
            .iter()
            .map(|(_, sequence)| *sequence)
            .collect::<Vec<_>>(),
        dispatched,
        "each contest decides the sequence its Down started"
    );
    assert_ne!(
        joined[0].1, joined[1].1,
        "a reused pointer is a new sequence"
    );
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

/// A standalone operation whose member fails still delivers the decisions it
/// committed to observers before the failure propagates: nothing else would
/// deliver them.
fn a_failed_operation_still_reaches_observers() {
    struct FailingWinner;
    impl GestureArenaMember for FailingWinner {
        fn accept_gesture(&self, _: GestureContest) {
            std::panic::panic_any("winner failure");
        }
        fn reject_gesture(&self, _: GestureContest) {}
    }
    let log = Log::default();
    let arena = GestureArena::new();
    let _observer = record_into(&arena, &log, &[]);
    let winner = Rc::new(FailingWinner);
    let p = pointer(1);
    let _entry = arena.add(p, &winner);
    arena.close(p);
    let drained = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.drain_deferred_resolutions()
    }));
    let payload = drained.expect_err("the winner's failure propagates");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"winner failure"));
    assert_eq!(
        logged(&log),
        ["observed joined ?", "observed closed", "observed won ?"]
    );
}

/// A clock that fails stamping a committed verdict loses none of it: the
/// members hear their verdicts and observers the decision, then the clock's
/// failure propagates.
fn a_failing_clock_loses_no_committed_verdict() {
    use std::sync::atomic::{AtomicBool, Ordering};
    #[derive(Debug)]
    struct Clock {
        now: web_time::Instant,
        armed: AtomicBool,
    }
    impl flui_foundation::MonotonicClock for Clock {
        fn now(&self) -> web_time::Instant {
            if self.armed.load(Ordering::Relaxed) {
                std::panic::panic_any("clock failure");
            }
            self.now
        }
    }
    let clock = Arc::new(Clock {
        now: web_time::Instant::now(),
        armed: AtomicBool::new(false),
    });
    let log = Log::default();
    let arena = GestureArena::with_clock(clock.clone());
    let (a, b) = (Member::new("a", &log), Member::new("b", &log));
    let _observer = record_into(&arena, &log, &[&a, &b]);
    let p = pointer(1);
    let entry = arena.add(p, &a);
    let _b_entry = arena.add(p, &b);
    // An eager claim while open wins when the contest closes.
    entry.resolve(GestureDisposition::Accepted);
    log.borrow_mut().clear();
    clock.armed.store(true, Ordering::Relaxed);
    let closed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| arena.close(p)));
    clock.armed.store(false, Ordering::Relaxed);
    let payload = closed.expect_err("the clock's failure propagates");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"clock failure"));
    assert_eq!(
        logged(&log),
        [
            "b rejected",
            "a accepted",
            "observed closed",
            "observed rejected b",
            "observed won a",
        ]
    );
}

/// A clock that fails stamping a verdict before a member's callback fails in
/// the same operation is the failure raised, whether the verdict was stamped
/// before the operation delivered (a close) or inside it (a claim on a closed
/// contest); nothing committed is lost to either failure.
fn a_clock_failure_before_a_member_failure_is_the_one_raised() {
    use std::sync::atomic::{AtomicBool, Ordering};
    #[derive(Debug)]
    struct Clock {
        now: web_time::Instant,
        armed: AtomicBool,
    }
    impl flui_foundation::MonotonicClock for Clock {
        fn now(&self) -> web_time::Instant {
            if self.armed.load(Ordering::Relaxed) {
                std::panic::panic_any("clock failure");
            }
            self.now
        }
    }
    struct FailingLoser {
        log: Log,
    }
    impl GestureArenaMember for FailingLoser {
        fn accept_gesture(&self, _: GestureContest) {}
        fn reject_gesture(&self, _: GestureContest) {
            self.log.borrow_mut().push("b rejected".to_owned());
            std::panic::panic_any("loser failure");
        }
    }
    type Case = (&'static str, bool, &'static str);
    let cases: [Case; 2] = [
        ("a close settling an eager claim", true, "observed closed"),
        ("a claim on a closed contest", false, "observed claimed a"),
    ];
    for (name, eager, decided) in cases {
        let clock = Arc::new(Clock {
            now: web_time::Instant::now(),
            armed: AtomicBool::new(false),
        });
        let log = Log::default();
        let arena = GestureArena::with_clock(clock.clone());
        let a = Member::new("a", &log);
        let b = Rc::new(FailingLoser {
            log: Rc::clone(&log),
        });
        let _observer = record_into(&arena, &log, &[&a]);
        let p = pointer(1);
        let entry = arena.add(p, &a);
        let _b_entry = arena.add(p, &b);
        if eager {
            entry.resolve(GestureDisposition::Accepted);
        } else {
            arena.close(p);
        }
        log.borrow_mut().clear();
        clock.armed.store(true, Ordering::Relaxed);
        let decide = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if eager {
                arena.close(p);
            } else {
                entry.resolve(GestureDisposition::Accepted);
            }
        }));
        clock.armed.store(false, Ordering::Relaxed);
        let payload = decide.expect_err("a failure propagates");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"clock failure"),
            "{name}: the earlier failure is raised"
        );
        assert_eq!(
            logged(&log),
            [
                "b rejected",
                "a accepted",
                decided,
                "observed rejected ?",
                "observed won a",
            ],
            "{name}"
        );
    }
}
