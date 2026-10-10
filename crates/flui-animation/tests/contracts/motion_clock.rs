//! Consumer contracts for a presentation's animation clock.
//!
//! Reference values are analytic: animation time is the integral of the rate
//! over raw time, computed here in integer nanoseconds with rational rates.

use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, InvalidPlaybackRate, MotionClock, PlaybackRate,
};
use proptest::prelude::*;

use crate::run_table;

const fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn rate(value: f64) -> PlaybackRate {
    PlaybackRate::new(value).expect("test rates are finite and non-negative")
}

fn now_after(clock: &mut MotionClock, raw: Duration) -> Duration {
    clock.frame(raw).now().as_duration()
}

#[test]
fn normal_and_preserve_use_distinct_duration_timelines() {
    use flui_animation::AnimationBehavior;
    use flui_platform_api::MotionPreference;
    for nested in [false, true] {
        for (scale, expected) in [(2.0, 0.2), (0.5, 0.8)] {
            let registry = flui_animation::Vsync::new();
            let child = flui_animation::Vsync::new();
            let _seat = nested.then(|| registry.attach_child(&child).expect("nested registry"));
            let target = if nested { &child } else { &registry };
            let normal = AnimationController::builder(ms(1000)).build_on(Some(target));
            let preserve = AnimationController::builder(ms(1000))
                .behavior(AnimationBehavior::Preserve)
                .build_on(Some(target));
            normal.controller().forward().expect("normal run");
            preserve.controller().forward().expect("preserved run");
            let mut clock = MotionClock::new();
            clock.set_system_motion(
                MotionPreference::from_duration_scale(scale).expect("positive scale"),
            );
            registry.tick_all(&clock.frame(Duration::ZERO));
            registry.tick_all(&clock.frame(ms(400)));
            assert!(
                (normal.controller().value() - expected).abs() < 1e-9,
                "nested {nested}, scale {scale}: normal value {} must be {expected}",
                normal.controller().value()
            );
            assert!(
                (preserve.controller().value() - 0.4).abs() < 1e-9,
                "preserved run ignores the duration scale"
            );
        }
    }
}

#[test]
fn reduced_motion_settles_normal_runs_on_the_next_tick() {
    use flui_animation::{AnimationBehavior, AnimationStatus};
    use flui_platform_api::MotionPreference;
    use std::{
        cell::RefCell,
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll, Waker},
    };
    for nested in [false, true] {
        let registry = flui_animation::Vsync::new();
        let child = flui_animation::Vsync::new();
        let _seat = nested.then(|| registry.attach_child(&child).expect("nested registry"));
        let target = if nested { &child } else { &registry };
        let normal = AnimationController::builder(ms(1000)).build_on(Some(target));
        let preserve = AnimationController::builder(ms(1000))
            .behavior(AnimationBehavior::Preserve)
            .build_on(Some(target));
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&statuses);
        let _subscription = normal
            .controller()
            .subscribe_status(Rc::new(move |status| recorded.borrow_mut().push(status)));
        let mut completed = normal.controller().forward().expect("normal run");
        let mut continuing = preserve.controller().forward().expect("preserved run");
        let mut clock = MotionClock::new();
        clock.set_system_motion(MotionPreference::Reduce);
        assert_eq!(
            normal.controller().value(),
            0.0,
            "policy change defers completion to the frame"
        );
        registry.tick_all(&clock.frame(Duration::ZERO));
        assert_eq!(normal.controller().value(), 1.0);
        assert_eq!(
            *statuses.borrow(),
            [AnimationStatus::Forward, AnimationStatus::Completed]
        );
        assert_eq!(
            Pin::new(&mut completed).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(()))
        );
        registry.tick_all(&clock.frame(ms(400)));
        assert!((preserve.controller().value() - 0.4).abs() < 1e-9);
        assert_eq!(
            Pin::new(&mut continuing).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        );
        assert_eq!(
            statuses.borrow().len(),
            2,
            "terminal status is delivered once"
        );
    }
}

