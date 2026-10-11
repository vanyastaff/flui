//! `UiRuntime::pump`, the frame transaction (ADR-0083 §1): apply commands, begin
//! frame, draw frame, end frame, at the one timestamp the pump's clock
//! returns — and `pump_background`, the frames-disabled wake.
//!
//! Each test drives the pump itself, never a hand-assembled
//! `OwnerFrame::drive_frame` around `draw_frame`, so each fails against a pump
//! that skips or reorders the phase it names.

use std::time::Duration;

use flui_animation::{Animation as _, AnimationController};

use super::*;
use flui_foundation::ManualClock;

/// A post-frame callback scheduled on the UI runtime's owner-local lane runs once,
/// after the pipeline, and sees the layout this same pump committed.
///
/// Fails against a pump that skips end frame (no call), that drives the
/// pipeline after end frame (the callback sees no layout), or that ends the
/// frame without the UI runtime's local lane (the lane is never drained).
pub(crate) fn pump_post_frame_callback_observes_this_frames_committed_layout() {
    use flui_rendering::prelude::Leaf;
    use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, PaintCx, RenderBox};

    #[derive(Debug, Default)]
    struct FixedBox;
    impl flui_foundation::Diagnosticable for FixedBox {}
    impl RenderBox for FixedBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;
        fn perform_layout(
            &mut self,
            _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
        ) -> flui_rendering::RenderResult<flui_foundation::geometry::Size> {
            Ok(flui_foundation::geometry::Size::new(40.0, 24.0))
        }
        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    }

    let mut ui_runtime = UiRuntime::for_test();
    let pipeline = ui_runtime.pipeline_for_test();
    let root = pipeline.with_mut(|owner| {
        let root = owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(FixedBox));
        owner.set_root_id(Some(root));
        root
    });
    assert_eq!(
        pipeline.with(|owner| owner.box_size(root)),
        None,
        "nothing is laid out before the first frame"
    );

    let observed = Arc::new(RwLock::new(None));
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_cb = Arc::clone(&observed);
    let calls_cb = Arc::clone(&calls);
    let pipeline_cb = pipeline;
    // `PipelineCell` is `!Send`, so the callback goes on the owner-local
    // lane, which only a pump that ends its frame with that lane drains.
    ui_runtime
        .widgets()
        .with_build_owner(|owner| owner.post_frame_handle().cloned())
        .expect("owner-local post-frame handle installed by UiRuntime::construct")
        .schedule(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            *observed_cb.write() = pipeline_cb.with(|owner| owner.box_size(root));
        })
        .expect("the ui_runtime's local post-frame lane outlives this call");

    // The pump lays the root out tight to the sink's surface (at the test
    // ui_runtime's device pixel ratio of 1), so the surface is the box's own size.
    let _ = ui_runtime.pump(
        &mut ManualClock::new(),
        &mut ScriptedSink::always_presents().with_size(40, 24),
    );

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the pump's end frame drains the ui_runtime's local post-frame lane exactly once"
    );
    assert_eq!(
        *observed.read(),
        Some(flui_foundation::geometry::Size::new(40.0, 24.0)),
        "the post-frame callback must observe THIS pump's committed layout"
    );
}

