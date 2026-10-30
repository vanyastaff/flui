//! Controller robustness: `is_animating` means "a run is installed" through
//! every wrapper, a curved run never publishes a non-finite or out-of-bounds
//! value, extreme durations and times never panic or poison the timeline, and
//! a disposed controller neither changes nor keeps what it is handed.
//!
//! The contract rows run in the ordinary suite.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, CurvedAnimation,
    DrivenController, ProxyAnimation, RepeatMode, ReverseAnimation, Tween, TweenAnimation, Vsync,
    curve::{Curve, Linear},
};
use flui_foundation::Listenable;

/// A one-second `[0, 1]` controller advanced only by explicit times.
fn controller() -> AnimationController {
    AnimationController::builder(Duration::from_secs(1)).build()
}

/// `is_animating` as answered by the controller and by each wrapper over it.
fn is_animating_everywhere(controller: &AnimationController) -> Vec<(&'static str, bool)> {
    let source: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(controller.clone());
    vec![
        ("controller", controller.is_animating()),
        (
            "proxy",
            ProxyAnimation::new(std::rc::Rc::clone(&source)).is_animating(),
        ),
        (
            "reverse",
            ReverseAnimation::new(std::rc::Rc::clone(&source)).is_animating(),
        ),
        (
            "curved",
            CurvedAnimation::new(std::rc::Rc::clone(&source), Linear).is_animating(),
        ),
        (
            "tween",
            TweenAnimation::new(Tween::new(10.0, 20.0), std::rc::Rc::clone(&source)).is_animating(),
        ),
        ("switch", AnimationSwitch::new(source, None).is_animating()),
    ]
}

fn assert_is_animating_everywhere(controller: &AnimationController, expected: bool) {
    let answers = is_animating_everywhere(controller);
    let want: Vec<_> = answers.iter().map(|(name, _)| (*name, expected)).collect();
    assert_eq!(
        answers, want,
        "every wrapper answers is_animating like its source"
    );
}

// --- is_animating ---------------------------------------------------------------

fn is_animating_while_a_run_is_installed() {
    let controller = controller();
    let _run = controller.forward().expect("run starts");
    controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    assert_is_animating_everywhere(&controller, true);
}

fn is_animating_after_set_value_stops_the_run() {
    let controller = controller();
    let _run = controller.forward().expect("run starts");
    controller.set_value(0.5).expect("no build is running");
    assert_is_animating_everywhere(&controller, false);
}

fn is_animating_after_the_run_completes() {
    let controller = controller();
    let _run = controller.forward().expect("run starts");
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_is_animating_everywhere(&controller, false);
}

fn a_constant_phase_does_not_install_a_run() {
    use flui_animation::{AnimationSwitch, ConstantAnimation, Tween, TweenAnimation};
    use std::rc::Rc;
    for status in [
        AnimationStatus::Forward,
        AnimationStatus::Reverse,
        AnimationStatus::ForwardSettled,
        AnimationStatus::ReverseSettled,
    ] {
        let constant = ConstantAnimation::with_status(0.5_f64, status);
        assert!(!constant.is_animating(), "constant {status:?} has no run");
        let source: Rc<dyn Animation<f64>> = Rc::new(constant);
        for (name, animation) in [
            ("source", Rc::clone(&source)),
            (
                "proxy",
                Rc::new(ProxyAnimation::new(Rc::clone(&source))) as Rc<dyn Animation<f64>>,
            ),
            (
                "reverse",
                Rc::new(ReverseAnimation::new(Rc::clone(&source))),
            ),
            (
                "curve",
                Rc::new(CurvedAnimation::new(Rc::clone(&source), Linear)),
            ),
            (
                "tween",
                Rc::new(TweenAnimation::new(
                    Tween::new(10.0, 20.0),
                    Rc::clone(&source),
                )),
            ),
            (
                "switch",
                Rc::new(AnimationSwitch::new(Rc::clone(&source), None)),
            ),
        ] {
            assert!(
                !animation.is_animating(),
                "{name} over constant {status:?} has no run"
            );
        }
    }
}

