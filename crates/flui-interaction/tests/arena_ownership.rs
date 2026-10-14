//! ADR-0208 rows of `gesture_lifecycle_matrix`: nested composition, the
//! `Sequenced` hand-off, the delay ceiling, device admission and the
//! cancellation a claimed contact sends the rest of its route.

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    PointerButton, PointerEvent, PointerKind, make_down_event_for_id_with_button,
    make_move_event_for_id, make_up_event_for_id_with_button,
};
use flui_interaction::gesture::{
    CompositionError, CompositionKind, GestureArena, GestureArenaMember, GestureComposition,
    GestureDisposition, GestureObservation, GestureProfiles, GestureSettings,
    GestureSettingsProvider, ObservedComposition,
};
use flui_interaction::pointer::PointerDispatch;
use flui_interaction::{
    DragAxis, DragGestureRecognizer, GestureBinding, HitTestEntry, HitTestResult, InteractionLane,
    ManualClock, PointerId, PointerKindSet, RecognizerSet, RenderId,
};
use flui_platform_api::{GesturePreferences, pointer::CancelReason, pointer::PenTool};

type Log = Rc<RefCell<Vec<(&'static str, GestureDisposition)>>>;

struct Probe {
    name: &'static str,
    log: Log,
}

impl GestureArenaMember for Probe {
    fn accept_gesture(&self, _: PointerId) {
        self.log
            .borrow_mut()
            .push((self.name, GestureDisposition::Accepted));
    }
    fn reject_gesture(&self, _: PointerId) {
        self.log
            .borrow_mut()
            .push((self.name, GestureDisposition::Rejected));
    }
}

fn probe(name: &'static str, log: &Log) -> Rc<Probe> {
    Rc::new(Probe {
        name,
        log: Rc::clone(log),
    })
}

fn id(raw: u64) -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero pointer id"))
}

fn at(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

fn down(pointer: PointerId, position: Offset<f64>, kind: PointerKind) -> PointerEvent {
    make_down_event_for_id_with_button(pointer, position, kind, PointerButton::PRIMARY)
        .expect("valid fixture sample")
}

use GestureDisposition::{Accepted, Rejected};

pub(crate) fn nested_composition_waits_on_an_enclosing_preferred_branch() {
    let arena = GestureArena::new();
    let (preferred, fallback) = arena
        .compose(GestureComposition::RequireFailure)
        .expect("root composition")
        .into_branches();
    let (nested, sibling) = fallback
        .compose(GestureComposition::Exclusive)
        .expect("a branch composes again")
        .into_branches();
    assert_eq!(
        nested.composition_path().collect::<Vec<_>>(),
        [CompositionKind::Exclusive, CompositionKind::RequireFailure],
        "each node reports its kind, innermost first"
    );
    let log = Log::default();
    let pointer = id(1);
    let (first, inner, other) = (
        probe("preferred", &log),
        probe("nested", &log),
        probe("sibling", &log),
    );
    let preferred_entry = preferred.add(pointer, &first);
    let nested_entry = nested.add(pointer, &inner);
    let _sibling_entry = sibling.add(pointer, &other);
    arena.close(pointer);

    nested_entry.resolve(Accepted);
    assert!(
        log.borrow().is_empty(),
        "a fallback one level down still waits for the enclosing preferred branch"
    );
    preferred_entry.resolve(Rejected);
    assert_eq!(
        *log.borrow(),
        [
            ("preferred", Rejected),
            ("sibling", Rejected),
            ("nested", Accepted)
        ]
    );
}

pub(crate) fn sequenced_hold_hands_its_won_contact_to_the_successor() {
    let arena = GestureArena::new();
    let branch = arena.sequenced().expect("sequenced composition");
    assert_eq!(
        branch.successor().composition_path().collect::<Vec<_>>(),
        [CompositionKind::Sequenced]
    );
    let log = Log::default();
    let joined = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&joined);
    let _observer = arena
        .observe(move |observation: &GestureObservation| {
            if let GestureObservation::Joined { composition, .. } = observation {
                observed
                    .borrow_mut()
                    .push(composition.first().map(ObservedComposition::kind));
            }
        })
        .expect("observer registration");
    let pointer = id(2);
    let (hold, successor, competitor) = (
        probe("hold", &log),
        probe("successor", &log),
        probe("competitor", &log),
    );
    let hold_entry = branch.hold().add(pointer, &hold);
    let successor_entry = branch.successor().add(pointer, &successor);
    let _competitor_entry = arena.add(pointer, &competitor);
    arena.close(pointer);
    assert_eq!(
        *joined.borrow(),
        [
            Some(CompositionKind::Sequenced),
            Some(CompositionKind::Sequenced),
            None
        ],
        "each admission reports the relation it joined under"
    );

    successor_entry.resolve(Accepted);
    assert!(
        log.borrow().is_empty(),
        "a successor never wins the arena itself"
    );
    hold_entry.resolve(Accepted);
    assert_eq!(
        *log.borrow(),
        [("competitor", Rejected), ("hold", Accepted)],
        "the hold's win keeps its successor admitted"
    );
    assert!(branch.hand_off(pointer), "the won contact hands off");
    assert_eq!(log.borrow().last(), Some(&("successor", Accepted)));
    assert!(!branch.hand_off(pointer), "a contact hands off once");
    assert_eq!(log.borrow().len(), 3, "every member settles exactly once");
}

