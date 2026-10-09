//! Controller robustness: `is_animating` means "a run is installed" through
//! every wrapper, a curved run never publishes a non-finite or out-of-bounds
//! value, extreme durations and times never panic or poison the timeline, and
//! a disposed controller neither changes nor keeps what it is handed.
//!
//! Rows that hold today run in the table. A row whose behaviour is not there
//! yet is its own `#[ignore = "contract: …"]` test, so `--run-ignored` shows
//! it failing on the assertion that names the behaviour.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, CurvedAnimation, ProxyAnimation,
    ReverseAnimation, Vsync,
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
            CurvedAnimation::new(source, Linear).is_animating(),
        ),
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
    controller.set_value(0.5);
    assert_is_animating_everywhere(&controller, false);
}

fn is_animating_after_the_run_completes() {
    let controller = controller();
    let _run = controller.forward().expect("run starts");
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_is_animating_everywhere(&controller, false);
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
    assert_eq!(controller.status(), AnimationStatus::Completed);
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
    controller.set_value(from);
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
    let controller = controller();
    controller.set_value(0.3);
    let notified = Arc::new(Mutex::new(0_usize));
    let sink = Arc::clone(&notified);
    controller.add_listener(std::rc::Rc::new(move || {
        *sink.lock().expect("notification count") += 1;
    }));
    controller.dispose();

    controller.set_value(0.8);

    assert_eq!(
        controller.value(),
        0.3,
        "set_value after dispose leaves the value"
    );
    assert_eq!(*notified.lock().expect("notification count"), 0);
}

fn disposed_controller_drops_a_late_status_listener() {
    let controller = controller();
    controller.dispose();
    let probe = Arc::new(());
    let capture = Arc::clone(&probe);
    let _id = controller.add_status_listener(std::rc::Rc::new(move |_| {
        let _ = &capture;
    }));
    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "a status listener added after dispose is not retained"
    );
}

fn disposed_controller_drops_a_late_value_listener() {
    let controller = controller();
    controller.dispose();
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

fn registered_run() -> (Vsync, AnimationController) {
    let vsync = Vsync::new();
    let controller = controller();
    let _registration = vsync.register(controller.clone());
    let _run = controller.forward().expect("run starts");
    (vsync, controller)
}

fn finite_clock_time_anchors_a_run() {
    let (vsync, controller) = registered_run();
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
        AnimationStatus::Completed,
        "the run completes once time resumes"
    );
}

fn backwards_frame_time_resumes_the_run() {
    let (vsync, controller) = registered_run();
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
    assert_eq!(controller.status(), AnimationStatus::Completed);
}

fn repeated_frame_time_announces_completion_once() {
    let (vsync, controller) = registered_run();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&statuses);
    controller.add_status_listener(std::rc::Rc::new(move |status| {
        sink.lock().expect("status log").push(status);
    }));
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
        [AnimationStatus::Completed]
    );
}

#[test]
fn controller_robustness_contract() {
    crate::run_table(&[
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
