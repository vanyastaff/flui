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

/// The hosted frame's manual time, not wall time, selects the measured
/// intermediate point; the default policy retains its immediate harness path.
pub(crate) fn presentation_resampling_uses_the_owner_frame_clock() {
    use flui_platform_api::pointer::{
        PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
        PointerMove, PointerPosition, PointerPress, PointerRelease, PointerSample,
    };
    use flui_platform_api::{EventTime, PlatformInput};
    use flui_testing::{HeadlessHost, HeadlessWindow, PointerResampling};
    use std::cell::RefCell;
    use std::time::Duration;
    for enabled in [false, true] {
        let observed = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&observed);
        let terminal = Rc::new(Cell::new(0));
        let ends = Rc::clone(&terminal);
        let view = Listener::new()
            .child(SizedBox::square(200.0))
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_move(move |_, event| {
                let PointerEvent::Move(event) = event.global else {
                    panic!("Move")
                };
                seen.borrow_mut().push((
                    event.current().position.get().x,
                    event.current().time.as_nanos(),
                ));
            })
            .on_pointer_up(move |_, _| ends.set(ends.get() + 1));
        let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
        if enabled {
            host.set_pointer_resampling(host.primary_window(), PointerResampling::FrameAligned)
                .expect("idle presentation admits opt-in");
        }
        host.attach(&view).expect("mount Listener");
        let _ = host.pump(Duration::from_millis(16));
        let pointer = PointerInfo::new(
            PointerId::try_from(1_u64).expect("nonzero contact"),
            PointerKind::Touch,
        );
        let sample = |x, ms: u64| {
            PointerSample::new(
                EventTime::from_nanos(ms * 1_000_000),
                PointerPosition::try_new(flui_foundation::geometry::Point::new(x, 10.0))
                    .expect("finite measured position"),
            )
        };
        host.dispatch(PlatformInput::Pointer(PointerEvent::Down(
            PointerPress::new(
                pointer,
                PointerButton::PRIMARY,
                PointerButtons::only(PointerButton::PRIMARY),
                sample(10.0, 0),
            ),
        )));
        if enabled {
            assert!(
                host.set_pointer_resampling(host.primary_window(), PointerResampling::Disabled)
                    .is_err(),
                "an active sequence keeps its admitted policy"
            );
            host.set_pointer_resampling(host.primary_window(), PointerResampling::FrameAligned)
                .expect("idempotent policy is allowed");
        }
        host.dispatch(PlatformInput::Pointer(PointerEvent::Move(
            PointerMove::new(
                pointer,
                PointerButtons::only(PointerButton::PRIMARY),
                sample(10.0, 0),
            ),
        )));
        host.clock().advance(Duration::from_millis(100));
        host.dispatch(PlatformInput::Pointer(PointerEvent::Move(
            PointerMove::new(
                pointer,
                PointerButtons::only(PointerButton::PRIMARY),
                sample(110.0, 100),
            ),
        )));
        if enabled {
            assert!(
                observed.borrow().is_empty(),
                "resampling waits for the owner's frame"
            );
            let _ = host.pump(Duration::ZERO);
            let emitted = observed.borrow();
            assert_eq!(emitted.len(), 2);
            assert_eq!(emitted[0], (10.0, 0));
            assert!(
                (emitted[1].0 - 72.0).abs() < 1e-9,
                "38 ms lookback samples 62 percent of the measured segment: {emitted:?}"
            );
            assert_eq!(emitted[1].1, 62_000_000);
            drop(emitted);
            let _ = host.pump(Duration::from_millis(38));
            assert_eq!(
                observed.borrow().last(),
                Some(&(110.0, 100_000_000)),
                "accepted sample debt survives to the next owner frame"
            );
        } else {
            assert_eq!(&*observed.borrow(), &[(10.0, 0), (110.0, 100_000_000)]);
        }
        host.dispatch(PlatformInput::Pointer(PointerEvent::Up(
            PointerRelease::new(
                pointer,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample(110.0, 101),
            ),
        )));
        assert_eq!(terminal.get(), 1, "Up remains synchronous");
        host.set_pointer_resampling(host.primary_window(), PointerResampling::Disabled)
            .expect("terminal releases policy admission");
    }
}

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