#[test]
fn parked_repeat_resumes_from_zero_under_full() {
    use flui_platform_api::MotionPreference;
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let registry = flui_animation::Vsync::new();
    let owner = AnimationController::builder(ms(1000))
        .initial_value(0.5)
        .build_on(Some(&registry));
    let mut future = owner.controller().repeat(false).expect("infinite repeat");
    let mut clock = MotionClock::new();
    clock.set_system_motion(MotionPreference::Reduce);
    registry.tick_all(&clock.frame(Duration::ZERO));
    assert_eq!(
        owner.controller().value(),
        0.0,
        "park at the first leg's start"
    );
    assert!(
        !registry.has_running(),
        "parked repeat does not order more frames"
    );
    assert_eq!(
        Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
    registry.tick_all(&clock.frame(ms(5000)));
    clock.set_system_motion(MotionPreference::NoPreference);
    registry.tick_all(&clock.frame(ms(6000)));
    assert_eq!(
        owner.controller().value(),
        0.0,
        "resume anchors at this frame"
    );
    registry.tick_all(&clock.frame(ms(6250)));
    assert!((owner.controller().value() - 0.25).abs() < 1e-9);
    assert!(registry.has_running());
    assert_eq!(
        Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
}

#[test]
fn policy_flips_mid_run() {
    use flui_animation::{AnimationBehavior, MotionPreference};
    for nested in [false, true] {
        let registry = flui_animation::Vsync::new();
        let child = flui_animation::Vsync::new();
        let _seat = nested.then(|| registry.attach_child(&child).expect("nested registry"));
        let target = if nested { &child } else { &registry };
        let normal = AnimationController::builder(ms(1000)).build_on(Some(target));
        let preserve = AnimationController::builder(ms(1000))
            .behavior(AnimationBehavior::Preserve)
            .build_on(Some(target));
        let finite = normal.controller().forward().expect("normal run");
        let continuing = preserve.controller().forward().expect("preserved run");
        let mut clock = MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(ms(200)));
        clock.set_preference(MotionPreference::Reduce);
        clock.set_preference(MotionPreference::Full);
        assert_eq!(normal.controller().value(), 0.2);
        registry.tick_all(&clock.frame(ms(400)));
        assert_eq!(normal.controller().value(), 0.4);
        assert!(!finite.is_complete(), "a superseded policy does not settle");
        clock.set_preference(MotionPreference::Reduce);
        assert_eq!(
            normal.controller().value(),
            0.4,
            "settlement waits for a frame"
        );
        registry.tick_all(&clock.frame(ms(500)));
        assert_eq!(normal.controller().value(), 1.0);
        assert!(finite.is_complete());
        assert_eq!(preserve.controller().value(), 0.5);
        assert!(!continuing.is_complete());
        let repeated = normal
            .controller()
            .repeat(false)
            .expect("repeat under Reduce");
        registry.tick_all(&clock.frame(ms(600)));
        assert_eq!(normal.controller().value(), 0.0);
        clock.set_preference(MotionPreference::Full);
        registry.tick_all(&clock.frame(ms(800)));
        assert_eq!(
            normal.controller().value(),
            0.0,
            "resume has a fresh anchor"
        );
        registry.tick_all(&clock.frame(ms(900)));
        assert!((normal.controller().value() - 0.1).abs() < 1e-9);
        assert_eq!(preserve.controller().value(), 0.9);
        assert!(!repeated.is_complete());
    }
}

/// An external simulation with an observable time coordinate, including holes
/// and sources whose own completion predicate never succeeds.
#[derive(Debug)]
struct GridSimulation {
    non_finite_after: f64,
    done_at: f64,
}

impl flui_animation::Simulation for GridSimulation {
    fn x(&self, time: f64) -> f64 {
        if time >= self.non_finite_after {
            f64::NAN
        } else {
            time
        }
    }

    fn dx(&self, _time: f64) -> f64 {
        1.0
    }

    fn is_done(&self, time: f64) -> bool {
        time >= self.done_at
    }
}

fn settle_simulation(source: impl flui_animation::Simulation + 'static, expected: f64) {
    use flui_animation::{AnimationStatus, MotionPreference};
    use std::{cell::Cell, rc::Rc};
    let registry = flui_animation::Vsync::new();
    let owner = AnimationController::builder(ms(1000))
        .unbounded()
        .initial_value(7.0)
        .build_on(Some(&registry));
    let controller = owner.controller();
    let run = controller.animate_with(source).expect("simulation run");
    let delivered = Rc::new(Cell::new(0));
    let count = Rc::clone(&delivered);
    let _listener = controller.subscribe_status(Rc::new(move |status| {
        assert_eq!(status, AnimationStatus::Completed);
        count.set(count.get() + 1);
    }));
    let mut clock = MotionClock::new();
    let initial = controller.value();
    clock.set_preference(MotionPreference::Reduce);
    assert_eq!(
        controller.value(),
        initial,
        "policy does not sample user code"
    );
    registry.tick_all(&clock.frame(Duration::ZERO));
    assert!((controller.value() - expected).abs() < 1e-12);
    assert!(run.is_complete());
    registry.tick_all(&clock.frame(ms(1000)));
    assert_eq!(delivered.get(), 1);
    assert!(!registry.has_running());
}

fn reduced_friction_reaches_its_analytic_rest() {
    use flui_animation::simulation::{FrictionSimulation, Tolerance};
    settle_simulation(
        FrictionSimulation::new(0.135, 5.0, 300.0, Tolerance::DEFAULT).expect("friction"),
        5.0 - 300.0 / 0.135_f64.ln(),
    );
}

fn reduced_spring_reaches_its_exact_endpoint() {
    use flui_animation::simulation::{SpringDescription, SpringSimulation, Tolerance};
    settle_simulation(
        SpringSimulation::try_new(
            SpringDescription::new(1.0, 100.0, 20.0).expect("spring description"),
            7.0,
            13.0,
            -4.0,
            Tolerance::DEFAULT,
        )
        .expect("spring"),
        13.0,
    );
}

fn reduced_simulation_uses_the_first_finished_grid_sample() {
    settle_simulation(
        GridSimulation {
            non_finite_after: f64::INFINITY,
            done_at: 2.1,
        },
        4.0,
    );
}

fn reduced_simulation_uses_the_last_finite_grid_sample() {
    settle_simulation(
        GridSimulation {
            non_finite_after: f64::INFINITY,
            done_at: f64::INFINITY,
        },
        64.0,
    );
    settle_simulation(
        GridSimulation {
            non_finite_after: 4.0,
            done_at: f64::INFINITY,
        },
        2.0,
    );
}

fn reduced_non_finite_simulation_keeps_the_published_value() {
    settle_simulation(
        GridSimulation {
            non_finite_after: 0.125,
            done_at: f64::INFINITY,
        },
        0.0,
    );
}

#[test]
fn simulation_settle_grid() {
    run_table(&[
        (
            "friction analytic rest",
            reduced_friction_reaches_its_analytic_rest,
        ),
        (
            "spring exact endpoint",
            reduced_spring_reaches_its_exact_endpoint,
        ),
        (
            "first finished sample",
            reduced_simulation_uses_the_first_finished_grid_sample,
        ),
        (
            "last finite sample",
            reduced_simulation_uses_the_last_finite_grid_sample,
        ),
        (
            "no finite sample",
            reduced_non_finite_simulation_keeps_the_published_value,
        ),
    ]);
}

#[test]
fn reduced_settle_retires_simulation_sources_after_delivery() {
    use flui_animation::{AnimationRunFuture, AnimationStatus, MotionPreference, Simulation};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Debug)]
    struct RestartOnDrop {
        controller: AnimationController,
        restarted: Rc<RefCell<Option<AnimationRunFuture>>>,
        drops: Rc<Cell<usize>>,
        panic_on_drop: bool,
    }

    impl Simulation for RestartOnDrop {
        fn x(&self, time: f64) -> f64 {
            time
        }
        fn dx(&self, _time: f64) -> f64 {
            1.0
        }
        fn is_done(&self, time: f64) -> bool {
            time >= 0.25
        }
    }

    impl Drop for RestartOnDrop {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            assert_eq!(self.controller.status(), AnimationStatus::Completed);
            assert_eq!(self.controller.value(), 0.25);
            *self.restarted.borrow_mut() = Some(
                self.controller
                    .forward_from(Some(0.0))
                    .expect("restart from Drop"),
            );
            assert!(!self.panic_on_drop, "simulation retirement panic");
        }
    }

    for (panic_listener, panic_drop) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let registry = flui_animation::Vsync::new();
        let child = flui_animation::Vsync::new();
        let _seat = registry.attach_child(&child).expect("nested registry");
        let owner = AnimationController::builder(ms(1000)).build_on(Some(&child));
        let peer = AnimationController::builder(ms(1000)).build_on(Some(&registry));
        let restarted = Rc::new(RefCell::new(None));
        let drops = Rc::new(Cell::new(0));
        let run = owner
            .controller()
            .animate_with(RestartOnDrop {
                controller: owner.controller().clone(),
                restarted: Rc::clone(&restarted),
                drops: Rc::clone(&drops),
                panic_on_drop: panic_drop,
            })
            .expect("simulation run");
        let peer_run = peer.controller().forward().expect("peer run");
        let completions = Rc::new(Cell::new(0));
        let observed = Rc::clone(&completions);
        let _listener = owner.controller().subscribe_status(Rc::new(move |status| {
            if status == AnimationStatus::Completed {
                observed.set(observed.get() + 1);
                assert!(
                    !(panic_listener && observed.get() == 1),
                    "simulation listener panic"
                );
            }
        }));
        let mut clock = MotionClock::new();
        clock.set_preference(MotionPreference::Reduce);
        let settled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.tick_all(&clock.frame(Duration::ZERO));
        }));
        assert_eq!(settled.is_err(), panic_listener || panic_drop);
        if let Err(payload) = settled {
            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&if panic_listener {
                    "simulation listener panic"
                } else {
                    "simulation retirement panic"
                })
            );
        }
        assert!(
            run.is_complete(),
            "completion precedes outgoing source retirement"
        );
        assert!(
            peer_run.is_complete(),
            "failure does not consume the parent's accepted tail"
        );
        let expected_drops = usize::from(!panic_listener);
        assert_eq!(drops.get(), expected_drops);
        if panic_listener {
            // ADR-0127 retains outgoing user ownership after the first failure;
            // even a caught listener panic must prevent a competing destructor.
            assert!(restarted.borrow().is_none());
            assert_eq!(owner.controller().value(), 0.25);
            *restarted.borrow_mut() = Some(
                owner
                    .controller()
                    .forward_from(Some(0.0))
                    .expect("recovery run"),
            );
        }
        assert_eq!(owner.controller().value(), 0.0);
        assert!(!restarted.borrow().as_ref().expect("new run").is_complete());
        registry.tick_all(&clock.frame(ms(16)));
        assert_eq!(completions.get(), 2);
        assert_eq!(drops.get(), expected_drops);
        assert!(restarted.borrow().as_ref().expect("new run").is_complete());
        assert_eq!(owner.controller().value(), 1.0);
        assert!(!registry.has_running());
    }
}