pub(crate) fn sequenced_successor_fails_with_its_hold() {
    let arena = GestureArena::new();
    let branch = arena.sequenced().expect("sequenced composition");
    let log = Log::default();
    let pointer = id(3);
    let (hold, successor, competitor) = (
        probe("hold", &log),
        probe("successor", &log),
        probe("competitor", &log),
    );
    let hold_entry = branch.hold().add(pointer, &hold);
    let _successor_entry = branch.successor().add(pointer, &successor);
    let _competitor_entry = arena.add(pointer, &competitor);
    arena.close(pointer);

    hold_entry.resolve(Rejected);
    assert_eq!(
        *log.borrow(),
        [("hold", Rejected), ("successor", Rejected)],
        "a successor whose hold failed fails with it"
    );
    arena.drain_deferred_resolutions();
    assert_eq!(log.borrow().last(), Some(&("competitor", Accepted)));
}

pub(crate) fn unhanded_successor_is_rejected_when_its_contact_ends() {
    let arena = GestureArena::new();
    let branch = arena.sequenced().expect("sequenced composition");
    let log = Log::default();
    let pointer = id(4);
    let (hold, successor) = (probe("hold", &log), probe("successor", &log));
    let hold_entry = branch.hold().add(pointer, &hold);
    let _successor_entry = branch.successor().add(pointer, &successor);
    arena.close(pointer);
    hold_entry.resolve(Accepted);
    arena.sweep(pointer);
    assert_eq!(*log.borrow(), [("hold", Accepted), ("successor", Rejected)]);
    assert!(
        !branch.hand_off(pointer),
        "an ended contact cannot hand off"
    );
}

pub(crate) fn composition_refuses_a_path_past_the_delay_ceiling() {
    let hold = Duration::from_millis(1300);
    let arena =
        GestureArena::new().with_settings(GestureSettings::default().with_long_press_timeout(hold));
    let sequenced = arena
        .sequenced()
        .expect("a 1300 ms hold fits the 1500 ms ceiling");
    assert_eq!(
        sequenced
            .hold()
            .compose(GestureComposition::RequireFailure)
            .err(),
        Some(CompositionError::DelayCeilingExceeded {
            sum: hold + GestureSettings::default().double_tap_timeout(),
            ceiling: GestureSettings::default().max_resolution_delay(),
        }),
        "a double-tap wait below the hold exceeds the ceiling"
    );

    // The host's touch-and-hold floor raises the hold this scope uses, but the
    // ceiling sums authored values (ADR-0212 §3).
    let observed = GestureSettings::resolve_preferences(
        &GestureSettings::default(),
        &GesturePreferences::default().with_long_press_timeout(hold),
        None,
    )
    .expect("observation resolves");
    let scope = GestureSettingsProvider::from(GestureProfiles::uniform(observed))
        .overridden_by(GestureSettings::default());
    assert_eq!(
        scope.snapshot_for(PointerKind::Touch).long_press_timeout(),
        hold,
        "the floor applies to the scope's contacts"
    );
    let floored = GestureArena::new().with_settings(scope);
    assert!(
        floored
            .sequenced()
            .expect("authored hold")
            .hold()
            .compose(GestureComposition::RequireFailure)
            .is_ok(),
        "a raised floor never turns a valid tree into a composition error"
    );
}

