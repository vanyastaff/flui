//! End-to-end pointer routing: a `Listener` widget's callbacks fire for pointer
//! events that reach it through the real hit-test + dispatch path. Covers the
//! `HitTestBehavior` contract — `DeferToChild` (the default) fires only when a
//! descendant is hit, `Opaque` fires for any pointer within bounds.

use std::cell::Cell;
use std::rc::Rc;

use crate::common::{lay_out, tight};
use flui_interaction::PointerDispatch;
use flui_view::EventCx;
use flui_widgets::prelude::HitTestBehavior;
use flui_widgets::{Listener, SizedBox};

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

pub(crate) fn listener_routes_down_and_up_to_their_own_callbacks() {
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

pub(crate) fn listener_admission_keeps_terminal_delivery_and_weak_ownership() {
    use std::cell::RefCell;
    use flui_interaction::{CancelOutcome, GestureArenaMember, GestureRecognizer, PointerId};

    struct ContactObserver(Rc<RefCell<Vec<&'static str>>>);
    impl GestureArenaMember for ContactObserver {
        fn accept_gesture(&self, _: PointerId) {}
        fn reject_gesture(&self, _: PointerId) {}
    }
    impl GestureRecognizer for ContactObserver {
        fn add_pointer(&self, _: PointerDispatch<'_>) {
            self.0.borrow_mut().push("down");
        }
        fn handle_event(&self, dispatch: PointerDispatch<'_>) {
            use flui_rendering::hit_testing::PointerEvent;
            self.0.borrow_mut().push(match dispatch.local {
                PointerEvent::Up(_) => "up",
                PointerEvent::Move(_) => "move",
                _ => "other",
            });
        }
        fn cancel(&self) -> CancelOutcome { CancelOutcome::Idle }
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let recognizer = Rc::new(ContactObserver(Rc::clone(&events)));
    let admitted = Rc::new(Cell::new(true));
    let predicate = Rc::clone(&admitted);
    let raw = Rc::clone(&events);
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(move |_, _| raw.borrow_mut().push("raw"))
            .recognizer_when(&recognizer, move |_| predicate.get())
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );
    laid.dispatch_pointer_down(40.0, 40.0);
    admitted.set(false);
    laid.dispatch_pointer_move(41.0, 40.0);
    laid.dispatch_pointer_up(41.0, 40.0);
    assert_eq!(&*events.borrow(), &["raw", "down", "move", "up"]);
    events.borrow_mut().clear();
    drop(recognizer);
    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_up(40.0, 40.0);
    assert_eq!(&*events.borrow(), &["raw"], "the cached handler owns no recognizer");
}

// ============================================================================
// Event context (ADR-0086): the listener takes the owner's writer source
// from its render-object context and opens one write per event.
// ============================================================================

pub(crate) mod event_cx {

    use crate::common::{ProbeSignals, SignalProbe, lay_out, tight};
    use flui_painting::styling::Color;
    use flui_view::SignalWriteExt;
    use flui_widgets::{ColoredBox, Listener};

    fn target() -> ColoredBox {
        ColoredBox::new(Color::rgb(10, 20, 30))
    }

    pub(crate) fn a_refused_write_in_a_pointer_callback_is_reported_not_panicked() {
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