#[test]
fn motion_policy_resolves_preference_against_the_system() {
    use flui_animation::{AnimationBehavior, MotionPolicy, MotionPreference};
    use flui_platform_api::MotionPreference as SystemMotion;
    let scaled = SystemMotion::from_duration_scale(2.0).expect("finite scale");
    for (preference, system, policy, expected) in [
        (
            MotionPreference::FollowSystem,
            SystemMotion::NoPreference,
            MotionPolicy::Full,
            0.3,
        ),
        (
            MotionPreference::FollowSystem,
            SystemMotion::Reduce,
            MotionPolicy::Reduce,
            1.0,
        ),
        (
            MotionPreference::FollowSystem,
            scaled,
            MotionPolicy::Full,
            0.15,
        ),
        (
            MotionPreference::Reduce,
            SystemMotion::NoPreference,
            MotionPolicy::Reduce,
            1.0,
        ),
        (
            MotionPreference::Reduce,
            SystemMotion::Reduce,
            MotionPolicy::Reduce,
            1.0,
        ),
        (MotionPreference::Reduce, scaled, MotionPolicy::Reduce, 1.0),
        (
            MotionPreference::Full,
            SystemMotion::NoPreference,
            MotionPolicy::Full,
            0.3,
        ),
        (
            MotionPreference::Full,
            SystemMotion::Reduce,
            MotionPolicy::Full,
            0.3,
        ),
        (MotionPreference::Full, scaled, MotionPolicy::Full, 0.3),
    ] {
        let registry = flui_animation::Vsync::new();
        let normal = AnimationController::builder(ms(1000)).build_on(Some(&registry));
        let preserve = AnimationController::builder(ms(1000))
            .behavior(AnimationBehavior::Preserve)
            .build_on(Some(&registry));
        normal.controller().forward().expect("normal run");
        preserve.controller().forward().expect("preserved run");
        let mut clock = MotionClock::new();
        clock.set_preference(preference);
        clock.set_system_motion(system);
        assert_eq!(clock.policy(), policy);
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(ms(300)));
        assert!(
            (normal.controller().value() - expected).abs() < 1e-9,
            "{preference:?}, {system:?}: expected {expected}, got {}",
            normal.controller().value()
        );
        assert!((preserve.controller().value() - 0.3).abs() < 1e-9);
    }
}

#[test]
fn duration_scale_changes_keep_the_published_motion_seam() {
    use flui_animation::{AnimationBehavior, MotionPreference};
    use flui_platform_api::MotionPreference as SystemMotion;
    let registry = flui_animation::Vsync::new();
    let normal = AnimationController::builder(ms(1000)).build_on(Some(&registry));
    let preserve = AnimationController::builder(ms(1000))
        .behavior(AnimationBehavior::Preserve)
        .build_on(Some(&registry));
    normal.controller().forward().expect("normal run");
    preserve.controller().forward().expect("preserved run");
    let mut clock = MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    for (raw, system, preference, expected) in [
        (200, None, None, 0.2),
        (
            400,
            Some(SystemMotion::from_duration_scale(2.0).expect("scale")),
            None,
            0.3,
        ),
        (
            500,
            Some(SystemMotion::from_duration_scale(0.5).expect("scale")),
            None,
            0.5,
        ),
        (600, None, Some(MotionPreference::Full), 0.6),
        (700, None, Some(MotionPreference::FollowSystem), 0.8),
        (650, None, None, 0.8),
    ] {
        let seam = normal.controller().value();
        if let Some(system) = system {
            clock.set_system_motion(system);
        }
        if let Some(preference) = preference {
            clock.set_preference(preference);
        }
        assert_eq!(
            normal.controller().value(),
            seam,
            "configuration does not sample a new position"
        );
        registry.tick_all(&clock.frame(ms(raw)));
        assert!(
            (normal.controller().value() - expected).abs() < 1e-9,
            "frame {raw}: expected {expected}, got {}",
            normal.controller().value()
        );
    }
    assert!(
        (preserve.controller().value() - 0.7).abs() < 1e-9,
        "preserve time only follows accepted raw frames"
    );
}

#[test]
fn reduced_settle_preserves_peer_delivery_and_reentrant_runs() {
    use flui_animation::AnimationStatus;
    use flui_platform_api::MotionPreference;
    use std::{
        cell::{Cell, RefCell},
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll, Waker},
    };
    for (panic_first, panic_peer) in [(false, false), (true, false), (false, true), (true, true)] {
        let registry = flui_animation::Vsync::new();
        let child = flui_animation::Vsync::new();
        let _seat = registry.attach_child(&child).expect("nested registry");
        let first = AnimationController::builder(ms(1000)).build_on(Some(&child));
        let peer = AnimationController::builder(ms(1000)).build_on(Some(&registry));
        let _peer_subscription = peer.controller().subscribe_status(Rc::new(move |status| {
            if status == AnimationStatus::Completed {
                assert!(!panic_peer, "peer settle callback panic");
            }
        }));
        let restarted = Rc::new(RefCell::new(None));
        let calls = Rc::new(Cell::new(0));
        let source = first.controller().clone();
        let restart = Rc::clone(&restarted);
        let count = Rc::clone(&calls);
        let _subscription = first.controller().subscribe_status(Rc::new(move |status| {
            if status == AnimationStatus::Completed {
                count.set(count.get() + 1);
                if count.get() == 1 {
                    *restart.borrow_mut() =
                        Some(source.forward_from(Some(0.0)).expect("reentrant run"));
                    assert!(!panic_first, "first settle callback panic");
                }
            }
        }));
        let mut old = first.controller().forward().expect("first run");
        let mut other = peer.controller().forward().expect("peer run");
        let mut clock = MotionClock::new();
        clock.set_system_motion(MotionPreference::Reduce);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.tick_all(&clock.frame(Duration::ZERO));
        }));
        assert_eq!(result.is_err(), panic_first || panic_peer);
        if let Err(payload) = &result {
            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&if panic_first {
                    "first settle callback panic"
                } else {
                    "peer settle callback panic"
                }),
                "the first failure remains authoritative"
            );
        }
        assert_eq!(
            calls.get(),
            1,
            "at most one settle per controller this frame"
        );
        assert_eq!(
            first.controller().value(),
            0.0,
            "the new run waits for the next frame"
        );
        assert_eq!(
            peer.controller().value(),
            1.0,
            "parent peer settled after the child callback"
        );
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(Pin::new(&mut old).poll(&mut context), Poll::Ready(Ok(())));
        assert_eq!(Pin::new(&mut other).poll(&mut context), Poll::Ready(Ok(())));
        {
            let mut replacement = restarted.borrow_mut();
            assert_eq!(
                Pin::new(replacement.as_mut().expect("restarted run")).poll(&mut context),
                Poll::Pending
            );
        }
        registry.tick_all(&clock.frame(ms(16)));
        assert_eq!(calls.get(), 2);
        assert_eq!(first.controller().value(), 1.0);
        assert_eq!(
            Pin::new(restarted.borrow_mut().as_mut().expect("restarted run")).poll(&mut context),
            Poll::Ready(Ok(()))
        );
        assert!(
            !registry.has_running(),
            "all admitted work completed after recovery"
        );
    }
}

