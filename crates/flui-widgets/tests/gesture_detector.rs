//! End-to-end gesture recognition: a `GestureDetector`'s `on_tap` fires when the
//! user taps (pointer down then up) its child. Drives the real recognizer +
//! presentation-owned arena through the hit-test + binding dispatch path. The
//! binding alone closes and sweeps the shared arena.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{lay_out, tight};
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, GestureDetector};

pub(crate) fn nested_native_scale_loser_recovers_touch_after_winner_terminal() {
    use flui_foundation::geometry::{Offset, Point};
    use flui_interaction::events::{
        make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_platform_api::{
        EventTime,
        pointer::{
            PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerEvent, PointerId, PointerInfo,
            PointerKind, PointerPosition,
        },
    };
    use flui_widgets::{Center, HitTestBehavior, SizedBox};
    use std::{cell::RefCell, rc::Rc};

    for terminal in [PanZoomPhase::End, PanZoomPhase::Cancelled] {
        let outer = Rc::new(RefCell::new(Vec::new()));
        let inner = Rc::new(RefCell::new(Vec::new()));
        let (outer_start, outer_update, outer_end, outer_cancel) =
            (outer.clone(), outer.clone(), outer.clone(), outer.clone());
        let (inner_start, inner_update, inner_end, inner_cancel) =
            (inner.clone(), inner.clone(), inner.clone(), inner.clone());
        let laid = lay_out(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_scale_start(move |_, _| outer_start.borrow_mut().push("start"))
                .on_scale_update(move |_, _| outer_update.borrow_mut().push("update"))
                .on_scale_end(move |_, _| outer_end.borrow_mut().push("end"))
                .on_scale_cancel(move |_| outer_cancel.borrow_mut().push("cancel"))
                .child(
                    Center::new().child(
                        SizedBox::new(100.0, 100.0).child(
                            GestureDetector::new()
                                .on_scale_start(move |_, _| inner_start.borrow_mut().push("start"))
                                .on_scale_update(move |_, _| {
                                    inner_update.borrow_mut().push("update");
                                })
                                .on_scale_end(move |_, _| inner_end.borrow_mut().push("end"))
                                .on_scale_cancel(move |_| inner_cancel.borrow_mut().push("cancel"))
                                .child(ColoredBox::new(Color::RED)),
                        ),
                    ),
                ),
            tight(200.0, 200.0),
        );
        let source = PointerInfo::new(
            PointerId::try_from(500_u64).expect("nonzero native source"),
            PointerKind::Mouse,
        );
        let send = |time, phase| {
            laid.dispatch_pointer_event(&PointerEvent::PanZoom(PanZoomEvent::new(
                source,
                EventTime::from_nanos(time),
                PointerPosition::try_new(Point::new(100.0, 100.0)).expect("finite focal point"),
                phase,
            )));
        };
        let zoom = |scale| {
            PanZoomPhase::Update(
                PanZoomTransform::try_new(Offset::ZERO, scale, 0.0).expect("finite native scale"),
            )
        };
        for round in 0_u64..2 {
            inner.borrow_mut().clear();
            outer.borrow_mut().clear();
            send(round * 100, PanZoomPhase::Start);
            send(round * 100 + 10, zoom(1.2));
            send(round * 100 + 20, zoom(1.5));
            send(round * 100 + 30, terminal);
            assert_eq!(
                inner.borrow().as_slice(),
                [
                    "start",
                    "update",
                    "update",
                    if terminal == PanZoomPhase::End {
                        "end"
                    } else {
                        "cancel"
                    }
                ],
                "the exact leaf winner owns all updates and its terminal"
            );
            assert!(
                outer.borrow().is_empty(),
                "a dormant losing ancestor publishes no native callbacks"
            );
            let first = PointerId::try_from(501_u64).expect("nonzero first touch");
            let second = PointerId::try_from(502_u64).expect("nonzero second touch");
            // Both contacts hit only the ancestor, above the centered leaf.
            for (pointer, x) in [(first, 20.0), (second, 80.0)] {
                laid.dispatch_pointer_event(
                    &make_down_event_for_id(pointer, Offset::new(x, 20.0), PointerKind::Touch)
                        .expect("finite touch Down"),
                );
            }
            laid.dispatch_pointer_event(
                &make_move_event_for_id(second, Offset::new(180.0, 20.0), PointerKind::Touch)
                    .expect("finite touch Move"),
            );
            assert_eq!(
                outer.borrow().first().copied(),
                Some("start"),
                "losing native admission must not block a later two-contact gesture"
            );
            for (pointer, x) in [(first, 20.0), (second, 180.0)] {
                laid.dispatch_pointer_event(
                    &make_up_event_for_id(pointer, Offset::new(x, 20.0), PointerKind::Touch)
                        .expect("finite touch Up"),
                );
            }
            assert_eq!(
                outer
                    .borrow()
                    .iter()
                    .filter(|event| **event == "end")
                    .count(),
                1
            );
            assert_eq!(
                inner.borrow().len(),
                4,
                "touch recovery cannot alter the native winner"
            );
        }
    }
}

pub(crate) fn exclusive_drag_callbacks_have_one_arena_winner() {
    use std::{cell::RefCell, rc::Rc};
    let calls = Rc::new(RefCell::new(Vec::new()));
    let (pan_start, pan_end, horizontal_start, horizontal_end, horizontal_cancel) = (
        Rc::clone(&calls),
        Rc::clone(&calls),
        Rc::clone(&calls),
        Rc::clone(&calls),
        Rc::clone(&calls),
    );
    let scoped = lay_out(
        GestureDetector::new()
            .exclusive_drags()
            .on_pan_start(move |_, _| pan_start.borrow_mut().push("pan start"))
            .on_pan_end(move |_, _| pan_end.borrow_mut().push("pan end"))
            .on_horizontal_drag_start(move |_, _| {
                horizontal_start.borrow_mut().push("horizontal start");
            })
            .on_horizontal_drag_end(move |_, _| horizontal_end.borrow_mut().push("horizontal end"))
            .on_horizontal_drag_cancel(move |_| {
                horizontal_cancel.borrow_mut().push("horizontal cancel");
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );
    scoped.dispatch_pointer_down(10.0, 50.0);
    scoped.dispatch_pointer_move(50.0, 50.0);
    scoped.dispatch_pointer_up(60.0, 50.0);
    assert_eq!(
        calls.borrow().as_slice(),
        ["horizontal cancel", "pan start", "pan end"]
    );
}

#[derive(Clone, flui_view::prelude::StatefulView)]
struct ComposedDetector {
    detector: GestureDetector,
}

struct ComposedDetectorState {
    branch: Option<flui_interaction::GestureArena>,
}

impl flui_view::StatefulView for ComposedDetector {
    type State = ComposedDetectorState;
    fn create_state(&self) -> Self::State {
        ComposedDetectorState { branch: None }
    }
}

impl flui_view::ViewState<ComposedDetector> for ComposedDetectorState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        let arena = flui_widgets::GestureArenaScope::of(ctx);
        let (first, _) = arena
            .compose(flui_interaction::arena::GestureCompetition::Exclusive)
            .expect("presentation root")
            .into_branches();
        self.branch = Some(first);
    }
    fn build(
        &self,
        view: &ComposedDetector,
        _: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        flui_widgets::GestureArenaScope::new(
            self.branch.as_ref().expect("mounted branch").clone(),
            view.detector.clone(),
        )
    }
}

pub(crate) fn a_detector_in_a_composed_scope_preserves_double_tap_timing() {
    use std::time::Duration;
    for double in [false, true] {
        let taps = Arc::new(AtomicUsize::new(0));
        let doubles = Arc::new(AtomicUsize::new(0));
        let (tap, double_tap) = (Arc::clone(&taps), Arc::clone(&doubles));
        let mut scoped = lay_out(
            ComposedDetector {
                detector: GestureDetector::new()
                    .on_tap(move |_| {
                        tap.fetch_add(1, Ordering::SeqCst);
                    })
                    .on_double_tap(move |_| {
                        double_tap.fetch_add(1, Ordering::SeqCst);
                    })
                    .child(ColoredBox::new(Color::rgb(10, 20, 30))),
            },
            tight(100.0, 100.0),
        );
        scoped.dispatch_pointer_down(50.0, 50.0);
        scoped.dispatch_pointer_up(50.0, 50.0);
        scoped.pump_for(Duration::from_millis(50));
        assert_eq!(taps.load(Ordering::SeqCst), 0);
        if double {
            scoped.dispatch_pointer_down(50.0, 50.0);
            scoped.dispatch_pointer_up(50.0, 50.0);
        }
        scoped.pump_for(Duration::from_millis(400));
        assert_eq!(taps.load(Ordering::SeqCst), usize::from(!double));
        assert_eq!(doubles.load(Ordering::SeqCst), usize::from(double));
    }
}

#[derive(Clone, flui_view::prelude::StatefulView)]
struct ConfiguredGesture {
    settings: flui_interaction::GestureSettings,
    detector: GestureDetector,
}

struct ConfiguredGestureState {
    arena: Option<flui_interaction::GestureArena>,
}

impl flui_view::StatefulView for ConfiguredGesture {
    type State = ConfiguredGestureState;

    fn create_state(&self) -> Self::State {
        ConfiguredGestureState { arena: None }
    }
}

impl flui_view::ViewState<ConfiguredGesture> for ConfiguredGestureState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.arena = Some(flui_widgets::GestureArenaScope::of(ctx));
    }

    fn build(
        &self,
        view: &ConfiguredGesture,
        _: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        flui_widgets::GestureArenaScope::new(
            self.arena
                .as_ref()
                .expect("mounted presentation arena")
                .clone(),
            view.detector.clone(),
        )
        .settings(view.settings.clone())
    }
}

