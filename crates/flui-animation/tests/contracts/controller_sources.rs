//! Controller-owned user code runs without its state lock and cannot commit stale samples.

use flui_animation::{
    Animation, AnimationController,
    curve::Curve,
    simulation::{Simulation, Tolerance},
};
use flui_scheduler::ticker::TickerFuture;
use std::{
    future::Future,
    io::Read,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum Action {
    Read,
    Replace,
    Stop,
    Nested,
    PanicAfterStop,
}
#[derive(Clone, Copy)]
enum Method {
    Position,
    Velocity,
    Done,
    Curve,
}

struct Hook {
    controller: AnimationController,
    action: Action,
    armed: AtomicBool,
    replacement: Arc<Mutex<Option<TickerFuture>>>,
}
impl Hook {
    fn run(&self) {
        if !self.armed.swap(false, Ordering::SeqCst) {
            return;
        }
        match self.action {
            Action::Read => {
                assert!(self.controller.value().is_finite());
                let _ = self.controller.status();
            }
            Action::Replace => {
                let next = self
                    .controller
                    .animate_to(0.9, Some(Duration::from_secs(1)))
                    .expect("replacement run");
                *self.replacement.lock().expect("replacement future") = Some(next);
            }
            Action::Stop => {
                self.controller.stop().expect("stop from sample");
            }
            Action::Nested => self.controller.tick_at(0.75),
            Action::PanicAfterStop => {
                self.controller.stop().expect("stop before sample panic");
                panic!("authoritative sample failure");
            }
        }
    }
}
struct CustomSimulation {
    hook: Arc<Hook>,
    method: Method,
}
impl Simulation for CustomSimulation {
    fn x(&self, time: f64) -> f64 {
        if time > 0.0 && matches!(self.method, Method::Position) {
            self.hook.run();
        }
        time.min(1.0)
    }
    fn dx(&self, _: f64) -> f64 {
        if matches!(self.method, Method::Velocity) {
            self.hook.run();
        }
        1.0
    }
    fn is_done(&self, time: f64) -> bool {
        if matches!(self.method, Method::Done) {
            self.hook.run();
        }
        time >= 1.0
    }
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}
struct CustomCurve(Arc<Hook>);
impl Curve for CustomCurve {
    fn transform(&self, t: f64) -> f64 {
        self.0.run();
        t
    }
}

fn poll(future: &mut TickerFuture) -> Poll<Result<(), flui_scheduler::ticker::TickerCanceled>> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}
fn exercise(method: Method, action: Action) {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let replacement = Arc::new(Mutex::new(None));
    let hook = Arc::new(Hook {
        controller: controller.clone(),
        action,
        armed: AtomicBool::new(true),
        replacement: replacement.clone(),
    });
    let mut original = if matches!(method, Method::Curve) {
        controller
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                Arc::new(CustomCurve(hook)),
            )
            .expect("curved run")
    } else {
        controller
            .animate_with(CustomSimulation { hook, method })
            .expect("simulation run")
    };
    if matches!(method, Method::Velocity) {
        assert_eq!(controller.velocity(), 1.0);
    } else {
        controller.tick_at(0.25);
    }
    match action {
        Action::Read => {
            if !matches!(method, Method::Velocity) {
                assert_eq!(controller.value(), 0.25);
            }
            controller.tick_at(1.0);
            assert_eq!(poll(&mut original), Poll::Ready(Ok(())));
        }
        Action::Replace => {
            assert_eq!(
                controller.value(),
                0.0,
                "stale sample cannot overwrite replacement start"
            );
            assert!(matches!(poll(&mut original), Poll::Ready(Err(_))));
            let mut next = replacement
                .lock()
                .expect("replacement")
                .take()
                .expect("new future installed");
            assert!(poll(&mut next).is_pending());
            controller.tick_at(1.0);
            assert_eq!(controller.value(), 0.9);
            assert_eq!(poll(&mut next), Poll::Ready(Ok(())));
        }
        Action::Stop => {
            assert_eq!(controller.value(), 0.0);
            assert!(matches!(poll(&mut original), Poll::Ready(Err(_))));
            controller.tick_at(0.8);
            assert_eq!(controller.value(), 0.0);
        }
        Action::Nested => {
            assert_eq!(
                controller.value(),
                0.75,
                "outer sample must not rewind nested tick"
            );
            controller.tick_at(1.0);
            assert_eq!(poll(&mut original), Poll::Ready(Ok(())));
        }
        Action::PanicAfterStop => unreachable!(),
    }
    controller.dispose();
}
fn position_may_read() {
    exercise(Method::Position, Action::Read);
}
fn velocity_may_read() {
    exercise(Method::Velocity, Action::Read);
}
fn completion_may_read() {
    exercise(Method::Done, Action::Read);
}
fn curve_may_read() {
    exercise(Method::Curve, Action::Read);
}
fn position_may_replace() {
    exercise(Method::Position, Action::Replace);
}
fn curve_may_replace() {
    exercise(Method::Curve, Action::Replace);
}
fn position_may_stop() {
    exercise(Method::Position, Action::Stop);
}
fn curve_may_stop() {
    exercise(Method::Curve, Action::Stop);
}
fn nested_position_tick_wins() {
    exercise(Method::Position, Action::Nested);
}
fn nested_curve_tick_wins() {
    exercise(Method::Curve, Action::Nested);
}