#[test]
fn reduced_motion_settles_finite_and_paused_runs_once() {
    use flui_animation::AnimationStatus;
    use std::{
        cell::Cell,
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll, Waker},
    };
    for (reverse, repeat_count, expected, status) in [
        (false, None, 1.0, AnimationStatus::Completed),
        (true, None, 0.0, AnimationStatus::Dismissed),
        (false, Some(1), 1.0, AnimationStatus::Completed),
        (true, Some(1), 1.0, AnimationStatus::Completed),
        (true, Some(2), 0.0, AnimationStatus::Dismissed),
        (true, Some(3), 1.0, AnimationStatus::Completed),
    ] {
        for paused in [false, true] {
            let registry = flui_animation::Vsync::new();
            let owner = AnimationController::builder(ms(1000))
                .initial_value(if reverse && repeat_count.is_none() {
                    1.0
                } else {
                    0.0
                })
                .build_on(Some(&registry));
            let controller = owner.controller();
            let mut run = if let Some(count) = repeat_count {
                controller.repeat_with(None, None, reverse, None, Some(count))
            } else if reverse {
                controller.reverse()
            } else {
                controller.forward()
            }
            .expect("finite run");
            if paused {
                controller.set_playback_rate(PlaybackRate::PAUSED);
            }
            let delivered = Rc::new(Cell::new(0));
            let count = Rc::clone(&delivered);
            let _subscription = controller.subscribe_status(Rc::new(move |received| {
                assert_eq!(received, status);
                count.set(count.get() + 1);
            }));
            let mut clock = MotionClock::new();
            clock.set_rate(PlaybackRate::PAUSED);
            clock.set_preference(flui_animation::MotionPreference::Reduce);
            registry.tick_all(&clock.frame(Duration::ZERO));
            assert_eq!(
                controller.value(),
                expected,
                "reverse={reverse}, count={repeat_count:?}, paused={paused}"
            );
            assert_eq!(controller.status(), status);
            assert_eq!(
                Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
                Poll::Ready(Ok(()))
            );
            registry.tick_all(&clock.frame(ms(10_000)));
            assert_eq!(
                delivered.get(),
                1,
                "one terminal transition despite paused time"
            );
            assert!(!registry.has_running());
        }
    }
}

#[test]
fn reduced_motion_admission_wakes_a_paused_run() {
    use std::{cell::Cell, rc::Rc};
    for nested in [false, true] {
        let registry = flui_animation::Vsync::new();
        let child = flui_animation::Vsync::new();
        let _seat = nested.then(|| registry.attach_child(&child).expect("child"));
        let wakes = Rc::new(Cell::new(0));
        let received = Rc::clone(&wakes);
        registry.set_frame_requester(Some(Rc::new(move || received.set(received.get() + 1))));
        let mut clock = MotionClock::new();
        let owner = AnimationController::builder(ms(1000)).build_on(Some(if nested {
            &child
        } else {
            &registry
        }));
        owner.controller().set_playback_rate(PlaybackRate::PAUSED);
        owner
            .controller()
            .forward()
            .expect("install paused playback");
        registry.tick_all(&clock.frame(Duration::ZERO));
        owner.controller().stop().expect("stop installed run");
        clock.set_preference(flui_animation::MotionPreference::Reduce);
        registry.tick_all(&clock.frame(ms(1)));
        wakes.set(0);
        owner.controller().forward().expect("paused run");
        assert!(
            wakes.get() > 0,
            "paused normal work still needs a policy settlement frame, nested={nested}"
        );
        registry.tick_all(&clock.frame(ms(16)));
        assert_eq!(owner.controller().value(), 1.0);
        assert!(!registry.has_running());
    }
}

#[test]
fn muted_registry_settles_on_unmute() {
    use flui_animation::MotionPreference;
    use std::{cell::Cell, rc::Rc};
    for mute_parent in [false, true] {
        for paused in [false, true] {
            let registry = flui_animation::Vsync::new();
            let child = flui_animation::Vsync::new();
            let _seat = registry.attach_child(&child).expect("nested registry");
            let owner = AnimationController::builder(ms(1000)).build_on(Some(&child));
            let run = owner.controller().forward().expect("normal run");
            if paused {
                owner.controller().set_playback_rate(PlaybackRate::PAUSED);
            }
            let mut clock = MotionClock::new();
            registry.tick_all(&clock.frame(Duration::ZERO));
            let gate = if mute_parent { &registry } else { &child };
            gate.set_muted(true);
            clock.set_preference(MotionPreference::Reduce);
            registry.tick_all(&clock.frame(ms(16)));
            assert_eq!(owner.controller().value(), 0.0);
            assert!(!run.is_complete(), "muting defers settlement");
            let wakes = Rc::new(Cell::new(0));
            let count = Rc::clone(&wakes);
            registry.set_frame_requester(Some(Rc::new(move || count.set(count.get() + 1))));
            wakes.set(0);
            gate.set_muted(false);
            assert!(
                wakes.get() > 0,
                "unmute must request settlement, parent={mute_parent}, paused={paused}"
            );
            wakes.set(0);
            let count = Rc::clone(&wakes);
            registry.set_frame_requester(Some(Rc::new(move || count.set(count.get() + 1))));
            assert!(
                wakes.get() > 0,
                "replacement hook inherits undelivered settlement"
            );
            registry.tick_all(&clock.frame(ms(16)));
            assert_eq!(owner.controller().value(), 1.0);
            assert!(run.is_complete());
            assert!(!registry.has_running());
            wakes.set(0);
            registry.set_frame_requester(None);
            let count = Rc::clone(&wakes);
            registry.set_frame_requester(Some(Rc::new(move || count.set(count.get() + 1))));
            assert_eq!(
                wakes.get(),
                0,
                "delivered settlement creates no continuation"
            );
            let repeated = owner.controller().repeat(false).expect("parked repeat");
            registry.tick_all(&clock.frame(ms(32)));
            assert!(!registry.has_running());
            gate.set_muted(true);
            clock.set_preference(MotionPreference::Full);
            registry.tick_all(&clock.frame(ms(64)));
            wakes.set(0);
            gate.set_muted(false);
            assert!(wakes.get() > 0, "unmute requests a parked resumption");
            registry.tick_all(&clock.frame(ms(64)));
            assert_eq!(
                owner.controller().value(),
                0.0,
                "resumption anchors at this frame"
            );
            assert!(!repeated.is_complete());
        }
    }
}