/// A UI runtime on its own manual-clock source over a headless test window.
fn manual_clock_ui_runtime(clock: &ManualClock) -> UiRuntime {
    UiRuntime::new(
        test_window(),
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime")
}

pub(crate) fn a_stopping_realm_ticks_no_presentation() {
    use flui_animation::Curve;
    use std::cell::Cell;
    use std::rc::Rc;

    struct CountedCurve(Rc<Cell<usize>>);
    impl Curve for CountedCurve {
        fn transform(&self, time: f64) -> f64 {
            self.0.set(self.0.get() + 1);
            time
        }
        fn slope(&self, _: f64) -> f64 {
            1.0
        }
    }

    let mut clock = ManualClock::new();
    let mut runtime = manual_clock_ui_runtime(&clock);
    runtime
        .attach_root_widget(&flui_widgets::SizedBox::square(10.0))
        .expect("primary root");
    let mut sink = ScriptedSink::always_presents();
    let _ = runtime.pump(&mut clock, &mut sink);
    let secondary = runtime.install_second_presentation_for_test();
    runtime
        .attach_root_widget_to_for_test(secondary, &flui_widgets::SizedBox::square(10.0))
        .expect("secondary root");
    let _ = runtime.pump(&mut clock, &mut sink);
    let primary_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&runtime.vsync()));
    let secondary_owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(
        &runtime
            .presentations
            .get(secondary)
            .expect("secondary")
            .vsync(),
    ));
    let samples = Rc::new(Cell::new(0));
    for owner in [&primary_owner, &secondary_owner] {
        owner
            .controller()
            .animate_to_curved(1.0, None, CountedCurve(Rc::clone(&samples)))
            .expect("live curve");
    }
    let _ = runtime.pump(&mut clock, &mut sink);
    clock.advance(Duration::from_millis(250));
    let _ = runtime.pump(&mut clock, &mut sink);
    assert!((primary_owner.controller().value() - 0.25).abs() < 1e-9);
    assert!((secondary_owner.controller().value() - 0.25).abs() < 1e-9);
    let sampled = samples.get();
    assert!(sampled >= 2, "both sources were reached before stopping");
    runtime.stop_presentations();
    runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
    for _ in 0..3 {
        clock.advance(Duration::from_secs(1));
        let _ = runtime.pump(&mut clock, &mut sink);
        assert_eq!(
            samples.get(),
            sampled,
            "stopping withdraws every presentation's tick authority"
        );
        assert!((primary_owner.controller().value() - 0.25).abs() < 1e-9);
        assert!((secondary_owner.controller().value() - 0.25).abs() < 1e-9);
    }
}

pub(crate) fn step_during_a_tick_applies_next_frame() {
    use flui_foundation::Listenable as _;
    use flui_protocol::MotionRequest;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    for address_primary in [true, false] {
        let mut clock = ManualClock::new();
        let mut runtime = manual_clock_ui_runtime(&clock);
        let primary = runtime.presentation_id();
        runtime
            .attach_root_widget(&flui_widgets::SizedBox::square(10.0))
            .expect("primary root");
        let mut sink = ScriptedSink::always_presents();
        let _ = runtime.pump(&mut clock, &mut sink);
        let secondary = runtime.install_second_presentation_for_test();
        runtime
            .attach_root_widget_to_for_test(secondary, &flui_widgets::SizedBox::square(10.0))
            .expect("secondary root");
        let _ = runtime.pump(&mut clock, &mut sink);
        let target = if address_primary { primary } else { secondary };
        let window = runtime.dev_agent_window(target).expect("addressed agent");
        let primary_owner =
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(&runtime.vsync()));
        let secondary_owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(
            &runtime
                .presentations
                .get(secondary)
                .expect("secondary")
                .vsync(),
        ));
        primary_owner.controller().forward().expect("primary run");
        secondary_owner
            .controller()
            .forward()
            .expect("secondary run");
        let _ = runtime.pump(&mut clock, &mut sink);
        let armed = Rc::new(Cell::new(true));
        let reply = Rc::new(RefCell::new(None));
        primary_owner.controller().add_listener(Rc::new({
            let armed = Rc::clone(&armed);
            let reply = Rc::clone(&reply);
            move || {
                if armed.replace(false) {
                    let answer = window
                        .motion(MotionRequest::new().with_rate(0.0).with_step_ms(100))
                        .expect("admitted during traversal");
                    *reply.borrow_mut() = Some(answer);
                }
            }
        }));
        clock.advance(Duration::from_millis(250));
        let _ = runtime.pump(&mut clock, &mut sink);
        assert!(!armed.get(), "listener admitted the command");
        assert!((primary_owner.controller().value() - 0.25).abs() < 1e-9);
        assert!((secondary_owner.controller().value() - 0.25).abs() < 1e-9);
        assert!(
            reply
                .borrow_mut()
                .as_mut()
                .expect("queued answer")
                .try_take()
                .is_none(),
            "a command admitted during traversal remains pending until the next pump"
        );
        let addressed_frames = runtime
            .presentations
            .get(target)
            .expect("target")
            .clock()
            .produced_count();
        clock.advance(Duration::from_millis(250));
        let _ = runtime.pump(&mut clock, &mut sink);
        let accepted = reply
            .borrow_mut()
            .as_mut()
            .expect("answer")
            .try_take()
            .expect("answered at the next idle boundary")
            .expect("valid compound request");
        assert_eq!(accepted.rate, 0.0);
        assert!((accepted.time_ms - 350.0).abs() < 1e-9);
        let (addressed, sibling) = if address_primary {
            (primary_owner.controller(), secondary_owner.controller())
        } else {
            (secondary_owner.controller(), primary_owner.controller())
        };
        assert!((addressed.value() - 0.35).abs() < 1e-9);
        assert!((sibling.value() - 0.5).abs() < 1e-9);
        assert_eq!(
            runtime
                .presentations
                .get(target)
                .expect("target")
                .clock()
                .produced_count(),
            addressed_frames + 1
        );
        clock.advance(Duration::from_millis(250));
        let _ = runtime.pump(&mut clock, &mut sink);
        assert!((addressed.value() - 0.35).abs() < 1e-9);
        assert!((sibling.value() - 0.75).abs() < 1e-9);
        assert_eq!(
            runtime
                .presentations
                .get(target)
                .expect("target")
                .clock()
                .produced_count(),
            addressed_frames + 1,
            "the addressed step consumes one frame; sibling continuation cannot revive it"
        );
        assert!(
            reply
                .borrow_mut()
                .as_mut()
                .expect("consumed answer")
                .try_take()
                .is_none(),
            "the accepted reply is delivered once"
        );
    }
}

