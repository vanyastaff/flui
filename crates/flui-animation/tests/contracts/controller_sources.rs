//! Controller-owned user code runs without its state lock and cannot commit stale samples.

use crate::child_process;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, ConstantAnimation,
    CurvedAnimation, ProxyAnimation, StatusCallback,
    curve::{Curve, Split},
    simulation::{Simulation, Tolerance},
};
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use flui_scheduler::{Ticker, UpdateScheduler, ticker::TickerFuture};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
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

#[derive(Clone, Debug)]
struct TerminalProbe {
    label: &'static str,
    drops: Arc<Mutex<Vec<&'static str>>>,
    panics: bool,
}
impl Drop for TerminalProbe {
    fn drop(&mut self) {
        self.drops
            .lock()
            .expect("terminal observations")
            .push(self.label);
        assert!(!self.panics, "{}", self.label);
    }
}
#[derive(Clone, Debug)]
struct TerminalCurve(
    #[expect(
        dead_code,
        reason = "the curve owns this destructor probe without reading it"
    )]
    TerminalProbe,
);
impl Curve for TerminalCurve {
    fn transform(&self, time: f64) -> f64 {
        time
    }
}
#[derive(Clone, Copy)]
enum TerminalOwner {
    Controller,
    Proxy,
    Curved,
    Switch,
}

fn terminal_fixture(
    kind: TerminalOwner,
    first: TerminalProbe,
    second: TerminalProbe,
) -> Box<dyn std::any::Any + Send> {
    match kind {
        TerminalOwner::Controller => {
            let controller = AnimationController::without_ticker(Duration::from_secs(1));
            controller.add_status_listener(Arc::new(move |_| {
                let _capture = &first;
            }));
            controller.add_status_listener(Arc::new(move |_| {
                let _capture = &second;
            }));
            Box::new(controller)
        }
        TerminalOwner::Proxy => {
            let proxy = ProxyAnimation::new(Arc::new(ConstantAnimation::completed(0.5)));
            proxy.add_status_listener(Arc::new(move |_| {
                let _capture = &first;
            }));
            proxy.add_status_listener(Arc::new(move |_| {
                let _capture = &second;
            }));
            Box::new(proxy)
        }
        TerminalOwner::Curved => Box::new(
            CurvedAnimation::new(
                Arc::new(ConstantAnimation::completed(0.5)),
                TerminalCurve(first),
            )
            .with_reverse_curve(TerminalCurve(second)),
        ),
        TerminalOwner::Switch => {
            let switch = AnimationSwitch::new(
                Arc::new(ConstantAnimation::completed(0.75)),
                Some(Arc::new(ConstantAnimation::completed(0.25))),
            );
            switch.add_status_listener(Arc::new(move |_| {
                let _capture = &first;
            }));
            switch.add_status_listener(Arc::new(move |_| {
                let _capture = &second;
            }));
            Box::new(switch)
        }
    }
}

fn terminal_owner_matrix(kind: TerminalOwner) {
    for (panics, incoming) in [(false, false), (true, false), (true, true)] {
        let drops = Arc::new(Mutex::new(Vec::new()));
        let probe = |label| TerminalProbe {
            label,
            drops: drops.clone(),
            panics,
        };
        let owner = terminal_fixture(kind, probe("terminal first"), probe("terminal second"));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            if incoming {
                let _owner = owner;
                panic!("terminal incoming");
            }
            drop(owner);
        }));
        if panics {
            let payload = outcome.expect_err("terminal failure propagates");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some(if incoming {
                    "terminal incoming"
                } else {
                    "terminal first"
                })
            );
            flui_foundation::panic::retain_opaque_payload(payload);
        } else {
            assert!(outcome.is_ok());
        }
        assert_eq!(
            *drops.lock().expect("terminal observations"),
            if incoming {
                vec![]
            } else if panics {
                vec!["terminal first"]
            } else {
                vec!["terminal first", "terminal second"]
            }
        );
        // A new independent owner still releases its healthy resources.
        let next = Arc::new(Mutex::new(Vec::new()));
        let probe = |label| TerminalProbe {
            label,
            drops: next.clone(),
            panics: false,
        };
        drop(terminal_fixture(
            kind,
            probe("next first"),
            probe("next second"),
        ));
        assert_eq!(
            *next.lock().expect("next owner"),
            vec!["next first", "next second"]
        );
    }
}
fn controller_terminal_owner() {
    terminal_owner_matrix(TerminalOwner::Controller);
}
fn proxy_terminal_owner() {
    terminal_owner_matrix(TerminalOwner::Proxy);
}
fn curved_terminal_owner() {
    terminal_owner_matrix(TerminalOwner::Curved);
}
fn switch_terminal_owner() {
    terminal_owner_matrix(TerminalOwner::Switch);
}