pub(crate) fn listener_capture_retains_one_target_and_drop_delivers_loss() {
    use flui_interaction::PointerCapture;
    use flui_platform_api::pointer::{CancelReason, PointerEvent};
    use std::cell::RefCell;

    let token = Rc::new(RefCell::new(None::<PointerCapture>));
    let held = token.clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let child_down = log.clone();
    let child_move = log.clone();
    let child_cancel = log.clone();
    let parent_down = log.clone();
    let parent_move = log.clone();
    let parent_cancel = log.clone();
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(move |_, dispatch| {
                parent_down.borrow_mut().push("parent down");
                assert!(dispatch.capture().is_err(), "nested child claimed first");
            })
            .on_pointer_move(move |_, _| parent_move.borrow_mut().push("parent move"))
            .on_pointer_cancel(move |_, _| parent_cancel.borrow_mut().push("parent cancel"))
            .child(
                Listener::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_pointer_down(move |_, dispatch| {
                        child_down.borrow_mut().push("child down");
                        let capture = dispatch.capture().expect("actual Listener Down authority");
                        *held.borrow_mut() = Some(capture);
                    })
                    .on_pointer_move(move |_, _| child_move.borrow_mut().push("child move"))
                    .on_pointer_cancel(move |_, dispatch| {
                        let PointerEvent::Cancel(cancel) = dispatch.global else {
                            panic!("cancel callback");
                        };
                        assert_eq!(cancel.reason, CancelReason::CaptureLost);
                        child_cancel.borrow_mut().push("child cancel");
                    })
                    .child(SizedBox::new(80.0, 80.0)),
            ),
        tight(80.0, 80.0),
    );
    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_move(200.0, 200.0);
    assert_eq!(&*log.borrow(), &["child down", "parent down", "child move"]);
    let released = token.borrow_mut().take().expect("retained token");
    drop(released);
    assert_eq!(
        log.borrow().last(),
        Some(&"child move"),
        "Drop invokes no event callback"
    );
    laid.dispatch_pointer_up(200.0, 200.0);
    assert_eq!(
        &*log.borrow(),
        &["child down", "parent down", "child move", "child cancel"]
    );
    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_up(40.0, 40.0);
    let terminal_token = token.borrow_mut().take().expect("next contact token");
    drop(terminal_token);
    assert_eq!(
        log.borrow()
            .iter()
            .filter(|event| **event == "child cancel")
            .count(),
        1,
        "terminal invalidates retained capture authority"
    );
}

