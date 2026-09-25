//! Property-based tests for `Color` invariants: hex case/prefix
//! insensitivity, the `to_hex`/`from_hex` roundtrip, and `lighten`/`darken`
//! direction-of-change, checked for arbitrary inputs rather than pinned
//! examples.

use flui_types::styling::Color;
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

proptest! {
    #[test]
    fn prop_argb_roundtrips(c in arb_color()) {
        prop_assert_eq!(Color::from_argb(c.to_argb()), c);
    }

    /// The f32 views agree with each other and scale by 255: converting back
    /// with `from_rgba_f32_array` is lossless.
    #[test]
    fn prop_f32_channel_views(c in arb_color()) {
        let array = c.to_rgba_f32_array();
        prop_assert_eq!(Color::from_rgba_f32_array(array), c);
        prop_assert_eq!(c.to_f32_array(), array);
        prop_assert_eq!(c.to_rgba_f32(), (array[0], array[1], array[2], array[3]));
        prop_assert_eq!(
            [c.red_f32(), c.green_f32(), c.blue_f32(), c.alpha_f32()],
            array
        );
        prop_assert_eq!(c.opacity(), array[3]);
    }

    #[test]
    fn prop_multiply_is_commutative_with_white_identity(a in arb_color(), b in arb_color()) {
        prop_assert_eq!(a.multiply(b), b.multiply(a));
        prop_assert_eq!(a.multiply(Color::rgba(255, 255, 255, 255)), a);
        prop_assert_eq!(a.multiply(Color::TRANSPARENT), Color::TRANSPARENT);
    }

    #[test]
    fn prop_is_light_is_not_dark(c in arb_color()) {
        prop_assert_eq!(c.is_light(), !c.is_dark());
        let text = if c.is_dark() { Color::WHITE } else { Color::BLACK };
        prop_assert_eq!(c.contrasting_text_color(), text);
    }

    #[test]
    fn prop_blend_over_batch_blends_each(colors in prop::collection::vec(arb_color(), 0..8), bg in arb_color()) {
        let expected: Vec<Color> = colors.iter().map(|c| c.blend_over(bg)).collect();
        prop_assert_eq!(Color::blend_over_batch(&colors, bg), expected);
    }
}

#[test]
fn argb_layout() {
    assert_eq!(Color::rgba(0x12, 0x34, 0x56, 0x78).to_argb(), 0x7812_3456);
}

#[test]
fn f32_array_is_clamped() {
    assert_eq!(
        Color::from_rgba_f32_array([2.0, -1.0, 0.5, 1.0]),
        Color::rgba(255, 0, 127, 255)
    );
}

#[test]
fn transparency_and_opacity_checks() {
    for (a, transparent, opaque) in [
        (0, true, false),
        (1, false, false),
        (254, false, false),
        (255, false, true),
    ] {
        let c = Color::rgba(9, 9, 9, a);
        assert_eq!(
            (c.is_transparent(), c.is_opaque()),
            (transparent, opaque),
            "a = {a}"
        );
    }
}

#[test]
fn multiply_truncates_the_channel_product() {
    // 200 * 128 / 255 = 100.39; 255 * 128 / 255 = 128.
    assert_eq!(
        Color::rgba(200, 100, 50, 255).multiply(Color::rgba(128, 255, 0, 128)),
        Color::rgba(100, 100, 0, 128)
    );
}

/// Rec. 709 weights; mid gray sits on either side of the 0.5 dark/light
/// threshold at 127 and 128.
#[test]
fn luminance_weights_and_threshold() {
    let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
    assert!(close(Color::RED.luminance(), 0.2126));
    assert!(close(Color::GREEN.luminance(), 0.7152));
    assert!(close(Color::BLUE.luminance(), 0.0722));
    assert!(close(Color::WHITE.luminance(), 1.0));
    assert!(close(Color::BLACK.luminance(), 0.0));
    assert!(Color::rgb(127, 127, 127).is_dark());
    assert!(Color::rgb(128, 128, 128).is_light());
}