fn shared_terminal_owners_keep_independent_aliases_live() {
    let drops = Arc::new(Mutex::new(Vec::new()));
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    let alias = controller.clone();
    let probe = TerminalProbe {
        label: "controller alias",
        drops: drops.clone(),
        panics: false,
    };
    controller.add_status_listener(Arc::new(move |_| {
        let _capture = &probe;
    }));
    drop(controller);
    assert!(drops.lock().expect("alias observations").is_empty());
    alias.forward().expect("surviving controller");
    alias.tick_at(1.0);
    assert_eq!(alias.value(), 1.0);
    drop(alias);
    assert_eq!(
        *drops.lock().expect("alias observations"),
        vec!["controller alias"]
    );

    let parent = Arc::new(AnimationController::without_ticker(Duration::from_secs(1)));
    let proxy = ProxyAnimation::new(parent.clone());
    let alias = proxy.clone();
    let observed = Arc::new(AtomicUsize::new(0));
    let callback_observed = observed.clone();
    alias.add_status_listener(Arc::new(move |_| {
        callback_observed.fetch_add(1, Ordering::SeqCst);
    }));
    drop(proxy);
    let _run = parent.forward().expect("parent run");
    assert_eq!(observed.load(Ordering::SeqCst), 1);
    drop(alias);
    parent.tick_at(1.0);
    assert_eq!(
        observed.load(Ordering::SeqCst),
        1,
        "last proxy detaches its forwarder"
    );
}

fn ticker_terminal_cancels_before_retiring_callback() {
    let scheduler = UpdateScheduler::new();
    let drops = Arc::new(Mutex::new(Vec::new()));
    let probe = TerminalProbe {
        label: "ticker first",
        drops: drops.clone(),
        panics: true,
    };
    let mut ticker = Ticker::new_with_scheduler(&scheduler);
    ticker.start(move |_| {
        let _capture = &probe;
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(ticker)))
        .expect_err("ticker capture failure");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("ticker first")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(
        scheduler.transient_callback_count(),
        0,
        "cancellation precedes capture retirement"
    );
    assert_eq!(
        *drops.lock().expect("ticker retirement"),
        vec!["ticker first"]
    );

    let probe = TerminalProbe {
        label: "ticker retained",
        drops: drops.clone(),
        panics: true,
    };
    let mut ticker = Ticker::new_with_scheduler(&scheduler);
    ticker.start(move |_| {
        let _capture = &probe;
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _ticker = ticker;
        panic!("ticker incoming");
    }))
    .expect_err("incoming ticker failure");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("ticker incoming")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(scheduler.transient_callback_count(), 0);
    assert_eq!(
        *drops.lock().expect("ticker retirement"),
        vec!["ticker first"]
    );
}