fn completion_may_replace() {
    exercise(Method::Done, Action::Replace);
}
fn completion_may_stop() {
    exercise(Method::Done, Action::Stop);
}
fn nested_completion_tick_wins() {
    exercise(Method::Done, Action::Nested);
}

struct SourceDrop {
    controller: AnimationController,
    drops: Arc<AtomicUsize>,
}
impl Drop for SourceDrop {
    fn drop(&mut self) {
        assert!(self.controller.value().is_finite());
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl Simulation for SourceDrop {
    fn x(&self, time: f64) -> f64 {
        time.min(1.0)
    }
    fn dx(&self, _: f64) -> f64 {
        1.0
    }
    fn is_done(&self, time: f64) -> bool {
        time >= 1.0
    }
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}
fn source_retirement(operation: fn(&AnimationController)) {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let drops = Arc::new(AtomicUsize::new(0));
    let _future = controller
        .animate_with(SourceDrop {
            controller: controller.clone(),
            drops: drops.clone(),
        })
        .expect("owned source");
    operation(&controller);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "controller retires its source outside the lock"
    );
    controller.dispose();
}
fn stop_retires_source() {
    source_retirement(|controller| controller.stop().expect("stop"));
}
fn replacement_retires_source() {
    source_retirement(|controller| {
        let _future = controller.forward().expect("forward");
    });
}
fn completion_retires_source() {
    source_retirement(|controller| controller.tick_at(1.0));
}
fn dispose_retires_source() {
    source_retirement(AnimationController::dispose);
}

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("opaque capture destructor");
    }
}
struct HostileSimulation {
    hook: Arc<Hook>,
    _first: Bomb,
    _second: Bomb,
    initial: bool,
}
impl Simulation for HostileSimulation {
    fn x(&self, time: f64) -> f64 {
        assert!(!self.initial, "authoritative initial sample failure");
        if time > 0.0 {
            self.hook.run();
        }
        0.0
    }
    fn dx(&self, _: f64) -> f64 {
        0.0
    }
    fn is_done(&self, _: f64) -> bool {
        false
    }
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}
fn hostile_source_after_sample_failure(initial: bool) {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let drops = Arc::new(AtomicUsize::new(0));
    let source = HostileSimulation {
        hook: Arc::new(Hook {
            controller: controller.clone(),
            action: Action::PanicAfterStop,
            armed: AtomicBool::new(true),
            replacement: Arc::new(Mutex::new(None)),
        }),
        _first: Bomb(drops.clone()),
        _second: Bomb(drops.clone()),
        initial,
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _future = controller
            .animate_with(source)
            .expect("finite initial source");
        controller.tick_at(0.25);
    }));
    let payload = outcome.expect_err("source callback fails");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some(if initial {
            "authoritative initial sample failure"
        } else {
            "authoritative sample failure"
        })
    );
    std::mem::forget(payload);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "opaque aggregate retained after source failure"
    );
    controller.set_value(0.0);
    let mut next = controller.forward().expect("subsequent run");
    controller.tick_at(1.0);
    assert_eq!(controller.value(), 1.0);
    assert_eq!(poll(&mut next), Poll::Ready(Ok(())));
    controller.dispose();
}
fn initial_failure_retains_hostile_source() {
    hostile_source_after_sample_failure(true);
}
fn sample_failure_retains_hostile_source() {
    hostile_source_after_sample_failure(false);
}