#[test]
fn channel_setters_and_conversions() {
    let c = Color::rgba(1, 2, 3, 4);
    assert_eq!(c.with_alpha(9), Color::rgba(1, 2, 3, 9));
    assert_eq!(c.with_red(9), Color::rgba(9, 2, 3, 4));
    assert_eq!(c.with_green(9), Color::rgba(1, 9, 3, 4));
    assert_eq!(c.with_blue(9), Color::rgba(1, 2, 9, 4));
    assert_eq!(Color::from((1, 2, 3)), Color::rgb(1, 2, 3));
    assert_eq!(Color::from((1, 2, 3, 4)), c);
    assert_eq!(Color::from([1, 2, 3]), Color::rgb(1, 2, 3));
    assert_eq!(Color::from([1, 2, 3, 4]), c);
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

/// Alpha interpolates linearly alongside the Oklab color.
#[test]
fn lerp_oklab_interpolates_alpha() {
    let from = Color::rgba(0, 0, 0, 100);
    let to = Color::rgba(0, 0, 0, 200);
    assert_eq!(Color::lerp_oklab(from, to, 0.5).a, 150);
}

/// The midpoint is the same from either end; red and green differ on all
/// three Oklab axes, so this reaches the `a` and `b` terms that a gray
/// lerp leaves at zero.
#[test]
fn lerp_oklab_midpoint_is_symmetric() {
    for (x, y) in [
        (Color::RED, Color::GREEN),
        (Color::BLUE, Color::rgb(255, 200, 0)),
    ] {
        assert_eq!(
            Color::lerp_oklab(x, y, 0.5),
            Color::lerp_oklab(y, x, 0.5),
            "{x:?} {y:?}"
        );
        assert_ne!(Color::lerp_oklab(x, y, 0.25), Color::lerp_oklab(y, x, 0.25));
    }
}

/// The largest per-channel difference between two colors, in 8-bit units.
fn max_channel_distance(a: Color, b: Color) -> u8 {
    let (x, y) = (a.to_argb().to_be_bytes(), b.to_argb().to_be_bytes());
    x.iter()
        .zip(y)
        .map(|(p, q)| p.abs_diff(q))
        .max()
        .unwrap_or(0)
}

proptest! {
    /// The default epsilon is one 8-bit unit: colors whose every channel,
    /// alpha included, is within one unit compare equal, and no others.
    #[test]
    fn prop_approx_eq_tolerates_one_unit_per_channel(
        a in arb_color(),
        channel in 0usize..4,
        delta in -3i16..=3,
    ) {
        use flui_types::geometry::ApproxEq;
        let mut bytes = a.to_argb().to_be_bytes();
        bytes[channel] = (i16::from(bytes[channel]) + delta).clamp(0, 255) as u8;
        let b = Color::from_argb(u32::from_be_bytes(bytes));
        prop_assert_eq!(a.approx_eq(&b), max_channel_distance(a, b) <= 1, "{:?} {:?}", a, b);
        prop_assert_eq!(a.approx_eq_eps(&b, 0.0), a == b);
        prop_assert!(a.approx_eq_eps(&b, 3.0 / 255.0 + 1e-6));
    }
}

/// Every adjacent pair of channel values, in every channel: the pairs a
/// normalized-float subtraction gets wrong are scattered (3 and 4 is one).
#[test]
fn approx_eq_accepts_every_one_unit_step() {
    use flui_types::geometry::ApproxEq;
    for channel in 0..4 {
        for n in 0..255u8 {
            let at = |v: u8| {
                let mut bytes = [255, 0, 0, 0];
                bytes[channel] = v;
                Color::from_argb(u32::from_be_bytes(bytes))
            };
            assert!(
                at(n).approx_eq(&at(n + 1)),
                "channel {channel}: {n} vs {}",
                n + 1
            );
            assert!(
                !at(n).approx_eq_eps(&at(n + 1), 0.0),
                "channel {channel}: {n}"
            );
        }
    }
}