fn a_switch_tracks_activity_after_hopping_to_a_live_source() {
    use std::rc::Rc;

    let registry = Vsync::new();
    let current = AnimationController::builder(Duration::from_secs(1))
        .initial_value(0.8)
        .build_on(Some(&registry));
    let next = AnimationController::builder(Duration::from_secs(1))
        .initial_value(0.3)
        .build_on(Some(&registry));
    let current_source: Rc<dyn Animation<f64>> = Rc::new(current.controller().clone());
    let next_source: Rc<dyn Animation<f64>> = Rc::new(next.controller().clone());
    let switch = AnimationSwitch::new(Rc::clone(&current_source), Some(Rc::clone(&next_source)));
    let tween = TweenAnimation::new(Tween::new(10.0, 20.0), Rc::new(switch.clone()));
    let run = next.controller().forward().unwrap();
    assert!(
        !switch.is_animating(),
        "the next train does not own the output yet"
    );
    assert!(!tween.is_animating());
    next.controller().tick_at(Duration::from_millis(600));
    assert!(
        Rc::ptr_eq(&switch.current(), &next_source),
        "the trains crossed"
    );
    assert!(switch.is_animating(), "the adopted source still has a run");
    assert!(tween.is_animating());
    assert!(
        (tween.value() - 19.0).abs() < 1e-9,
        "the tween reads the adopted value"
    );
    assert!(run.is_pending());

    current.controller().forward().unwrap();
    next.controller().tick_at(Duration::from_millis(700));
    assert!(run.is_complete());
    assert!(
        current.controller().is_animating(),
        "the old source is independently live"
    );
    assert!(
        !switch.is_animating(),
        "the old train cannot keep the output active"
    );
    assert!(!tween.is_animating());
    assert_eq!(tween.value(), 20.0);

    next.controller().reverse().unwrap();
    assert!(switch.is_animating());
    assert!(tween.is_animating());
    switch.dispose();
    assert!(
        !switch.is_animating(),
        "a disposed switch has no output run"
    );
    assert!(!tween.is_animating());
    assert!(
        next.controller().is_animating(),
        "disposal does not cancel the source"
    );
}

#[cfg(feature = "serde")]
fn admission_error_wire_values() {
    use flui_animation::{AnimationError, AnimationInput, RangeDefect};

    let controller = AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .build();
    for (error, expected) in [
        (
            controller
                .animate_to(f64::NAN, None)
                .expect_err("NaN target"),
            serde_json::json!({"NonFiniteInput": {"input": "Target", "value": "NaN"}}),
        ),
        (
            controller.forward().expect_err("missing upper endpoint"),
            serde_json::json!({"UnboundedTarget": {"input": "Target", "value": "Infinity"}}),
        ),
        (
            controller.reverse().expect_err("missing lower endpoint"),
            serde_json::json!({"UnboundedTarget": {"input": "Target", "value": "-Infinity"}}),
        ),
    ] {
        let wire = serde_json::to_value(&error).expect("serialize refused admission");
        let decoded: AnimationError =
            serde_json::from_value(wire.clone()).expect("decode refused admission");
        assert_eq!(wire, expected);
        match decoded {
            AnimationError::NonFiniteInput { input, value } => {
                assert_eq!(input, AnimationInput::Target);
                assert!(value.is_nan());
            }
            AnimationError::UnboundedTarget { input, value } => {
                assert_eq!(input, AnimationInput::Target);
                assert!(value.is_infinite());
                assert_eq!(
                    value.is_sign_negative(),
                    expected["UnboundedTarget"]["value"] == "-Infinity"
                );
            }
            other => panic!("unexpected decoded refusal: {other:?}"),
        }
    }
    let error = controller
        .repeat(flui_animation::RepeatMode::Restart)
        .expect_err("no finite range to repeat inside");
    let wire = serde_json::to_value(&error).expect("serialize repeat refusal");
    assert_eq!(wire["InvalidRepeatRange"]["min"], "-Infinity");
    assert_eq!(wire["InvalidRepeatRange"]["max"], "Infinity");
    assert_eq!(wire["InvalidRepeatRange"]["lower"], "-Infinity");
    assert_eq!(wire["InvalidRepeatRange"]["upper"], "Infinity");
    assert!(matches!(
        serde_json::from_value::<AnimationError>(wire).expect("decode repeat refusal"),
        AnimationError::InvalidRepeatRange { min, max, defect: RangeDefect::NonFinite, .. }
            if min == f64::NEG_INFINITY && max == f64::INFINITY
    ));
    for rejected in [
        serde_json::Value::Null,
        serde_json::json!("nan"),
        serde_json::json!("1.0"),
    ] {
        assert!(
            serde_json::from_value::<AnimationError>(serde_json::json!({
                "NonFiniteInput": {"input": "Target", "value": rejected}
            }))
            .is_err()
        );
    }
}

#[cfg(feature = "serde")]
fn directional_phase_wire_names() {
    for (phase, wire) in [
        (AnimationStatus::Forward, "\"Forward\""),
        (AnimationStatus::Reverse, "\"Reverse\""),
        (AnimationStatus::ForwardSettled, "\"ForwardSettled\""),
        (AnimationStatus::ReverseSettled, "\"ReverseSettled\""),
    ] {
        assert_eq!(
            serde_json::to_string(&phase).expect("serialize phase"),
            wire
        );
        assert_eq!(
            serde_json::from_str::<AnimationStatus>(wire).expect("deserialize phase"),
            phase
        );
    }
    for former in ["\"Completed\"", "\"Dismissed\""] {
        assert!(serde_json::from_str::<AnimationStatus>(former).is_err());
    }
}

// --- curved runs ------------------------------------------------------------------

