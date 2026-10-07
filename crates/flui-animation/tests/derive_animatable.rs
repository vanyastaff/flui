//! `#[derive(Animatable)]` as a downstream crate uses it.
//!
//! This lives in `tests/` (a separate crate that depends on flui-animation) so
//! the derive's `::flui_animation::TwoWayConverter` path resolves the same way
//! it does for a real downstream user.

// The derive copies fields verbatim and the asserted values are exactly
// representable in f64, so exact-equality round-trip assertions are correct.

use flui_animation::{Keyframes, Lerp, Linear, TwoWayConverter};

#[derive(Clone, TwoWayConverter)]
struct Translation {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, TwoWayConverter)]
struct Pair(f64, f64);

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
        ("finite extreme lerp", derived_extreme_lerp),
        ("vector round trip", named_struct_round_trips_through_vector),
        ("componentwise lerp", derived_lerp_is_componentwise),
        ("keyframe value", derived_type_is_a_keyframe_value),
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
