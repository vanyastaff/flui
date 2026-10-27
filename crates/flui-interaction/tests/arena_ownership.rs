//! ADR-0208 rows of `gesture_lifecycle_matrix`: nested composition, the
//! `Sequenced` hand-off, the delay ceiling, device admission and the
//! cancellation a claimed contact sends the rest of its route.

use flui_interaction::gesture::GestureContest;
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
use flui_platform::{GesturePreferences, pointer::CancelReason, pointer::PenTool};

type Log = Rc<RefCell<Vec<(&'static str, GestureDisposition)>>>;

struct Probe {
    name: &'static str,
    log: Log,
}

impl GestureArenaMember for Probe {
    fn accept_gesture(&self, _: GestureContest) {
        self.log
            .borrow_mut()
            .push((self.name, GestureDisposition::Accepted));
    }
    fn reject_gesture(&self, _: GestureContest) {
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

/// An earlier generation a hold kept past its Up sweeps while the same
/// pointer's next contact is down, its hold won and its successor carried:
/// the sweep ends only its own contest's successors.
pub(crate) fn a_swept_earlier_generation_keeps_the_next_contacts_successors() {
    let arena = GestureArena::new();
    let branch = arena.sequenced().expect("sequenced composition");
    let log = Log::default();
    let pointer = id(1);
    let earlier = probe("earlier", &log);
    let earlier_entry = arena.add(pointer, &earlier);
    earlier_entry.hold();
    arena.close(pointer);
    arena.sweep(pointer);

    let (hold, successor) = (probe("hold", &log), probe("successor", &log));
    let hold_entry = branch.hold().add(pointer, &hold);
    let _successor_entry = branch.successor().add(pointer, &successor);
    arena.close(pointer);
    hold_entry.resolve(Accepted);
    earlier_entry.release();
    assert_eq!(
        *log.borrow(),
        [("hold", Accepted), ("earlier", Accepted)],
        "the earlier contest settles alone"
    );
    assert!(branch.hand_off(pointer), "the next contact still hands off");
    assert_eq!(log.borrow().last(), Some(&("successor", Accepted)));
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

/// A window that counts the frames asked of it.
struct RedrawCounter(std::sync::atomic::AtomicUsize);

impl RedrawCounter {
    fn requests(&self) -> usize {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl flui_platform::PlatformWindow for RedrawCounter {
    fn id(&self) -> flui_platform::WindowId {
        flui_platform::WindowId(1)
    }
    fn request_redraw(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    flui_platform::window_takes_no_frame_verbs!();

    fn set_cursor(&self, _: flui_platform::CursorIcon) -> Result<(), flui_platform::CursorError> {
        Ok(())
    }
    fn state(&self) -> flui_platform::WindowState {
        let size = flui_foundation::geometry::Size::new(100.0, 100.0);
        flui_platform::WindowState::new(flui_platform::WindowGeometry::new(None, size), size)
            .with_focused(true)
            .with_metrics(flui_foundation::geometry::WindowMetrics::from_logical(size, 1.0).ok())
    }
    fn apply(
        &self,
        command: flui_platform::AdmittedCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        command.against(&flui_platform::WindowCapabilities::NONE)?;
        Ok(())
    }
    fn text_input(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }
    fn drag_source(&self) -> Option<std::sync::Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }
}

/// Hover freezes while a moving member owns a contact (ADR-0208 rule 6): no
/// region is entered or exited, by the dragging mouse or by another one
/// hovering, until the contact ends; the first refresh after it hit-tests
/// where each source went, and the end of the latch asks for the frame that
/// runs it.
pub(crate) fn hover_freezes_while_a_scroll_is_latched() {
    use flui_interaction::events::PointerButtons;
    use flui_interaction::pointer::{MouseRegionCallbacks, MouseTrackerAnnotation};
    // (dragging device, the device whose hover is watched, heard under the
    // latch, heard after the refresh that follows it)
    type Case = (
        PointerKind,
        PointerId,
        &'static [&'static str],
        &'static [&'static str],
    );
    let cases: [Case; 2] = [
        (PointerKind::Mouse, id(22), &[], &["enter B"]),
        (
            PointerKind::Touch,
            id(21),
            &["enter A"],
            &["enter A", "exit A", "enter B"],
        ),
    ];
    for (dragging, hovering, frozen_heard, heard) in cases {
        let lane = InteractionLane::try_new().expect("owner lane");
        let binding = GestureBinding::with_clock(Arc::new(ManualClock::new()));
        let window = Arc::new(RedrawCounter(std::sync::atomic::AtomicUsize::new(0)));
        let wake: Arc<dyn flui_platform::PlatformWindow> = window.clone();
        flui_interaction::__runtime::set_pointer_capture_wake(&binding, Arc::downgrade(&wake));
        let arena = binding.arena().clone();
        let scroll = DragGestureRecognizer::builder(arena, DragAxis::Horizontal).build();
        let log = Events::default();
        let (regions, scroller) = lane.enter(|| {
            let handle = lane.dispatch_handle();
            let region = |enter: &'static str, exit: &'static str| {
                let (entered, exited) = (Rc::clone(&log), Rc::clone(&log));
                handle
                    .register_mouse_region(MouseRegionCallbacks {
                        on_enter: Some(Rc::new(move |_, _| entered.borrow_mut().push(enter))),
                        on_exit: Some(Rc::new(move |_, _| exited.borrow_mut().push(exit))),
                        ..MouseRegionCallbacks::default()
                    })
                    .expect("register region")
            };
            let mut set = RecognizerSet::default();
            set.attach(&scroll);
            let scroller = handle
                .register_pointer(move |dispatch| set.dispatch(dispatch))
                .expect("register scroller");
            (
                [region("enter A", "exit A"), region("enter B", "exit B")],
                scroller,
            )
        });
        // Region A left of x = 50, region B right of it, over one scroller.
        let route = move |position: Offset<f64>| {
            let (index, region) = if position.dx < 50.0 {
                (1, regions[0])
            } else {
                (2, regions[1])
            };
            let id = RenderId::new(index);
            let mut route = HitTestResult::new();
            route.add(
                HitTestEntry::new(id).mouse_annotation(MouseTrackerAnnotation::new(id, region)),
            );
            route.add(HitTestEntry::new(RenderId::new(3)).pointer_target(scroller));
            route
        };
        let send = |event: PointerEvent| {
            lane.enter(|| {
                binding.handle_pointer_event(&event, route);
                binding.flush_pending_moves();
                binding.drain_deferred_arena_resolutions();
            });
        };
        let hover = |x: f64| {
            let mut event =
                make_move_event_for_id(hovering, at(x, 0.0), PointerKind::Mouse).expect("move");
            if let PointerEvent::Move(update) = &mut event {
                update.buttons = PointerButtons::NONE;
            }
            event
        };
        let contact = id(22);
        let moved = |x: f64| make_move_event_for_id(contact, at(x, 0.0), dragging).expect("move");
        if dragging == PointerKind::Touch {
            send(hover(10.0));
        }
        send(down(contact, at(10.0, 0.0), dragging));
        send(moved(40.0));
        assert!(binding.is_hover_frozen(), "the drag owns the contact");
        if dragging == PointerKind::Mouse {
            send(moved(80.0));
        } else {
            send(hover(80.0));
        }
        let frozen: Vec<_> = log.borrow().clone();
        let requested = window.requests();
        send(
            make_up_event_for_id_with_button(
                contact,
                at(80.0, 0.0),
                dragging,
                PointerButton::PRIMARY,
            )
            .expect("finite up"),
        );
        assert!(!binding.is_hover_frozen(), "{dragging:?}");
        assert!(
            window.requests() > requested,
            "{dragging:?}: the latch's end asks for the frame whose refresh catches hover up"
        );
        lane.enter(|| {
            binding
                .mouse_tracker()
                .update_all_devices(|position, _| route(position));
        });
        assert_eq!(frozen, frozen_heard, "{dragging:?}: frozen under the latch");
        assert_eq!(
            *log.borrow(),
            heard,
            "{dragging:?}: the refresh after the latch"
        );
    }
}

/// Hold on Down (ADR-0208 rule 4): a contact pressed on a moving scrollable
/// admits a hold member that beats tap and long-press on that contact, not a
/// drag. The press stays hit-testable: the tap and the long press are
/// admitted, then lose to the hold.
pub(crate) fn hold_on_down_beats_tap_and_long_press_not_drag() {
    use flui_interaction::{
        HoldEnd, HoldGestureRecognizer, LongPressGestureRecognizer, TapGestureRecognizer,
    };
    type Case = (&'static [f64], Duration, &'static [&'static str]);
    let cases: [Case; 3] = [
        (&[], Duration::from_millis(16), &["hold", "released Lifted"]),
        (
            &[],
            Duration::from_millis(700),
            &["hold", "released Lifted"],
        ),
        (
            &[2.0, 40.0, 60.0],
            Duration::from_millis(16),
            &["hold", "released Yielded", "list"],
        ),
    ];
    // The item inside the scrollable joins before the hold and loses to it;
    // a tap around the scrollable joins after it and is refused.
    for (kind, tap_outside) in [
        (PointerKind::Touch, false),
        (PointerKind::Mouse, false),
        (PointerKind::Touch, true),
    ] {
        for (moves, wait, heard) in cases {
            let lane = InteractionLane::try_new().expect("owner lane");
            let clock = ManualClock::new();
            let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
            let arena = binding.arena().clone();
            let log = Events::default();
            let entry = |name: &'static str| {
                let log = Rc::clone(&log);
                move || log.borrow_mut().push(name)
            };
            let (on_tap, on_long_press, on_list, on_hold) = (
                entry("tap"),
                entry("long press"),
                entry("list"),
                entry("hold"),
            );
            let tap = TapGestureRecognizer::builder(arena.clone())
                .on_tap(move |_| on_tap())
                .build();
            let long_press = LongPressGestureRecognizer::builder(arena.clone())
                .on_long_press(on_long_press)
                .build();
            let list = DragGestureRecognizer::builder(arena.clone(), DragAxis::Vertical)
                .on_start(move |_| on_list())
                .build();
            let released = Rc::clone(&log);
            let hold = HoldGestureRecognizer::builder(arena.clone())
                .on_hold(move |_| on_hold())
                .on_release(move |end| {
                    released.borrow_mut().push(match end {
                        HoldEnd::Lifted => "released Lifted",
                        HoldEnd::Yielded => "released Yielded",
                        _ => "released otherwise",
                    });
                })
                .build();
            let reasons = Rc::new(RefCell::new(Vec::new()));
            let seen = Rc::clone(&reasons);
            let _observer = arena
                .observe(move |observation: &flui_interaction::GestureObservation| {
                    if let flui_interaction::GestureObservation::Rejected { reason, .. } =
                        observation
                    {
                        seen.borrow_mut().push(*reason);
                    }
                })
                .expect("observer registration");
            let route = lane.enter(|| {
                let handle = lane.dispatch_handle();
                let mut item = RecognizerSet::default();
                item.attach(&tap);
                item.attach(&long_press);
                let mut scrollable = RecognizerSet::default();
                scrollable.attach(&list);
                scrollable.attach(&hold);
                let levels = if tap_outside {
                    [scrollable, item]
                } else {
                    [item, scrollable]
                };
                let mut route = HitTestResult::new();
                for (index, set) in levels.into_iter().enumerate() {
                    let target = handle
                        .register_pointer(move |dispatch| set.dispatch(dispatch))
                        .expect("register target");
                    route.add(HitTestEntry::new(RenderId::new(index + 1)).pointer_target(target));
                }
                route
            });
            let send = |event: PointerEvent| {
                lane.enter(|| {
                    binding.handle_pointer_event(&event, |_| route.clone());
                    binding.flush_pending_moves();
                    binding.drain_deferred_arena_resolutions();
                });
            };
            let pointer = id(10);
            send(down(pointer, at(0.0, 0.0), kind));
            clock.advance(wait);
            lane.enter(|| {
                binding.tick_deadlines();
                binding.drain_deferred_arena_resolutions();
            });
            let mut y = 0.0;
            for &step in moves {
                y = step;
                send(make_move_event_for_id(pointer, at(0.0, y), kind).expect("finite move"));
            }
            send(
                make_up_event_for_id_with_button(pointer, at(0.0, y), kind, PointerButton::PRIMARY)
                    .expect("finite up"),
            );
            assert_eq!(
                *log.borrow(),
                heard,
                "{kind:?}, tap outside: {tap_outside}, moves {moves:?}, held {wait:?}"
            );
            // The tap and long press that joined before the hold are rejected
            // as held out; ones that would join after it are never admitted.
            let held_out = reasons
                .borrow()
                .iter()
                .filter(|reason| **reason == flui_interaction::gesture::RejectionReason::HeldOut)
                .count();
            assert_eq!(
                held_out,
                if tap_outside { 0 } else { 2 },
                "{kind:?}, tap outside: {tap_outside}: rejections {:?}",
                reasons.borrow()
            );
        }
    }
}

/// A hold records its contact before the members it beats hear they lost
/// (ADR-0208 rule 4): a beaten member whose rejection fails leaves the hold
/// tracking, announced and released, and one whose rejection abandons the
/// contest leaves no hold announced for it.
pub(crate) fn a_hold_tracks_its_contact_before_beaten_members_hear() {
    use flui_interaction::gesture::GestureRecognizer;
    use flui_interaction::{HoldEnd, HoldGestureRecognizer};

    struct Beaten {
        on_reject: Box<dyn Fn()>,
    }
    impl GestureArenaMember for Beaten {
        fn accept_gesture(&self, _: GestureContest) {}
        fn reject_gesture(&self, _: GestureContest) {
            (self.on_reject)();
        }
    }

    type Case = (
        &'static str,
        fn(&GestureArena, PointerId),
        bool,
        &'static [&'static str],
    );
    let cases: [Case; 2] = [
        (
            "a failing rejection",
            |_, _| std::panic::panic_any("beaten member failure"),
            true,
            &["hold", "released Lifted"],
        ),
        (
            "a rejection that abandons the contest",
            |arena, pointer| arena.abandon(pointer),
            false,
            &[],
        ),
    ];
    for (name, on_reject, fails, heard) in cases {
        let arena = GestureArena::new();
        let log = Events::default();
        let (held, released) = (Rc::clone(&log), Rc::clone(&log));
        let hold = HoldGestureRecognizer::builder(arena.clone())
            .on_hold(move |_| held.borrow_mut().push("hold"))
            .on_release(move |end| {
                released.borrow_mut().push(match end {
                    HoldEnd::Lifted => "released Lifted",
                    HoldEnd::Yielded => "released Yielded",
                    _ => "released otherwise",
                });
            })
            .build();
        let pointer = id(1);
        let reentry = arena.clone();
        let beaten = Rc::new(Beaten {
            on_reject: Box::new(move || on_reject(&reentry, pointer)),
        });
        let _beaten_entry = arena.add(pointer, &beaten);
        let pressed = down(pointer, at(0.0, 0.0), PointerKind::Touch);
        let admitted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hold.add_pointer(PointerDispatch::at_root(&pressed));
        }));
        assert_eq!(
            admitted.is_err(),
            fails,
            "{name}: the beaten member's failure"
        );
        let lifted = make_up_event_for_id_with_button(
            pointer,
            at(0.0, 0.0),
            PointerKind::Touch,
            PointerButton::PRIMARY,
        )
        .expect("finite up");
        hold.handle_event(PointerDispatch::at_root(&lifted));
        assert_eq!(*log.borrow(), heard, "{name}");
    }
}

/// A recognizer that recognized nothing by its release leaves the contest:
/// an inner long press released early, a force press on a device without
/// pressure or a swipe that never crossed its slop cannot win the Up sweep by
/// its place on the route, so the outer tap still fires.
pub(crate) fn an_unrecognized_inner_gesture_leaves_the_outer_tap_its_contact() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::{
        ForcePressGestureRecognizer, LongPressGestureRecognizer, SwipeGestureRecognizer,
        SwipeStart, TapGestureRecognizer,
    };
    /// Builds the inner recognizer into the leaf's set; the result keeps it alive.
    type Inner = fn(&GestureArena, Events, &mut RecognizerSet) -> Rc<dyn std::any::Any>;
    let inners: [(&str, Inner); 3] = [
        ("long press", |arena, log, set| {
            let recognizer = LongPressGestureRecognizer::builder(arena.clone())
                .on_long_press(move || log.borrow_mut().push("inner"))
                .build();
            set.attach(&recognizer);
            recognizer
        }),
        ("force press", |arena, log, set| {
            let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
                .on_start(move |_| log.borrow_mut().push("inner"))
                .build();
            set.attach(&recognizer);
            recognizer
        }),
        ("swipe", |arena, log, set| {
            let recognizer = SwipeGestureRecognizer::builder(
                arena.clone(),
                SwipeStart::Anywhere(Axis::Horizontal),
            )
            .on_swipe(move |_| log.borrow_mut().push("inner"))
            .build();
            set.attach(&recognizer);
            recognizer
        }),
    ];
    for (name, inner) in inners {
        let lane = InteractionLane::try_new().expect("owner lane");
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let arena = binding.arena().clone();
        let log = Events::default();
        let tapped = Rc::clone(&log);
        let tap = TapGestureRecognizer::builder(arena.clone())
            .on_tap(move |_| tapped.borrow_mut().push("outer tap"))
            .build();
        let mut leaf = RecognizerSet::default();
        let _inner = inner(&arena, Rc::clone(&log), &mut leaf);
        let mut outer = RecognizerSet::default();
        outer.attach(&tap);
        let route = lane.enter(|| {
            let handle = lane.dispatch_handle();
            let mut route = HitTestResult::new();
            for (index, set) in [leaf, outer].into_iter().enumerate() {
                let target = handle
                    .register_pointer(move |dispatch| set.dispatch(dispatch))
                    .expect("register target");
                route.add(HitTestEntry::new(RenderId::new(index + 1)).pointer_target(target));
            }
            route
        });
        let send = |event: PointerEvent| {
            lane.enter(|| {
                binding.handle_pointer_event(&event, |_| route.clone());
                binding.flush_pending_moves();
                binding.drain_deferred_arena_resolutions();
            });
        };
        let pointer = id(30);
        send(down(pointer, at(0.0, 0.0), PointerKind::Touch));
        clock.advance(Duration::from_millis(100));
        lane.enter(|| {
            binding.tick_deadlines();
            binding.drain_deferred_arena_resolutions();
        });
        send(
            make_up_event_for_id_with_button(
                pointer,
                at(0.0, 0.0),
                PointerKind::Touch,
                PointerButton::PRIMARY,
            )
            .expect("finite up"),
        );
        assert_eq!(*log.borrow(), ["outer tap"], "inner {name}");
        assert!(arena.is_empty(), "inner {name}: the contest settled");
    }
}