/// A linear curve that answers `NaN` strictly inside `(0.3, 0.7)`.
#[derive(Debug)]
struct NanInTheMiddle;

impl Curve for NanInTheMiddle {
    fn transform(&self, t: f64) -> f64 {
        if t > 0.3 && t < 0.7 { f64::NAN } else { t }
    }
}

/// A curve that answers the same value for every interior `t`.
#[derive(Debug)]
struct Constant(f64);

impl Curve for Constant {
    fn transform(&self, _t: f64) -> f64 {
        self.0
    }
}

fn curved_run_never_publishes_a_non_finite_value() {
    let controller = controller();
    let published = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&published);
    let reader = controller.clone();
    controller.add_listener(std::rc::Rc::new(move || {
        sink.lock().expect("published values").push(reader.value());
    }));
    let _run = controller
        .animate_to_curved(1.0, Some(Duration::from_secs(1)), Arc::new(NanInTheMiddle))
        .expect("curved run");

    controller.tick_at(std::time::Duration::from_secs_f64(0.2));
    assert_eq!(controller.value(), 0.2);
    controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    assert_eq!(
        controller.value(),
        0.2,
        "a non-finite curve sample keeps the last finite value"
    );
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(controller.value(), 1.0, "the run still lands on its target");
    assert_eq!(controller.status(), AnimationStatus::ForwardSettled);
    let published = published.lock().expect("published values").clone();
    assert!(
        published.iter().all(|value| value.is_finite()),
        "value listeners never read a non-finite value: {published:?}"
    );
}

fn overshooting_curve_stays_within_bounds() {
    for (curve, bound) in [(1.5, 1.0), (-0.5, 0.0)] {
        let controller = controller();
        let _run = controller
            .animate_to_curved(1.0, Some(Duration::from_secs(1)), Arc::new(Constant(curve)))
            .expect("curved run");
        controller.tick_at(std::time::Duration::from_secs_f64(0.5));
        assert_eq!(
            controller.value(),
            bound,
            "a curve answering {curve} is clamped into the controller's bounds"
        );
    }
}

// --- Duration::MAX ------------------------------------------------------------------

fn max_duration_start_from(from: f64, start: fn(&AnimationController)) {
    let controller = AnimationController::builder(Duration::MAX).build();
    controller.set_value(from).expect("no build is running");
    let started = catch_unwind(AssertUnwindSafe(|| start(&controller)));
    assert!(
        started.is_ok(),
        "a Duration::MAX run starts without panicking"
    );
    assert_eq!(
        controller.value(),
        from,
        "starting the run leaves the value"
    );
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    let value = controller.value();
    assert!(
        value.is_finite() && (0.0..=1.0).contains(&value),
        "one second into an endless run the value is finite and in bounds: {value}"
    );
}

fn max_duration_forward_over_the_full_range() {
    max_duration_start_from(0.0, |controller| {
        let _run = controller.forward().expect("forward starts");
    });
}

fn max_duration_forward() {
    max_duration_start_from(0.25, |controller| {
        let _run = controller.forward().expect("forward starts");
    });
}

fn max_duration_reverse() {
    max_duration_start_from(0.25, |controller| {
        let _run = controller.reverse().expect("reverse starts");
    });
}

fn max_duration_animate_to() {
    max_duration_start_from(0.25, |controller| {
        let _run = controller
            .animate_to(0.75, Some(Duration::MAX))
            .expect("animate_to starts");
    });
}

// --- dispose ----------------------------------------------------------------------------

fn disposed_controller_ignores_set_value() {
    let mut owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = owner.controller().clone();
    controller.set_value(0.3).expect("no build is running");
    let notified = Arc::new(Mutex::new(0_usize));
    let sink = Arc::clone(&notified);
    controller.add_listener(std::rc::Rc::new(move || {
        *sink.lock().expect("notification count") += 1;
    }));
    owner.dispose();

    assert_eq!(
        controller.set_value(0.8),
        Err(flui_animation::AnimationError::Disposed)
    );

    assert_eq!(
        controller.value(),
        0.3,
        "set_value after dispose leaves the value"
    );
    assert_eq!(*notified.lock().expect("notification count"), 0);
}

fn disposed_controller_drops_a_late_status_listener() {
    let mut owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = owner.controller().clone();
    owner.dispose();
    let probe = Arc::new(());
    let capture = Arc::clone(&probe);
    controller
        .subscribe_status(std::rc::Rc::new(move |_| {
            let _ = &capture;
        }))
        .detach();
    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "a status listener added after dispose is not retained"
    );
}

fn disposed_controller_drops_a_late_value_listener() {
    let mut owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = owner.controller().clone();
    owner.dispose();
    let probe = Arc::new(());
    let capture = Arc::clone(&probe);
    let _id = controller.add_listener(std::rc::Rc::new(move || {
        let _ = &capture;
    }));
    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "a value listener added after dispose is not retained"
    );
}

// --- frame time edges ------------------------------------------------------------------

