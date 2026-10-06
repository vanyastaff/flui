//! Consumer contracts for a presentation's animation clock.
//!
//! Reference values are analytic: animation time is the integral of the rate
//! over raw time, computed here in integer nanoseconds with rational rates.

use std::time::Duration;

use flui_animation::{InvalidPlaybackRate, MotionClock, PlaybackRate};
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
        ("change_before_the_first_frame", change_before_the_first_frame),
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
    assert_eq!(now_after(&mut clock, ms(100) - Duration::from_nanos(1)), at);
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

#[test]
fn backwards_raw_time_holds_the_timeline() {
    run_table(&[
        ("back_by_one_nanosecond", back_by_one_nanosecond),
        ("back_by_ten_seconds", back_by_ten_seconds),
        ("back_to_zero", back_to_zero),
        ("repeated_raw_time_is_the_same_tick", repeated_raw_time_is_the_same_tick),
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
    assert_eq!(now_after(&mut clock, Duration::from_secs(1_000_000)), Duration::MAX);
    clock.set_rate(PlaybackRate::NORMAL);
    assert_eq!(now_after(&mut clock, Duration::from_secs(2_000_000)), Duration::MAX);
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