struct TerminalParent {
    values: Mutex<Vec<(ListenerId, ListenerCallback)>>,
    statuses: Mutex<Vec<(ListenerId, StatusCallback)>>,
    removed: Arc<Mutex<Vec<&'static str>>>,
    fail_removal: bool,
    registrations: AtomicUsize,
    fail_registration_at: usize,
    fail_value: bool,
    sample: f64,
    fail_status: AtomicBool,
    #[allow(dead_code, reason = "held only for its destructor")]
    probe: Option<TerminalProbe>,
    reenter: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
impl std::fmt::Debug for TerminalParent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalParent").finish_non_exhaustive()
    }
}
impl TerminalParent {
    fn register(&self) {
        let index = self.registrations.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(
            index != self.fail_registration_at,
            "parent registration failure"
        );
    }
    fn removed(&self, kind: &'static str) {
        self.removed
            .lock()
            .expect("subscription observations")
            .push(kind);
        let callback = self.reenter.lock().expect("removal hook").take();
        if let Some(callback) = callback {
            callback();
        }
        assert!(!self.fail_removal, "{}", kind);
    }
}
impl Listenable for TerminalParent {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.register();
        let mut values = self.values.lock().expect("parent values");
        let id = ListenerId::new(values.len() + 1);
        values.push((id, callback));
        id
    }
    fn remove_listener(&self, id: ListenerId) {
        let removed = {
            let mut values = self.values.lock().expect("parent values");
            values
                .iter()
                .position(|(candidate, _)| *candidate == id)
                .map(|i| values.remove(i))
        };
        drop(removed);
        self.removed("value removal");
    }
    fn remove_all_listeners(&self) {
        let removed = std::mem::take(&mut *self.values.lock().expect("parent values"));
        drop(removed);
    }
}
impl Animation<f64> for TerminalParent {
    fn value(&self) -> f64 {
        assert!(!self.fail_value, "parent value failure");
        self.sample
    }
    fn status(&self) -> AnimationStatus {
        assert!(
            !self.fail_status.load(Ordering::SeqCst),
            "parent status failure"
        );
        AnimationStatus::Completed
    }
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.register();
        let mut statuses = self.statuses.lock().expect("parent statuses");
        let id = ListenerId::new(statuses.len() + 1);
        statuses.push((id, callback));
        id
    }
    fn remove_status_listener(&self, id: ListenerId) {
        let removed = {
            let mut statuses = self.statuses.lock().expect("parent statuses");
            statuses
                .iter()
                .position(|(candidate, _)| *candidate == id)
                .map(|i| statuses.remove(i))
        };
        drop(removed);
        self.removed("status removal");
    }
}
fn terminal_parent(fail_removal: bool) -> Arc<TerminalParent> {
    Arc::new(TerminalParent {
        values: Mutex::new(Vec::new()),
        statuses: Mutex::new(Vec::new()),
        removed: Arc::new(Mutex::new(Vec::new())),
        fail_removal,
        registrations: AtomicUsize::new(0),
        fail_registration_at: 0,
        fail_value: false,
        sample: 0.5,
        fail_status: AtomicBool::new(false),
        probe: None,
        reenter: Mutex::new(None),
    })
}
fn parent_subscriptions_detach_all_after_failure() {
    for incoming in [false, true] {
        let parent = terminal_parent(true);
        let proxy = ProxyAnimation::new(parent.clone());
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            if incoming {
                let _proxy = proxy;
                panic!("subscription incoming");
            }
            drop(proxy);
        }))
        .expect_err("subscription failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some(if incoming {
                "subscription incoming"
            } else {
                "value removal"
            })
        );
        flui_foundation::panic::retain_opaque_payload(failure);
        assert!(parent.values.lock().expect("parent values").is_empty());
        assert!(parent.statuses.lock().expect("parent statuses").is_empty());
        assert_eq!(
            *parent.removed.lock().expect("subscription observations"),
            vec!["value removal", "status removal"]
        );
    }
    let parent = terminal_parent(false);
    let curved = CurvedAnimation::new(parent.clone(), flui_animation::curve::Linear);
    let alias = curved.clone();
    drop(curved);
    assert!(parent.removed.lock().expect("curved aliases").is_empty());
    drop(alias);
    assert_eq!(
        *parent.removed.lock().expect("curved aliases"),
        vec!["value removal", "status removal"]
    );
}
fn switch_disposal_commits_before_reentrant_parent_removal() {
    let parent = terminal_parent(false);
    let switch = Arc::new(AnimationSwitch::new(parent.clone(), None));
    let weak = Arc::downgrade(&switch);
    *parent.reenter.lock().expect("removal hook") = Some(Arc::new(move || {
        let switch = weak.upgrade().expect("independent switch alias");
        switch.dispose();
        assert_eq!(switch.value(), 0.5);
    }));
    switch.dispose();
    assert_eq!(
        *parent.removed.lock().expect("switch removal"),
        vec!["value removal", "status removal"]
    );
    drop(switch);
    assert_eq!(
        *parent.removed.lock().expect("switch removal"),
        vec!["value removal", "status removal"]
    );
}
fn split_owned_curves_retire_independently() {
    for (panics, incoming, invalid) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let drops = Arc::new(Mutex::new(Vec::new()));
        let probe = |label| {
            TerminalCurve(TerminalProbe {
                label,
                drops: drops.clone(),
                panics,
            })
        };
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let split = Split::with_curves(
                if invalid { f64::NAN } else { 0.5 },
                probe("split first"),
                probe("split second"),
            );
            assert_eq!(split.split(), 0.5);
            assert_eq!(split.begin_curve().transform(0.25), 0.25);
            assert_eq!(split.end_curve().transform(0.75), 0.75);
            assert_eq!(split.transform(0.75), 0.75);
            if incoming {
                let _split = split;
                panic!("split incoming");
            }
            drop(split);
        }));
        if panics {
            let payload = failure.expect_err("split failure");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some(if invalid {
                    "split must be in range [0.0, 1.0]"
                } else if incoming {
                    "split incoming"
                } else {
                    "split first"
                })
            );
            flui_foundation::panic::retain_opaque_payload(payload);
        } else {
            assert!(failure.is_ok());
        }
        assert_eq!(
            *drops.lock().expect("split observations"),
            if incoming || invalid {
                vec![]
            } else if panics {
                vec!["split first"]
            } else {
                vec!["split first", "split second"]
            }
        );
    }
    let next = Arc::new(Mutex::new(Vec::new()));
    let curve = |label| {
        TerminalCurve(TerminalProbe {
            label,
            drops: next.clone(),
            panics: false,
        })
    };
    let split = Split::with_curves(
        0.5,
        curve("healthy next first"),
        curve("healthy next second"),
    );
    assert_eq!(split.transform(0.25), 0.25);
    drop(split);
    assert_eq!(
        *next.lock().expect("next split ownership"),
        vec!["healthy next first", "healthy next second"]
    );
}