fn registered_run() -> (Vsync, DrivenController) {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let _run = owner.controller().forward().expect("run starts");
    (vsync, owner)
}

fn finite_clock_time_anchors_a_run() {
    let (vsync, owner) = registered_run();
    let controller = owner.controller();
    assert!(controller.value().is_finite());
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
    );
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
    );
    assert_eq!(
        controller.value(),
        0.5,
        "the run is anchored at the first finite frame"
    );
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
    );
    assert_eq!(
        controller.status(),
        AnimationStatus::ForwardSettled,
        "the run completes once time resumes"
    );
}

fn backwards_frame_time_resumes_the_run() {
    let (vsync, owner) = registered_run();
    let controller = owner.controller();
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(10.0)),
    );
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(9.0)),
    );
    assert_eq!(
        controller.value(),
        0.0,
        "time before the anchor samples the run's start"
    );
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(10.5)),
    );
    assert_eq!(controller.value(), 0.5);
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(11.0)),
    );
    assert_eq!(controller.status(), AnimationStatus::ForwardSettled);
}

fn repeated_frame_time_announces_completion_once() {
    let (vsync, owner) = registered_run();
    let controller = owner.controller();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&statuses);
    controller
        .subscribe_status(std::rc::Rc::new(move |status| {
            sink.lock().expect("status log").push(status);
        }))
        .detach();
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
    );
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
    );
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
    );
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(2.0)),
    );
    assert_eq!(
        *statuses.lock().expect("status log"),
        [AnimationStatus::ForwardSettled]
    );
}

fn explicit_finish_completes_the_original_finite_run() {
    use flui_animation::{MotionClock, PlaybackRate};
    use std::{cell::RefCell, rc::Rc};

    for (target, reverse) in [(1.0, false), (0.0, true), (0.25, false), (0.75, true)] {
        for paused in [false, true] {
            let registry = Vsync::new();
            let owner = AnimationController::builder(Duration::from_secs(1))
                .initial_value(0.4)
                .build_on(Some(&registry));
            let controller = owner.controller();
            let run = if reverse {
                controller.animate_back(target, Some(Duration::from_secs(1)))
            } else {
                controller.animate_to(target, Some(Duration::from_secs(1)))
            }
            .expect("finite admitted run");
            let mut clock = MotionClock::new();
            registry.tick_all(&clock.frame(Duration::ZERO));
            registry.tick_all(&clock.frame(Duration::from_millis(400)));
            assert_ne!(
                controller.value(),
                target,
                "finish starts at an interior sample"
            );
            assert!(run.is_pending());
            if paused {
                controller.set_playback_rate(PlaybackRate::try_new(0.0).unwrap());
            }
            let events = Rc::new(RefCell::new(Vec::new()));
            let value_events = Rc::clone(&events);
            let observed = controller.clone();
            let original = run.clone();
            let listener = controller.add_listener(Rc::new(move || {
                assert_eq!(observed.value(), target);
                assert!(!observed.is_animating());
                assert!(
                    original.is_complete(),
                    "outcome is committed before listeners"
                );
                value_events.borrow_mut().push("value");
            }));
            let status_events = Rc::clone(&events);
            let observed = controller.clone();
            let phase = if reverse {
                AnimationStatus::ReverseSettled
            } else {
                AnimationStatus::ForwardSettled
            };
            let subscription = controller.subscribe_status(Rc::new(move |status| {
                assert_eq!(
                    observed.value(),
                    target,
                    "endpoint precedes status delivery"
                );
                assert_eq!(status, phase);
                status_events.borrow_mut().push("status");
            }));
            let completion_events = Rc::clone(&events);
            run.when_complete_or_cancel(move |outcome| {
                assert!(
                    outcome.is_ok(),
                    "finish preserves successful original completion"
                );
                completion_events.borrow_mut().push("outcome");
            });
            controller.finish().expect("declared finite endpoint");
            assert!(run.is_complete());
            assert_eq!(controller.value(), target);
            assert_eq!(controller.status(), phase);
            assert_eq!(*events.borrow(), ["value", "status", "outcome"]);
            assert!(!registry.has_running());
            controller.finish().expect("idle finish is silent");
            registry.tick_all(&clock.frame(Duration::from_secs(2)));
            assert_eq!(*events.borrow(), ["value", "status", "outcome"]);
            controller.remove_listener(listener);
            drop(subscription);
        }
    }
}