pub(crate) fn scoped_estimator_controls_delivered_drag_velocity() {
    use std::{cell::Cell, rc::Rc};

    use flui_foundation::geometry::Point;
    use flui_interaction::{GestureSettings, processing::VelocityEstimator};
    use flui_platform_api::{
        EventTime,
        pointer::{
            PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
            PointerMove, PointerPosition, PointerPress, PointerRelease, PointerSample,
        },
    };

    // Four samples 10 ms apart at x = 10, 40, 60, 70 distinguish all four
    // algorithms: an exactly quadratic deceleration has terminal LSQ slope
    // 500 px/s; the weighted recent intervals give 2550/1950 px/s, and the
    // impulse integration gives 1589.9257985831982 px/s. A constant-speed
    // trace would pass even if the configured strategy were ignored.
    for (estimator, expected) in [
        (VelocityEstimator::LeastSquares, 500.0),
        (VelocityEstimator::Impulse, 1_589.925_798_583_198_2),
        (VelocityEstimator::Ios, 2550.0),
        (VelocityEstimator::Macos, 1950.0),
    ] {
        for kind in [PointerKind::Touch, PointerKind::Mouse] {
            for horizontal in [false, true] {
                let delivered = Rc::new(Cell::new(None));
                let observed = Rc::clone(&delivered);
                let detector = if horizontal {
                    GestureDetector::new().on_horizontal_drag_end(move |_, details| {
                        assert!(
                            observed
                                .replace(Some(details.velocity.pixels_per_second.dx))
                                .is_none()
                        );
                    })
                } else {
                    GestureDetector::new().on_pan_end(move |_, details| {
                        assert!(
                            observed
                                .replace(Some(details.velocity.pixels_per_second.dx))
                                .is_none()
                        );
                    })
                }
                .child(ColoredBox::new(Color::rgb(10, 20, 30)));
                let laid = lay_out(
                    ConfiguredGesture {
                        settings: GestureSettings::default().with_velocity_estimator(estimator),
                        detector,
                    },
                    tight(150.0, 100.0),
                );
                let info =
                    PointerInfo::new(PointerId::try_from(1_u64).expect("authored contact"), kind);
                let sample = |millis: u64, x| {
                    PointerSample::new(
                        EventTime::from_nanos(millis * 1_000_000),
                        PointerPosition::try_new(Point::new(x, 40.0)).expect("finite position"),
                    )
                };
                let held = PointerButtons::NONE.with(PointerButton::PRIMARY);
                laid.dispatch_pointer_event(&PointerEvent::Down(PointerPress::new(
                    info,
                    PointerButton::PRIMARY,
                    PointerButtons::NONE,
                    sample(0, 10.0),
                )));
                laid.dispatch_pointer_event(&PointerEvent::Move(
                    PointerMove::new(info, held, sample(30, 70.0))
                        .with_coalesced(vec![sample(10, 40.0), sample(20, 60.0)]),
                ));
                laid.dispatch_pointer_event(&PointerEvent::Up(PointerRelease::new(
                    info,
                    PointerButton::PRIMARY,
                    held,
                    sample(30, 70.0),
                )));
                let actual = delivered
                    .get()
                    .expect("completed drag delivers an end callback");
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "{estimator:?}, {kind:?}, horizontal={horizontal}: got {actual}, expected {expected}"
                );
            }
        }
    }
}