#[test]
fn rebinding_a_parked_repeat_requests_its_new_policy_sample() {
    use flui_animation::MotionPreference;
    use std::{cell::Cell, rc::Rc};
    for nested in [false, true] {
        for sampled_destination in [false, true] {
            let old = flui_animation::Vsync::new();
            let mut owner = AnimationController::builder(ms(1000)).build_on(Some(&old));
            let run = owner.controller().repeat(false).expect("infinite repeat");
            let mut reduced = MotionClock::new();
            reduced.set_preference(MotionPreference::Reduce);
            old.tick_all(&reduced.frame(Duration::ZERO));
            assert!(!old.has_running());
            let registry = flui_animation::Vsync::new();
            let child = flui_animation::Vsync::new();
            let _seat = nested.then(|| registry.attach_child(&child).expect("nested registry"));
            let target = if nested { &child } else { &registry };
            let mut full = MotionClock::new();
            if sampled_destination {
                registry.tick_all(&full.frame(ms(100)));
            }
            let wakes = Rc::new(Cell::new(0));
            let count = Rc::clone(&wakes);
            registry.set_frame_requester(Some(Rc::new(move || count.set(count.get() + 1))));
            owner.rebind(Some(target)).expect("move parked repeat");
            assert!(
                wakes.get() > 0,
                "the new clock must observe its run, nested={nested}, sampled={sampled_destination}"
            );
            assert!(old.is_empty());
            registry.tick_all(&full.frame(ms(200)));
            assert_eq!(owner.controller().value(), 0.0);
            registry.tick_all(&full.frame(ms(450)));
            assert_eq!(owner.controller().value(), 0.25);
            assert!(!run.is_complete());
            assert!(registry.has_running());
        }
    }
}

#[test]
fn preserve_repeat_resumes_after_rebinding_under_reduce() {
    use flui_animation::{AnimationBehavior, MotionPreference, Vsync};
    use std::{cell::Cell, rc::Rc};

    for exhausted_origin in [false, true] {
        for nested in [false, true] {
            for sampled_destination in [false, true] {
                let old = Vsync::new();
                let mut owner = AnimationController::builder(ms(1000))
                    .behavior(AnimationBehavior::Preserve)
                    .build_on(exhausted_origin.then_some(&old));
                let run = owner.controller().repeat(false).expect("preserved repeat");
                if exhausted_origin {
                    old.tick_all(&MotionClock::new().frame(Duration::MAX));
                }
                assert!(!old.has_running());
                assert!(!run.is_complete());

                let registry = Vsync::new();
                let child = Vsync::new();
                let _seat = nested.then(|| registry.attach_child(&child).expect("nested"));
                let mut reduced = MotionClock::new();
                reduced.set_preference(MotionPreference::Reduce);
                if sampled_destination {
                    registry.tick_all(&reduced.frame(ms(100)));
                }
                let wakes = Rc::new(Cell::new(0));
                let count = Rc::clone(&wakes);
                registry.set_frame_requester(Some(Rc::new(move || count.set(count.get() + 1))));
                owner
                    .rebind(Some(if nested { &child } else { &registry }))
                    .expect("rebind");
                assert!(wakes.get() > 0, "Preserve requests resumption under Reduce");
                registry.tick_all(&reduced.frame(ms(200)));
                assert_eq!(owner.controller().value(), 0.0, "fresh resumption anchor");
                registry.tick_all(&reduced.frame(ms(450)));
                assert_eq!(
                    owner.controller().value(),
                    0.25,
                    "Preserve ignores Reduce: exhausted_origin={exhausted_origin}, nested={nested}, sampled_destination={sampled_destination}"
                );
                assert!(registry.has_running());
                assert!(!run.is_complete());
                assert!(old.is_empty());
            }
        }
    }
}

#[test]
fn tiny_scale_saturates_and_completes_once() {
    use std::{
        cell::Cell,
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll, Waker},
    };
    for scale in [1e-300, f64::from_bits(1)] {
        let registry = flui_animation::Vsync::new();
        let owner = AnimationController::builder(ms(1000)).build_on(Some(&registry));
        let preserve = AnimationController::builder(ms(1000))
            .behavior(flui_animation::AnimationBehavior::Preserve)
            .build_on(Some(&registry));
        preserve.controller().forward().expect("preserved run");
        let delivered = Rc::new(Cell::new(0));
        let count = Rc::clone(&delivered);
        let _subscription = owner.controller().subscribe_status(Rc::new(move |status| {
            if status == flui_animation::AnimationStatus::Completed {
                count.set(count.get() + 1);
            }
        }));
        owner.controller().forward().expect("finite run");
        let mut clock = MotionClock::new();
        clock.set_system_motion(
            flui_platform_api::MotionPreference::from_duration_scale(scale)
                .expect("positive scale"),
        );
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(ms(1)));
        assert_eq!(owner.controller().value(), 1.0);
        registry.tick_all(&clock.frame(ms(2)));
        assert_eq!(delivered.get(), 1);
        owner
            .controller()
            .forward_from(Some(0.0))
            .expect("next finite run after saturation");
        registry.tick_all(&clock.frame(ms(3)));
        assert_eq!(
            owner.controller().value(),
            1.0,
            "newly accepted work remains deliverable after clock saturation"
        );
        assert_eq!(delivered.get(), 2, "each run completes once");
        assert!(
            (preserve.controller().value() - 0.003).abs() < 1e-9,
            "normal saturation cannot exhaust preserve time"
        );
        preserve.controller().stop().expect("stop preserved run");
        let mut repeating = owner
            .controller()
            .repeat(false)
            .expect("infinite work at exhausted time");
        registry.tick_all(&clock.frame(ms(4)));
        assert_eq!(owner.controller().value(), 0.0);
        assert!(
            !registry.has_running(),
            "an exhausted timeline parks infinite work"
        );
        registry.tick_all(&clock.frame(ms(5)));
        assert!(
            !registry.has_running(),
            "Full alone does not resume an exhausted clock"
        );
        assert_eq!(
            Pin::new(&mut repeating).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        );
    }
}

#[test]
fn a_registry_ticked_by_two_clocks_never_regresses() {
    for nested in [false, true] {
        let registry = flui_animation::Vsync::new();
        let mut primary = MotionClock::new();
        registry.tick_all(&primary.frame(ms(1000)));
        let child = flui_animation::Vsync::new();
        let _child_seat = nested.then(|| registry.attach_child(&child).expect("child admitted"));
        let owner = AnimationController::builder(Duration::from_secs(1))
            .build_on(Some(if nested { &child } else { &registry }));
        owner.controller().forward().expect("new run");

        let mut stale = MotionClock::new();
        registry.tick_all(&stale.frame(ms(100)));
        assert_eq!(owner.controller().value(), 0.0);
        registry.tick_all(&primary.frame(ms(1100)));
        assert_eq!(
            owner.controller().value(),
            0.1,
            "an older clock anchors the new run at the last accepted registry time"
        );
    }
}

