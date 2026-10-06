//! Keyframe tracks and staggered delays: placement by `Duration`, exact
//! boundaries, curve ownership, clamped and looped reads, typed build errors,
//! no published non-finite sample, cubic segments through the keyframe times,
//! and Motion-style stagger delays.
//!
//! Reference values are analytic: linear interpolation, `t²`, the cubic
//! Hermite basis at `s = 1/2`, and Motion's `stagger` formula
//! `step · |origin − i|` evaluated by hand.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use flui_animation::{
    Animatable, Curve, Curves, Keyframes, KeyframesError, Linear, Stagger, StaggerOrigin,
};
use proptest::prelude::*;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// `t²` on `[0, 1]`: an analytic, non-linear easing.
#[derive(Debug, Clone, Copy)]
struct Square;

impl Curve for Square {
    fn transform(&self, t: f64) -> f64 {
        t * t
    }
}

/// A curve that returns a fixed value strictly inside `(0, 1)`.
#[derive(Debug, Clone, Copy)]
struct Broken(f64);

impl Curve for Broken {
    fn transform(&self, t: f64) -> f64 {
        if t <= 0.0 || t >= 1.0 { t } else { self.0 }
    }
}

/// A curve that panics past the middle of its segment.
#[derive(Debug, Clone, Copy)]
struct PanicsLate;

impl Curve for PanicsLate {
    fn transform(&self, t: f64) -> f64 {
        assert!(t <= 0.5, "curve failure inside the segment");
        t
    }
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0),
        "{what}: {actual} != {expected}"
    );
}

/// 0 → 10 in 100 ms, hold 50 ms, → 20 in 100 ms; 400 ms in all.
fn ramp_hold_ramp() -> Keyframes<f64> {
    Keyframes::builder(0.0, ms(400))
        .to(10.0, ms(100), Linear)
        .hold(ms(50))
        .to(20.0, ms(100), Linear)
        .build()
        .expect("the segments fit in 400 ms")
}

// ---- placement --------------------------------------------------------------

fn segments_follow_each_other() {
    let track = ramp_hold_ramp();
    assert_eq!(track.total(), ms(400));
    assert_close(track.value_at(ms(50)), 5.0, "first ramp midpoint");
    assert_close(track.value_at(ms(125)), 10.0, "inside the hold");
    assert_close(track.value_at(ms(200)), 15.0, "second ramp midpoint");
    assert_close(track.value_at(ms(350)), 20.0, "held until total");
}

fn delay_is_a_leading_hold() {
    let track = Keyframes::builder(0.0, ms(300))
        .hold(ms(100))
        .to(1.0, ms(200), Linear)
        .build()
        .expect("fits");
    assert_close(track.value_at(ms(99)), 0.0, "still waiting");
    assert_close(track.value_at(ms(200)), 0.5, "half way through the motion");
}

#[test]
fn keyframes_segments_follow_each_other() {
    crate::run_table(&[
        ("segments in order", segments_follow_each_other),
        ("delay as leading hold", delay_is_a_leading_hold),
    ]);
}

// ---- exact boundaries -------------------------------------------------------

fn eased_boundaries_are_keyframes() {
    let track = Keyframes::builder(0.1, ms(300))
        .to(0.7, ms(100), Curves::EaseInOut)
        .to(0.3, ms(100), Curves::EaseIn)
        .build()
        .expect("fits");
    assert_eq!(track.value_at(ms(0)).to_bits(), 0.1_f64.to_bits());
    assert_eq!(track.value_at(ms(100)).to_bits(), 0.7_f64.to_bits());
    assert_eq!(track.value_at(ms(200)).to_bits(), 0.3_f64.to_bits());
}

fn jump_is_right_continuous() {
    let track = Keyframes::builder(0.0, ms(200))
        .to(1.0, ms(100), Linear)
        .jump(5.0)
        .to(6.0, ms(100), Linear)
        .build()
        .expect("fits");
    // One nanosecond before the jump the ramp is 1e-8 short of its key.
    assert_close(
        track.value_at(Duration::from_nanos(99_999_999)),
        1.0 - 1e-8,
        "before",
    );
    assert_eq!(
        track.value_at(ms(100)),
        5.0,
        "at the jump: the value after it"
    );
    assert_close(track.value_at(ms(150)), 5.5, "after");
}

