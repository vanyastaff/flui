//! `GestureBinding`'s services through the public binding: keyed by
//! `InputSurface`, and drained only after the dispatch that closed them
//! (ADR-0217 §2, ADR-0207 §5 and §8).

use flui_interaction::gesture::GestureContest;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::geometry::Offset;
use flui_interaction::__runtime::{CloseMode, close_gestures};
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id, make_scroll_event,
    make_up_event_for_id,
};
use flui_interaction::{
    EventPropagation, GestureArenaMember, GestureBinding, HitTestEntry, HitTestResult,
    InputSurface, InteractionLane, PointerId, PointerRouteHandler, RenderId,
};
use flui_platform_api::pointer::ScrollPhase;
use static_assertions::{assert_impl_all, assert_not_impl_any};

assert_not_impl_any!(GestureBinding: Send, Sync);
assert_impl_all!(InputSurface: Copy, Send, Sync);

fn run_rows(rows: &[(&str, fn())]) {
    for &(name, row) in rows {
        if let Err(payload) = std::panic::catch_unwind(row) {
            eprintln!("row `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

fn pointer(id: u64) -> PointerId {
    PointerId::try_from(id).expect("nonzero pointer id")
}

fn hover(id: u64, at: Offset<f64>) -> PointerEvent {
    make_move_event_for_id(pointer(id), at, PointerKind::Mouse).expect("finite hover")
}

fn wheel(phase: ScrollPhase) -> PointerEvent {
    let PointerEvent::Scroll(mut scroll) =
        make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite wheel")
    else {
        unreachable!("a scroll helper builds a scroll")
    };
    scroll.phase = Some(phase);
    PointerEvent::Scroll(scroll)
}

/// Count every Move the binding's pointer router delivers.
fn count_router_moves(binding: &GestureBinding) -> Rc<Cell<usize>> {
    let moves = Rc::new(Cell::new(0));
    let seen = Rc::clone(&moves);
    let handler: PointerRouteHandler = Rc::new(move |event| {
        if matches!(event, PointerEvent::Move(_)) {
            seen.set(seen.get() + 1);
        }
    });
    binding.pointer_router().add_global_handler(handler);
    moves
}

#[test]
fn binding_services_are_keyed_by_surface() {
    run_rows(&[
        (
            "hover_moves_on_two_surfaces_coalesce_apart",
            hover_moves_on_two_surfaces_coalesce_apart,
        ),
        (
            "hover_position_is_kept_per_surface",
            hover_position_is_kept_per_surface,
        ),
        (
            "a_wheel_latch_stays_on_its_surface",
            a_wheel_latch_stays_on_its_surface,
        ),
        (
            "a_contact_stays_with_the_surface_that_admitted_it",
            a_contact_stays_with_the_surface_that_admitted_it,
        ),
        (
            "embedded_surfaces_are_distinct",
            embedded_surfaces_are_distinct,
        ),
    ]);
}

fn hover_moves_on_two_surfaces_coalesce_apart() {
    let binding = GestureBinding::new();
    let embedded = binding.embed_surface().expect("surface");
    let moves = count_router_moves(&binding);
    binding.handle_pointer_event(&hover(1, Offset::new(1.0, 1.0)), |_| HitTestResult::new());
    let _ = binding.dispatch_pointer_on(embedded, &hover(1, Offset::new(2.0, 2.0)), |_| {
        HitTestResult::new()
    });
    assert_eq!(
        binding.pending_move_count(),
        2,
        "one queued move per surface"
    );
    assert_eq!(binding.flush_pending_moves(), 2);
    assert_eq!(
        moves.get(),
        2,
        "neither surface's last hover replaced the other's"
    );
}

fn hover_position_is_kept_per_surface() {
    let binding = GestureBinding::new();
    let embedded = binding.embed_surface().expect("surface");
    let PointerEvent::Move(update) = hover(1, Offset::new(5.0, 5.0)) else {
        unreachable!("a move helper builds a move")
    };
    let _ = binding.dispatch_pointer_on(embedded, &PointerEvent::Move(update.clone()), |_| {
        HitTestResult::new()
    });
    assert_eq!(
        binding.mouse_tracker().source_position(&update.pointer),
        None,
        "hovering an embedded surface leaves the window's hover untouched"
    );
    binding.handle_pointer_event(&hover(1, Offset::new(7.0, 7.0)), |_| HitTestResult::new());
    assert_eq!(
        binding.mouse_tracker().source_position(&update.pointer),
        Some(Offset::new(7.0, 7.0))
    );
}

fn a_wheel_latch_stays_on_its_surface() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let embedded = binding.embed_surface().expect("surface");
    let window_consumed = Rc::new(Cell::new(0));
    let embedded_consumed = Rc::new(Cell::new(0));
    lane.enter(|| {
        let path_for = |counter: &Rc<Cell<usize>>, id| {
            let calls = Rc::clone(counter);
            let target = handle
                .register_scroll(move |_| {
                    calls.set(calls.get() + 1);
                    EventPropagation::Stop
                })
                .expect("scroll consumer");
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(id)).scroll_target(target));
            path
        };
        let window_path = path_for(&window_consumed, 1);
        let embedded_path = path_for(&embedded_consumed, 2);
        binding.handle_pointer_event(&wheel(ScrollPhase::Began), |_| window_path.clone());
        let _ = binding.dispatch_pointer_on(embedded, &wheel(ScrollPhase::Changed), |_| {
            embedded_path.clone()
        });
        binding.handle_pointer_event(&wheel(ScrollPhase::Changed), |_| embedded_path.clone());
    });
    assert_eq!(
        (window_consumed.get(), embedded_consumed.get()),
        (2, 1),
        "the window's latch neither captures the embedded surface nor yields to it"
    );
}

fn a_contact_stays_with_the_surface_that_admitted_it() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let embedded = binding.embed_surface().expect("surface");
    let log = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let seen = Rc::clone(&log);
        let target = handle
            .register_pointer(move |dispatch| {
                seen.borrow_mut().push(match dispatch.global {
                    PointerEvent::Down(_) => "down",
                    PointerEvent::Move(_) => "move",
                    PointerEvent::Up(_) => "up",
                    _ => "other",
                });
            })
            .expect("window target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        let id = pointer(3);
        binding.handle_pointer_event(
            &make_down_event_for_id(id, Offset::ZERO, PointerKind::Touch).expect("down"),
            |_| path.clone(),
        );
        // The drag left the window: the embedded surface reports its tail.
        let _ = binding.dispatch_pointer_on(
            embedded,
            &make_move_event_for_id(id, Offset::new(4.0, 0.0), PointerKind::Touch).expect("move"),
            |_| panic!("a captured touch contact is not hit-tested"),
        );
        assert_eq!(binding.flush_pending_moves(), 1);
        let _ = binding.dispatch_pointer_on(
            embedded,
            &make_up_event_for_id(id, Offset::new(4.0, 0.0), PointerKind::Touch).expect("up"),
            |_| panic!("a terminal is not hit-tested"),
        );
    });
    assert_eq!(*log.borrow(), ["down", "move", "up"]);
    assert_eq!(binding.active_pointer_count(), 0);
}