pub(crate) fn listener_unmount_preserves_one_captured_contact_terminal() {
    use flui_interaction::PointerCapture;
    use flui_platform_api::pointer::{CancelReason, PointerEvent};
    use std::cell::RefCell;

    let token = Rc::new(RefCell::new(None::<PointerCapture>));
    let held = token.clone();
    let callbacks = Rc::new(RefCell::new(Vec::new()));
    let moved = callbacks.clone();
    let cancelled = callbacks.clone();
    let stage = Rc::new(Cell::new("down"));
    let move_stage = stage.clone();
    let cancel_stage = stage.clone();
    let down_time = Rc::new(Cell::new(None));
    let captured_time = down_time.clone();
    let mut laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(move |_, dispatch| {
                let PointerEvent::Down(press) = dispatch.global else {
                    panic!("Down callback");
                };
                captured_time.set(Some(press.sample.time));
                *held.borrow_mut() = Some(dispatch.capture().expect("mounted Down authority"));
            })
            .on_pointer_move(move |_, dispatch| {
                moved
                    .borrow_mut()
                    .push((move_stage.get(), dispatch.global.clone()));
            })
            .on_pointer_cancel(move |_, dispatch| {
                cancelled
                    .borrow_mut()
                    .push((cancel_stage.get(), dispatch.global.clone()));
            })
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );
    laid.dispatch_pointer_down(40.0, 40.0);
    stage.set("unmount");
    laid.pump_widget(SizedBox::new(80.0, 80.0));
    assert!(
        callbacks.borrow().is_empty(),
        "unmount withdraws future admission; observed {:?}",
        callbacks.borrow()
    );
    let retired_capture = token
        .borrow_mut()
        .take()
        .expect("capture outlives its widget");
    stage.set("release");
    drop(retired_capture);
    assert!(
        callbacks.borrow().is_empty(),
        "release invokes no event callback; observed {:?}",
        callbacks.borrow()
    );
    stage.set("next motion");
    laid.dispatch_pointer_move(200.0, 200.0);
    stage.set("native terminal");
    laid.dispatch_pointer_up(200.0, 200.0);
    {
        let observed = callbacks.borrow();
        assert_eq!(
            observed.len(),
            1,
            "cached contact receives one terminal, no released Move: {observed:?}"
        );
        let (delivery_stage, PointerEvent::Cancel(cancel)) = &observed[0] else {
            panic!("cached contact owes CaptureLost cleanup: {observed:?}");
        };
        assert_eq!(*delivery_stage, "next motion");
        assert_eq!(cancel.reason, CancelReason::CaptureLost);
        assert_eq!(
            Some(cancel.time),
            down_time.get(),
            "loss preserves the accepted contact timestamp"
        );
    }

    let new_contacts = Rc::new(Cell::new(0));
    let new_down = new_contacts.clone();
    laid.pump_widget(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(move |_, dispatch| {
                let capture = dispatch
                    .capture()
                    .expect("replacement Listener captures fresh contact");
                new_down.set(new_down.get() + 1);
                drop(capture);
            })
            .child(SizedBox::new(80.0, 80.0)),
    );
    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_up(40.0, 40.0);
    assert_eq!(
        new_contacts.get(),
        1,
        "replacement tree admits a fresh captured contact"
    );
    assert_eq!(
        callbacks.borrow().len(),
        1,
        "new contact cannot redeliver old terminal"
    );
}

pub(crate) fn listener_admission_keeps_terminal_delivery_and_weak_ownership() {
    use flui_interaction::{CancelOutcome, GestureArenaMember, GestureRecognizer, PointerId};
    use std::cell::RefCell;

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
        fn cancel(&self) -> CancelOutcome {
            CancelOutcome::Idle
        }
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
    assert_eq!(
        &*events.borrow(),
        &["raw"],
        "the cached handler owns no recognizer"
    );
}

pub(crate) fn listener_raw_observer_panic_still_delivers_the_recognizer_event() {
    use flui_interaction::{CancelOutcome, GestureArenaMember, GestureRecognizer, PointerId};
    struct Observer(Rc<Cell<usize>>);
    impl GestureArenaMember for Observer {
        fn accept_gesture(&self, _: PointerId) {}
        fn reject_gesture(&self, _: PointerId) {}
    }
    impl GestureRecognizer for Observer {
        fn add_pointer(&self, _: PointerDispatch<'_>) {
            self.0.set(self.0.get() + 1);
        }
        fn handle_event(&self, _: PointerDispatch<'_>) {}
        fn cancel(&self) -> CancelOutcome {
            CancelOutcome::Idle
        }
    }
    let delivered = Rc::new(Cell::new(0));
    let recognizer = Rc::new(Observer(Rc::clone(&delivered)));
    let first = Rc::new(Cell::new(true));
    let raw = Rc::clone(&first);
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(move |_, _| {
                if raw.replace(false) {
                    panic!("raw observer first failure");
                }
            })
            .recognizer(&recognizer)
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        laid.dispatch_pointer_down(40.0, 40.0);
    }))
    .expect_err("the raw callback's first panic leaves dispatch");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"raw observer first failure")
    );
    assert_eq!(
        delivered.get(),
        1,
        "accepted dispatch reaches attachments despite the observer failure"
    );
    laid.dispatch_pointer_up(40.0, 40.0);
    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_up(40.0, 40.0);
    assert_eq!(delivered.get(), 2, "the next contact remains deliverable");
}

