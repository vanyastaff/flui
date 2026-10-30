//! Required samples and committed notices share one addressed owner's custody.

use flui_animation::curve::Curve;
use flui_animation::simulation::Simulation;
use flui_animation::{Animation, AnimationController, DrivenController, MotionClock, Vsync};
use flui_foundation::Listenable;
use flui_foundation::panic::{FailureClass, PanicRecovery};
use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

struct FailingCurve(Rc<Cell<bool>>);

impl Curve for FailingCurve {
    fn transform(&self, t: f64) -> f64 {
        if self.0.replace(false) {
            std::panic::panic_any("source computation");
        }
        t
    }
}

struct FailingSimulation {
    armed: Rc<Cell<bool>>,
    fail_done: bool,
}

impl Simulation for FailingSimulation {
    fn x(&self, time: f64) -> f64 {
        if !self.fail_done && self.armed.replace(false) {
            std::panic::panic_any("source computation");
        }
        time
    }

    fn dx(&self, _: f64) -> f64 {
        1.0
    }

    fn is_done(&self, time: f64) -> bool {
        if self.fail_done && self.armed.replace(false) {
            std::panic::panic_any("source computation");
        }
        time >= 1.0
    }
}

#[derive(Clone, Copy)]
enum Fault {
    Delivery,
    Curve,
    Position,
    Done,
}

fn admit(vsync: &Vsync, fault: Fault, armed: Rc<Cell<bool>>) -> DrivenController {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(vsync));
    let controller = owner.controller();
    match fault {
        Fault::Delivery => {
            controller.add_listener(Rc::new(move || {
                if armed.replace(false) {
                    std::panic::panic_any("first delivery");
                }
            }));
            controller.forward().expect("delivery run");
        }
        Fault::Curve => {
            controller
                .animate_to_curved(1.0, Some(Duration::from_secs(1)), FailingCurve(armed))
                .expect("curve run");
        }
        Fault::Position | Fault::Done => {
            controller
                .animate_with(FailingSimulation {
                    armed,
                    fail_done: matches!(fault, Fault::Done),
                })
                .expect("simulation run");
        }
    }
    owner
}

fn sample(first: Fault, second: Option<Fault>) {
    let vsync = Vsync::new();
    // This healthy commit precedes either failure and must never be rolled back.
    let healthy = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    healthy.controller().forward().expect("healthy run");
    let first_armed = Rc::new(Cell::new(false));
    let first_owner = admit(&vsync, first, Rc::clone(&first_armed));
    let second_armed = Rc::new(Cell::new(false));
    let second_owner = second.map(|fault| admit(&vsync, fault, Rc::clone(&second_armed)));
    let tail = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    tail.controller().forward().expect("healthy tail run");
    let mut clock = MotionClock::new();
    vsync.tick_all(&clock.frame(Duration::ZERO));
    first_armed.set(true);
    second_armed.set(true);
    let mut recovery = PanicRecovery::new();
    vsync.tick_all_with_recovery(
        &clock.frame(Duration::from_millis(250)),
        &mut recovery.scope(),
    );
    assert_eq!(
        healthy.controller().value(),
        0.25,
        "accepted earlier sample remains"
    );
    assert_eq!(
        tail.controller().value(),
        0.25,
        "live frame peers remain deliverable"
    );
    let first_class = if matches!(first, Fault::Delivery) {
        FailureClass::Delivery
    } else {
        FailureClass::Computation
    };
    let blocked = !matches!(first, Fault::Delivery)
        || second.is_some_and(|fault| !matches!(fault, Fault::Delivery));
    assert_eq!(recovery.first_failure_class(), Some(first_class));
    assert_eq!(
        recovery.blocking_failure_class(),
        blocked.then_some(FailureClass::Computation)
    );
    let failure = recovery
        .into_failure()
        .expect("the sampling owner receives first failure");
    let message = if matches!(first, Fault::Delivery) {
        "first delivery"
    } else {
        "source computation"
    };
    assert_eq!(failure.payload().downcast_ref::<&str>(), Some(&message));
    assert_eq!(
        first_owner.controller().value(),
        if matches!(first, Fault::Delivery) {
            0.25
        } else {
            0.0
        }
    );
    if let (Some(owner), Some(fault)) = (&second_owner, second) {
        assert_eq!(
            owner.controller().value(),
            if matches!(fault, Fault::Delivery) {
                0.25
            } else {
                0.0
            }
        );
    }
    drop(failure);
    let mut next = PanicRecovery::new();
    vsync.tick_all_with_recovery(&clock.frame(Duration::from_millis(500)), &mut next.scope());
    assert!(
        next.into_failure().is_none(),
        "the repaired operation progresses"
    );
    assert_eq!(first_owner.controller().value(), 0.5);
    if let Some(owner) = second_owner {
        assert_eq!(owner.controller().value(), 0.5);
    }
    assert_eq!(tail.controller().value(), 0.5);
}