fn embedded_surfaces_are_distinct() {
    let binding = GestureBinding::new();
    let first = binding.embed_surface().expect("first surface");
    let second = binding.embed_surface().expect("second surface");
    assert_ne!(first, second);
    assert_ne!(first, InputSurface::Window);
}

// ----------------------------------------------------------------------------
// Close during dispatch
// ----------------------------------------------------------------------------

/// An arena member that logs its rejection and may run `on_reject` then.
struct LoggingMember {
    log: Rc<RefCell<Vec<&'static str>>>,
    on_reject: Box<dyn Fn()>,
}

impl GestureArenaMember for LoggingMember {
    fn accept_gesture(&self, _: GestureContest) {
        self.log.borrow_mut().push("accept");
    }

    fn reject_gesture(&self, _: GestureContest) {
        self.log.borrow_mut().push("reject");
        (self.on_reject)();
    }
}

/// A binding whose first global router handler closes it on Down, and whose
/// arena holds a member for another, still-open contact.
struct ClosingBinding {
    binding: Rc<GestureBinding>,
    log: Rc<RefCell<Vec<&'static str>>>,
    _member: Rc<LoggingMember>,
}

impl ClosingBinding {
    /// Press a fresh contact, then log that the dispatch returned.
    fn press(&self) {
        self.binding.handle_pointer_event(
            &make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch).expect("down"),
            |_| HitTestResult::new(),
        );
        self.log.borrow_mut().push("returned");
    }

    fn log(&self) -> Vec<&'static str> {
        self.log.borrow().clone()
    }
}