fn explicit_finish_uses_the_last_repeat_leg() {
    use flui_animation::{Iterations, RepeatMode, RepeatSpec, ValueRange};
    let range = ValueRange::try_new(0.2, 0.8).expect("ordered range");
    for mode in [RepeatMode::Restart, RepeatMode::Alternate] {
        for count in [1, 2, 3, 4] {
            let controller = controller();
            let run = controller
                .repeat(
                    RepeatSpec::new(mode)
                        .with_range(range)
                        .with_iterations(Iterations::Finite(count)),
                )
                .expect("finite repeat");
            controller.tick_at(Duration::from_millis(250));
            let (endpoint, phase) = if mode == RepeatMode::Alternate && count % 2 == 0 {
                (0.2, AnimationStatus::ReverseSettled)
            } else {
                (0.8, AnimationStatus::ForwardSettled)
            };
            controller.finish().expect("finite repeat has a final leg");
            assert_eq!(controller.value(), endpoint);
            assert_eq!(controller.status(), phase);
            assert!(run.is_complete());
            assert!(!controller.is_animating());
        }
    }
}

fn explicit_finish_refuses_an_undeclared_endpoint_without_sampling() {
    use flui_animation::simulation::Simulation;
    use std::{cell::Cell, rc::Rc};

    struct UnknownEnd(Rc<Cell<usize>>);
    impl Simulation for UnknownEnd {
        fn x(&self, time: f64) -> f64 {
            self.0.set(self.0.get() + 1);
            0.25 + time * 0.1
        }
        fn dx(&self, _time: f64) -> f64 {
            0.1
        }
        fn is_done(&self, _time: f64) -> bool {
            false
        }
    }
    for custom in [false, true] {
        let controller = controller();
        let samples = Rc::new(Cell::new(0));
        let run = if custom {
            controller.animate_with(UnknownEnd(Rc::clone(&samples)))
        } else {
            controller.repeat(RepeatMode::Alternate)
        }
        .expect("admitted motion without a declared end");
        controller.tick_at(Duration::from_millis(400));
        let before = (controller.value(), controller.status(), samples.get());
        assert!(
            matches!(
                controller.finish(),
                Err(flui_animation::AnimationError::NoFiniteEndpoint)
            ),
            "no guessed endpoint"
        );
        assert_eq!(
            (controller.value(), controller.status(), samples.get()),
            before
        );
        assert!(run.is_pending());
        assert!(controller.is_animating());
        controller.tick_at(Duration::from_millis(700));
        assert_ne!(
            controller.value(),
            before.0,
            "refusal leaves the original run deliverable"
        );
        controller.stop().expect("caller may still cancel");
        assert!(run.is_canceled());
        let next = controller
            .animate_to(0.9, Some(Duration::from_secs(1)))
            .unwrap();
        controller
            .finish()
            .expect("subsequent finite run remains healthy");
        assert!(next.is_complete());
        assert_eq!(controller.value(), 0.9);
    }
}

fn explicit_finish_reaches_native_physical_endpoints() {
    use flui_animation::{MotionSpec, SpringDescription};
    let slow = SpringDescription::with_damping_ratio(1.0, 1e-12, 1.0).unwrap();
    for (fling, input, endpoint) in [(true, 1e-6, 1.0), (true, -1e-6, 0.0), (false, 0.75, 0.75)] {
        let controller = AnimationController::builder(Duration::from_secs(1))
            .initial_value(0.4)
            .build();
        let run = if fling {
            controller.fling_with(input, Some(slow))
        } else {
            controller.retarget(input, &MotionSpec::Spring(slow))
        }
        .unwrap();
        controller.tick_at(Duration::from_millis(400));
        assert_ne!(controller.value(), endpoint);
        controller.finish().unwrap();
        assert_eq!(controller.value(), endpoint);
        assert!(run.is_complete());
        assert!(!controller.is_animating());
        controller.tick_at(Duration::MAX);
        assert_eq!(controller.value(), endpoint);
    }
}

fn explicit_finish_distinguishes_parked_owned_work_and_disposal() {
    use flui_animation::{AnimationError, MotionClock, MotionOverride};
    let registry = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let controller = owner.controller().clone();
    let run = controller.repeat(RepeatMode::Alternate).unwrap();
    let mut clock = MotionClock::new();
    clock.set_override(MotionOverride::Reduce);
    registry.tick_all(&clock.frame(Duration::ZERO));
    assert!(run.is_pending());
    assert!(!registry.has_running(), "parked work requests no frames");
    assert_eq!(controller.finish(), Err(AnimationError::NoFiniteEndpoint));
    assert!(run.is_pending());
    clock.set_override(MotionOverride::Full);
    registry.tick_all(&clock.frame(Duration::from_millis(500)));
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(controller.value() > 0.0, "refused parked work can resume");
    owner.dispose();
    assert!(run.is_canceled());
    assert_eq!(controller.finish(), Err(AnimationError::Disposed));
}