#[derive(Debug)]
struct PartialCloneCurve {
    probe: TerminalProbe,
    fail_clone: bool,
}
impl Clone for PartialCloneCurve {
    fn clone(&self) -> Self {
        assert!(!self.fail_clone, "split clone failure");
        Self {
            probe: self.probe.clone(),
            fail_clone: false,
        }
    }
}
impl Curve for PartialCloneCurve {
    fn transform(&self, time: f64) -> f64 {
        time
    }
}
fn split_partial_clone_retains_completed_field() {
    let drops = Arc::new(Mutex::new(Vec::new()));
    let curve = |label, fail_clone| PartialCloneCurve {
        probe: TerminalProbe {
            label,
            drops: drops.clone(),
            panics: true,
        },
        fail_clone,
    };
    let split = Split::with_curves(
        0.5,
        curve("cloned first", false),
        curve("cloned second", true),
    );
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| split.clone()))
        .expect_err("second clone fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("split clone failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert!(
        drops.lock().expect("clone observations").is_empty(),
        "partially cloned first curve retained during second clone failure"
    );
    assert_eq!(split.transform(0.25), 0.25, "original remains usable");
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(split)))
        .expect_err("original retirement failure");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("cloned first")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(
        *drops.lock().expect("clone observations"),
        vec!["cloned first"]
    );
}