/// Forest isolation needs a private installation seam; input and frames still
/// use the actual addressed ingress, cached route and owner pump.
#[test]
fn resampling_is_presentation_local_and_preserves_delivery_after_failure() {
    crate::table_test::run_table(
        "resampling_is_presentation_local_and_preserves_delivery_after_failure",
        &[
            ("healthy", resampling_siblings_healthy as fn()),
            ("single_failure", resampling_siblings_single_failure),
            ("competing_failures", resampling_siblings_competing_failures),
        ],
    );
}

fn resampling_siblings_healthy() {
    resampling_sibling_case(false, false);
}
fn resampling_siblings_single_failure() {
    resampling_sibling_case(true, false);
}
fn resampling_siblings_competing_failures() {
    resampling_sibling_case(true, true);
}

fn resampling_sibling_case(fail: bool, compete: bool) {
    use crate::presentation::PointerResampling;
    use flui_platform_api::pointer::*;
    use flui_platform_api::{EventTime, PlatformInput};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    let mut clock = ManualClock::new();
    let mut realm = manual_clock_ui_runtime(&clock);
    let a = realm.presentation_id();
    realm
        .attach_root_widget(&flui_widgets::SizedBox::square(200.0))
        .expect("A root");
    // One sink acknowledges one producer's tree. Commit A before mounting B,
    // otherwise the first pump can acknowledge only B and A's input stays held.
    let mut sink = ScriptedSink::always_presents().with_size(200, 200);
    assert!(realm.pump(&mut clock, &mut sink).presented(), "A commits");
    let b = realm.install_second_presentation_for_test();
    realm
        .attach_root_widget_to_for_test(b, &flui_widgets::SizedBox::square(200.0))
        .expect("B root");
    realm
        .set_pointer_resampling(a, PointerResampling::FrameAligned)
        .expect("A idle");
    let a_seen = Rc::new(RefCell::new(Vec::new()));
    let b_seen = Rc::new(RefCell::new(Vec::new()));
    let armed = Rc::new(Cell::new(fail));
    let seen = Rc::clone(&a_seen);
    let flag = Rc::clone(&armed);
    realm
        .gestures()
        .pointer_router()
        .add_global_handler(Rc::new(move |event| {
            if let PointerEvent::Move(event) = event {
                seen.borrow_mut().push(event.current().position.get().x);
                let count = seen.borrow().len();
                assert!(!(flag.get() && count == 1), "first presentation sample");
                assert!(
                    !(flag.get() && compete && count == 2),
                    "second presentation sample"
                );
            }
        }));
    let seen = Rc::clone(&b_seen);
    realm
        .presentation_gestures_for_test(b)
        .pointer_router()
        .add_global_handler(Rc::new(move |event| {
            if let PointerEvent::Move(event) = event {
                seen.borrow_mut().push(event.current().position.get().x);
            }
        }));
    assert!(realm.pump(&mut clock, &mut sink).presented(), "B commits");
    clock.advance(Duration::from_millis(1_000));
    let pointer = PointerInfo::new(
        PointerId::try_from(1_u64).expect("contact"),
        PointerKind::Touch,
    );
    let sample = |x, ms: u64| {
        PointerSample::new(
            EventTime::from_nanos(ms * 1_000_000),
            PointerPosition::try_new(flui_foundation::geometry::Point::new(x, 10.0))
                .expect("finite position"),
        )
    };
    for id in [a, b] {
        realm.enter(|realm| {
            realm.handle_input_addressed(
                id,
                PlatformInput::Pointer(PointerEvent::Down(PointerPress::new(
                    pointer,
                    PointerButton::PRIMARY,
                    PointerButtons::only(PointerButton::PRIMARY),
                    sample(10.0, 1_000),
                ))),
            )
        });
        realm.enter(|realm| {
            realm.handle_input_addressed(
                id,
                PlatformInput::Pointer(PointerEvent::Move(PointerMove::new(
                    pointer,
                    PointerButtons::only(PointerButton::PRIMARY),
                    sample(10.0, 1_000),
                ))),
            )
        });
    }
    clock.advance(Duration::from_millis(100));
    for id in [a, b] {
        realm.enter(|realm| {
            realm.handle_input_addressed(
                id,
                PlatformInput::Pointer(PointerEvent::Move(PointerMove::new(
                    pointer,
                    PointerButtons::only(PointerButton::PRIMARY),
                    sample(110.0, 1_100),
                ))),
            )
        });
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        realm.pump(&mut clock, &mut sink)
    }));
    if fail {
        let payload = result.expect_err("first sample callback failed");
        assert_eq!(
            payload.downcast_ref::<&str>().copied(),
            Some("first presentation sample")
        );
    } else {
        let _ = result.expect("healthy samples");
    }
    assert_eq!(
        &*b_seen.borrow(),
        &[110.0],
        "default sibling coalesces on the same pump"
    );
    assert_eq!(
        a_seen.borrow().len(),
        2,
        "accepted interpolation follows even after its first observer failed"
    );
    assert!((a_seen.borrow()[1] - 72.0).abs() < 1e-9);
    assert!(
        realm.needs_redraw(),
        "accepted future sample debt arms the next frame"
    );
    armed.set(false);
    clock.advance(Duration::from_millis(38));
    let _ = realm.pump(&mut clock, &mut sink);
    assert_eq!(
        a_seen.borrow().last(),
        Some(&110.0),
        "tail survives containment"
    );
    assert_eq!(
        &*b_seen.borrow(),
        &[110.0],
        "a sibling has no borrowed sample debt"
    );
    assert!(
        !realm.needs_redraw(),
        "tracked contact alone does not demand another frame"
    );
}