fn closing_binding(on_reject: impl Fn(&Weak<GestureBinding>) + 'static) -> ClosingBinding {
    closing_binding_then(|| {}, on_reject)
}

/// [`closing_binding`] whose closing handler runs `after_close` once it closed.
fn closing_binding_then(
    after_close: impl Fn() + 'static,
    on_reject: impl Fn(&Weak<GestureBinding>) + 'static,
) -> ClosingBinding {
    let binding = Rc::new(GestureBinding::new());
    let log = Rc::new(RefCell::new(Vec::new()));
    let weak = Rc::downgrade(&binding);
    let member = Rc::new(LoggingMember {
        log: Rc::clone(&log),
        on_reject: Box::new(move || on_reject(&weak)),
    });
    let _entry = binding.arena().add(pointer(9), &member);
    let closer_log = Rc::clone(&log);
    let closer_binding = Rc::downgrade(&binding);
    let closer: PointerRouteHandler = Rc::new(move |event| {
        if matches!(event, PointerEvent::Down(_)) {
            let binding = closer_binding.upgrade().expect("binding");
            close_gestures(&binding, CloseMode::Ordinary);
            closer_log.borrow_mut().push("closed");
            after_close();
        }
    });
    binding.pointer_router().add_global_handler(closer);
    let later_log = Rc::clone(&log);
    let later: PointerRouteHandler = Rc::new(move |_| later_log.borrow_mut().push("later"));
    binding.pointer_router().add_global_handler(later);
    ClosingBinding {
        binding,
        log,
        _member: member,
    }
}

#[test]
fn a_closed_binding_drains_services_after_dispatch() {
    run_rows(&[
        (
            "a_close_from_a_callback_retires_after_the_dispatch",
            a_close_from_a_callback_retires_after_the_dispatch,
        ),
        (
            "a_drain_callback_that_dispatches_is_refused",
            a_drain_callback_that_dispatches_is_refused,
        ),
        (
            "a_drain_callback_that_closes_again_settles_once",
            a_drain_callback_that_closes_again_settles_once,
        ),
        (
            "a_callback_that_closes_then_panics_retires_preserving_its_failure",
            a_callback_that_closes_then_panics_retires_preserving_its_failure,
        ),
    ]);
}

fn a_callback_that_closes_then_panics_retires_preserving_its_failure() {
    let fixture = closing_binding_then(
        || std::panic::panic_any("closer failed"),
        |_| panic!("a preserving retirement runs no rejection"),
    );
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fixture.press()))
        .expect_err("the callback's failure reaches the dispatch's caller");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"closer failed"),
        "the first failure is the callback's, not the retirement's"
    );
    assert_eq!(
        fixture.log(),
        ["closed"],
        "neither the later handler nor the arena member ran after the failure"
    );
    let binding = &fixture.binding;
    assert_eq!(binding.active_pointer_count(), 0);
    assert!(binding.arena().is_empty(), "the arena retired its members");
    binding.handle_pointer_event(
        &make_down_event_for_id(pointer(2), Offset::ZERO, PointerKind::Touch).expect("down"),
        |_| panic!("a closed binding hit-tests nothing"),
    );
}

fn a_close_from_a_callback_retires_after_the_dispatch() {
    let fixture = closing_binding(|_| {});
    fixture.press();
    let binding = &fixture.binding;
    assert_eq!(
        fixture.log(),
        ["closed", "reject", "returned"],
        "the close withdrew the router at once and rejected the arena only after its handler returned"
    );
    assert_eq!(binding.active_pointer_count(), 0);
    assert!(binding.arena().is_empty());
}