pub(crate) fn closed_arena_refuses_composition() {
    let lane = InteractionLane::try_new().expect("owner lane");
    let binding = GestureBinding::with_clock(Arc::new(ManualClock::new()));
    lane.enter(|| {
        flui_interaction::__runtime::close_gestures(
            &binding,
            flui_interaction::__runtime::CloseMode::Ordinary,
        );
    });
    assert_eq!(
        binding.arena().compose(GestureComposition::Exclusive).err(),
        Some(CompositionError::Closed)
    );
}

pub(crate) fn allowed_devices_refuse_other_kinds_before_the_arena() {
    let arena = GestureArena::new();
    let pen_only = DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
        .allowed_devices(PointerKindSet::PEN)
        .build();
    let mut set = RecognizerSet::default();
    set.attach(&pen_only);
    let finger = down(id(5), at(0.0, 0.0), PointerKind::Touch);
    set.dispatch(PointerDispatch::at_root(&finger));
    assert_eq!(arena.member_count(id(5)), 0, "a finger is refused");
    let pen = down(id(6), at(0.0, 0.0), PointerKind::Pen { tool: PenTool::Tip });
    set.dispatch(PointerDispatch::at_root(&pen));
    assert_eq!(arena.member_count(id(6)), 1, "the pen is admitted");
}

type Events = Rc<RefCell<Vec<&'static str>>>;

fn event_name(event: &PointerEvent) -> &'static str {
    match event {
        PointerEvent::Down(_) => "down",
        PointerEvent::Move(_) => "move",
        PointerEvent::Up(_) => "up",
        PointerEvent::Cancel(cancel) if cancel.reason == CancelReason::Claimed => "claimed",
        PointerEvent::Cancel(_) => "cancel",
        _ => "other",
    }
}

/// The winner's listener keeps the contact; every other listener on the
/// route, losers' and observers' alike, hears one `Cancel(Claimed)` and
/// nothing more (ADR-0208 rule 5).
pub(crate) fn claimed_contact_cancels_every_other_listener_once() {
    for kind in [PointerKind::Touch, PointerKind::Mouse] {
        let lane = InteractionLane::try_new().expect("owner lane");
        let binding = GestureBinding::with_clock(Arc::new(ManualClock::new()));
        let arena = binding.arena().clone();
        let starts = Events::default();
        let started = Rc::clone(&starts);
        let winner = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
            .on_start(move |_| started.borrow_mut().push("winner"))
            .build();
        let loser = DragGestureRecognizer::builder(arena.clone(), DragAxis::Vertical).build();
        let (inner, outer, observer) = (Events::default(), Events::default(), Events::default());
        let route = lane.enter(|| {
            let handle = lane.dispatch_handle();
            let target = |recognizer: Option<&Rc<DragGestureRecognizer>>, log: &Events| {
                let mut set = RecognizerSet::default();
                if let Some(recognizer) = recognizer {
                    set.attach(recognizer);
                }
                let log = Rc::clone(log);
                handle
                    .register_pointer(move |dispatch| {
                        log.borrow_mut().push(event_name(dispatch.local));
                        set.dispatch(dispatch);
                    })
                    .expect("register target")
            };
            let targets = [
                target(Some(&winner), &inner),
                target(Some(&loser), &outer),
                target(None, &observer),
            ];
            let mut route = HitTestResult::new();
            for (index, target) in targets.into_iter().enumerate() {
                route.add(HitTestEntry::new(RenderId::new(index + 1)).pointer_target(target));
            }
            route
        });
        let pointer = id(7);
        let send = |event: PointerEvent| {
            lane.enter(|| {
                binding.handle_pointer_event(&event, |_| route.clone());
                binding.flush_pending_moves();
                binding.drain_deferred_arena_resolutions();
            });
        };
        send(down(pointer, at(0.0, 0.0), kind));
        send(make_move_event_for_id(pointer, at(40.0, 2.0), kind).expect("finite move"));
        send(make_move_event_for_id(pointer, at(60.0, 2.0), kind).expect("finite move"));
        send(
            make_up_event_for_id_with_button(pointer, at(60.0, 2.0), kind, PointerButton::PRIMARY)
                .expect("finite up"),
        );
        assert_eq!(*starts.borrow(), ["winner"], "{kind:?}");
        assert_eq!(*inner.borrow(), ["down", "move", "move", "up"], "{kind:?}");
        // The claiming Move was already committed to the whole route.
        for (name, log) in [("loser", &outer), ("observer", &observer)] {
            assert_eq!(
                *log.borrow(),
                ["down", "move", "claimed"],
                "{name}, {kind:?}"
            );
        }
    }
}