fn jump_at_zero_replaces_the_start() {
    let track = Keyframes::builder(0.0, ms(100))
        .jump(3.0)
        .hold(ms(100))
        .build()
        .expect("fits");
    assert_eq!(track.value_at(Duration::ZERO), 3.0);
}

#[test]
fn keyframes_boundaries_are_exact() {
    crate::run_table(&[
        ("eased boundaries", eased_boundaries_are_keyframes),
        ("jump", jump_is_right_continuous),
        ("jump at zero", jump_at_zero_replaces_the_start),
    ]);
}

// ---- curve ownership --------------------------------------------------------

fn curve_shapes_only_its_own_segment() {
    let track = Keyframes::builder(0.0, ms(200))
        .to(1.0, ms(100), Linear)
        .to(2.0, ms(100), Square)
        .build()
        .expect("fits");
    // The square curve arrives at 2.0; the segment before it stays linear.
    assert_close(track.value_at(ms(50)), 0.5, "linear segment");
    assert_close(track.value_at(ms(150)), 1.25, "square segment: 1 + 0.5²");
}

#[test]
fn keyframes_curve_belongs_to_arriving_segment() {
    crate::run_table(&[("square after linear", curve_shapes_only_its_own_segment)]);
}

// ---- clamp and loop ---------------------------------------------------------

fn clamps_past_total() {
    let track = ramp_hold_ramp();
    assert_eq!(track.value_at(ms(400) + ms(1)), track.value_at(ms(400)));
    assert_close(track.value_at(Duration::MAX), 20.0, "far past total");
}

fn loops_modulo_total() {
    let track = ramp_hold_ramp();
    assert_close(track.value_at_looped(ms(450)), 5.0, "second cycle");
    assert_close(
        track.value_at_looped(ms(400) * 7 + ms(200)),
        15.0,
        "eighth cycle",
    );
    assert_eq!(
        track.value_at_looped(ms(800)),
        0.0,
        "a cycle boundary is time 0"
    );
}

fn duration_max_loops_without_panic() {
    let track = Keyframes::builder(0.0, Duration::from_secs(1))
        .to(1.0, Duration::from_secs(1), Linear)
        .build()
        .expect("fits");
    // Duration::MAX is u64::MAX seconds and 999_999_999 ns: 0.999_999_999 s
    // into its cycle.
    assert_close(
        track.value_at_looped(Duration::MAX),
        0.999_999_999,
        "Duration::MAX mod 1 s",
    );
}

#[test]
fn keyframes_clamp_and_loop() {
    crate::run_table(&[
        ("clamp", clamps_past_total),
        ("loop", loops_modulo_total),
        (
            "duration_max_loops_without_panic",
            duration_max_loops_without_panic,
        ),
    ]);
}

// ---- progress ---------------------------------------------------------------

fn progress_maps_to_time() {
    let track = ramp_hold_ramp();
    assert_close(track.transform(0.0), 0.0, "t = 0");
    assert_close(track.transform(0.5), 15.0, "t = 0.5 is 200 ms");
    assert_close(track.transform(1.0), 20.0, "t = 1");
    assert_close(track.transform(-1.0), 0.0, "t < 0 clamps");
    assert_close(track.transform(2.0), 20.0, "t > 1 clamps");
    assert_close(track.transform(f64::NAN), 0.0, "NaN reads as 0");
    assert_close(track.transform(f64::INFINITY), 20.0, "+inf");
    assert_close(track.transform(f64::NEG_INFINITY), 0.0, "-inf");
}

fn progress_on_the_longest_track() {
    let track = Keyframes::builder(0.0, Duration::MAX)
        .to(1.0, Duration::MAX, Linear)
        .build()
        .expect("fits");
    assert_close(track.transform(1.0), 1.0, "t = 1 of Duration::MAX");
    assert_close(track.transform(0.5), 0.5, "t = 0.5 of Duration::MAX");
}