fn a_drain_callback_that_dispatches_is_refused() {
    let fixture = closing_binding(|binding| {
        let binding = binding.upgrade().expect("binding");
        binding.handle_pointer_event(
            &make_down_event_for_id(pointer(2), Offset::ZERO, PointerKind::Touch).expect("down"),
            |_| panic!("a closed binding hit-tests nothing"),
        );
        assert_eq!(binding.flush_pending_moves(), 0);
    });
    fixture.press();
    assert_eq!(fixture.log(), ["closed", "reject", "returned"]);
    assert_eq!(fixture.binding.active_pointer_count(), 0);
}

fn a_drain_callback_that_closes_again_settles_once() {
    let fixture = closing_binding(|binding| {
        let binding = binding.upgrade().expect("binding");
        close_gestures(&binding, CloseMode::Ordinary);
    });
    fixture.press();
    assert_eq!(
        fixture.log(),
        ["closed", "reject", "returned"],
        "the reentrant close is settled after the drain, without a second rejection"
    );
    assert!(fixture.binding.arena().is_empty());
}

// ----------------------------------------------------------------------------
// Nested dispatch
// ----------------------------------------------------------------------------

/// A claim reached while the winner's target is delivered owes the rest of
/// the route its `Cancel(Claimed)` after that packet; a dispatch nested in
/// the winner's callback leaves the outer packet's delivery in order.
#[test]
fn a_nested_dispatch_leaves_the_outer_claim_to_the_outer_dispatch() {
    use flui_interaction::events::make_up_event_for_id;
    use flui_interaction::{DragAxis, DragGestureRecognizer, RecognizerSet};
    use flui_platform_api::pointer::CancelReason;

    fn name(event: &PointerEvent) -> &'static str {
        match event {
            PointerEvent::Down(_) => "down",
            PointerEvent::Move(_) => "move",
            PointerEvent::Up(_) => "up",
            PointerEvent::Cancel(cancel) if cancel.reason == CancelReason::Claimed => "claimed",
            PointerEvent::Cancel(_) => "cancel",
            _ => "other",
        }
    }

    let lane = InteractionLane::try_new().expect("lane");
    let binding = Rc::new(GestureBinding::new());
    let winner =
        DragGestureRecognizer::builder(binding.arena().clone(), DragAxis::Horizontal).build();
    // A rival, so the winner claims the contact instead of winning by default.
    let loser = DragGestureRecognizer::builder(binding.arena().clone(), DragAxis::Vertical).build();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let route = lane.enter(|| {
        let handle = lane.dispatch_handle();
        let mut set = RecognizerSet::default();
        set.attach(&winner);
        let nested = Rc::downgrade(&binding);
        let inner = handle
            .register_pointer(move |dispatch| {
                set.dispatch(dispatch);
                if matches!(dispatch.local, PointerEvent::Move(_)) {
                    // A hover of another mouse, dispatched from the callback.
                    let binding = nested.upgrade().expect("binding");
                    binding.handle_pointer_event(&hover(9, Offset::ZERO), |_| HitTestResult::new());
                }
            })
            .expect("winner target");
        let log = Rc::clone(&observed);
        let mut rival = RecognizerSet::default();
        rival.attach(&loser);
        let outer = handle
            .register_pointer(move |dispatch| {
                log.borrow_mut().push(name(dispatch.local));
                rival.dispatch(dispatch);
            })
            .expect("rival target");
        let mut route = HitTestResult::new();
        route.add(HitTestEntry::new(RenderId::new(1)).pointer_target(inner));
        route.add(HitTestEntry::new(RenderId::new(2)).pointer_target(outer));
        route
    });
    let id = pointer(4);
    let send = |event: PointerEvent| {
        lane.enter(|| {
            binding.handle_pointer_event(&event, |_| route.clone());
            binding.flush_pending_moves();
            binding.drain_deferred_arena_resolutions();
        });
    };
    send(make_down_event_for_id(id, Offset::ZERO, PointerKind::Touch).expect("down"));
    send(make_move_event_for_id(id, Offset::new(60.0, 0.0), PointerKind::Touch).expect("move"));
    send(make_up_event_for_id(id, Offset::new(60.0, 0.0), PointerKind::Touch).expect("up"));
    assert_eq!(
        *observed.borrow(),
        ["down", "move", "claimed"],
        "the observer heard the claiming Move before its Cancel(Claimed)"
    );
}