/// A long-press recognizer on the UI runtime's gesture arena fires once the
/// UI runtime's manual clock passes its deadline, with no wall time elapsed.
///
/// Fails against a UI runtime whose arena reads the wall clock: 600 ms of manual
/// time is microseconds of wall time, the 500 ms deadline has not passed when
/// the pump polls it, and the callback never runs.
#[test]
fn a_ui_runtime_on_a_manual_clock_fires_gesture_deadlines_on_that_clock() {
    use flui_interaction::settings::GestureSettings;
    use flui_interaction::{GestureRecognizer as _, LongPressGestureRecognizer, PointerId};

    let mut clock = ManualClock::new();
    let mut ui_runtime = manual_clock_ui_runtime(&clock);
    let fired = Arc::new(AtomicBool::new(false));
    let fired_in_callback = Arc::clone(&fired);
    let recognizer = LongPressGestureRecognizer::builder(ui_runtime.gestures().arena().clone())
        .settings(
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
        )
        .on_long_press_start(move |_details| fired_in_callback.store(true, Ordering::SeqCst))
        .build();
    let position = flui_foundation::geometry::Offset::new(10.0, 10.0);
    let down = flui_interaction::events::make_down_event_for_id(
        PointerId::new(std::num::NonZeroU64::MIN),
        position,
        flui_interaction::PointerKind::Touch,
    )
    .expect("valid Down sample");
    recognizer.add_pointer(flui_interaction::PointerDispatch::at_root(&down));

    let mut sink = ScriptedSink::always_presents();
    clock.advance(Duration::from_millis(300));
    let _ = ui_runtime.pump(&mut clock, &mut sink);
    assert!(
        !fired.load(Ordering::SeqCst),
        "300 ms of manual time has not reached the 500 ms deadline"
    );

    clock.advance(Duration::from_millis(300));
    let _ = ui_runtime.pump(&mut clock, &mut sink);
    assert!(
        fired.load(Ordering::SeqCst),
        "the pump polls the arena's deadlines on the ui_runtime's manual clock"
    );
}

