//! `#[derive(TwoWayConverter)]` as a downstream crate uses it.
//!
//! This lives in `tests/` (a separate crate that depends on flui-animation) so
//! the derive's `::flui_animation::TwoWayConverter` path resolves the same way
//! it does for a real downstream user.

// Scalar round-trip assertions use exactly representable f64 values.
// Color conversion has its own representation and precision contract.

use flui_animation::{Keyframes, Lerp, Linear, TwoWayConverter};
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;

#[derive(Clone, TwoWayConverter)]
struct Translation {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, TwoWayConverter)]
struct Pair(f64, f64);

#[derive(Clone, TwoWayConverter)]
struct Appearance {
    position: Offset<f64>,
    color: Color,
}

#[derive(Clone, TwoWayConverter)]
struct NestedAppearance(Appearance, f64);

fn nested_fields_keep_their_interpolation_contracts() {
    let begin = NestedAppearance(
        Appearance {
            position: Offset::ZERO,
            color: Color::rgb(255, 0, 0),
        },
        2.0,
    );
    let end = NestedAppearance(
        Appearance {
            position: Offset::new(20.0, 40.0),
            color: Color::rgba(0, 0, 255, 0),
        },
        6.0,
    );
    let middle = begin.lerp_to(&end, 0.5);
    assert_eq!(middle.0.position, Offset::new(10.0, 20.0));
    assert_eq!(middle.1, 4.0);
    assert!(
        middle.0.color.r >= 254 && middle.0.color.g <= 1 && middle.0.color.b <= 1,
        "transparent blue cannot muddy the red fade: {:?}",
        middle.0.color
    );
    assert!((i16::from(middle.0.color.a) - 128).abs() <= 1);
    let rebuilt = NestedAppearance::from_vector([2.0, -4.0, 0.0, 0.0, 0.0, 0.0, 0.75]);
    assert_eq!(rebuilt.0.position, Offset::new(2.0, -4.0));
    assert_eq!(rebuilt.0.color, Color::TRANSPARENT);
    assert_eq!(rebuilt.1, 0.75);
}

fn nested_values_retarget_on_one_registered_motion() {
    use flui_animation::{AnimatedValue, ArcCurve, MotionClock, MotionSpec, Vsync};
    use std::time::Duration;

    let vsync = Vsync::new();
    let mut clock = MotionClock::new();
    let mut owner = AnimatedValue::new(
        NestedAppearance(
            Appearance {
                position: Offset::ZERO,
                color: Color::rgb(255, 0, 0),
            },
            0.0,
        ),
        MotionSpec::Curve {
            duration: Duration::from_secs(1),
            curve: ArcCurve::new(Linear),
        },
        Some(&vsync),
    )
    .expect("finite nested value");
    let first = owner
        .animate_to(NestedAppearance(
            Appearance {
                position: Offset::new(20.0, 40.0),
                color: Color::rgba(0, 0, 255, 0),
            },
            1.0,
        ))
        .expect("first nested motion");
    vsync.tick_all(&clock.frame(Duration::ZERO));
    vsync.tick_all(&clock.frame(Duration::from_millis(500)));
    let seam = (owner.value(), owner.velocity());
    assert!(seam.0.0.position.dx > 0.0 && seam.0.0.position.dy > 0.0);
    assert!(seam.0.0.color.r >= 253 && seam.0.0.color.b <= 2);
    assert!(seam.0.0.color.a > 0 && seam.0.0.color.a < 255);
    let second = owner
        .animate_to(NestedAppearance(
            Appearance {
                position: Offset::new(-20.0, -40.0),
                color: Color::rgb(0, 255, 0),
            },
            2.0,
        ))
        .expect("nested interruption");
    assert!(first.is_canceled() && second.is_pending());
    assert_eq!(owner.value().0.position, seam.0.0.position);
    assert_eq!(owner.value().0.color, seam.0.0.color);
    assert_eq!(owner.velocity(), seam.1);
    // A one-microsecond difference isolates the inherited derivative from
    // the new segment's acceleration toward its opposite target.
    vsync.tick_all(&clock.frame(Duration::from_micros(500_001)));
    let next = owner.value();
    for (delta, velocity) in [
        (next.0.position.dx - seam.0.0.position.dx, seam.1[0]),
        (next.0.position.dy - seam.0.0.position.dy, seam.1[1]),
    ] {
        assert!(
            (delta / 1e-6 - velocity).abs() < 0.001,
            "incoming velocity {velocity}, next finite difference {}",
            delta / 1e-6
        );
    }
    vsync.tick_all(&clock.frame(Duration::from_secs(10)));
    assert!(second.is_complete());
    let settled = owner.value();
    assert_eq!(settled.0.position, Offset::new(-20.0, -40.0));
    assert_eq!(settled.0.color, Color::rgb(0, 255, 0));
    assert_eq!(settled.1, 2.0);
}