struct CurveDrop {
    _source: SourceDrop,
}
impl Curve for CurveDrop {
    fn transform(&self, t: f64) -> f64 {
        t
    }
}
fn curve_retirement_may_reenter() {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let drops = Arc::new(AtomicUsize::new(0));
    let _future = controller
        .animate_to_curved(
            1.0,
            Some(Duration::from_secs(1)),
            Arc::new(CurveDrop {
                _source: SourceDrop {
                    controller: controller.clone(),
                    drops: drops.clone(),
                },
            }),
        )
        .expect("owned curve");
    controller.stop().expect("curve stop");
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    controller.dispose();
}
fn callback_retirement_may_reenter() {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let drops = Arc::new(AtomicUsize::new(0));
    let probe = SourceDrop {
        controller: controller.clone(),
        drops: drops.clone(),
    };
    let id = controller.add_status_listener(Arc::new(move |_| {
        let _owned = &probe;
    }));
    controller.remove_status_listener(id);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let probe = SourceDrop {
        controller: controller.clone(),
        drops: drops.clone(),
    };
    controller.add_status_listener(Arc::new(move |_| {
        let _owned = &probe;
    }));
    controller.dispose();
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}
fn status_failure_retains_retired_source_and_callback() {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let drops = Arc::new(AtomicUsize::new(0));
    let source = HostileSimulation {
        hook: Arc::new(Hook {
            controller: controller.clone(),
            action: Action::Read,
            armed: AtomicBool::new(false),
            replacement: Arc::new(Mutex::new(None)),
        }),
        _first: Bomb(drops.clone()),
        _second: Bomb(drops.clone()),
        initial: false,
    };
    let mut original = controller.animate_with(source).expect("source installed");
    let callback_id = Arc::new(Mutex::new(None));
    let callback_id_read = callback_id.clone();
    let callback_controller = controller.clone();
    let first = Bomb(drops.clone());
    let second = Bomb(drops.clone());
    let id = controller.add_status_listener(Arc::new(move |_| {
        let _opaque = (&first, &second);
        let id = callback_id_read
            .lock()
            .expect("callback id")
            .take()
            .expect("registered id");
        callback_controller.remove_status_listener(id);
        panic!("authoritative status failure");
    }));
    *callback_id.lock().expect("install callback id") = Some(id);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        controller.stop().expect("stop source");
    }));
    let payload = outcome.expect_err("status callback fails");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some("authoritative status failure")
    );
    std::mem::forget(payload);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "retired source and last callback snapshot retained"
    );
    assert!(
        matches!(poll(&mut original), Poll::Ready(Err(_))),
        "cancellation delivered despite notification failure"
    );
    controller.set_value(0.0);
    let mut next = controller
        .forward()
        .expect("next run after notification failure");
    controller.tick_at(1.0);
    assert_eq!(poll(&mut next), Poll::Ready(Ok(())));
    assert_eq!(controller.value(), 1.0);
    controller.dispose();
}

