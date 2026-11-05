//! Outcomes, refusal atomicity, reentry and recovery use the real controller
//! commands; run and sample identity matrices take private counter injection.

use super::*;
use crate::AnimationRunFuture;
use crate::repeat::RepeatMode;
use flui_foundation::Listenable;

#[path = "../tests/support/child_process.rs"]
mod child_process;

fn controller(ms: u64) -> AnimationController {
    AnimationController::builder(Duration::from_millis(ms)).build()
}

fn exhausted_run_identities_refuse_without_displacing_the_last_run() {
    for (name, start) in [
        (
            "forward",
            (|c: &AnimationController| c.forward_from(Some(0.1)))
                as fn(&AnimationController) -> Result<AnimationRunFuture, AnimationError>,
        ),
        ("reverse", |c: &AnimationController| {
            c.reverse_from(Some(0.9))
        }),
        ("target", |c: &AnimationController| c.animate_to(0.9, None)),
        ("repeat", |c: &AnimationController| {
            c.repeat(RepeatMode::Restart)
        }),
        ("fling", |c: &AnimationController| c.fling(1.0)),
        ("simulation", |c: &AnimationController| {
            c.animate_with(Box::new(InstantSimulation { value: 0.9 }))
        }),
    ] {
        let c = controller(100);
        c.inner.borrow_mut().run_generation = u64::MAX - 1;
        let last = c.forward().expect("last identity is available");
        c.tick_at(Duration::from_millis(25));
        let held = c.value();
        assert!(
            matches!(start(&c), Err(AnimationError::IdentityExhausted)),
            "{name}: terminal identity refuses a new run"
        );
        assert_eq!(c.value(), held, "{name}: refusal commits no input value");
        assert_eq!(c.run_generation(), u64::MAX);
        assert!(
            last.is_pending(),
            "{name}: refusal does not cancel accepted work"
        );
        c.tick_at(Duration::from_millis(100));
        assert!(last.is_complete());
        c.reset()
            .expect("reset still works after run identity exhaustion");
        assert!(
            matches!(start(&c), Err(AnimationError::IdentityExhausted)),
            "{name}: refusal is permanent"
        );
    }
}

fn disposed_status_admission_is_inert() {
    let mut owner = AnimationController::builder(Duration::from_millis(100)).build_on(None);
    owner.dispose();
    let c = owner.controller();
    let drops = Rc::new(std::cell::Cell::new(0));
    struct Capture(Rc<std::cell::Cell<usize>>, AnimationController);
    impl Drop for Capture {
        fn drop(&mut self) {
            let _value = self.1.value();
            self.0.set(self.0.get() + 1);
        }
    }
    let subscription = c.subscribe_status({
        let capture = Capture(Rc::clone(&drops), c.clone());
        Rc::new(move |_| {
            let _keep = &capture;
        })
    });
    assert_eq!(drops.get(), 1);
    drop(subscription);
    assert_eq!(drops.get(), 1);
}

#[test]
fn disposed_value_registrations_retire_their_captures() {
    fn row() {
        struct Capture(AnimationController, Rc<std::cell::Cell<usize>>);
        impl Drop for Capture {
            fn drop(&mut self) {
                assert!(self.0.value().is_finite());
                self.1.set(self.1.get() + 1);
            }
        }
        for observer in [false, true] {
            let mut owner = AnimationController::builder(Duration::from_millis(100)).build_on(None);
            owner.dispose();
            let c = owner.controller();
            let drops = Rc::new(std::cell::Cell::new(0));
            for _ in 0..2 {
                let capture = Capture(c.clone(), Rc::clone(&drops));
                let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if observer {
                        c.listen_with_recovery(Rc::new(move |_| {
                            let _keep = &capture;
                        }))
                        .detach();
                    } else {
                        c.listen(Rc::new(move || {
                            let _keep = &capture;
                        }))
                        .detach();
                    }
                }));
                assert!(failure.is_ok(), "a disposed refusal stays inert");
            }
            assert_eq!(drops.get(), 2, "every refusal drops its capture");
            assert!(!c.is_animating());
            assert_eq!(c.set_value(0.5), Err(AnimationError::Disposed));
            assert_eq!(
                c.value(),
                0.0,
                "refusal leaves the disposed controller terminal"
            );
        }
    }
    child_process::run_single(
        "controller::tests::disposed_value_registrations_retire_their_captures",
        row,
    );
}

fn exhausted_sample_identities_cancel_without_reissuing_a_stale_sample() {
    let c = controller(100);
    let run = c.forward().expect("run");
    c.inner.borrow_mut().sample_epoch = u64::MAX - 1;
    c.tick_at(Duration::from_millis(25));
    assert_eq!(c.value(), 0.25);
    c.tick_at(Duration::from_millis(50));
    assert_eq!(
        c.value(),
        0.25,
        "no sample identity can wrap to an older one"
    );
    assert!(
        run.is_canceled(),
        "accepted work receives a terminal outcome"
    );
    assert!(!c.is_animating());
    assert!(
        matches!(c.forward(), Err(AnimationError::IdentityExhausted)),
        "new runs cannot reuse exhausted sample identities"
    );
    c.reset().expect("healthy state edits remain available");
    assert!(
        matches!(c.forward(), Err(AnimationError::IdentityExhausted)),
        "refusal survives reset"
    );
}

struct InstantSimulation {
    value: f64,
}

impl Simulation for InstantSimulation {
    fn x(&self, _: f64) -> f64 {
        self.value
    }
    fn dx(&self, _: f64) -> f64 {
        0.0
    }
    fn is_done(&self, _: f64) -> bool {
        true
    }
}

#[test]
fn failure_modes_are_contained() {
    crate::test_cases::run_cases(&[
        (
            "disposed status admission",
            disposed_status_admission_is_inert,
        ),
        (
            "exhausted run identities refuse without displacing the last run",
            exhausted_run_identities_refuse_without_displacing_the_last_run,
        ),
        (
            "exhausted sample identities cancel without reissuing a stale sample",
            exhausted_sample_identities_cancel_without_reissuing_a_stale_sample,
        ),
    ]);
}