fn constructors_preserve_incoming_sources_and_partial_subscriptions() {
    let drops = Arc::new(Mutex::new(Vec::new()));
    let mut parent = terminal_parent(false);
    Arc::get_mut(&mut parent)
        .expect("unique test parent")
        .fail_registration_at = 2;
    let curve = TerminalCurve(TerminalProbe {
        label: "constructor curve",
        drops: drops.clone(),
        panics: true,
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        CurvedAnimation::new(parent.clone(), curve)
    }))
    .expect_err("second registration fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("parent registration failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert!(drops.lock().expect("constructor custody").is_empty());
    assert!(
        parent
            .values
            .lock()
            .expect("partial subscription")
            .is_empty()
    );
    assert_eq!(
        *parent.removed.lock().expect("partial subscription"),
        vec!["value removal"]
    );

    let mut parent = terminal_parent(false);
    Arc::get_mut(&mut parent)
        .expect("unique test parent")
        .fail_registration_at = 2;
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ProxyAnimation::new(parent.clone())
    }))
    .expect_err("proxy status registration fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("parent registration failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert!(
        parent
            .values
            .lock()
            .expect("proxy partial subscription")
            .is_empty()
    );

    let mut current = terminal_parent(false);
    Arc::get_mut(&mut current)
        .expect("unique current parent")
        .sample = 0.75;
    let mut next = terminal_parent(false);
    Arc::get_mut(&mut next)
        .expect("unique next parent")
        .fail_registration_at = 1;
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        AnimationSwitch::new(current.clone(), Some(next.clone()))
    }))
    .expect_err("next value registration fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("parent registration failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert!(
        current
            .values
            .lock()
            .expect("switch admitted subscription")
            .is_empty(),
        "accepted current registration must detach when later next registration fails"
    );
    assert_eq!(
        *current
            .removed
            .lock()
            .expect("switch admitted subscription"),
        vec!["value removal"]
    );

    let mut current = terminal_parent(false);
    let mut next = terminal_parent(false);
    {
        let current = Arc::get_mut(&mut current).expect("unique current parent");
        current.fail_value = true;
        current.probe = Some(TerminalProbe {
            label: "current constructor owner",
            drops: drops.clone(),
            panics: true,
        });
    }
    Arc::get_mut(&mut next).expect("unique next parent").probe = Some(TerminalProbe {
        label: "next constructor owner",
        drops: drops.clone(),
        panics: true,
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        AnimationSwitch::new(current, Some(next))
    }))
    .expect_err("initial parent sample fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("parent value failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert!(drops.lock().expect("constructor custody").is_empty());

    let old = terminal_parent(false);
    let proxy = ProxyAnimation::new(old.clone());
    old.fail_status.store(true, Ordering::SeqCst);
    let mut replacement = terminal_parent(false);
    Arc::get_mut(&mut replacement)
        .expect("unique replacement")
        .probe = Some(TerminalProbe {
        label: "replacement constructor owner",
        drops: drops.clone(),
        panics: true,
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        proxy.set_parent(replacement);
    }))
    .expect_err("old parent status fails");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("parent status failure")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert!(drops.lock().expect("constructor custody").is_empty());
    old.fail_status.store(false, Ordering::SeqCst);
    assert_eq!(proxy.value(), 0.5);
    drop(proxy);
}