/// Run at 1 for 500 ms, switch to `to`, then frame at 600 ms.
fn rebase_from_normal(to: f64, expected_at_600: Duration) {
    let mut clock = MotionClock::new();
    assert_eq!(now_after(&mut clock, ms(500)), ms(500));
    clock.set_rate(rate(to));
    assert_eq!(
        clock.now().as_duration(),
        ms(500),
        "changing the rate must not move time"
    );
    assert_eq!(now_after(&mut clock, ms(600)), expected_at_600);
}

fn one_to_five() {
    rebase_from_normal(5.0, ms(1_000));
}

fn one_to_a_tenth() {
    rebase_from_normal(0.1, ms(510));
}

fn one_to_paused() {
    rebase_from_normal(0.0, ms(500));
}

fn paused_to_one() {
    let mut clock = MotionClock::new();
    clock.set_rate(PlaybackRate::PAUSED);
    assert_eq!(now_after(&mut clock, ms(10_000)), Duration::ZERO);
    clock.set_rate(PlaybackRate::NORMAL);
    assert_eq!(now_after(&mut clock, ms(10_016)), ms(16));
}

fn change_before_the_first_frame() {
    let mut clock = MotionClock::new();
    clock.set_rate(rate(2.0));
    assert_eq!(now_after(&mut clock, ms(100)), ms(200));
}

#[test]
fn rate_change_rebases_without_a_jump() {
    run_table(&[
        ("one_to_five", one_to_five),
        ("one_to_a_tenth", one_to_a_tenth),
        ("one_to_paused", one_to_paused),
        ("paused_to_one", paused_to_one),
        (
            "change_before_the_first_frame",
            change_before_the_first_frame,
        ),
    ]);
}

fn paused_clock_steps_exactly() {
    let mut clock = MotionClock::new();
    assert_eq!(now_after(&mut clock, ms(40)), ms(40));
    clock.set_rate(PlaybackRate::PAUSED);
    assert!(clock.is_paused());
    assert_eq!(clock.step(ms(16)), clock.now());
    assert_eq!(clock.now().as_duration(), ms(56));
    assert_eq!(now_after(&mut clock, ms(5_000)), ms(56));
}

fn step_is_exact_at_any_rate() {
    let mut clock = MotionClock::new();
    clock.set_rate(rate(3.0));
    assert_eq!(now_after(&mut clock, ms(10)), ms(30));
    assert_eq!(clock.step(ms(16)).as_duration(), ms(46));
    assert_eq!(now_after(&mut clock, ms(20)), ms(76));
}

fn step_saturates() {
    let mut clock = MotionClock::new();
    clock.step(Duration::MAX);
    assert_eq!(clock.step(ms(1)).as_duration(), Duration::MAX);
}

#[test]
fn stepping_moves_time_by_exactly_the_step() {
    run_table(&[
        ("paused_clock_steps_exactly", paused_clock_steps_exactly),
        ("step_is_exact_at_any_rate", step_is_exact_at_any_rate),
        ("step_saturates", step_saturates),
    ]);
}

#[test]
fn invalid_rates_are_refused_and_leave_state_unchanged() {
    for (value, expected) in [
        (f64::NAN, "NonFinite"),
        (f64::INFINITY, "NonFinite"),
        (f64::NEG_INFINITY, "NonFinite"),
        (-1.0, "Negative"),
        (-f64::MIN_POSITIVE, "Negative"),
    ] {
        let refused = PlaybackRate::try_from(value);
        let kind = match refused {
            Err(InvalidPlaybackRate::NonFinite(_)) => "NonFinite",
            Err(InvalidPlaybackRate::Negative(_)) => "Negative",
            Err(_) => "other",
            Ok(_) => "accepted",
        };
        assert_eq!(kind, expected, "rate {value}");
    }
    assert!(rate(-0.0).is_paused(), "negative zero is the paused rate");
    assert!(rate(-0.0).get().is_sign_positive());
}

fn back_by_one_nanosecond() {
    let mut clock = MotionClock::new();
    let at = now_after(&mut clock, ms(100));
    assert_eq!(now_after(&mut clock, Duration::from_nanos(99_999_999)), at);
    assert_eq!(now_after(&mut clock, ms(116)), ms(116));
}

fn back_by_ten_seconds() {
    let mut clock = MotionClock::new();
    assert_eq!(now_after(&mut clock, ms(20_000)), ms(20_000));
    assert_eq!(now_after(&mut clock, ms(10_000)), ms(20_000));
    // The raw origin is not moved by the stale frame.
    assert_eq!(now_after(&mut clock, ms(20_016)), ms(20_016));
}

fn back_to_zero() {
    let mut clock = MotionClock::new();
    assert_eq!(now_after(&mut clock, ms(300)), ms(300));
    assert_eq!(now_after(&mut clock, Duration::ZERO), ms(300));
}

fn repeated_raw_time_is_the_same_tick() {
    let mut clock = MotionClock::new();
    let first = clock.frame(ms(48));
    assert_eq!(clock.frame(ms(48)), first);
}

fn stale_manual_sample_holds_the_run() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let _run = controller.forward().expect("live manual controller");
    controller.tick_at(std::time::Duration::from_secs_f64(0.6));
    let displayed = controller.value();
    controller.tick_at(std::time::Duration::from_secs_f64(0.2));
    assert_eq!(
        controller.value(),
        displayed,
        "a stale sample cannot rewind a run"
    );
    controller.tick_at(std::time::Duration::from_secs_f64(0.7));
    assert!((controller.value() - 0.7).abs() < 1e-12);
}

#[test]
fn controller_rate_preserves_elapsed_and_paused_delivery() {
    run_table(&[
        ("tween", controller_rate_tween),
        ("curve", controller_rate_curve),
        ("repeat", controller_rate_repeat),
        ("spring", controller_rate_spring),
    ]);
}

fn controller_rate_tween() {
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let _run = controller.forward().expect("live controller");
    controller.tick_at(ms(400));
    controller.set_playback_rate(PlaybackRate::PAUSED);
    controller.tick_at(ms(500));
    assert_eq!(
        controller.value(),
        0.5,
        "pending rate preserves the next sample"
    );
    assert_eq!(controller.velocity(), 0.0);
    controller.tick_at(Duration::from_secs(20));
    assert_eq!(controller.value(), 0.5);
    controller.set_playback_rate(rate(2.0));
    controller.tick_at(Duration::from_secs(21));
    assert_eq!(
        controller.value(),
        0.5,
        "resume cannot count the paused gap"
    );
    controller.tick_at(ms(21_100));
    assert!((controller.value() - 0.7).abs() < 1e-12);
    assert_eq!(controller.velocity(), 2.0);
    controller.reverse().expect("fresh reverse run");
    controller.tick_at(ms(100));
    assert!(
        (controller.value() - 0.5).abs() < 1e-12,
        "rate survives restart"
    );
    assert_eq!(controller.velocity(), -2.0);
}