/// A win nobody claimed cancels no listener (ADR-0208 rule 5): a lone member's
/// default win leaves a raw listener beside it the whole contact, and a hold
/// whose win carries a successor claims the contact only at the hand-off.
pub(crate) fn unclaimed_wins_cancel_no_listener() {
    use flui_interaction::{LongPressGestureRecognizer, TapGestureRecognizer};
    for kind in [PointerKind::Touch, PointerKind::Mouse] {
        let lane = InteractionLane::try_new().expect("owner lane");
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let arena = binding.arena().clone();
        let tap = TapGestureRecognizer::builder(arena.clone()).build();
        let branch = Rc::new(arena.sequenced().expect("sequenced composition"));
        let hold = LongPressGestureRecognizer::builder(branch.hold()).build();
        let successor = DragGestureRecognizer::builder(branch.successor(), DragAxis::Free).build();
        let rival = TapGestureRecognizer::builder(arena.clone()).build();
        let (lone, held, observer) = (Events::default(), Events::default(), Events::default());
        let routes = lane.enter(|| {
            let handle = lane.dispatch_handle();
            let target = |set: RecognizerSet, log: &Events| {
                let log = Rc::clone(log);
                handle
                    .register_pointer(move |dispatch| {
                        log.borrow_mut().push(event_name(dispatch.local));
                        set.dispatch(dispatch);
                    })
                    .expect("register target")
            };
            let mut tap_set = RecognizerSet::default();
            tap_set.attach(&tap);
            let mut hold_set = RecognizerSet::default();
            hold_set.attach(&hold);
            hold_set.attach(&successor);
            // A competitor, so the hold wins by claiming at its deadline.
            hold_set.attach(&rival);
            let observing = target(RecognizerSet::default(), &observer);
            [target(tap_set, &lone), target(hold_set, &held)].map(|inner| {
                let mut route = HitTestResult::new();
                route.add(HitTestEntry::new(RenderId::new(1)).pointer_target(inner));
                route.add(HitTestEntry::new(RenderId::new(2)).pointer_target(observing));
                route
            })
        });
        let send = |event: PointerEvent, route: &HitTestResult| {
            lane.enter(|| {
                binding.handle_pointer_event(&event, |_| route.clone());
                binding.flush_pending_moves();
                binding.drain_deferred_arena_resolutions();
            });
        };
        let moved = |pointer, x| make_move_event_for_id(pointer, at(x, 0.0), kind).expect("move");
        let up = |pointer, x| {
            make_up_event_for_id_with_button(pointer, at(x, 0.0), kind, PointerButton::PRIMARY)
                .expect("up")
        };

        let pointer = id(8);
        send(down(pointer, at(0.0, 0.0), kind), &routes[0]);
        send(moved(pointer, 0.5), &routes[0]);
        send(up(pointer, 0.5), &routes[0]);
        assert_eq!(*lone.borrow(), ["down", "move", "up"], "{kind:?}");
        assert_eq!(
            *observer.borrow(),
            ["down", "move", "up"],
            "a default win claims nothing: {kind:?}"
        );

        observer.borrow_mut().clear();
        let pointer = id(9);
        send(down(pointer, at(0.0, 0.0), kind), &routes[1]);
        clock.advance(Duration::from_millis(700));
        lane.enter(|| {
            binding.tick_deadlines();
            binding.drain_deferred_arena_resolutions();
        });
        send(moved(pointer, 0.5), &routes[1]);
        assert_eq!(
            *observer.borrow(),
            ["down", "move"],
            "the hold's win carries its successor and claims nothing: {kind:?}"
        );
        lane.enter(|| assert!(branch.hand_off(pointer), "{kind:?}"));
        send(moved(pointer, 40.0), &routes[1]);
        send(up(pointer, 40.0), &routes[1]);
        assert_eq!(
            *observer.borrow(),
            ["down", "move", "claimed"],
            "the hand-off claims the contact: {kind:?}"
        );
        assert_eq!(*held.borrow(), ["down", "move", "move", "up"], "{kind:?}");
    }
}