fn explicit_finish_preserves_a_reentrant_replacement() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    for channel in ["value", "status", "outcome"] {
        let controller = controller();
        let original = controller.forward().unwrap();
        controller.tick_at(Duration::from_millis(400));
        let replacement = Rc::new(RefCell::new(None));
        let action: Rc<dyn Fn()> = {
            let controller = controller.clone();
            let original = original.clone();
            let replacement = Rc::clone(&replacement);
            let armed = Cell::new(true);
            Rc::new(move || {
                if !armed.replace(false) {
                    return;
                }
                assert_eq!(controller.value(), 1.0);
                assert!(original.is_complete());
                assert!(!controller.is_animating());
                controller.finish().unwrap();
                *replacement.borrow_mut() = Some(
                    controller
                        .animate_back(0.25, Some(Duration::from_secs(1)))
                        .unwrap(),
                );
            })
        };
        let value_listener =
            (channel == "value").then(|| controller.add_listener(Rc::clone(&action)));
        let status_subscription = (channel == "status").then(|| {
            let action = Rc::clone(&action);
            controller.subscribe_status(Rc::new(move |status| {
                if status == AnimationStatus::ForwardSettled {
                    action();
                }
            }))
        });
        if channel == "outcome" {
            let action = Rc::clone(&action);
            original.when_complete_or_cancel(move |outcome| {
                assert!(outcome.is_ok());
                action();
            });
        }
        controller.finish().unwrap();
        assert!(original.is_complete());
        assert!(
            controller.is_animating(),
            "{channel}: replacement survives outer finish"
        );
        let next = replacement
            .borrow()
            .as_ref()
            .expect("callback admitted a replacement")
            .clone();
        assert!(next.is_pending());
        controller.tick_at(Duration::from_millis(500));
        assert!((controller.value() - 0.625).abs() < 1e-12);
        controller.tick_at(Duration::from_secs(1));
        assert_eq!(controller.value(), 0.25);
        assert!(next.is_complete());
        if let Some(listener) = value_listener {
            controller.remove_listener(listener);
        }
        drop(status_subscription);
    }
}

fn explicit_finish_retires_sources_with_failure_custody() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct RetiringCurve {
        controller: AnimationController,
        drops: Rc<Cell<usize>>,
        replacement: Rc<RefCell<Option<flui_animation::AnimationRunFuture>>>,
        fails: bool,
    }
    impl Curve for RetiringCurve {
        fn transform(&self, time: f64) -> f64 {
            time
        }
    }
    impl Drop for RetiringCurve {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            assert_eq!(self.controller.value(), 1.0);
            assert!(!self.controller.is_animating());
            let next = self
                .controller
                .animate_back(0.25, Some(Duration::from_secs(1)))
                .unwrap();
            *self.replacement.borrow_mut() = Some(next);
            assert!(!self.fails, "source retirement failure");
        }
    }
    for mode in [
        "healthy",
        "source",
        "value",
        "status",
        "value+source",
        "status+source",
    ] {
        let controller = controller();
        let drops = Rc::new(Cell::new(0));
        let replacement = Rc::new(RefCell::new(None));
        let original = controller
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                RetiringCurve {
                    controller: controller.clone(),
                    drops: Rc::clone(&drops),
                    replacement: Rc::clone(&replacement),
                    fails: mode.contains("source"),
                },
            )
            .unwrap();
        controller.tick_at(Duration::from_millis(400));
        let value_listener = mode.contains("value").then(|| {
            let armed = Cell::new(true);
            controller.add_listener(Rc::new(move || {
                assert!(!armed.replace(false), "value delivery failure");
            }))
        });
        let status_subscription = mode.contains("status").then(|| {
            let armed = Cell::new(true);
            controller.subscribe_status(Rc::new(move |status| {
                if status == AnimationStatus::ForwardSettled {
                    assert!(!armed.replace(false), "status delivery failure");
                }
            }))
        });
        let outcomes = Rc::new(Cell::new(0));
        let delivered = Rc::clone(&outcomes);
        original.when_complete_or_cancel(move |outcome| {
            assert!(outcome.is_ok());
            delivered.set(delivered.get() + 1);
        });
        let result = catch_unwind(AssertUnwindSafe(|| controller.finish()));
        if mode == "healthy" {
            assert_eq!(result.unwrap(), Ok(()));
        } else {
            let failure = result.expect_err("the first failure is propagated after cleanup");
            let expected = if mode.contains("value") {
                "value delivery failure"
            } else if mode.contains("status") {
                "status delivery failure"
            } else {
                "source retirement failure"
            };
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some(expected)
            );
        }
        assert!(
            original.is_complete(),
            "failure cannot turn accepted success into cancellation"
        );
        assert_eq!(outcomes.get(), 1);
        let failed_before_retirement = mode.contains("value") || mode.contains("status");
        assert_eq!(drops.get(), usize::from(!failed_before_retirement));
        if let Some(listener) = value_listener {
            controller.remove_listener(listener);
        }
        drop(status_subscription);
        let next = if let Some(next) = replacement.borrow_mut().take() {
            next
        } else {
            controller
                .animate_back(0.25, Some(Duration::from_secs(1)))
                .unwrap()
        };
        controller.tick_at(Duration::from_secs(1));
        assert_eq!(
            controller.value(),
            0.25,
            "{mode}: the next admitted run makes progress"
        );
        assert!(next.is_complete());
        assert_eq!(outcomes.get(), 1);
    }
}

