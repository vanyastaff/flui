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
            registry.tick_all(&clock.frame(Duration::ZERO))
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
fn tiny_scale_saturates_and_completes_once() {
    use std::{cell::Cell, rc::Rc};
    for scale in [1e-300, f64::from_bits(1)] {
        let registry = flui_animation::Vsync::new();
        let owner = AnimationController::builder(ms(1000)).build_on(Some(&registry));
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
        assert!(!registry.has_running());
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