#[test]
fn keyframes_progress_maps_to_time() {
    crate::run_table(&[
        ("progress", progress_maps_to_time),
        ("longest track", progress_on_the_longest_track),
    ]);
}

/// Two linear segments, 0 → 1 → 2, lasting `first` and `last`.
fn two_ramps(first: Duration, last: Duration) -> Keyframes<f64> {
    Keyframes::builder(0.0, first + last)
        .to(1.0, first, Linear)
        .to(2.0, last, Linear)
        .build()
        .expect("fits")
}

fn huge_segments_keep_interior_progress() {
    let half = Duration::MAX / 2;
    let track = two_ramps(half, half);
    assert_eq!(track.transform(0.0), 0.0);
    assert_close(track.transform(0.25), 0.5, "middle of the first half");
    assert_close(track.value_at(half), 1.0, "the join");
    assert_close(track.transform(0.75), 1.5, "middle of the second half");
    assert_eq!(track.transform(1.0), 2.0);
}

fn tiny_last_segment_reaches_its_end_and_interior() {
    let track = two_ramps(Duration::from_secs(1), Duration::from_nanos(10));
    assert_close(
        track.value_at(Duration::new(1, 5)),
        1.5,
        "half way through 10 ns",
    );
    assert_eq!(track.transform(1.0), 2.0);
}

fn tiny_first_segment_keeps_its_interior() {
    let track = two_ramps(Duration::from_nanos(10), Duration::from_secs(1));
    assert_eq!(track.value_at(Duration::ZERO), 0.0);
    assert_close(track.value_at(Duration::from_nanos(5)), 0.5, "5 of 10 ns");
}

#[test]
fn keyframes_extreme_durations_keep_relative_progress() {
    crate::run_table(&[
        ("huge segments", huge_segments_keep_interior_progress),
        (
            "tiny last segment",
            tiny_last_segment_reaches_its_end_and_interior,
        ),
        ("tiny first segment", tiny_first_segment_keeps_its_interior),
    ]);
}

// ---- build errors -----------------------------------------------------------

fn zero_total() {
    let error = Keyframes::builder(0.0, Duration::ZERO)
        .build()
        .expect_err("the track is invalid");
    assert_eq!(error, KeyframesError::ZeroTotal);
}

fn overrun() {
    let error = Keyframes::builder(0.0, ms(100))
        .to(1.0, ms(60), Linear)
        .to(2.0, ms(60), Linear)
        .build()
        .expect_err("the track is invalid");
    assert_eq!(
        error,
        KeyframesError::Overrun {
            index: 1,
            end: ms(120),
            total: ms(100)
        }
    );
}

fn duration_overflow() {
    let error = Keyframes::builder(0.0, Duration::MAX)
        .hold(Duration::MAX)
        .hold(Duration::from_nanos(1))
        .build()
        .expect_err("the track is invalid");
    assert_eq!(error, KeyframesError::DurationOverflow { index: 1 });
}

fn non_finite_start() {
    let error = Keyframes::builder(f64::INFINITY, ms(100))
        .build()
        .expect_err("the track is invalid");
    assert_eq!(error, KeyframesError::NonFiniteValue { index: 0 });
}

fn non_finite_keyframe() {
    let error = Keyframes::builder(0.0, ms(300))
        .to(1.0, ms(100), Linear)
        .hold(ms(50))
        .cubic(f64::NAN, ms(100))
        .build()
        .expect_err("the track is invalid");
    assert_eq!(error, KeyframesError::NonFiniteValue { index: 3 });
}

fn non_finite_jump() {
    let error = Keyframes::builder(0.0, ms(100))
        .jump(f64::NEG_INFINITY)
        .build()
        .expect_err("the track is invalid");
    assert_eq!(error, KeyframesError::NonFiniteValue { index: 1 });
}