#[test]
fn controller_robustness_contract() {
    crate::run_table(&[
        (
            "typed input refusals",
            typed_refusals_preserve_the_original_run,
        ),
        (
            "overflowing repeat span",
            overflowing_repeat_span_preserves_the_run,
        ),
        (
            "explicit finish releases its owner during delivery",
            explicit_finish_allows_owner_release_during_delivery,
        ),
        (
            "explicit finish delivers a failing future waker",
            explicit_finish_preserves_success_after_waker_failure,
        ),
        (
            "explicit finish retains failure custody",
            explicit_finish_retires_sources_with_failure_custody,
        ),
        (
            "explicit finish preserves reentrant replacements",
            explicit_finish_preserves_a_reentrant_replacement,
        ),
        (
            "explicit finish reaches physical endpoints",
            explicit_finish_reaches_native_physical_endpoints,
        ),
        (
            "explicit finish distinguishes parked work and disposal",
            explicit_finish_distinguishes_parked_owned_work_and_disposal,
        ),
        (
            "explicit finish preserves the original future",
            explicit_finish_completes_the_original_finite_run,
        ),
        (
            "explicit finish lands on the final repeat leg",
            explicit_finish_uses_the_last_repeat_leg,
        ),
        (
            "explicit finish refuses unknown endpoints atomically",
            explicit_finish_refuses_an_undeclared_endpoint_without_sampling,
        ),
        #[cfg(feature = "serde")]
        ("admission error wire values", admission_error_wire_values),
        #[cfg(feature = "serde")]
        ("directional phase wire names", directional_phase_wire_names),
        (
            "a constant phase does not install a run",
            a_constant_phase_does_not_install_a_run,
        ),
        (
            "switch activity follows the adopted live train",
            a_switch_tracks_activity_after_hopping_to_a_live_source,
        ),
        (
            "is_animating after the run completes",
            is_animating_after_the_run_completes,
        ),
        ("Duration::MAX forward", max_duration_forward),
        ("Duration::MAX reverse", max_duration_reverse),
        ("Duration::MAX animate_to", max_duration_animate_to),
        (
            "backwards frame time resumes the run",
            backwards_frame_time_resumes_the_run,
        ),
        (
            "repeated frame time announces completion once",
            repeated_frame_time_announces_completion_once,
        ),
    ]);
}

fn explicit_finish_allows_owner_release_during_delivery() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    for channel in ["value", "status", "outcome"] {
        let registry = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let controller = owner.controller().clone();
        let run = controller.forward().unwrap();
        controller.tick_at(Duration::from_millis(400));
        let owner = Rc::new(RefCell::new(Some(owner)));
        let releases = Rc::new(Cell::new(0));
        let release: Rc<dyn Fn()> = {
            let owner = Rc::clone(&owner);
            let releases = Rc::clone(&releases);
            let controller = controller.clone();
            let run = run.clone();
            Rc::new(move || {
                assert_eq!(controller.value(), 1.0);
                assert!(run.is_complete());
                let outgoing = owner.borrow_mut().take();
                drop(outgoing);
                releases.set(releases.get() + 1);
            })
        };
        if channel == "value" {
            controller.add_listener(Rc::clone(&release));
        } else if channel == "status" {
            let release = Rc::clone(&release);
            controller
                .subscribe_status(Rc::new(move |_| release()))
                .detach();
        } else {
            let release = Rc::clone(&release);
            run.when_complete_or_cancel(move |outcome| {
                assert!(outcome.is_ok());
                release();
            });
        }
        controller.finish().unwrap();
        assert!(
            run.is_complete(),
            "{channel}: committed success survives owner release"
        );
        assert_eq!(releases.get(), 1);
        assert!(owner.borrow().is_none());
        assert!(registry.is_empty());
        assert_eq!(
            controller.finish(),
            Err(flui_animation::AnimationError::Disposed)
        );
    }
}

fn explicit_finish_preserves_success_after_waker_failure() {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Wake, Waker};

    struct FailingWake(Arc<AtomicUsize>);
    impl Wake for FailingWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("completion wake failure");
        }
    }
    let controller = controller();
    let mut run = controller.forward().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(FailingWake(Arc::clone(&calls))));
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    let failure = catch_unwind(AssertUnwindSafe(|| controller.finish())).expect_err("wake fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("completion wake failure")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(controller.value(), 1.0);
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    ));
    controller.finish().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let next = controller.reverse().unwrap();
    controller.finish().unwrap();
    assert_eq!(controller.value(), 0.0);
    assert!(next.is_complete());
}