/// A presentation's minimum produce interval is measured on the UI runtime's
/// manual clock: a second demanded pump inside the interval is withheld, and
/// one after the manual clock crosses it produces, with no wall time spent.
///
/// Fails against a presentation whose `FrameClock` reads the wall clock: the
/// second pump, microseconds of wall time after the first, stays withheld
/// even after the manual clock has moved past the interval.
#[test]
fn a_manual_clock_ui_runtime_gates_its_min_produce_interval_on_that_clock() {
    let mut clock = ManualClock::new();
    let mut ui_runtime = manual_clock_ui_runtime(&clock);
    ui_runtime
        .presentations
        .primary()
        .clock()
        .set_min_produce_interval(Some(Duration::from_millis(100)));
    let mut sink = ScriptedSink::always_presents();

    ui_runtime.request_redraw();
    let _ = ui_runtime.pump(&mut clock, &mut sink);
    assert_eq!(
        ui_runtime_produced(&ui_runtime),
        1,
        "the first demanded pump produces"
    );

    clock.advance(Duration::from_millis(40));
    ui_runtime.request_redraw();
    let _ = ui_runtime.pump(&mut clock, &mut sink);
    assert_eq!(
        ui_runtime_produced(&ui_runtime),
        1,
        "40 ms of manual time is inside the 100 ms interval: the frame is withheld"
    );

    clock.advance(Duration::from_millis(80));
    ui_runtime.request_redraw();
    let _ = ui_runtime.pump(&mut clock, &mut sink);
    assert_eq!(
        ui_runtime_produced(&ui_runtime),
        2,
        "120 ms of manual time is past the interval: the frame is produced"
    );
}

/// A frame drawn outside a pump ticks the UI runtime's `Vsync` registry on the
/// UI runtime's clock source, not the wall clock: 50 ms of manual time into a
/// 100 ms run is halfway.
///
/// Fails against a UI runtime whose out-of-pump frame time reads the wall clock:
/// microseconds pass, and the value stays near 0.
#[test]
fn a_frame_outside_a_pump_ticks_vsync_on_the_ui_runtimes_clock() {
    let clock = ManualClock::new();
    let ui_runtime = manual_clock_ui_runtime(&clock);
    let mut owner = AnimationController::builder(Duration::from_millis(100))
        .build_on(Some(&ui_runtime.vsync()));
    let controller = owner.controller();
    controller.forward().expect("fresh controller forwards");
    let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

    // Anchors the run at the ui_runtime's current time.
    let _ = ui_runtime.draw_frame(constraints);
    clock.advance(Duration::from_millis(50));
    let _ = ui_runtime.draw_frame(constraints);

    let value = controller.value();
    assert!(
        (value - 0.5).abs() < 1e-4,
        "50 ms of the ui_runtime's clock into a 100 ms run is halfway (value={value})"
    );
    owner.dispose();
}

fn ui_runtime_produced(ui_runtime: &UiRuntime) -> u64 {
    ui_runtime.presentations.primary().clock().produced_count()
}