#[test]
fn keyframes_build_rejects_invalid_tracks() {
    crate::run_table(&[
        ("zero total", zero_total),
        ("overrun", overrun),
        ("duration overflow", duration_overflow),
        ("non-finite start", non_finite_start),
        ("non-finite keyframe", non_finite_keyframe),
        ("non-finite jump", non_finite_jump),
    ]);
}

// ---- non-finite samples -----------------------------------------------------

fn nan_curve_publishes_segment_start() {
    let track = Keyframes::builder(0.0, ms(200))
        .to(1.0, ms(100), Linear)
        .to(3.0, ms(100), Broken(f64::NAN))
        .build()
        .expect("fits");
    assert_eq!(track.value_at(ms(150)), 1.0);
}

fn infinite_curve_publishes_segment_start() {
    let track = Keyframes::builder(2.0, ms(100))
        .to(3.0, ms(100), Broken(f64::INFINITY))
        .build()
        .expect("fits");
    assert_eq!(track.value_at(ms(50)), 2.0);
}

fn overflowing_cubic_stays_finite() {
    let track = Keyframes::builder(0.0, ms(300))
        .cubic(1e308, ms(100))
        .cubic(-1e308, ms(100))
        .cubic(1e308, ms(100))
        .build()
        .expect("finite keyframes");
    for step in 0..=300 {
        let value = track.value_at(ms(step));
        assert!(value.is_finite(), "non-finite sample at {step} ms");
    }
    assert_eq!(track.value_at(ms(100)), 1e308);
}

fn overflowing_ramp_stays_finite() {
    let track = Keyframes::builder(1e308, ms(100))
        .to(-1e308, ms(100), Linear)
        .build()
        .expect("finite keyframes");
    for step in 0..=100 {
        assert!(track.value_at(ms(step)).is_finite(), "at {step} ms");
    }
    assert_eq!(track.value_at(ms(100)), -1e308);
}

#[test]
fn keyframes_never_publish_non_finite() {
    crate::run_table(&[
        ("NaN curve", nan_curve_publishes_segment_start),
        ("infinite curve", infinite_curve_publishes_segment_start),
        ("overflowing cubic", overflowing_cubic_stays_finite),
        ("overflowing ramp", overflowing_ramp_stays_finite),
    ]);
}

// ---- cubic segments ---------------------------------------------------------

fn cubic_hits_a_late_keyframe_at_its_time() {
    // Keys (0, 0), (0.9 s, 0.5), (1 s, 1): a spline solved by time, not by
    // parameter, reads exactly 0.5 at 0.9 s.
    let track = Keyframes::builder(0.0, Duration::from_secs(1))
        .cubic(0.5, ms(900))
        .cubic(1.0, ms(100))
        .build()
        .expect("fits");
    assert_eq!(track.value_at(ms(900)), 0.5);
    assert_eq!(track.value_at(Duration::from_secs(1)), 1.0);
}

fn cubic_hermite_midpoint() {
    // Keys 0, 1, 0 at 0, 1, 2 s. The end velocities are zero and the middle
    // tangent (0 − 0) / 2 s is zero, so each half is the Hermite basis
    // h01(1/2) = 1/2 of its rise.
    let track = Keyframes::builder(0.0, Duration::from_secs(2))
        .cubic(1.0, Duration::from_secs(1))
        .cubic(0.0, Duration::from_secs(1))
        .build()
        .expect("fits");
    assert_close(track.value_at(ms(500)), 0.5, "rising half");
    assert_close(track.value_at(ms(1500)), 0.5, "falling half");
    assert_eq!(track.value_at(ms(1000)), 1.0);
}

#[test]
fn cubic_keyframes_pass_through_keys() {
    crate::run_table(&[
        ("late keyframe", cubic_hits_a_late_keyframe_at_its_time),
        ("Hermite midpoint", cubic_hermite_midpoint),
    ]);
}