pub(crate) fn scoped_settings_control_touch_recognition_thresholds() {
    use std::{cell::Cell, rc::Rc};

    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{GestureSettings, PointerId};

    for family in ["tap", "pan", "horizontal drag"] {
        for configured in [false, true] {
            let callbacks = Rc::new(Cell::new(0));
            let observed = Rc::clone(&callbacks);
            let competing_taps = Rc::new(Cell::new(0));
            let detector = match family {
                "tap" => GestureDetector::new().on_tap(move |_| observed.set(observed.get() + 1)),
                "pan" => GestureDetector::new()
                    .on_pan_start(move |_, _| observed.set(observed.get() + 1)),
                _ => GestureDetector::new()
                    .on_horizontal_drag_start(move |_, _| observed.set(observed.get() + 1)),
            };
            // A sole arena member is accepted by default before its slop
            // decision. A competing tap keeps drag admission undecided until
            // movement distinguishes the configured threshold.
            let detector = if family == "tap" {
                detector
            } else {
                let taps = Rc::clone(&competing_taps);
                detector.on_tap(move |_| taps.set(taps.get() + 1))
            }
            .child(ColoredBox::new(Color::rgb(10, 20, 30)));
            let settings = if configured {
                GestureSettings::default()
                    .try_with_touch_slop(60.0)
                    .expect("valid touch slop")
                    .try_with_pan_slop(10.0)
                    .expect("valid pan slop")
                    .try_with_pan_slop_horizontal(10.0)
                    .expect("valid horizontal slop")
            } else {
                GestureSettings::default()
            };
            let laid = lay_out(
                ConfiguredGesture { settings, detector },
                tight(150.0, 100.0),
            );
            let pointer = PointerId::try_from(1_u64).expect("authored touch contact");
            let start = Offset::new(40.0, 40.0);
            let end = Offset::new(if family == "tap" { 70.0 } else { 52.0 }, 40.0);
            for event in [
                make_down_event_for_id(pointer, start, PointerKind::Touch).expect("finite Down"),
                make_move_event_for_id(pointer, end, PointerKind::Touch).expect("finite Move"),
                make_up_event_for_id(pointer, end, PointerKind::Touch).expect("finite Up"),
            ] {
                laid.dispatch_pointer_event(&event);
            }
            assert_eq!(
                callbacks.get(),
                usize::from(configured),
                "{family}, configured={configured}"
            );
            if family != "tap" {
                assert_eq!(
                    competing_taps.get(),
                    usize::from(!configured),
                    "the competing tap wins only below the drag threshold"
                );
            }
        }
    }
}

pub(crate) fn scoped_settings_control_gesture_deadlines() {
    use std::{cell::Cell, rc::Rc, time::Duration};

    use flui_foundation::geometry::Offset;
    use flui_interaction::GestureSettings;
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};

    for family in ["long press", "double tap"] {
        for configured in [false, true] {
            let callbacks = Rc::new(Cell::new(0));
            let observed = Rc::clone(&callbacks);
            let detector = if family == "long press" {
                GestureDetector::new().on_long_press(move |_| observed.set(observed.get() + 1))
            } else {
                GestureDetector::new().on_double_tap(move |_| observed.set(observed.get() + 1))
            }
            .child(ColoredBox::new(Color::rgb(10, 20, 30)));
            let settings = if configured {
                GestureSettings::default()
                    .with_long_press_timeout(Duration::from_millis(100))
                    .with_double_tap_timeout(Duration::from_millis(100))
            } else {
                GestureSettings::default()
            };
            let mut laid = lay_out(
                ConfiguredGesture { settings, detector },
                tight(100.0, 100.0),
            );
            let contacts = flui_testing::widgets::PointerContacts::new();
            let position = Offset::new(40.0, 40.0);
            let first = contacts.begin();
            laid.dispatch_pointer_event(
                &make_down_event_for_id(first, position, PointerKind::Touch).expect("finite Down"),
            );
            if family == "double tap" {
                laid.dispatch_pointer_event(
                    &make_up_event_for_id(first, position, PointerKind::Touch).expect("finite Up"),
                );
            }
            laid.pump_for(Duration::from_millis(200));
            if family == "double tap" {
                let second = contacts.begin();
                laid.dispatch_pointer_event(
                    &make_down_event_for_id(second, position, PointerKind::Touch)
                        .expect("finite Down"),
                );
                laid.dispatch_pointer_event(
                    &make_up_event_for_id(second, position, PointerKind::Touch).expect("finite Up"),
                );
            }
            let expected = if family == "long press" {
                configured
            } else {
                !configured
            };
            assert_eq!(
                callbacks.get(),
                usize::from(expected),
                "{family}, configured={configured}"
            );
        }
    }
}