fn typed_refusals_preserve_the_original_run() {
    use flui_animation::{AnimationError, AnimationInput, AnimationRunFuture};
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    type Attempt = fn(&AnimationController) -> Result<AnimationRunFuture, AnimationError>;
    let attempts: [(AnimationInput, Attempt); 3] = [
        (AnimationInput::Target, |controller| {
            controller.animate_to(f64::NAN, None)
        }),
        (AnimationInput::From, |controller| {
            controller.forward_from(Some(f64::NAN))
        }),
        (AnimationInput::Velocity, |controller| {
            controller.fling(f64::NAN)
        }),
    ];
    let controller = controller();
    let mut run = controller.forward().expect("finite run");
    controller.tick_at(Duration::from_millis(250));
    let mut context = Context::from_waker(Waker::noop());
    for (expected, attempt) in attempts {
        assert!(
            matches!(attempt(&controller), Err(AnimationError::NonFiniteInput {
            input, value,
        }) if input == expected && value.is_nan())
        );
        assert_eq!(controller.value(), 0.25);
        assert_eq!(controller.status(), AnimationStatus::Forward);
        assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Pending);
    }
    controller.tick_at(Duration::from_secs(1));
    assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Ready(Ok(())));

    controller.reset().expect("live controller");
    let mut clamped = controller
        .animate_to(f64::INFINITY, None)
        .expect("finite bound");
    controller.tick_at(Duration::from_secs(1));
    assert_eq!(controller.value(), 1.0);
    assert_eq!(
        Pin::new(&mut clamped).poll(&mut context),
        Poll::Ready(Ok(()))
    );

    let unbounded = AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .build();
    let mut run = unbounded.animate_to(10.0, None).expect("finite target");
    unbounded.tick_at(Duration::from_millis(250));
    assert!(
        matches!(unbounded.forward(), Err(AnimationError::UnboundedTarget {
        input: AnimationInput::Target, value,
    }) if value == f64::INFINITY)
    );
    assert_eq!(unbounded.value(), 2.5);
    assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Pending);
    unbounded.tick_at(Duration::from_secs(1));
    assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Ready(Ok(())));

    let extreme = AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .initial_value(-f64::MAX)
        .build();
    let mut run = extreme
        .animate_to(-f64::MAX / 2.0, None)
        .expect("finite span");
    assert!(
        matches!(extreme.animate_to(f64::MAX, None), Err(AnimationError::SpanOverflow {
        from, to,
    }) if from == -f64::MAX && to == f64::MAX)
    );
    assert_eq!(extreme.value(), -f64::MAX);
    assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Pending);
    extreme.tick_at(Duration::from_secs(1));
    assert_eq!(extreme.value(), -f64::MAX / 2.0);
    assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Ready(Ok(())));
}

fn overflowing_repeat_span_preserves_the_run() {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    let controller = AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .build();
    let mut run = controller.animate_to(10.0, None).expect("finite run");
    controller.tick_at(Duration::from_millis(250));
    assert_eq!(controller.value(), 2.5);
    // Finite endpoints whose span overflows never become a repeat range.
    assert_eq!(
        flui_animation::ValueRange::try_new(-f64::MAX, f64::MAX),
        Err(flui_animation::RangeDefect::SpanOverflow),
        "finite endpoints must not admit an overflowing repeat span"
    );
    // The repeat this unbounded controller can still be asked for, over its
    // infinite bounds, is refused before admission.
    let refused = controller.repeat(flui_animation::RepeatMode::Alternate);
    assert!(
        matches!(refused, Err(flui_animation::AnimationError::InvalidRepeatRange {
        min, max, defect: flui_animation::RangeDefect::NonFinite, ..
    }) if min == f64::NEG_INFINITY && max == f64::INFINITY),
        "an unbounded controller has no range to repeat inside"
    );
    assert_eq!(
        controller.value(),
        2.5,
        "refusal preserves the displayed value"
    );
    assert!(controller.is_animating());
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Pending);
    controller.tick_at(Duration::from_secs(1));
    assert_eq!(controller.value(), 10.0);
    assert_eq!(Pin::new(&mut run).poll(&mut context), Poll::Ready(Ok(())));
}

#[test]
fn is_animating_tracks_an_installed_run() {
    is_animating_while_a_run_is_installed();
}

#[test]
fn is_animating_false_after_set_value() {
    is_animating_after_set_value_stops_the_run();
}

#[test]
fn curved_run_holds_the_last_finite_value() {
    curved_run_never_publishes_a_non_finite_value();
}

#[test]
fn curved_run_clamps_overshoot() {
    overshooting_curve_stays_within_bounds();
}

#[test]
fn max_duration_full_range_starts() {
    max_duration_forward_over_the_full_range();
}

#[test]
fn disposed_set_value_is_refused() {
    disposed_controller_ignores_set_value();
}

#[test]
fn disposed_drops_late_status_listener() {
    disposed_controller_drops_a_late_status_listener();
}

#[test]
fn disposed_drops_late_value_listener() {
    disposed_controller_drops_a_late_value_listener();
}

#[test]
fn finite_clock_time_is_anchored() {
    finite_clock_time_anchors_a_run();
}
