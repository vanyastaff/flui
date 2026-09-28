//! End-to-end pointer routing: a `Listener` widget's callbacks fire for pointer
//! events that reach it through the real hit-test + dispatch path. Covers the
//! `HitTestBehavior` contract — `DeferToChild` (the default) fires only when a
//! descendant is hit, `Opaque` fires for any pointer within bounds.

use std::cell::Cell;
use std::rc::Rc;

use crate::common::{lay_out, size, tight};
use flui_interaction::PointerDispatch;
use flui_interaction::events::pointer::{
    PointerButtons, PointerGesture, PointerGestureEvent, PointerInfo, PointerState, PointerType,
    PointerUpdate,
};
use flui_types::Color;
use flui_types::{Offset, geometry::px};
use flui_view::EventCx;
use flui_widgets::prelude::HitTestBehavior;
use flui_widgets::{ColoredBox, Listener, PointerPanZoomEvent, SizedBox};

/// A counter callback + a readable handle.
fn counter() -> (
    Rc<Cell<usize>>,
    impl Fn(&mut EventCx<'_>, PointerDispatch<'_>) + 'static,
) {
    let count = Rc::new(Cell::new(0));
    let in_cb = Rc::clone(&count);
    (
        count,
        move |_cx: &mut EventCx<'_>, _event: PointerDispatch<'_>| {
            in_cb.set(in_cb.get() + 1);
        },
    )
}

fn pan_zoom_counter() -> (
    Rc<Cell<usize>>,
    impl Fn(&mut EventCx<'_>, &PointerPanZoomEvent) + 'static,
) {
    let count = Rc::new(Cell::new(0));
    let in_cb = Rc::clone(&count);
    (
        count,
        move |_cx: &mut EventCx<'_>, event: &PointerPanZoomEvent| {
            assert!(
                event.is_update(),
                "current FLUI PointerEvent::Gesture conversion should produce pan/zoom updates",
            );
            in_cb.set(in_cb.get() + 1);
        },
    )
}

#[test]
fn default_listener_fires_on_pointer_landing_on_a_hittable_child() {
    let (downs, on_down) = counter();

    // Default behavior is DeferToChild: a hittable `ColoredBox` child (fills the
    // 100×100 bounds, hit-tests true) lets the listener register and receive.
    let laid = lay_out(
        Listener::new()
            .on_pointer_down(on_down)
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    assert_eq!(laid.size(laid.root()), size(100.0, 100.0));

    laid.dispatch_pointer_down(20.0, 20.0);
    assert_eq!(
        downs.get(),
        1,
        "DeferToChild fires when the pointer lands on a hittable child",
    );
}

#[test]
fn default_listener_does_not_fire_without_a_hittable_target() {
    let (downs, on_down) = counter();

    // DeferToChild + a childless `SizedBox` (RenderConstrainedBox hit-tests
    // false when childless) → nothing under the listener is hit → the listener
    // does NOT register, so the callback never fires.
    let laid = lay_out(
        Listener::new()
            .on_pointer_down(on_down)
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(20.0, 20.0);
    assert_eq!(
        downs.get(),
        0,
        "DeferToChild must NOT fire when no descendant is hit",
    );
}

#[test]
fn opaque_listener_fires_within_bounds_even_without_a_hittable_child() {
    let (downs, on_down) = counter();

    // Opaque registers for any pointer within its own bounds, regardless of
    // whether a child was hit.
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(on_down)
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(20.0, 20.0);
    assert_eq!(downs.get(), 1, "Opaque fires for any pointer within bounds");
}

#[test]
fn listener_routes_down_and_up_to_their_own_callbacks() {
    let (downs, on_down) = counter();
    let (ups, on_up) = counter();

    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(on_down)
            .on_pointer_up(on_up)
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );

    laid.dispatch_pointer_down(40.0, 40.0);
    assert_eq!(downs.get(), 1, "down routes to on_pointer_down");
    assert_eq!(ups.get(), 0, "down does not invoke on_pointer_up");

    laid.dispatch_pointer_up(40.0, 40.0);
    assert_eq!(ups.get(), 1, "up routes to on_pointer_up");
    assert_eq!(downs.get(), 1, "up does not re-invoke on_pointer_down");
}

#[test]
fn listener_routes_buttonless_move_to_hover_callback() {
    let (moves, on_move) = counter();
    let (hovers, on_hover) = counter();

    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_move(on_move)
            .on_pointer_hover(on_hover)
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );

    let mut current = PointerState::default();
    current.position.x = 40.0;
    current.position.y = 40.0;
    current.buttons = PointerButtons::new();
    let event = flui_interaction::events::PointerEvent::Move(PointerUpdate {
        pointer: PointerInfo {
            pointer_id: Some(flui_interaction::PointerId::PRIMARY),
            pointer_type: PointerType::Mouse,
            persistent_device_id: None,
        },
        current,
        coalesced: Vec::new(),
        predicted: Vec::new(),
    });

    laid.dispatch_pointer_event(&event);

    assert_eq!(
        hovers.get(),
        1,
        "buttonless PointerEvent::Move should route to on_pointer_hover",
    );
    assert_eq!(
        moves.get(),
        0,
        "buttonless hover must not route to on_pointer_move",
    );

    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_move(40.0, 40.0);
    laid.dispatch_pointer_up(40.0, 40.0);
    assert_eq!(
        moves.get(),
        1,
        "pressed-button PointerEvent::Move should still route to on_pointer_move",
    );
}