pub(crate) fn clearing_pan_callbacks_mid_drag_still_finishes_the_drag() {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_view::SignalWriteExt;
    use std::{cell::Cell, rc::Rc};

    let enabled = Rc::new(Cell::new(true));
    let starts = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let (gate, started, updated, ended) = (
        Rc::clone(&enabled),
        Rc::clone(&starts),
        Rc::clone(&updates),
        Rc::clone(&ends),
    );
    let signal = Rc::new(Cell::new(None));
    let remembered = Rc::clone(&signal);
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        let detector = GestureDetector::new();
        let detector = if gate.get() {
            let (started, updated, ended) =
                (Rc::clone(&started), Rc::clone(&updated), Rc::clone(&ended));
            detector
                .on_pan_start(move |_, _| started.set(started.get() + 1))
                .on_pan_update(move |_, _| updated.set(updated.get() + 1))
                .on_pan_end(move |_, _| ended.set(ended.get() + 1))
        } else {
            detector
        };
        detector.child(ColoredBox::new(Color::rgb(10, 20, 30)))
    });
    let mut laid = lay_out(probe.view(), tight(100.0, 100.0));
    let contacts = flui_testing::widgets::PointerContacts::new();
    let pointer = contacts.begin();
    laid.dispatch_pointer_event(
        &make_down_event_for_id(pointer, Offset::new(50.0, 10.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    laid.dispatch_pointer_event(
        &make_move_event_for_id(pointer, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(starts.get(), 1);
    enabled.set(false);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("write");
    laid.pump();
    laid.dispatch_pointer_event(
        &make_up_event_for_id(pointer, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(ends.get(), 0, "removed callbacks are not invoked");
    enabled.set(true);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 2))
        .expect("write");
    laid.pump();
    let before = updates.get();
    // Deliberately replay a stale sample for the released identity through the
    // public host boundary; the convenience Move helper requires a live Down.
    laid.dispatch_pointer_event(
        &make_move_event_for_id(pointer, Offset::new(50.0, 60.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(
        updates.get(),
        before,
        "the released contact cannot resume when callbacks return"
    );
    let fresh = contacts.begin();
    laid.dispatch_pointer_event(
        &make_down_event_for_id(fresh, Offset::new(50.0, 10.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    laid.dispatch_pointer_event(
        &make_move_event_for_id(fresh, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    laid.dispatch_pointer_event(
        &make_up_event_for_id(fresh, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(starts.get(), 2);
    assert_eq!(ends.get(), 1);
}

pub(crate) fn mounted_drag_policy_replaces_targets_before_cancellation_and_recovers() {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{DragPointerStrategy, GestureEndReason};
    use flui_view::SignalWriteExt;
    use std::{cell::Cell, rc::Rc};

    for cancel_panics in [false, true] {
        let policy = Rc::new(Cell::new(DragPointerStrategy::PrimaryOnly));
        let starts = Rc::new(Cell::new(0));
        let cancelled = Rc::new(Cell::new(0));
        let completed = Rc::new(Cell::new(0));
        let updates = Rc::new(std::cell::RefCell::new(Vec::new()));
        let signal = Rc::new(Cell::new(None));
        let (configured_policy, start_count, cancel_count, end_count, update_log, remembered) = (
            policy.clone(),
            starts.clone(),
            cancelled.clone(),
            completed.clone(),
            updates.clone(),
            signal.clone(),
        );
        let fail_once = Rc::new(Cell::new(cancel_panics));
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            remembered.set(Some(count));
            let (started, cancelled, ended, updated, fail) = (
                start_count.clone(),
                cancel_count.clone(),
                end_count.clone(),
                update_log.clone(),
                fail_once.clone(),
            );
            GestureDetector::new()
                .drag_pointer_strategy(configured_policy.get())
                .on_pan_start(move |_, _| started.set(started.get() + 1))
                .on_pan_update(move |_, details| updated.borrow_mut().push(details.delta.dy))
                .on_pan_end(move |_, details| match details.reason {
                    GestureEndReason::Completed => ended.set(ended.get() + 1),
                    GestureEndReason::Cancelled => {
                        cancelled.set(cancelled.get() + 1);
                        assert!(!fail.replace(false), "drag policy cancellation");
                    }
                })
                .child(ColoredBox::new(Color::rgb(10, 20, 30)))
        });
        let mut laid = lay_out(probe.view(), tight(100.0, 100.0));
        let send = |laid: &crate::common::LaidOut, id: u64, y, phase| {
            let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero touch");
            let position = Offset::new(50.0, y);
            let event = match phase {
                0 => make_down_event_for_id(pointer, position, PointerKind::Touch),
                1 => make_move_event_for_id(pointer, position, PointerKind::Touch),
                2 => make_up_event_for_id(pointer, position, PointerKind::Touch),
                _ => unreachable!("scripted phase"),
            }
            .expect("finite touch fixture");
            laid.dispatch_pointer_event(&event);
        };
        send(&laid, 2, 10.0, 0);
        send(&laid, 2, 50.0, 1);
        assert_eq!(starts.get(), 1);
        policy.set(DragPointerStrategy::ContinueWithRemaining);
        probe
            .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
            .expect("write policy rebuild");
        // Lifecycle update failures are recovered by substituting the failed
        // child, so this frame completes. Observe the host's contained report
        // rather than expecting a dropped-frame panic from the pump.
        let ((), log) = flui_testing::log_capture::capture(|| laid.pump());
        let reports: Vec<_> = log
            .records()
            .iter()
            .filter(|record| {
                record.message == "lifecycle panic contained; frame continued for this presentation"
            })
            .collect();
        if cancel_panics {
            assert_eq!(
                reports.len(),
                1,
                "outgoing cancellation is reported exactly once: {log}"
            );
            assert_eq!(
                reports[0].field("panic_message"),
                Some("drag policy cancellation"),
                "the original cancellation failure remains authoritative: {log}"
            );
            assert_eq!(reports[0].field("hook"), Some("Update"));
            assert_eq!(reports[0].field("internal_invariant"), Some("false"));
            assert_eq!(
                laid.count_elements_by_view_type::<GestureDetector>(),
                0,
                "the failed lifecycle actor is substituted"
            );
        } else {
            assert!(
                reports.is_empty(),
                "healthy policy replacement is not a lifecycle failure: {log}"
            );
            assert_eq!(laid.count_elements_by_view_type::<GestureDetector>(), 1);
        }
        assert_eq!(cancelled.get(), 1, "old accepted drag is cancelled once");
        send(&laid, 2, 50.0, 2);
        assert_eq!(
            completed.get(),
            0,
            "stale release cannot complete a replacement"
        );

        if cancel_panics {
            // The tree's recovery replaces the failed actor. Rebuild the live
            // parent to mount the configured healthy actor before fresh input.
            probe
                .write(|cx| signal.get().expect("mounted probe").set(cx, 2))
                .expect("rebuild after contained cancellation");
            laid.pump();
            assert_eq!(laid.count_elements_by_view_type::<GestureDetector>(), 1);
        }

        send(&laid, 2, 10.0, 0);
        send(&laid, 2, 50.0, 1);
        send(&laid, 3, 20.0, 0);
        send(&laid, 3, 30.0, 1);
        send(&laid, 2, 50.0, 2);
        assert_eq!(
            completed.get(),
            0,
            "mounted listener must route to the new continuation mode"
        );
        send(&laid, 3, 40.0, 1);
        assert_eq!(
            updates.borrow().last(),
            Some(&10.0),
            "replacement survives old cancellation failure and rebases handoff"
        );
        send(&laid, 3, 40.0, 2);
        assert_eq!((starts.get(), cancelled.get(), completed.get()), (2, 1, 1));
    }
}

pub(crate) fn authored_settings_replace_active_owners_and_preserve_equal_profiles() {
    crate::common::cases::run_cases(
        "authored drag owner replacement",
        &[
            ("pan", || authored_drag_owner_replacement(false)),
            ("horizontal drag", || authored_drag_owner_replacement(true)),
        ],
    );
}

fn authored_drag_owner_replacement(horizontal: bool) {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{GestureEndReason, GestureSettings};
    use flui_view::SignalWriteExt;
    use std::{cell::Cell, rc::Rc};

    for (changes, cancellation_panics) in [(false, false), (true, false), (true, true)] {
        let threshold = Rc::new(Cell::new(20.0));
        let starts = Rc::new(Cell::new(0));
        let cancelled = Rc::new(Cell::new(0));
        let completed = Rc::new(Cell::new(0));
        let fail_once = Rc::new(Cell::new(cancellation_panics));
        let signal = Rc::new(Cell::new(None));
        let (profile, started, cancels, ends, fail, remembered) = (
            threshold.clone(),
            starts.clone(),
            cancelled.clone(),
            completed.clone(),
            fail_once.clone(),
            signal.clone(),
        );
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            remembered.set(Some(count));
            let (started, cancels, ends, fail) =
                (started.clone(), cancels.clone(), ends.clone(), fail.clone());
            let on_start = move |_: &mut flui_view::EventCx<'_>,
                                 _: flui_interaction::DragStartDetails| {
                started.set(started.get() + 1);
            };
            let on_end = move |_: &mut flui_view::EventCx<'_>,
                               details: flui_interaction::DragEndDetails| {
                match details.reason {
                    GestureEndReason::Completed => ends.set(ends.get() + 1),
                    GestureEndReason::Cancelled => {
                        cancels.set(cancels.get() + 1);
                        assert!(!fail.replace(false), "authored settings cancellation");
                    }
                }
            };
            let detector = if horizontal {
                GestureDetector::new()
                    .on_horizontal_drag_start(on_start)
                    .on_horizontal_drag_end(on_end)
            } else {
                GestureDetector::new()
                    .on_pan_start(on_start)
                    .on_pan_end(on_end)
            };
            ConfiguredGesture {
                settings: GestureSettings::default()
                    .try_with_touch_slop(90.0)
                    .expect("tap remains a contender below the drag threshold")
                    .try_with_pan_slop(profile.get())
                    .expect("finite authored threshold")
                    .try_with_pan_slop_horizontal(profile.get())
                    .expect("finite authored axis threshold"),
                detector: detector
                    .on_tap(|_| {})
                    .child(ColoredBox::new(Color::rgb(10, 20, 30))),
            }
        });
        let mut laid = lay_out(probe.view(), tight(100.0, 100.0));
        let send = |laid: &crate::common::LaidOut, id: u64, y, phase| {
            let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero touch");
            let position = if horizontal {
                Offset::new(y, 50.0)
            } else {
                Offset::new(50.0, y)
            };
            let event = match phase {
                0 => make_down_event_for_id(pointer, position, PointerKind::Touch),
                1 => make_move_event_for_id(pointer, position, PointerKind::Touch),
                2 => make_up_event_for_id(pointer, position, PointerKind::Touch),
                _ => unreachable!("scripted phase"),
            }
            .expect("finite touch fixture");
            laid.dispatch_pointer_event(&event);
        };
        send(&laid, 2, 10.0, 0);
        send(&laid, 2, 50.0, 1);
        assert_eq!(starts.get(), 1, "initial owner accepts the pan");
        if changes {
            threshold.set(60.0);
        }
        probe
            .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
            .expect("rebuild authored scope");
        let ((), log) = flui_testing::log_capture::capture(|| laid.pump());
        assert_eq!(
            cancelled.get(),
            usize::from(changes),
            "only a changed authored profile cancels the accepted owner"
        );
        let reports: Vec<_> = log
            .records()
            .iter()
            .filter(|record| {
                record.message == "lifecycle panic contained; frame continued for this presentation"
            })
            .collect();
        assert_eq!(reports.len(), usize::from(cancellation_panics), "{log}");
        if cancellation_panics {
            assert_eq!(
                reports[0].field("panic_message"),
                Some("authored settings cancellation")
            );
            assert_eq!(
                laid.count_elements_by_view_type::<GestureDetector>(),
                0,
                "the failed lifecycle actor is substituted"
            );
        }
        send(&laid, 2, 50.0, 2);
        assert_eq!(
            completed.get(),
            usize::from(!changes),
            "a stale terminal cannot complete a replacement"
        );
        if cancellation_panics {
            probe
                .write(|cx| signal.get().expect("mounted probe").set(cx, 2))
                .expect("remount after containment");
            laid.pump();
        }
        send(&laid, 3, 10.0, 0);
        send(&laid, 3, 50.0, 1);
        assert_eq!(
            starts.get(),
            if changes { 1 } else { 2 },
            "fresh input obeys the replacement threshold"
        );
        send(&laid, 3, 90.0, 1);
        assert_eq!(starts.get(), 2);
        send(&laid, 3, 90.0, 2);
        assert_eq!(completed.get(), if changes { 1 } else { 2 });
    }
}

pub(crate) fn authored_settings_retire_tap_candidates_and_deadlines() {
    crate::common::cases::run_cases(
        "authored contact owner replacement",
        &[
            ("tap", || authored_contact_owner_replacement("tap")),
            ("long press", || {
                authored_contact_owner_replacement("long press");
            }),
            ("double tap", || {
                authored_contact_owner_replacement("double tap");
            }),
        ],
    );
}

fn authored_contact_owner_replacement(family: &'static str) {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_foundation::geometry::Offset;
    use flui_interaction::GestureSettings;
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};
    use flui_view::SignalWriteExt;
    use std::{cell::Cell, rc::Rc, time::Duration};

    let changed = Rc::new(Cell::new(false));
    let calls = Rc::new(Cell::new(0));
    let signal = Rc::new(Cell::new(None));
    let (profile, invoked, remembered) = (changed.clone(), calls.clone(), signal.clone());
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        let invoked = invoked.clone();
        let detector = match family {
            "tap" => GestureDetector::new().on_tap(move |_| invoked.set(invoked.get() + 1)),
            "long press" => {
                GestureDetector::new().on_long_press(move |_| invoked.set(invoked.get() + 1))
            }
            _ => GestureDetector::new().on_double_tap(move |_| invoked.set(invoked.get() + 1)),
        }
        .child(ColoredBox::new(Color::RED));
        let timeout = Duration::from_millis(if profile.get() { 400 } else { 100 });
        ConfiguredGesture {
            settings: GestureSettings::default()
                .with_long_press_timeout(timeout)
                .with_double_tap_timeout(timeout),
            detector,
        }
    });
    let mut laid = lay_out(probe.view(), tight(100.0, 100.0));
    let send = |laid: &crate::common::LaidOut, id: u64, down| {
        let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero touch");
        let position = Offset::new(50.0, 50.0);
        let event = if down {
            make_down_event_for_id(pointer, position, PointerKind::Touch)
        } else {
            make_up_event_for_id(pointer, position, PointerKind::Touch)
        }
        .expect("finite touch fixture");
        laid.dispatch_pointer_event(&event);
    };
    send(&laid, 1, true);
    if family == "double tap" {
        send(&laid, 1, false);
    }
    changed.set(true);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("replace authored contact profile");
    laid.pump();
    if family == "long press" {
        laid.pump_for(Duration::from_millis(200));
    } else if family == "double tap" {
        laid.pump_for(Duration::from_millis(50));
    }
    if family != "double tap" {
        send(&laid, 1, false);
    }
    assert_eq!(
        calls.get(),
        0,
        "outgoing contact or candidate is retired: {family}"
    );
    send(&laid, 2, true);
    if family == "long press" {
        laid.pump_for(Duration::from_millis(200));
        assert_eq!(calls.get(), 0, "fresh hold uses the replacement timeout");
        laid.pump_for(Duration::from_millis(250));
    }
    send(&laid, 2, false);
    if family == "double tap" {
        assert_eq!(
            calls.get(),
            0,
            "the retired first tap cannot complete a double tap"
        );
        laid.pump_for(Duration::from_millis(50));
        send(&laid, 3, true);
        send(&laid, 3, false);
    }
    assert_eq!(
        calls.get(),
        1,
        "fresh owner delivers its callback: {family}"
    );
}

pub(crate) fn authored_settings_retire_native_scale_session_before_fresh_admission() {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_foundation::geometry::{Offset, Point};
    use flui_interaction::GestureSettings;
    use flui_platform_api::{
        EventTime,
        pointer::{
            PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerEvent, PointerId, PointerInfo,
            PointerKind, PointerPosition,
        },
    };
    use flui_view::SignalWriteExt;
    use std::{cell::Cell, rc::Rc, time::Duration};

    let changed = Rc::new(Cell::new(false));
    let starts = Rc::new(Cell::new(0));
    let cancelled = Rc::new(Cell::new(0));
    let completed = Rc::new(Cell::new(0));
    let signal = Rc::new(Cell::new(None));
    let (profile, started, cancels, ends, remembered) = (
        changed.clone(),
        starts.clone(),
        cancelled.clone(),
        completed.clone(),
        signal.clone(),
    );
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        let (started, cancels, ends) = (started.clone(), cancels.clone(), ends.clone());
        ConfiguredGesture {
            settings: GestureSettings::default().with_long_press_timeout(Duration::from_millis(
                if profile.get() { 400 } else { 100 },
            )),
            detector: GestureDetector::new()
                .on_scale_start(move |_, _| started.set(started.get() + 1))
                .on_scale_cancel(move |_| cancels.set(cancels.get() + 1))
                .on_scale_end(move |_, _| ends.set(ends.get() + 1))
                .child(ColoredBox::new(Color::RED)),
        }
    });
    let mut laid = lay_out(probe.view(), tight(100.0, 100.0));
    let send = |laid: &crate::common::LaidOut, phase| {
        laid.dispatch_pointer_event(&PointerEvent::PanZoom(PanZoomEvent::new(
            PointerInfo::new(
                PointerId::try_from(1_u64).expect("nonzero pointer"),
                PointerKind::Mouse,
            ),
            EventTime::from_nanos(1),
            PointerPosition::try_new(Point::new(50.0, 50.0)).expect("finite position"),
            phase,
        )));
    };
    let update = || {
        PanZoomPhase::Update(
            PanZoomTransform::try_new(Offset::ZERO, 1.5, 0.0).expect("finite scale"),
        )
    };
    send(&laid, PanZoomPhase::Start);
    send(&laid, update());
    assert_eq!(starts.get(), 1);
    changed.set(true);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("replace authored scale owner");
    laid.pump();
    assert_eq!(
        cancelled.get(),
        1,
        "active native scale is retired exactly once"
    );
    send(&laid, PanZoomPhase::End);
    assert_eq!(
        completed.get(),
        0,
        "stale native terminal cannot complete a replacement"
    );
    send(&laid, PanZoomPhase::Start);
    send(&laid, update());
    send(&laid, PanZoomPhase::End);
    assert_eq!((starts.get(), cancelled.get(), completed.get()), (2, 1, 1));
}

pub(crate) fn mounted_native_begin_retains_estimator_before_first_claim() {
    use std::{cell::Cell, rc::Rc};

    use crate::gesture_settings::SettingsScope;
    use flui_foundation::geometry::{Offset, Point};
    use flui_interaction::{GestureSettings, GestureSettingsSource, processing::VelocityEstimator};
    use flui_platform_api::{
        EventTime,
        pointer::{
            PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerEvent, PointerId, PointerInfo,
            PointerKind, PointerPosition,
        },
    };

    let profile = |estimator| GestureSettings::default().with_velocity_estimator(estimator);
    let source = GestureSettingsSource::new(profile(VelocityEstimator::LeastSquares));
    let starts = Rc::new(Cell::new(0));
    let velocity = Rc::new(Cell::new(None));
    let (started, terminal) = (starts.clone(), velocity.clone());
    let laid = lay_out(
        SettingsScope::new(
            source.provider(),
            GestureDetector::new()
                .on_scale_start(move |_, _| started.set(started.get() + 1))
                .on_scale_end(move |_, details| terminal.set(Some(details.velocity)))
                .child(ColoredBox::new(Color::RED)),
        ),
        tight(100.0, 100.0),
    );
    for (session, expected) in [(1_u64, 5.0), (2, 15.899_257_985_831_98)] {
        let send = |millis, phase| {
            laid.dispatch_pointer_event(&PointerEvent::PanZoom(PanZoomEvent::new(
                PointerInfo::new(
                    PointerId::try_from(session).expect("nonzero pointer"),
                    PointerKind::Mouse,
                ),
                EventTime::from_nanos((session * 100 + millis) * 1_000_000),
                PointerPosition::try_new(Point::new(50.0, 50.0)).expect("finite position"),
                phase,
            )));
        };
        send(0, PanZoomPhase::Start);
        assert_eq!(
            starts.get(),
            session as usize - 1,
            "Begin stages policy without claiming or callbacks"
        );
        if session == 1 {
            source.replace(profile(VelocityEstimator::Impulse));
        }
        for (millis, scale) in [(10, 1.3), (20, 1.5), (30, 1.6)] {
            send(
                millis,
                PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::ZERO, scale, 0.0).expect("finite scale"),
                ),
            );
        }
        assert_eq!(
            starts.get(),
            session as usize,
            "first real update claims exactly once"
        );
        send(30, PanZoomPhase::End);
        let actual = velocity.get().expect("claimed native session completes");
        assert!(
            (actual - expected).abs() < 1e-6,
            "session{session} estimator admitted at Begin: actual{actual}, expected{expected}"
        );
    }
    for (session, terminal_phase) in [(3_u64, PanZoomPhase::End), (4, PanZoomPhase::Cancelled)] {
        source.replace(profile(VelocityEstimator::LeastSquares));
        velocity.set(None);
        let send = |millis, phase| {
            laid.dispatch_pointer_event(&PointerEvent::PanZoom(PanZoomEvent::new(
                PointerInfo::new(
                    PointerId::try_from(session).expect("nonzero pointer"),
                    PointerKind::Mouse,
                ),
                EventTime::from_nanos((session * 100 + millis) * 1_000_000),
                PointerPosition::try_new(Point::new(50.0, 50.0)).expect("finite position"),
                phase,
            )));
        };
        send(0, PanZoomPhase::Start);
        source.replace(profile(VelocityEstimator::Impulse));
        send(10, PanZoomPhase::Update(PanZoomTransform::IDENTITY));
        assert_eq!(
            starts.get(),
            session as usize - 1,
            "identity update leaves Begin unclaimed"
        );
        send(20, terminal_phase);
        assert!(
            velocity.get().is_none(),
            "unclaimed terminal has no completion callback"
        );
        send(
            30,
            PanZoomPhase::Update(
                PanZoomTransform::try_new(Offset::ZERO, 1.3, 0.0).expect("finite scale"),
            ),
        );
        assert_eq!(
            starts.get(),
            session as usize,
            "independent update starts after dormant retirement"
        );
        assert!(
            velocity.get().is_some(),
            "independent update completes after unclaimed terminal clears the staged Begin"
        );
    }
}

pub(crate) fn mounted_native_begin_refused_by_touch_cannot_claim_after_touch_terminal() {
    use std::{cell::Cell, rc::Rc};

    use flui_foundation::geometry::{Offset, Point};
    use flui_interaction::events::{make_down_event_for_id, make_up_event_for_id};
    use flui_platform_api::{
        EventTime,
        pointer::{
            PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerEvent, PointerId, PointerInfo,
            PointerKind, PointerPosition,
        },
    };

    let starts = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let (started, completed) = (starts.clone(), ends.clone());
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(|_| {})
            .on_scale_start(move |_, _| started.set(started.get() + 1))
            .on_scale_end(move |_, _| completed.set(completed.get() + 1))
            .child(ColoredBox::new(Color::RED)),
        tight(100.0, 100.0),
    );
    let touch = PointerId::try_from(100_u64).expect("nonzero touch");
    let native = PointerInfo::new(
        PointerId::try_from(200_u64).expect("nonzero native source"),
        PointerKind::Mouse,
    );
    let send = |millis: u64, phase| {
        laid.dispatch_pointer_event(&PointerEvent::PanZoom(PanZoomEvent::new(
            native,
            EventTime::from_nanos(millis * 1_000_000),
            PointerPosition::try_new(Point::new(50.0, 50.0)).expect("finite position"),
            phase,
        )));
    };
    let update = || {
        PanZoomPhase::Update(
            PanZoomTransform::try_new(Offset::ZERO, 1.3, 0.0).expect("finite scale"),
        )
    };
    laid.dispatch_pointer_event(
        &make_down_event_for_id(touch, Offset::new(50.0, 50.0), PointerKind::Touch)
            .expect("finite touch"),
    );
    send(0, PanZoomPhase::Start);
    assert_eq!(
        (starts.get(), ends.get()),
        (0, 0),
        "busy Begin cannot start native callbacks"
    );
    laid.dispatch_pointer_event(
        &make_up_event_for_id(touch, Offset::new(50.0, 50.0), PointerKind::Touch)
            .expect("finite touch"),
    );
    send(10, update());
    assert_eq!(
        (starts.get(), ends.get()),
        (0, 0),
        "refused Begin cannot become a delayed native session after touch terminal"
    );
    send(20, PanZoomPhase::End);
    send(30, update());
    assert_eq!(
        (starts.get(), ends.get()),
        (1, 1),
        "Update without Begin remains an independent completed step"
    );
    send(40, PanZoomPhase::Start);
    send(50, update());
    assert_eq!(
        (starts.get(), ends.get()),
        (2, 1),
        "a healthy new Begin admits a retained native session"
    );
    send(60, PanZoomPhase::End);
    assert_eq!((starts.get(), ends.get()), (2, 2));
}

pub(crate) fn unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival() {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_view::{IntoView, SignalWriteExt, ViewExt};
    use std::{cell::Cell, rc::Rc};

    let mounted = Rc::new(Cell::new(true));
    let cancelled = Rc::new(Cell::new(0));
    let rival_starts = Rc::new(Cell::new(0));
    let rival_ends = Rc::new(Cell::new(0));
    let signal = Rc::new(Cell::new(None));
    let (gate, cancels, remembered) = (
        Rc::clone(&mounted),
        Rc::clone(&cancelled),
        Rc::clone(&signal),
    );
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        let child = ColoredBox::new(Color::rgb(10, 20, 30));
        if gate.get() {
            let cancels = Rc::clone(&cancels);
            GestureDetector::new()
                .on_horizontal_drag_start(|_, _| -> () {
                    panic!("the retired contender must not start")
                })
                .on_horizontal_drag_cancel(move |_| cancels.set(cancels.get() + 1))
                .child(child)
                .into_view()
                .boxed()
        } else {
            child.into_view().boxed()
        }
    });
    let (started, ended) = (Rc::clone(&rival_starts), Rc::clone(&rival_ends));
    let mut laid = lay_out(
        GestureDetector::new()
            .on_pan_start(move |_, _| started.set(started.get() + 1))
            .on_pan_end(move |_, _| ended.set(ended.get() + 1))
            .child(probe.view()),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_down(10.0, 50.0);
    mounted.set(false);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("write");
    laid.pump();
    assert_eq!(
        cancelled.get(),
        1,
        "unmount explicitly cancels the admitted contender"
    );
    laid.dispatch_pointer_move(60.0, 50.0);
    laid.dispatch_pointer_up(60.0, 50.0);
    assert_eq!(
        rival_starts.get(),
        1,
        "the remaining live recognizer wins the contact"
    );
    assert_eq!(rival_ends.get(), 1);
    assert_eq!(
        cancelled.get(),
        1,
        "the cached terminal route cannot cancel the retired owner twice"
    );
}

pub(crate) fn gesture_detector_fires_on_tap_for_a_down_up_on_the_child() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            // A hit-testable child so the DeferToChild Listener registers.
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    assert_eq!(taps.load(Ordering::SeqCst), 0, "no tap before any pointer");

    // A tap = down then up at the same place.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on the child fires on_tap exactly once",
    );
}

