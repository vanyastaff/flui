//! Property-based tests for `Color` invariants: hex case/prefix
//! insensitivity, the `to_hex`/`from_hex` roundtrip, and `lighten`/`darken`
//! direction-of-change, checked for arbitrary inputs rather than pinned
//! examples.

use flui_types::styling::Color;
use proptest::prelude::*;

/// Half the generated colors are opaque: `to_hex` has a separate 6-digit
/// branch for `a == 255`, which a uniform alpha would reach 1 time in 256.
fn arb_color() -> impl Strategy<Value = Color> {
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

    /// `lighten` is monotonically non-decreasing per channel as `factor`
    /// grows.
    #[test]
    fn prop_lighten_is_monotonic_in_factor(c in arb_color(), f1 in 0.0f32..=1.0, f2 in 0.0f32..=1.0) {
        let (lo, hi) = if f1 <= f2 { (f1, f2) } else { (f2, f1) };
        let low = c.lighten(lo);
        let high = c.lighten(hi);
        prop_assert!(low.r <= high.r);
        prop_assert!(low.g <= high.g);
        prop_assert!(low.b <= high.b);
    }

    /// `darken` never moves a channel away from black and always preserves
    /// alpha.
    #[test]
    fn prop_darken_moves_toward_black_and_preserves_alpha(c in arb_color(), factor in 0.0f32..=1.0) {
        let d = c.darken(factor);
        prop_assert!(d.r <= c.r);
        prop_assert!(d.g <= c.g);
        prop_assert!(d.b <= c.b);
        prop_assert_eq!(d.a, c.a);
    }

    /// `factor` is how much of the original color survives darkening (0.0
    /// yields black, 1.0 leaves it unchanged), so `darken` is monotonically
    /// non-decreasing per channel as `factor` grows.
    #[test]
    fn prop_darken_is_monotonic_in_factor(c in arb_color(), f1 in 0.0f32..=1.0, f2 in 0.0f32..=1.0) {
        let (lo, hi) = if f1 <= f2 { (f1, f2) } else { (f2, f1) };
        let low = c.darken(lo);
        let high = c.darken(hi);
        prop_assert!(low.r <= high.r);
        prop_assert!(low.g <= high.g);
        prop_assert!(low.b <= high.b);
    }

    /// The endpoints of both blends: `lighten(0)` and `darken(1)` are the
    /// identity, `lighten(1)` is white and `darken(0)` is black, alpha kept.
    #[test]
    fn prop_lighten_and_darken_endpoints(c in arb_color()) {
        prop_assert_eq!(c.lighten(0.0), c);
        prop_assert_eq!(c.darken(1.0), c);
        prop_assert_eq!(c.lighten(1.0), Color::rgba(255, 255, 255, c.a));
        prop_assert_eq!(c.darken(0.0), Color::rgba(0, 0, 0, c.a));
    }

    /// `lerp` returns its first argument at `t <= 0` and its second at
    /// `t >= 1` (`t` is clamped), and a color lerped with itself is itself.
    #[test]
    fn prop_lerp_endpoints_and_identity(a in arb_color(), b in arb_color(), t in 0.0f32..=1.0, beyond in 0.0f32..=10.0) {
        prop_assert_eq!(Color::lerp(a, b, 0.0), a);
        prop_assert_eq!(Color::lerp(a, b, -beyond), a);
        prop_assert_eq!(Color::lerp(a, b, 1.0), b);
        prop_assert_eq!(Color::lerp(a, b, 1.0 + beyond), b);
        prop_assert_eq!(Color::lerp(a, a, t), a);
    }

    /// Every channel of `lerp(a, b, t)` lies between the same channel of
    /// `a` and `b`.
    #[test]
    fn prop_lerp_channels_stay_between_endpoints(a in arb_color(), b in arb_color(), t in 0.0f32..=1.0) {
        let m = Color::lerp(a, b, t);
        for (x, lo, hi) in [(m.r, a.r, b.r), (m.g, a.g, b.g), (m.b, a.b, b.b), (m.a, a.a, b.a)] {
            prop_assert!(lo.min(hi) <= x && x <= lo.max(hi));
        }
    }
}

/// `lerp` rounds each channel to nearest (half away from zero), matching
/// Flutter's `Color.lerp`; truncation would give `(0, 1, 2, 127)` here.
#[test]
fn lerp_rounds_to_nearest() {
    let from = Color::rgba(0, 0, 0, 0);
    let to = Color::rgba(1, 3, 5, 255);
    assert_eq!(Color::lerp(from, to, 0.5), Color::rgba(1, 2, 3, 128));
}

/// Directional and endpoint properties can't tell a correct blend from one
/// that jumps straight to the endpoint; one exact midpoint pins the formula.
/// `0.5` and these channels are exact in f32, so the truncation is exact too.
#[test]
fn lighten_and_darken_midpoint() {
    let c = Color::rgba(100, 50, 20, 7);
    assert_eq!(c.lighten(0.5), Color::rgba(177, 152, 137, 7));
    assert_eq!(c.darken(0.5), Color::rgba(50, 25, 10, 7));
}