#[cfg(feature = "serde")]
static SERDE_CURVE_DROPS: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "serde")]
#[derive(Debug)]
struct SerdeCurve(String);
#[cfg(feature = "serde")]
impl Curve for SerdeCurve {
    fn transform(&self, time: f64) -> f64 {
        time
    }
}
#[cfg(feature = "serde")]
impl Drop for SerdeCurve {
    fn drop(&mut self) {
        SERDE_CURVE_DROPS.fetch_add(1, Ordering::SeqCst);
        assert!(!self.0.starts_with("bomb"), "serde curve retirement");
    }
}
#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for SerdeCurve {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <String as serde::Deserialize>::deserialize(deserializer).map(Self)
    }
}
#[cfg(feature = "serde")]
enum SerdeInput {
    Number(f64),
    Text(&'static str),
    Failure,
}
#[cfg(feature = "serde")]
impl serde::de::IntoDeserializer<'_, serde::de::value::Error> for SerdeInput {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}
#[cfg(feature = "serde")]
impl<'de> serde::Deserializer<'de> for SerdeInput {
    type Error = serde::de::value::Error;
    fn deserialize_any<V: serde::de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self {
            Self::Number(value) => visitor.visit_f64(value),
            Self::Text(value) => visitor.visit_borrowed_str(value),
            Self::Failure => Err(serde::de::Error::custom("second curve decode failure")),
        }
    }
    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
    bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum
    identifier ignored_any }
}
#[cfg(feature = "serde")]
struct SerdeSplitMap(Vec<(&'static str, SerdeInput)>);
#[cfg(feature = "serde")]
impl<'de> serde::Deserializer<'de> for SerdeSplitMap {
    type Error = serde::de::value::Error;
    fn deserialize_any<V: serde::de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_map(serde::de::value::MapDeserializer::new(self.0.into_iter()))
    }
    fn deserialize_struct<V: serde::de::Visitor<'de>>(
        self,
        name: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        assert_eq!(name, "Split", "wire struct identity preserved");
        self.deserialize_any(visitor)
    }
    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
    bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map enum
    identifier ignored_any }
}
#[cfg(feature = "serde")]
fn split_partial_deserialization_preserves_errors_and_wire_name() {
    fn decode(
        split: f64,
        first: &'static str,
        second: SerdeInput,
    ) -> Result<Split<SerdeCurve, SerdeCurve>, serde::de::value::Error> {
        <Split<SerdeCurve, SerdeCurve> as serde::Deserialize>::deserialize(SerdeSplitMap(vec![
            ("split", SerdeInput::Number(split)),
            ("begin_curve", SerdeInput::Text(first)),
            ("end_curve", second),
        ]))
    }
    SERDE_CURVE_DROPS.store(0, Ordering::SeqCst);
    let failure = decode(0.5, "bomb first", SerdeInput::Failure).expect_err("partial decode error");
    assert_eq!(failure.to_string(), "second curve decode failure");
    assert_eq!(SERDE_CURVE_DROPS.load(Ordering::SeqCst), 0);
    let failure = decode(f64::NAN, "bomb first", SerdeInput::Text("bomb second"))
        .expect_err("invalid decoded range");
    assert_eq!(failure.to_string(), "split must be in range [0.0, 1.0]");
    assert_eq!(SERDE_CURVE_DROPS.load(Ordering::SeqCst), 0);
    let split = decode(0.5, "bomb first", SerdeInput::Text("bomb second")).expect("valid split");
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(split)))
        .expect_err("decoded ordinary retirement");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("serde curve retirement")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(SERDE_CURVE_DROPS.load(Ordering::SeqCst), 1);
    let split = decode(0.5, "bomb first", SerdeInput::Text("bomb second")).expect("incoming split");
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _split = split;
        panic!("serde incoming");
    }))
    .expect_err("incoming decoded failure");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("serde incoming")
    );
    flui_foundation::panic::retain_opaque_payload(failure);
    assert_eq!(SERDE_CURVE_DROPS.load(Ordering::SeqCst), 1);

    let split =
        decode(0.5, "healthy first", SerdeInput::Text("healthy second")).expect("next split");
    assert_eq!(split.transform(0.75), 0.75);
    drop(split);
    assert_eq!(SERDE_CURVE_DROPS.load(Ordering::SeqCst), 3);
}

#[test]
fn controller_sources_allow_reentry_and_preserve_run_ownership() {
    let cases: &[(&str, fn())] = &[
        (
            "constructor custody and partial subscriptions",
            constructors_preserve_incoming_sources_and_partial_subscriptions,
        ),
        #[cfg(feature = "serde")]
        (
            "split partial serde custody",
            split_partial_deserialization_preserves_errors_and_wire_name,
        ),
        (
            "split partial clone ownership",
            split_partial_clone_retains_completed_field,
        ),
        (
            "parent subscription terminal failures",
            parent_subscriptions_detach_all_after_failure,
        ),
        (
            "switch parent removal reentry",
            switch_disposal_commits_before_reentrant_parent_removal,
        ),
        (
            "split independent curve retirement",
            split_owned_curves_retire_independently,
        ),
        ("controller final owner", controller_terminal_owner),
        ("proxy final owner", proxy_terminal_owner),
        ("curved final owner", curved_terminal_owner),
        ("switch final owner", switch_terminal_owner),
        (
            "independent final-owner aliases",
            shared_terminal_owners_keep_independent_aliases_live,
        ),
        (
            "ticker final owner",
            ticker_terminal_cancels_before_retiring_callback,
        ),
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
    if let Some(selected) = child_process::selected_case() {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case")
            .1();
        child_process::pass();
    }
    let names: Vec<_> = cases.iter().map(|(name, _)| *name).collect();
    child_process::run_rows(
        "controller_sources::controller_sources_allow_reentry_and_preserve_run_ownership",
        &names,
    );
}
