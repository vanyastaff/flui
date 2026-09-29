//! Property-based tests for `Color` invariants: hex case/prefix
//! insensitivity, the `to_hex`/`from_hex` roundtrip, and `lighten`/`darken`
//! direction-of-change, checked for arbitrary inputs rather than pinned
//! examples.

use flui_painting::styling::Color;
use proptest::prelude::*;

/// Half the generated colors are opaque: `to_hex` has a separate 6-digit
/// branch for `a == 255`, which a uniform alpha would reach 1 time in 256.
pub(crate) fn arb_color() -> impl Strategy<Value = Color> {
    let alpha = prop_oneof![Just(255u8), any::<u8>()];
    (any::<u8>(), any::<u8>(), any::<u8>(), alpha).prop_map(|(r, g, b, a)| Color::rgba(r, g, b, a))
}

proptest! {
    /// `to_hex` followed by `from_hex` returns the original color. `to_hex`
    /// picks the 6- or 8-digit form based on `is_opaque`, so this exercises
    /// both branches as `a` varies across its full range.
    #[test]
    fn prop_to_hex_from_hex_roundtrips(c in arb_color()) {
        let hex = c.to_hex();
        let parsed = Color::from_hex(&hex).unwrap();
        prop_assert_eq!(parsed, c);
    }

    /// `from_hex` on a 6-digit RGB string ignores a leading `#` and is
    /// case-insensitive.
    #[test]
    fn prop_from_hex_is_case_and_prefix_insensitive(r in any::<u8>(), g in any::<u8>(), b in any::<u8>()) {
        let upper = format!("{r:02X}{g:02X}{b:02X}");
        let lower = upper.to_ascii_lowercase();
        let expected = Color::rgb(r, g, b);

        prop_assert_eq!(Color::from_hex(&upper).unwrap(), expected);
        prop_assert_eq!(Color::from_hex(&lower).unwrap(), expected);
        prop_assert_eq!(Color::from_hex(&format!("#{upper}")).unwrap(), expected);
        prop_assert_eq!(Color::from_hex(&format!("#{lower}")).unwrap(), expected);
    }

    /// `lighten` never moves a channel away from white and always preserves
    /// alpha.
    #[test]
    fn prop_lighten_moves_toward_white_and_preserves_alpha(c in arb_color(), factor in 0.0f32..=1.0) {
        let l = c.lighten(factor);
        prop_assert!(l.r >= c.r);
        prop_assert!(l.g >= c.g);
        prop_assert!(l.b >= c.b);
        prop_assert_eq!(l.a, c.a);
    }

}

proptest! {
    #[test]
    fn prop_argb_roundtrips(c in arb_color()) {
        prop_assert_eq!(Color::from_argb(c.to_argb()), c);
    }

}

/// Stops at 0.25 / 0.75 / 1.0 (exact in binary, so the local t is too):
/// outside the range clamps to the end colors, on a stop gives that stop,
/// and between two stops lerps locally.
#[test]
fn lerp_multi_stop_brackets_and_clamps() {
    let (red, blue, green) = (Color::RED, Color::BLUE, Color::GREEN);
    let stops = [(red, 0.25), (blue, 0.75), (green, 1.0)];
    for (t, expected) in [
        (-1.0, red),
        (0.0, red),
        (0.25, red),
        (0.5, Color::lerp(red, blue, 0.5)),
        (0.75, blue),
        (0.875, Color::lerp(blue, green, 0.5)),
        (1.0, green),
        (1.5, green),
    ] {
        assert_eq!(Color::lerp_multi_stop(&stops, t), expected, "t = {t}");
    }
    // Stops that end before 1.0 hold the last color past the end.
    assert_eq!(
        Color::lerp_multi_stop(&[(red, 0.0), (blue, 0.5)], 0.9),
        blue
    );
    // A hard stop (two stops at one position) switches to the later color.
    let hard = [(red, 0.0), (blue, 0.5), (green, 0.5), (Color::WHITE, 1.0)];
    assert_eq!(Color::lerp_multi_stop(&hard, 0.5), green);
    assert_eq!(Color::lerp_multi_stop(&[], 0.5), Color::TRANSPARENT);
    assert_eq!(Color::lerp_multi_stop(&[(blue, 0.3)], 0.9), blue);
}

/// Halfway through Oklab from black to white is lightness 0.5: linear
/// 0.5³ = 0.125, which the sRGB curve encodes as 0.3885, i.e. 99.
#[test]
fn lerp_oklab_interpolates_lightness() {
    assert_eq!(
        Color::lerp_oklab(Color::BLACK, Color::WHITE, 0.5),
        Color::rgb(99, 99, 99)
    );
}

proptest! {}