fn nested_springs_keep_each_fields_rest_units() {
    use flui_animation::{AnimatedValue, MotionClock, MotionSpec, SpringDescription, Vsync};
    use std::time::Duration;

    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let motion = MotionSpec::Spring(SpringDescription::with_damping_ratio(1.0, 100.0, 1.0));
    let mut owner = AnimatedValue::new(
        NestedAppearance(
            Appearance {
                position: Offset::ZERO,
                color: Color::BLACK,
            },
            0.0,
        ),
        motion.clone(),
        Some(&registry),
    )
    .expect("finite nested value");
    let goal = Offset::new(100.0, -100.0);
    let mut geometry =
        AnimatedValue::new(Offset::ZERO, motion, Some(&registry)).expect("finite geometry");
    let geometry_run = geometry.animate_to(goal).expect("finite geometry goal");
    let run = owner
        .animate_to(NestedAppearance(
            Appearance {
                position: goal,
                color: Color::BLACK,
            },
            0.005,
        ))
        .expect("finite target");
    registry.tick_all(&clock.frame(Duration::ZERO));
    for millis in (10..=5000).step_by(10) {
        registry.tick_all(&clock.frame(Duration::from_millis(millis)));
        if geometry_run.is_complete() {
            assert!(
                run.is_complete(),
                "nested geometry retains the field's threshold rather than the scalar threshold"
            );
            assert_eq!(owner.value().0.position, goal);
            assert_eq!(owner.value().1, 0.005);
            assert_eq!(owner.velocity(), [0.0; 7]);
            return;
        }
    }
    panic!("nested geometry must settle in the observed interval");
}

fn named_struct_round_trips_through_vector() {
    let t = Translation {
        x: 1.0,
        y: 2.0,
        z: 3.0,
    };
    assert_eq!(t.to_vector(), [1.0, 2.0, 3.0]);

    let back = Translation::from_vector([4.0, 5.0, 6.0]);
    assert_eq!((back.x, back.y, back.z), (4.0, 5.0, 6.0));
}

fn derived_lerp_is_componentwise() {
    let a = Translation {
        x: 0.0,
        y: 10.0,
        z: -4.0,
    };
    let b = Translation {
        x: 2.0,
        y: 20.0,
        z: 4.0,
    };
    let mid = a.lerp_to(&b, 0.5);
    assert_eq!((mid.x, mid.y, mid.z), (1.0, 15.0, 0.0));
    let past = Pair(0.0, 1.0).lerp_to(&Pair(1.0, 3.0), 1.5);
    assert_eq!((past.0, past.1), (1.5, 4.0), "extrapolates past 1");
}

fn derived_type_is_a_keyframe_value() {
    let ms = std::time::Duration::from_millis;
    let track = Keyframes::builder(Pair(0.0, 0.0), ms(100))
        .to(Pair(10.0, -10.0), ms(100), Linear)
        .build()
        .expect("fits");
    let half = track.value_at(ms(50));
    assert_eq!((half.0, half.1), (5.0, -5.0));
}

#[test]
fn two_way_converter_derive_contract() {
    crate::run_table(&[
        (
            "nested spring rest units",
            nested_springs_keep_each_fields_rest_units,
        ),
        ("finite extreme lerp", derived_extreme_lerp),
        ("vector round trip", named_struct_round_trips_through_vector),
        ("componentwise lerp", derived_lerp_is_componentwise),
        ("keyframe value", derived_type_is_a_keyframe_value),
        (
            "nested interpolation",
            nested_fields_keep_their_interpolation_contracts,
        ),
        (
            "nested registered motion",
            nested_values_retarget_on_one_registered_motion,
        ),
    ]);
}

fn derived_extreme_lerp() {
    use flui_foundation::geometry::MaybeLerp;
    let a = Translation {
        x: 1e308,
        y: -0.0,
        z: 2.0,
    };
    let b = Translation {
        x: -1e308,
        y: 0.0,
        z: 4.0,
    };
    let p = Pair(1e308, -0.0);
    let q = Pair(-1e308, 0.0);
    for (t, expected) in [(0.0, 1e308), (0.25, 5e307), (0.5, 0.0), (1.0, -1e308)] {
        assert_eq!(a.lerp_to(&b, t).x, expected);
        assert_eq!(p.lerp_to(&q, t).0, expected);
        assert_eq!(Pair::maybe_lerp(&p, &q, t).expect("compatible").0, expected);
    }
    assert_eq!(a.lerp_to(&b, 0.0).y.to_bits(), (-0.0_f64).to_bits());
    assert!(p.lerp_to(&q, f64::NAN).0.is_nan());
}