#[test]
fn listener_routes_scroll_to_pointer_signal_callback() {
    let (signals, on_signal) = counter();

    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_signal(on_signal)
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );

    let position = Offset::new(40.0, 40.0);
    let event = flui_interaction::events::make_scroll_event(position, Offset::new(0.0, 12.0));

    laid.dispatch_pointer_event(&event);

    assert_eq!(
        signals.get(),
        1,
        "scroll events are FLUI's concrete pointer-signal payload",
    );
}

#[test]
fn listener_routes_gesture_to_pan_zoom_update_callback() {
    let (updates, on_update) = pan_zoom_counter();

    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_pan_zoom_update(on_update)
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );

    let mut state = PointerState::default();
    state.position.x = 40.0;
    state.position.y = 40.0;
    let event = flui_interaction::events::PointerEvent::Gesture(PointerGestureEvent {
        pointer: PointerInfo {
            pointer_id: Some(flui_interaction::PointerId::PRIMARY),
            pointer_type: PointerType::Touch,
            persistent_device_id: None,
        },
        gesture: PointerGesture::Pinch(0.25),
        state,
    });

    laid.dispatch_pointer_event(&event);

    assert_eq!(
        updates.get(),
        1,
        "PointerEvent::Gesture should route through Listener's pan/zoom update callback",
    );
}

// ============================================================================
// Event context (ADR-0086): the listener takes the owner's writer source
// from its render-object context and opens one write per event.
// ============================================================================

mod event_cx {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::common::{ProbeSignals, SignalProbe, lay_out, tight};
    use flui_types::Color;
    use flui_view::SignalWriteExt;
    use flui_widgets::{ColoredBox, Listener};

    fn target() -> ColoredBox {
        ColoredBox::new(Color::rgb(10, 20, 30))
    }

    #[test]
    fn a_pointer_down_writes_a_signal_and_rebuilds_its_reader() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            Listener::new()
                .on_pointer_down(move |cx, _dispatch| count.update(cx, |n| *n += 1))
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));

        app.dispatch_pointer_down(50.0, 50.0);
        app.dispatch_pointer_up(50.0, 50.0);
        assert_eq!(probe.value(), Ok(1));
        app.tick();

        assert_eq!(probe.reads(), [0, 1], "the reader rebuilt once");
    }

    /// A rebuild replaces the handler inside the registered target; the new
    /// handler still opens its write from the owner's source.
    #[test]
    fn a_callback_replaced_on_rebuild_writes_through_the_update_path() {
        let step = Rc::new(Cell::new(1_u32));
        let step_in_build = Rc::clone(&step);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            let by = step_in_build.get();
            Listener::new()
                .on_pointer_down(move |cx, _dispatch| count.update(cx, |n| *n += by))
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));
        app.dispatch_pointer_down(50.0, 50.0);
        app.dispatch_pointer_up(50.0, 50.0);
        assert_eq!(probe.value(), Ok(1));

        step.set(10);
        app.pump();
        app.dispatch_pointer_down(50.0, 50.0);
        app.dispatch_pointer_up(50.0, 50.0);

        assert_eq!(probe.value(), Ok(11), "the rebuilt handler ran");
    }

    #[test]
    fn a_refused_write_in_a_pointer_callback_is_reported_not_panicked() {
        let probe = SignalProbe::new(|ProbeSignals { released, .. }| {
            Listener::new()
                .on_pointer_down(move |cx, _dispatch| released.set(cx, 1))
                .child(target())
        });
        let app = lay_out(probe.view(), tight(100.0, 100.0));

        let ((), log) = flui_testing::log_capture::capture(|| {
            app.dispatch_pointer_down(50.0, 50.0);
            app.dispatch_pointer_up(50.0, 50.0);
        });

        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        assert_eq!(probe.value(), Ok(0));
    }
}