fn controller_rate_curve() {
    rate_change_mid_run_keeps_value_continuous("curve");
}

fn controller_rate_repeat() {
    rate_change_mid_run_keeps_value_continuous("repeat");
}

fn controller_rate_spring() {
    rate_change_mid_run_keeps_value_continuous("spring");
}

fn rate_change_mid_run_keeps_value_continuous(kind: &str) {
    use flui_animation::Curves;
    use flui_animation::simulation::{SpringDescription, SpringSimulation, Tolerance};

    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let run = match kind {
        "curve" => controller.animate_to_curved(1.0, None, Curves::Decelerate),
        "repeat" => controller.repeat_with(None, None, false, None, None),
        "spring" => controller.animate_with(
            SpringSimulation::try_new(
                SpringDescription::new(1.0, 1.0, 2.0).expect("critical spring"),
                0.0,
                1.0,
                0.0,
                Tolerance::DEFAULT,
            )
            .expect("finite spring"),
        ),
        _ => unreachable!("table names a run kind"),
    }
    .expect("live run");
    let position = |time: f64| match kind {
        "curve" => 2.0 * time - time * time,
        "repeat" => time.fract(),
        "spring" => 1.0 - (1.0 + time) * (-time).exp(),
        _ => unreachable!("table names a run kind"),
    };
    let derivative = |time: f64| match kind {
        "curve" => 2.0 - 2.0 * time,
        "repeat" => 1.0,
        "spring" => time * (-time).exp(),
        _ => unreachable!("table names a run kind"),
    };
    controller.tick_at(ms(400));
    controller.set_playback_rate(PlaybackRate::PAUSED);
    controller.tick_at(ms(500));
    assert!(
        (controller.value() - position(0.5)).abs() < 1e-12,
        "{kind}: old rate reaches the boundary"
    );
    assert_eq!(controller.velocity(), 0.0);
    assert!(run.is_pending());
    let held_status = controller.status();
    controller.tick_at(Duration::from_secs(20));
    assert!(
        (controller.value() - position(0.5)).abs() < 1e-12,
        "{kind}: paused gap"
    );
    assert_eq!(controller.status(), held_status);
    assert!(run.is_pending());
    controller.set_playback_rate(rate(2.0));
    controller.tick_at(Duration::from_secs(21));
    assert!(
        (controller.value() - position(0.5)).abs() < 1e-12,
        "{kind}: resume preserves the seam"
    );
    controller.tick_at(ms(21_100));
    assert!(
        (controller.value() - position(0.7)).abs() < 1e-12,
        "{kind}: only the resumed interval scales"
    );
    // Decelerate uses the public Curve default's numerical slope.
    assert!(
        (controller.velocity() - 2.0 * derivative(0.7)).abs() < 1e-8,
        "{kind}: velocity uses the applied rate, actual={}, expected={}",
        controller.velocity(),
        2.0 * derivative(0.7)
    );
    assert!(run.is_pending());
}

#[test]
fn backwards_raw_time_holds_the_timeline() {
    run_table(&[
        ("back_by_one_nanosecond", back_by_one_nanosecond),
        ("back_by_ten_seconds", back_by_ten_seconds),
        ("back_to_zero", back_to_zero),
        (
            "stale_manual_sample_holds_the_run",
            stale_manual_sample_holds_the_run,
        ),
        (
            "repeated_raw_time_is_the_same_tick",
            repeated_raw_time_is_the_same_tick,
        ),
    ]);
}

fn a_million_second_gap() {
    let mut clock = MotionClock::new();
    let gap = Duration::from_secs(1_000_000);
    assert_eq!(now_after(&mut clock, gap), gap);
}

fn the_largest_raw_time() {
    let mut clock = MotionClock::new();
    clock.set_rate(rate(2.0));
    assert_eq!(now_after(&mut clock, Duration::MAX), Duration::MAX);
}

fn an_enormous_rate() {
    let mut clock = MotionClock::new();
    clock.set_rate(rate(1e300));
    assert_eq!(
        now_after(&mut clock, Duration::from_secs(1_000_000)),
        Duration::MAX
    );
    clock.set_rate(PlaybackRate::NORMAL);
    assert_eq!(
        now_after(&mut clock, Duration::from_secs(2_000_000)),
        Duration::MAX
    );
}

#[test]
fn huge_frame_gaps_saturate() {
    run_table(&[
        ("a_million_second_gap", a_million_second_gap),
        ("the_largest_raw_time", the_largest_raw_time),
        ("an_enormous_rate", an_enormous_rate),
    ]);
}

/// Rates as exact fractions `numerator / denominator`, so the reference
/// integral is computed in integers.
const RATES: [(u128, u128); 7] = [(0, 1), (1, 10), (1, 2), (3, 4), (1, 1), (2, 1), (5, 1)];

#[derive(Clone, Debug)]
enum Op {
    /// Frame `delta_ns` after the previous raw time.
    Frame(u64),
    /// Frame `delta_ns` before the previous raw time.
    Backwards(u64),
    SetRate(usize),
    Step(u64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0..10_000_000_000u64).prop_map(Op::Frame),
        1 => (1..10_000_000_000u64).prop_map(Op::Backwards),
        2 => (0..RATES.len()).prop_map(Op::SetRate),
        1 => (0..100_000_000u64).prop_map(Op::Step),
    ]
}