/// The one-sided second-order derivative estimates of `track` at `at`.
fn one_sided_slopes(track: &Keyframes<f64>, at: Duration, h: Duration) -> (f64, f64) {
    let f = |t: Duration| track.value_at(t);
    let hs = h.as_secs_f64();
    let left =
        (3.0 * f(at) - 4.0 * f(at.saturating_sub(h)) + f(at.saturating_sub(2 * h))) / (2.0 * hs);
    let right = (-3.0 * f(at) + 4.0 * f(at + h) - f(at + 2 * h)) / (2.0 * hs);
    (left, right)
}

proptest! {
    #[test]
    fn keyframes_boundaries_are_exact_for_any_durations(
        segments in prop::collection::vec((1_u64..5_000, -100.0..100.0_f64, 0_u8..4), 1..8),
    ) {
        let mut builder = Keyframes::builder(0.25, Duration::from_secs(60));
        let mut keys = vec![(Duration::ZERO, 0.25)];
        let mut at = Duration::ZERO;
        let mut current = 0.25;
        for &(millis, value, kind) in &segments {
            let over = ms(millis);
            builder = match kind {
                0 => builder.to(value, over, Curves::EaseInOut),
                1 => builder.cubic(value, over),
                2 => builder.hold(over),
                _ => builder.jump(value),
            };
            match kind {
                2 => at += over,
                3 => current = value,
                _ => {
                    at += over;
                    current = value;
                }
            }
            keys.push((at, current));
        }
        let track = builder.build().expect("at most 40 s of segments");
        // Later keys at the same time (a jump) win: right-continuity.
        for (index, &(time, value)) in keys.iter().enumerate() {
            if keys[index + 1..].iter().any(|&(later, _)| later == time) {
                continue;
            }
            prop_assert_eq!(track.value_at(time).to_bits(), value.to_bits());
        }
    }

    #[test]
    fn keyframes_evaluation_is_order_independent(
        times in prop::collection::vec(0_u64..2_000_000, 1..32),
        seed in any::<u64>(),
    ) {
        // A linear ramp of 1 per millisecond over 1 s, then held.
        let track = Keyframes::builder(0.0, ms(2_000))
            .to(1_000.0, ms(1_000), Linear)
            .build()
            .expect("fits");
        let reference = |micros: u64| (micros as f64 / 1_000.0).min(1_000.0);
        let forward: Vec<f64> = times.iter().map(|&t| track.value_at(Duration::from_micros(t))).collect();
        // A deterministic shuffle from the seed, then back in time.
        let mut order: Vec<usize> = (0..times.len()).collect();
        order.sort_by_key(|&i| (seed.rotate_left(i as u32) ^ i as u64, i));
        for &i in order.iter().rev() {
            let again = track.value_at(Duration::from_micros(times[i]));
            prop_assert_eq!(again.to_bits(), forward[i].to_bits());
            prop_assert!((again - reference(times[i])).abs() < 1e-9);
        }
    }

    #[test]
    fn cubic_keyframes_are_c1_at_joins(
        values in prop::collection::vec(-100.0..100.0_f64, 2..6),
        millis in prop::collection::vec(10_u64..2_000, 6),
        lead_linear in any::<bool>(),
    ) {
        let total: Duration = millis.iter().map(|&m| ms(m)).sum();
        let mut builder = Keyframes::builder(0.0, total);
        let mut joins = Vec::new();
        let mut at = Duration::ZERO;
        let mut scale: f64 = 0.0;
        let mut previous = 0.0;
        for (index, &value) in values.iter().enumerate() {
            let over = ms(millis[index]);
            builder = if index == 0 && lead_linear {
                builder.to(value, over, Linear)
            } else {
                builder.cubic(value, over)
            };
            scale = scale.max((value - previous).abs() / over.as_secs_f64());
            previous = value;
            at += over;
            joins.push((at, value));
        }
        let track = builder.build().expect("fits");
        joins.pop();
        let h = Duration::from_micros(1);
        for (time, value) in joins {
            prop_assert_eq!(track.value_at(time), value);
            let (left, right) = one_sided_slopes(&track, time, h);
            prop_assert!(
                (left - right).abs() <= 1e-6 * scale.max(1.0) + 1e-6,
                "join at {:?}: left {} right {}", time, left, right
            );
        }
    }
}

