//! `#[derive(Animatable)]` as a downstream crate uses it.
//!
//! This lives in `tests/` (a separate crate that depends on flui-animation) so
//! the derive's `::flui_animation::TwoWayConverter` path resolves the same way
//! it does for a real downstream user.

// The derive copies fields verbatim and the asserted values are exactly
// representable in f64, so exact-equality round-trip assertions are correct.

use flui_animation::{Animatable, TwoWayConverter};

#[derive(Clone, Animatable)]
struct Translation {
    x: f64,
    y: f64,
    z: f64,
}

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