/// The gesture arena's rule is that the first member to accept, or the last
/// member not to reject, wins. A drag past the slop makes the tap recognizer
/// reject itself, leaving the pan recognizer as the last remaining (and thus
/// winning) member.
pub(crate) fn gesture_detector_recognizes_a_pan_and_suppresses_the_tap() {
    let taps = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let updates = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(AtomicUsize::new(0));
    let (tap_cb, start_cb, update_cb, end_cb) = (
        Arc::clone(&taps),
        Arc::clone(&starts),
        Arc::clone(&updates),
        Arc::clone(&ends),
    );

    // The detector wants BOTH a tap and a pan; the arena must hand a real drag
    // to the pan recognizer and cancel the tap.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_start(move |_cx, _details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_update(move |_cx, _details| {
                update_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_end(move |_cx, _details| {
                end_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // Down, then a move well past the pan slop (60px > 18px) that crosses into a
    // drag, a second move, then up. Every position stays inside the 100×100
    // child so the headless harness (which re-hit-tests each event — no pointer
    // capture) keeps routing to the detector.
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.dispatch_pointer_move(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 90.0);
    laid.dispatch_pointer_up(50.0, 90.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "the drag started exactly once"
    );
    assert!(
        updates.load(Ordering::SeqCst) >= 1,
        "the drag reported at least one update as the pointer moved",
    );
    assert_eq!(
        ends.load(Ordering::SeqCst),
        1,
        "the drag ended exactly once on up"
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a drag past the slop cancels the competing tap — they are mutually exclusive",
    );
}

/// Once a drag has won its arena, `PointerCancel` takes the accepted branch
/// and fires `onEnd`, not `onCancel`. The terminal event must still leave the recognizer reusable.
pub(crate) fn horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector()
{
    let reasons = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let recorded = std::rc::Rc::clone(&reasons);
    let cancels = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (cancel_cb, end_cb, start_cb) =
        (Arc::clone(&cancels), Arc::clone(&ends), Arc::clone(&starts));

    let laid = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_start(move |_cx, _details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_cancel(move |_cx| {
                cancel_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_end(move |_cx, details| {
                recorded.borrow_mut().push(details.reason);
                end_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );

    laid.dispatch_pointer_down(20.0, 100.0);
    laid.dispatch_pointer_move(80.0, 100.0);
    assert_eq!(starts.load(Ordering::SeqCst), 1, "the drag started");

    laid.dispatch_pointer_cancel();
    assert_eq!(
        ends.load(Ordering::SeqCst),
        1,
        "a PointerCancel after arena acceptance ends the active drag"
    );
    assert_eq!(
        cancels.load(Ordering::SeqCst),
        0,
        "on_horizontal_drag_cancel is reserved for a sequence rejected before acceptance"
    );

    // A fresh contact afterward still completes normally.
    laid.dispatch_pointer_down(20.0, 100.0);
    laid.dispatch_pointer_move(80.0, 100.0);
    laid.dispatch_pointer_up(80.0, 100.0);
    assert_eq!(
        starts.load(Ordering::SeqCst),
        2,
        "a drag after a cancel still starts (the cancel did not wedge the recognizer)",
    );
    assert_eq!(ends.load(Ordering::SeqCst), 2);
    assert_eq!(
        *reasons.borrow(),
        [
            flui_widgets::GestureEndReason::Cancelled,
            flui_widgets::GestureEndReason::Completed
        ]
    );
}

// ============================================================================
// Event context (ADR-0086): every callback receives `&mut EventCx<'_>` and
// writes a signal through it; the write rebuilds the signal's reader.
// ============================================================================

pub(crate) mod event_cx {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::common::{LaidOut, ProbeSignals, SignalProbe, lay_out, tight};

    use flui_painting::styling::Color;
    use flui_testing::{A11yTree, Action, ActionRequest, TreeId};
    use flui_view::prelude::*;
    use flui_widgets::{ColoredBox, GestureDetector, Semantics, Text};

    fn target() -> ColoredBox {
        ColoredBox::new(Color::rgb(10, 20, 30))
    }

    /// A tap at the centre of a 100x100 detector.
    fn tap(app: &LaidOut) {
        app.dispatch_pointer_down(50.0, 50.0);
        app.dispatch_pointer_up(50.0, 50.0);
    }

    pub(crate) fn a_tap_writes_a_signal_and_rebuilds_its_reader() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            GestureDetector::new()
                .on_tap(move |cx| count.update(cx, |n| *n += 1))
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));

        tap(&app);
        assert_eq!(probe.value(), Ok(1));
        app.tick();

        assert_eq!(
            probe.reads(),
            [0, 1],
            "the reader rebuilt once, with the new value"
        );
    }

    /// The detector wrapped so that its semantics actions merge into one
    /// node labelled `Tap`.
    fn labelled(detector: GestureDetector) -> Semantics {
        Semantics::new()
            .container(true)
            .child(detector.child(Text::new("Tap")))
    }

    fn invoke_labelled_action(app: &LaidOut, tree: &A11yTree, action: Action) {
        let id = tree
            .find_by_label("Tap")
            .unwrap_or_else(|error| {
                panic!("one node labelled \"Tap\": {error}\n{}", tree.describe())
            })
            .id();
        app.invoke_semantics_action(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: id,
            data: None,
        })
        .expect("a click on a node advertising one resolves");
    }

    pub(crate) fn repeated_assistive_taps_are_delivered_once_each_and_keep_making_progress() {
        assert_repeated_actions_are_delivered_once_each(Action::Click);
    }

    pub(crate) fn repeated_assistive_long_presses_are_delivered_once_each_and_keep_making_progress()
    {
        assert_repeated_actions_are_delivered_once_each(Action::ShowContextMenu);
    }

    /// Two accepted `action`s of one kind run their handler twice on the next
    /// frame, never again on a later one, and a third still arrives after the
    /// batch drains. The detector advertises both kinds, so only the handler
    /// `action` reaches counts.
    fn assert_repeated_actions_are_delivered_once_each(action: Action) {
        let long_press = matches!(action, Action::ShowContextMenu);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            let detector = GestureDetector::new();
            let detector = if long_press {
                detector
                    .on_tap(|_cx| {})
                    .on_long_press(move |cx| count.update(cx, |n| *n += 1))
            } else {
                detector
                    .on_tap(move |cx| count.update(cx, |n| *n += 1))
                    .on_long_press(|_cx| {})
            };
            labelled(detector)
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));
        app.enable_semantics();
        app.pump();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");

        invoke_labelled_action(&app, &tree, action);
        invoke_labelled_action(&app, &tree, action);
        assert_eq!(probe.value(), Ok(0), "accepted actions are deferred");

        app.tick();
        assert_eq!(
            probe.value(),
            Ok(2),
            "coalescing wake demand must not coalesce two accepted activations"
        );
        app.tick();
        assert_eq!(probe.reads().last(), Some(&2), "the signal reader rebuilt");
        assert_eq!(
            probe.value(),
            Ok(2),
            "a later frame must not replay either action"
        );

        let tree = app.a11y_tree().expect("semantics remains available");
        invoke_labelled_action(&app, &tree, action);
        assert_eq!(probe.value(), Ok(2), "the next activation is deferred too");
        app.tick();
        assert_eq!(
            probe.value(),
            Ok(3),
            "delivery remains live after draining a batch"
        );
        app.tick();
        assert_eq!(
            probe.reads().last(),
            Some(&3),
            "the later write also rebuilds"
        );
        assert_eq!(
            probe.value(),
            Ok(3),
            "the later action is delivered only once"
        );
    }

    pub(crate) fn a_panicking_assistive_action_does_not_discard_the_fifo_tail() {
        let long_press_calls = Rc::new(Cell::new(0));
        let observed_long_press = Rc::clone(&long_press_calls);
        let mut app = lay_out(
            labelled(
                GestureDetector::new()
                    .on_tap(|_cx| -> () { panic!("intentional assistive tap panic") })
                    .on_long_press(move |_cx| {
                        observed_long_press.set(observed_long_press.get() + 1);
                    }),
            ),
            tight(100.0, 100.0),
        );
        app.enable_semantics();
        app.pump();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");
        invoke_labelled_action(&app, &tree, Action::Click);
        invoke_labelled_action(&app, &tree, Action::ShowContextMenu);

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.tick()));
        assert!(
            panicked.is_err(),
            "the user callback panic still propagates"
        );
        assert_eq!(
            long_press_calls.get(),
            0,
            "the tail has not run out of order"
        );

        app.tick();
        assert_eq!(
            long_press_calls.get(),
            1,
            "scheduler recovery retains the next accepted command"
        );
    }
}

pub(crate) fn viewer_reports_cancelled_then_completed_interactions() {
    use flui_widgets::{GestureEndReason, InteractiveViewer};
    use std::cell::RefCell;
    use std::rc::Rc;
    let reasons = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&reasons);
    let laid = lay_out(
        InteractiveViewer::new()
            .on_interaction_end(move |_cx, details| recorded.borrow_mut().push(details.reason))
            .child(target_for_viewer()),
        tight(200.0, 200.0),
    );
    laid.dispatch_pointer_down(30.0, 80.0);
    laid.dispatch_pointer_move(100.0, 80.0);
    laid.dispatch_pointer_cancel();
    laid.dispatch_pointer_down(30.0, 80.0);
    laid.dispatch_pointer_move(100.0, 80.0);
    laid.dispatch_pointer_up(100.0, 80.0);
    assert_eq!(
        *reasons.borrow(),
        [GestureEndReason::Cancelled, GestureEndReason::Completed]
    );
}

fn target_for_viewer() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}