// ---- stagger ----------------------------------------------------------------

/// Motion's `stagger(step, { from })`: `step · |origin − i|`, in half-steps so
/// a centred origin between two indices stays exact.
fn motion_delay(step: Duration, origin_doubled: i64, index: usize) -> Duration {
    let half_steps = (origin_doubled - 2 * index as i64).unsigned_abs();
    step * u32::try_from(half_steps).expect("small table") / 2
}

fn stagger_rows_for(count: usize) {
    let step = ms(100);
    let n = count as i64;
    let origins = [
        (StaggerOrigin::First, 0),
        (StaggerOrigin::Last, 2 * (n - 1)),
        (StaggerOrigin::Center, n - 1),
        (StaggerOrigin::Index(2), 4),
    ];
    for (origin, origin_doubled) in origins {
        let stagger = Stagger::new(step, origin);
        for index in 0..count {
            assert_eq!(
                stagger.delay(index, count),
                motion_delay(step, origin_doubled, index),
                "{origin:?}, index {index} of {count}"
            );
        }
    }
}

fn one_element() {
    stagger_rows_for(1);
}

fn four_elements() {
    stagger_rows_for(4);
    assert_eq!(
        Stagger::new(ms(100), StaggerOrigin::Center).delay(0, 4),
        ms(150)
    );
}

fn five_elements() {
    stagger_rows_for(5);
    assert_eq!(
        Stagger::new(ms(100), StaggerOrigin::Last).delay(0, 5),
        ms(400)
    );
}

fn huge_count_saturates() {
    let stagger = Stagger::new(Duration::from_secs(2), StaggerOrigin::First);
    assert_eq!(stagger.delay(usize::MAX, usize::MAX), Duration::MAX);
    let exact = Stagger::new(Duration::from_secs(1), StaggerOrigin::First);
    assert_eq!(
        exact.delay(1 << 40, usize::MAX),
        Duration::from_secs(1 << 40)
    );
}

#[test]
fn stagger_delays_follow_origin() {
    crate::run_table(&[
        ("n = 1", one_element),
        ("n = 4", four_elements),
        ("n = 5", five_elements),
        ("huge_count_saturates", huge_count_saturates),
    ]);
}

// ---- groups and stateless failure -------------------------------------------

fn tracks_share_one_progress() {
    let total = Duration::from_secs(6);
    let rotation = Keyframes::builder(0.0, total)
        .to(1080.0, total, Linear)
        .build()
        .expect("fits");
    let step = Keyframes::builder(0.0, total)
        .to(90.0, ms(300), Linear)
        .hold(ms(1200))
        .to(180.0, ms(300), Linear)
        .build()
        .expect("fits");
    let sweep = Keyframes::builder(0.1, total)
        .to(0.87, ms(3000), Linear)
        .to(0.1, ms(3000), Linear)
        .build()
        .expect("fits");
    // One controller value, three tracks, one clock: progress 0.25 is 1.5 s.
    let progress = 0.25;
    assert_close(rotation.transform(progress), 270.0, "rotation at 1.5 s");
    assert_close(step.transform(progress), 90.0, "step at 1.5 s");
    assert_close(sweep.transform(progress), 0.485, "sweep at 1.5 s");
}

#[test]
fn keyframes_group_shares_one_clock() {
    crate::run_table(&[("three indicator tracks", tracks_share_one_progress)]);
}

fn panicking_curve_leaves_track_intact() {
    let track = Keyframes::builder(0.0, ms(100))
        .to(1.0, ms(100), PanicsLate)
        .build()
        .expect("fits");
    let failed = catch_unwind(AssertUnwindSafe(|| track.value_at(ms(80))));
    assert!(failed.is_err(), "the curve panics late in the segment");
    assert_close(track.value_at(ms(40)), 0.4, "the next query samples afresh");
    assert_eq!(track.value_at(ms(100)), 1.0);
}

#[test]
fn keyframes_survive_panicking_curve() {
    crate::run_table(&[("panicking curve", panicking_curve_leaves_track_intact)]);
}