fn delivery_only() {
    sample(Fault::Delivery, None);
}
fn curve_computation() {
    sample(Fault::Curve, None);
}
fn position_computation() {
    sample(Fault::Position, None);
}
fn done_computation() {
    sample(Fault::Done, None);
}
fn delivery_then_computation() {
    sample(Fault::Delivery, Some(Fault::Curve));
}
fn computation_then_delivery() {
    sample(Fault::Curve, Some(Fault::Delivery));
}

fn reduced_policy_source_failure_preserves_the_run_and_healthy_peer() {
    let vsync = Vsync::new();
    let source = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let armed = Rc::new(Cell::new(false));
    let run = source
        .controller()
        .animate_with(FailingSimulation {
            armed: Rc::clone(&armed),
            fail_done: false,
        })
        .expect("custom source run");
    let healthy = AnimationController::builder(Duration::from_secs(1))
        .behavior(flui_animation::AnimationBehavior::Preserve)
        .build_on(Some(&vsync));
    healthy.controller().forward().expect("preserved peer");
    let mut clock = MotionClock::new();
    vsync.tick_all(&clock.frame(Duration::ZERO));
    armed.set(true);
    clock.set_system_motion(flui_platform::MotionPreference::Reduce);
    let mut recovery = PanicRecovery::new();
    vsync.tick_all_with_recovery(
        &clock.frame(Duration::from_millis(250)),
        &mut recovery.scope(),
    );
    assert_eq!(
        source.controller().value(),
        0.0,
        "a failed policy probe publishes no position"
    );
    assert!(
        run.is_pending(),
        "source failure cannot complete the admitted run"
    );
    assert_eq!(
        healthy.controller().value(),
        0.25,
        "independent preserved motion advances"
    );
    let failure = recovery.into_failure().expect("policy source failure");
    assert_eq!(failure.class(), FailureClass::Computation);
    assert_eq!(failure.blocking_class(), Some(FailureClass::Computation));
    drop(failure);
    let mut next = PanicRecovery::new();
    vsync.tick_all_with_recovery(&clock.frame(Duration::from_millis(500)), &mut next.scope());
    assert!(
        next.into_failure().is_none(),
        "the next policy sample may recover"
    );
    assert!(run.is_complete());
    assert_eq!(source.controller().value(), 1.0);
    assert_eq!(healthy.controller().value(), 0.5);
}

struct FailingWake;

impl Wake for FailingWake {
    fn wake(self: Arc<Self>) {
        panic!("terminal delivery");
    }

    fn wake_by_ref(self: &Arc<Self>) {
        panic!("terminal delivery");
    }
}

#[derive(Clone, Copy)]
enum TerminalDelivery {
    Status,
    Continuation,
    Waiter,
}

fn terminal_delivery(stage: TerminalDelivery) {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let controller = owner.controller();
    let mut run = controller.forward().expect("terminal run");
    let status_failure = matches!(stage, TerminalDelivery::Status).then(|| {
        controller.subscribe_status(Rc::new(|status| {
            if status == flui_animation::AnimationStatus::ForwardSettled {
                std::panic::panic_any("terminal delivery");
            }
        }))
    });
    match stage {
        TerminalDelivery::Status => {}
        TerminalDelivery::Continuation => run.when_complete_or_cancel(|outcome| {
            assert!(
                outcome.is_ok(),
                "completion committed before its continuation"
            );
            panic!("terminal delivery");
        }),
        TerminalDelivery::Waiter => {
            let waker = Waker::from(Arc::new(FailingWake));
            assert_eq!(
                Pin::new(&mut run).poll(&mut Context::from_waker(&waker)),
                Poll::Pending
            );
        }
    }
    let tail_delivered = Rc::new(Cell::new(false));
    let tail = Rc::clone(&tail_delivered);
    let tail_subscription = controller.subscribe_status(Rc::new(move |status| {
        if status == flui_animation::AnimationStatus::ForwardSettled {
            tail.set(true);
        }
    }));
    let mut clock = MotionClock::new();
    vsync.tick_all(&clock.frame(Duration::ZERO));
    let mut recovery = PanicRecovery::new();
    vsync.tick_all_with_recovery(&clock.frame(Duration::from_secs(1)), &mut recovery.scope());
    assert_eq!(controller.value(), 1.0, "terminal value remains published");
    assert!(
        run.is_complete(),
        "observer failure does not cancel accepted completion"
    );
    assert!(tail_delivered.get(), "live status tail is delivered");
    let receipt = recovery.into_failure().expect("terminal delivery failure");
    assert_eq!(receipt.class(), FailureClass::Delivery);
    assert_eq!(receipt.blocking_class(), None);
    assert_eq!(
        receipt.payload().downcast_ref::<&str>(),
        Some(&"terminal delivery")
    );
    drop(receipt);
    drop((status_failure, tail_subscription));
    controller.set_value(0.5).expect("no build is running");
    assert_eq!(controller.value(), 0.5);
}