proptest! {
    #[test]
    fn controller_local_time_is_the_integral_of_its_rate(
        changes in prop::collection::vec((0..10_000_u64, 0..RATES.len()), 1..64),
    ) {
        let controller = AnimationController::builder(Duration::from_secs(100)).build();
        let run = controller.forward().expect("long running tween");
        let mut raw_us = 0_u64;
        let mut integrated_ns = 0_u128;
        let mut applied = (1_u128, 1_u128);
        for (delta_us, index) in changes {
            let chosen = RATES[index];
            controller.set_playback_rate(rate(chosen.0 as f64 / chosen.1 as f64));
            raw_us += delta_us;
            integrated_ns += u128::from(delta_us) * 1000 * applied.0 / applied.1;
            controller.tick_at(Duration::from_micros(raw_us));
            let expected = integrated_ns as f64 / 100_000_000_000.0;
            prop_assert!((controller.value() - expected).abs() < 1e-12,
                "raw_us={}, actual={}, reference={}", raw_us, controller.value(), expected);
            prop_assert!(run.is_pending());
            applied = chosen;
        }
    }

    #[test]
    fn preserve_runs_identically_under_any_policy(
        changes in prop::collection::vec((0..100_000u64, 0..7usize, 0..3usize, 0..RATES.len()), 1..64),
    ) {
        use flui_animation::{AnimationBehavior, MotionPreference};
        use flui_platform_api::MotionPreference as SystemMotion;
        let registry = flui_animation::Vsync::new();
        let reference = flui_animation::Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(100))
            .behavior(AnimationBehavior::Preserve).build_on(Some(&registry));
        let baseline = AnimationController::builder(Duration::from_secs(100))
            .build_on(Some(&reference));
        owner.controller().forward().expect("preserved run");
        baseline.controller().forward().expect("reference run");
        let mut clock = MotionClock::new();
        let mut full = MotionClock::new();
        full.set_preference(MotionPreference::Full);
        registry.tick_all(&clock.frame(Duration::ZERO));
        reference.tick_all(&full.frame(Duration::ZERO));
        let mut raw = Duration::ZERO;
        for (delta, system, preference, rate_index) in changes {
            clock.set_system_motion(match system {
                0 => SystemMotion::Reduce,
                1 => SystemMotion::NoPreference,
                index => SystemMotion::from_duration_scale([0.25, 0.5, 2.0, 4.0, 10.0][index - 2]).expect("scale"),
            });
            clock.set_preference([MotionPreference::FollowSystem, MotionPreference::Reduce, MotionPreference::Full][preference]);
            let (numerator, denominator) = RATES[rate_index];
            #[expect(clippy::cast_precision_loss, reason = "small exact integers")]
            let chosen = rate(numerator as f64 / denominator as f64);
            clock.set_rate(chosen);
            full.set_rate(chosen);
            raw += Duration::from_micros(delta);
            registry.tick_all(&clock.frame(raw));
            reference.tick_all(&full.frame(raw));
            prop_assert_eq!(owner.controller().value(), baseline.controller().value());
            prop_assert_eq!(owner.controller().status(), baseline.controller().status());
        }
    }

    #[test]
    fn normal_timeline_integrates_inverse_scale_over_any_partition(
        changes in prop::collection::vec((0..100_000u64, 0..5usize, any::<bool>()), 1..64),
    ) {
        use flui_animation::MotionPreference;
        use flui_platform_api::MotionPreference as SystemMotion;
        // Power-of-two scales permit an independent exact integer integral.
        const INVERSE_SCALES: [(u64, u64); 5] = [(4, 1), (2, 1), (1, 1), (1, 2), (1, 4)];
        let registry = flui_animation::Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(100)).build_on(Some(&registry));
        owner.controller().forward().expect("normal run");
        let mut clock = MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        let mut raw = Duration::ZERO;
        let mut integral_ns = 0u64;
        for (delta_us, index, authored) in changes {
            clock.set_preference(if authored { MotionPreference::Full } else { MotionPreference::FollowSystem });
            clock.set_system_motion(SystemMotion::from_duration_scale([0.25, 0.5, 1.0, 2.0, 4.0][index]).expect("scale"));
            let (numerator, denominator) = if authored { (1, 1) } else { INVERSE_SCALES[index] };
            integral_ns += delta_us * 1000 * numerator / denominator;
            raw += Duration::from_micros(delta_us);
            registry.tick_all(&clock.frame(raw));
            #[expect(clippy::cast_precision_loss, reason = "bounded nanosecond reference")]
            let expected = integral_ns as f64 / 100_000_000_000.0;
            prop_assert!((owner.controller().value() - expected).abs() < 1e-10);
        }
    }

    #[test]
    fn animation_time_never_decreases(ops in prop::collection::vec(op(), 1..64)) {
        let mut clock = MotionClock::new();
        let mut raw = Duration::ZERO;
        let mut previous = clock.now();
        for op in ops {
            match op {
                Op::Frame(delta) => {
                    raw += Duration::from_nanos(delta);
                    let tick = clock.frame(raw);
                    prop_assert_eq!(tick.now(), clock.now());
                }
                Op::Backwards(delta) => {
                    let stale = raw.saturating_sub(Duration::from_nanos(delta));
                    let before = clock.now();
                    prop_assert_eq!(clock.frame(stale).now(), before);
                }
                Op::SetRate(index) => {
                    let (numerator, denominator) = RATES[index];
                    #[expect(clippy::cast_precision_loss, reason = "small exact integers")]
                    clock.set_rate(rate(numerator as f64 / denominator as f64));
                    prop_assert_eq!(clock.now(), previous);
                }
                Op::Step(dt) => {
                    clock.step(Duration::from_nanos(dt));
                }
            }
            prop_assert!(clock.now() >= previous);
            previous = clock.now();
        }
    }

    /// Animation time after any sequence of frames, rate changes and steps
    /// is `Σ Δraw · rate + Σ step`, independent of how the raw interval was
    /// cut into frames. The clock computes `Δ · rate` in `f64` once per rate
    /// epoch, so the tolerance is one nanosecond per epoch.
    #[test]
    fn timeline_is_the_integral_of_rate_over_any_frame_partition(
        ops in prop::collection::vec(op(), 1..64),
    ) {
        let mut clock = MotionClock::new();
        let mut raw: u128 = 0;
        // Reference: exact integral, rounded once per epoch like a duration.
        let mut settled: u128 = 0;
        let mut epoch_start: u128 = 0;
        let mut current = RATES[4];
        let mut epochs: u128 = 1;
        for op in ops {
            match op {
                Op::Frame(delta) => {
                    raw += u128::from(delta);
                    clock.frame(Duration::from_nanos(u64::try_from(raw).expect("bounded")));
                }
                Op::Backwards(_) => {}
                Op::SetRate(index) => {
                    let (numerator, denominator) = current;
                    settled += (raw - epoch_start) * numerator / denominator;
                    epoch_start = raw;
                    current = RATES[index];
                    epochs += 1;
                    #[expect(clippy::cast_precision_loss, reason = "small exact integers")]
                    clock.set_rate(rate(RATES[index].0 as f64 / RATES[index].1 as f64));
                }
                Op::Step(dt) => {
                    settled += u128::from(dt);
                    clock.step(Duration::from_nanos(dt));
                }
            }
        }
        // One final frame covering whatever raw time is left, so the
        // comparison sees every epoch closed by a frame.
        clock.frame(Duration::from_nanos(u64::try_from(raw).expect("bounded")));
        let (numerator, denominator) = current;
        let expected = settled + (raw - epoch_start) * numerator / denominator;
        let actual = clock.now().as_duration().as_nanos();
        prop_assert!(
            actual.abs_diff(expected) <= epochs,
            "actual {actual} ns, expected {expected} ns, tolerance {epochs} ns",
        );
    }

    /// Advancing in many frames lands where one frame to the same raw time
    /// lands.
    #[test]
    fn frame_partition_does_not_change_time(
        deltas in prop::collection::vec(0..1_000_000_000u64, 1..64),
        rate_index in 0..RATES.len(),
    ) {
        let (numerator, denominator) = RATES[rate_index];
        #[expect(clippy::cast_precision_loss, reason = "small exact integers")]
        let chosen = rate(numerator as f64 / denominator as f64);
        let mut stepped = MotionClock::new();
        let mut once = MotionClock::new();
        stepped.set_rate(chosen);
        once.set_rate(chosen);
        let mut raw = Duration::ZERO;
        for delta in deltas {
            raw += Duration::from_nanos(delta);
            stepped.frame(raw);
        }
        once.frame(raw);
        let expected = raw.as_nanos() * numerator / denominator;
        prop_assert!(stepped.now().as_duration().as_nanos().abs_diff(expected) <= 1);
        prop_assert!(stepped.now().as_duration().as_nanos().abs_diff(once.now().as_duration().as_nanos()) <= 1);
    }
}