pub(crate) fn custom_recognizer_competes_through_a_listener() {
    use flui_interaction::{
        ArenaMembership, CancelOutcome, GestureArenaMember, GestureRecognizer, GestureSettings,
        PointerId, PrimaryContact, TapGestureRecognizer,
    };
    use flui_rendering::hit_testing::PointerEvent;
    use flui_view::prelude::*;
    use flui_widgets::GestureArenaScope;
    use std::cell::RefCell;

    struct ExternalRecognizer {
        contact: PrimaryContact,
        events: Rc<RefCell<Vec<&'static str>>>,
    }
    impl GestureArenaMember for ExternalRecognizer {
        fn accept_gesture(&self, _: PointerId) {
            self.events.borrow_mut().push("custom won");
        }
        fn reject_gesture(&self, _: PointerId) {
            self.contact.finish();
        }
    }
    impl GestureRecognizer for ExternalRecognizer {
        fn add_pointer(&self, down: PointerDispatch<'_>) {
            self.contact
                .begin(down, &GestureSettings::default())
                .expect("new contact");
        }
        fn handle_event(&self, dispatch: PointerDispatch<'_>) {
            match dispatch.local {
                PointerEvent::Move(_) => self.contact.accept(),
                PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                    self.contact.finish();
                }
                _ => {}
            }
        }
        fn cancel(&self) -> CancelOutcome {
            if self.contact.withdraw().is_some() {
                CancelOutcome::Cancelled
            } else {
                CancelOutcome::Idle
            }
        }
    }
    #[derive(Clone)]
    struct Attached(Rc<RefCell<Vec<&'static str>>>);
    impl View for Attached {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    struct State {
        events: Rc<RefCell<Vec<&'static str>>>,
        custom: Option<Rc<ExternalRecognizer>>,
        tap: Option<Rc<TapGestureRecognizer>>,
    }
    impl StatefulView for Attached {
        type State = State;
        fn create_state(&self) -> State {
            State {
                events: Rc::clone(&self.0),
                custom: None,
                tap: None,
            }
        }
    }
    impl ViewState<Attached> for State {
        fn init_state(&mut self, ctx: &dyn LifecycleContext) {
            let arena = GestureArenaScope::of(ctx);
            let custom_events = Rc::clone(&self.events);
            self.custom = Some(Rc::new_cyclic(
                |this: &std::rc::Weak<ExternalRecognizer>| ExternalRecognizer {
                    contact: PrimaryContact::new(ArenaMembership::new(arena.clone(), this.clone())),
                    events: custom_events,
                },
            ));
            let tap_events = Rc::clone(&self.events);
            self.tap = Some(
                TapGestureRecognizer::builder(arena)
                    .on_tap(move |_| tap_events.borrow_mut().push("tap won"))
                    .build(),
            );
        }
        fn build(&self, _: &Attached, _: &dyn BuildContext) -> impl IntoView {
            Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .recognizer(self.tap.as_ref().expect("mounted tap"))
                .recognizer(self.custom.as_ref().expect("mounted custom recognizer"))
                .child(SizedBox::new(80.0, 80.0))
        }
    }
    let events = Rc::new(RefCell::new(Vec::new()));
    let laid = lay_out(Attached(Rc::clone(&events)), tight(80.0, 80.0));
    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_move(45.0, 40.0);
    laid.dispatch_pointer_up(45.0, 40.0);
    assert_eq!(
        &*events.borrow(),
        &["custom won"],
        "the external recognizer defeats the built-in tap through the presentation arena"
    );
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