fn status_delivery() {
    terminal_delivery(TerminalDelivery::Status);
}
fn continuation_delivery() {
    terminal_delivery(TerminalDelivery::Continuation);
}
fn waiter_delivery() {
    terminal_delivery(TerminalDelivery::Waiter);
}

struct RetiredListenerCapture(Rc<Cell<usize>>);

impl Drop for RetiredListenerCapture {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
        panic!("committed listener retirement");
    }
}

fn withdrawn_value_listener_retirement_preserves_the_commit() {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let controller = owner.controller();
    let run = controller.forward().expect("listener retirement run");
    let identity = Rc::new(Cell::new(None));
    let callback_identity = Rc::clone(&identity);
    let callback_controller = controller.clone();
    let armed = Rc::new(Cell::new(false));
    let callback_armed = Rc::clone(&armed);
    let drops = Rc::new(Cell::new(0));
    let capture = RetiredListenerCapture(Rc::clone(&drops));
    identity.set(Some(controller.add_listener(Rc::new(move || {
        let _capture = &capture;
        if callback_armed.get() {
            callback_controller.remove_listener(
                callback_identity
                    .take()
                    .expect("the live listener removes its own identity"),
            );
        }
    }))));
    let tail = Rc::new(Cell::new(0));
    let delivered = Rc::clone(&tail);
    controller.add_listener(Rc::new(move || delivered.set(delivered.get() + 1)));
    let mut clock = MotionClock::new();
    vsync.tick_all(&clock.frame(Duration::ZERO));
    armed.set(true);
    tail.set(0);
    let mut recovery = PanicRecovery::new();
    vsync.tick_all_with_recovery(
        &clock.frame(Duration::from_millis(250)),
        &mut recovery.scope(),
    );
    assert_eq!(controller.value(), 0.25, "the valid sample stays published");
    assert!(run.is_pending(), "retirement does not cancel the run");
    assert_eq!(tail.get(), 1, "the healthy value tail receives the commit");
    assert_eq!(drops.get(), 1, "healthy-path destruction still runs");
    let receipt = recovery
        .into_failure()
        .expect("listener retirement failure");
    assert_eq!(receipt.class(), FailureClass::Delivery);
    assert_eq!(receipt.blocking_class(), None);
    assert_eq!(
        receipt.payload().downcast_ref::<&str>(),
        Some(&"committed listener retirement")
    );
    drop(receipt);
    let mut next = PanicRecovery::new();
    vsync.tick_all_with_recovery(&clock.frame(Duration::from_millis(500)), &mut next.scope());
    assert!(next.into_failure().is_none(), "the next sample progresses");
    assert_eq!(controller.value(), 0.5);
    assert_eq!(tail.get(), 2);
}

#[test]
fn sampling_preserves_failure_origin_and_publication_integrity() {
    crate::run_table(&[
        ("committed delivery", delivery_only),
        ("curve computation", curve_computation),
        ("simulation position", position_computation),
        ("simulation completion", done_computation),
        ("delivery before computation", delivery_then_computation),
        ("computation before delivery", computation_then_delivery),
        (
            "reduced-policy source query",
            reduced_policy_source_failure_preserves_the_run_and_healthy_peer,
        ),
        ("committed status delivery", status_delivery),
        ("committed continuation delivery", continuation_delivery),
        ("committed waiter delivery", waiter_delivery),
        (
            "withdrawn value listener retirement",
            withdrawn_value_listener_retirement_preserves_the_commit,
        ),
    ]);
}