struct FailingRetirement;
impl Drop for FailingRetirement {
    fn drop(&mut self) {
        panic!("authoritative retirement failure");
    }
}
impl Simulation for FailingRetirement {
    fn x(&self, _: f64) -> f64 {
        0.0
    }
    fn dx(&self, _: f64) -> f64 {
        0.0
    }
    fn is_done(&self, _: f64) -> bool {
        false
    }
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}
fn ordinary_retirement_failure_preserves_new_run() {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let mut old = controller
        .animate_with(FailingRetirement)
        .expect("retiring source");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| controller.forward()));
    let payload = outcome.expect_err("old source retirement fails");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some("authoritative retirement failure")
    );
    std::mem::forget(payload);
    assert!(matches!(poll(&mut old), Poll::Ready(Err(_))));
    controller.tick_at(1.0);
    assert_eq!(
        controller.value(),
        1.0,
        "replacement remains usable after old source retirement failure"
    );
    controller.set_value(0.0);
    let mut next = controller.forward().expect("later run");
    controller.tick_at(1.0);
    assert_eq!(poll(&mut next), Poll::Ready(Ok(())));
    controller.dispose();
}
fn retirement_failure_retains_later_callback_envelope() {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let _future = controller
        .animate_with(FailingRetirement)
        .expect("retiring source");
    let drops = Arc::new(AtomicUsize::new(0));
    let first = Bomb(drops.clone());
    let second = Bomb(drops.clone());
    controller.add_status_listener(Arc::new(move |_| {
        let _opaque = (&first, &second);
    }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| controller.dispose()));
    let payload = outcome.expect_err("source retirement fails");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some("authoritative retirement failure")
    );
    std::mem::forget(payload);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "remaining callback envelope retained after first retirement failure"
    );
    controller.dispose();
    assert!(controller.value().is_finite());
    assert!(matches!(
        controller.forward(),
        Err(flui_animation::AnimationError::Disposed)
    ));
}

#[test]
fn controller_sources_allow_reentry_and_preserve_run_ownership() {
    let cases: &[(&str, fn())] = &[
        ("curve retirement may reenter", curve_retirement_may_reenter),
        (
            "callback retirement may reenter",
            callback_retirement_may_reenter,
        ),
        (
            "status failure retains sources",
            status_failure_retains_retired_source_and_callback,
        ),
        (
            "ordinary retirement failure preserves new run",
            ordinary_retirement_failure_preserves_new_run,
        ),
        (
            "retirement failure retains later callback",
            retirement_failure_retains_later_callback_envelope,
        ),
        ("position may read", position_may_read),
        ("velocity may read", velocity_may_read),
        ("completion may read", completion_may_read),
        ("completion may replace", completion_may_replace),
        ("completion may stop", completion_may_stop),
        ("nested completion tick wins", nested_completion_tick_wins),
        ("curve may read", curve_may_read),
        ("position may replace", position_may_replace),
        ("curve may replace", curve_may_replace),
        ("position may stop", position_may_stop),
        ("curve may stop", curve_may_stop),
        ("nested position tick wins", nested_position_tick_wins),
        ("nested curve tick wins", nested_curve_tick_wins),
        ("stop retires source", stop_retires_source),
        ("replacement retires source", replacement_retires_source),
        ("completion retires source", completion_retires_source),
        ("dispose retires source", dispose_retires_source),
        (
            "initial failure retains hostile source",
            initial_failure_retains_hostile_source,
        ),
        (
            "sample failure retains hostile source",
            sample_failure_retains_hostile_source,
        ),
    ];
    const SELECTED: &str = "FLUI_CONTROLLER_SOURCE_CASE";
    if let Ok(selected) = std::env::var(SELECTED) {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case")
            .1();
        return;
    }
    let mut failures = Vec::new();
    for (name, _) in cases {
        let mut child = std::process::Command::new(
            std::env::current_exe().expect("test executable"),
        )
        .args([
            "--exact",
            "controller_sources::controller_sources_allow_reentry_and_preserve_run_ownership",
            "--nocapture",
        ])
        .env(SELECTED, name)
        .env("RUST_BACKTRACE", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("controller child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut output = String::new();
            stdout.read_to_string(&mut output).expect("stdout read");
            output
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut output = String::new();
            stderr.read_to_string(&mut output).expect("stderr read");
            output
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill stalled child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
