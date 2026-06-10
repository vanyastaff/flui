//! Integration tests for `#[derive(Animatable)]`.
//!
//! These live in `tests/` (a separate crate that depends on flui-animation) so
//! the derive's `::flui_animation::TwoWayConverter` path resolves the same way
//! it does for a real downstream user.

// The derive copies fields verbatim and the asserted values are exactly
// representable in f32, so exact-equality round-trip assertions are correct.
#![allow(clippy::float_cmp)]

use flui_animation::{Animatable, AnimatedValue, SpringDescription, TwoWayConverter};

#[derive(Clone, Animatable)]
struct Translation {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Clone, Animatable)]
struct Pair(f32, f32);

#[test]
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

#[test]
fn tuple_struct_round_trips_through_vector() {
    let p = Pair(1.0, 2.0);
    assert_eq!(p.to_vector(), [1.0, 2.0]);

    let back = Pair::from_vector([3.0, 4.0]);
    assert_eq!((back.0, back.1), (3.0, 4.0));
}

#[test]
fn derived_type_is_spring_animatable() {
    let mut value = AnimatedValue::new(
        Translation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        SpringDescription::smooth(),
    );
    value.animate_to(Translation {
        x: 100.0,
        y: 50.0,
        z: 0.0,
    });
    for _ in 0..600 {
        value.advance(1.0 / 60.0);
    }
    let v = value.value();
    assert!((v.x - 100.0).abs() < 0.5, "x={}", v.x);
    assert!((v.y - 50.0).abs() < 0.5, "y={}", v.y);
    assert!(v.z.abs() < 0.5, "z={}", v.z);
}
