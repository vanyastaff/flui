//! `Color` contracts: the `to_hex`/`from_hex` roundtrip and the Porter-Duff
//! mirror algebra of `blend` checked for arbitrary colors, `blend_over`
//! against hand-computed values, multi-stop lerp, channel rounding and the
//! premultiplied lerp.

use flui_foundation::geometry::Lerp;
use flui_painting::paint::{BlendMode, BlendMode::*};
use flui_painting::styling::Color;
use proptest::prelude::*;

/// Half the generated colors are opaque: `to_hex` has a separate 6-digit
/// branch for `a == 255`, which a uniform alpha would reach 1 time in 256.
pub(crate) fn arb_color() -> impl Strategy<Value = Color> {
    let alpha = prop_oneof![Just(255u8), any::<u8>()];
    (any::<u8>(), any::<u8>(), any::<u8>(), alpha).prop_map(|(r, g, b, a)| Color::rgba(r, g, b, a))
}

/// Stops at 0.25 / 0.75 / 1.0 (exact in binary, so the local t is too):
/// outside the range clamps to the end colors, on a stop gives that stop,
/// and between two stops lerps locally.
pub(crate) fn lerp_multi_stop_brackets_and_clamps() {
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

const MIRRORS: [(BlendMode, BlendMode); 7] = [
    (SrcOver, DstOver),
    (SrcIn, DstIn),
    (SrcOut, DstOut),
    (SrcATop, DstATop),
    (Xor, Xor),
    (Plus, Plus),
    (Clear, Clear),
];

/// `to_hex` then `from_hex` returns the original color (both the 6- and
/// 8-digit branches), and swapping source and destination turns each
/// Porter-Duff operator into its mirror (`Xor`, `Plus` and `Clear` are their
/// own).
pub(crate) fn hex_roundtrips_and_porter_duff_modes_mirror() {
    proptest!(|(s in arb_color(), d in arb_color())| {
        let parsed = Color::from_hex(&s.to_hex()).expect("to_hex output must parse");
        prop_assert_eq!(parsed, s);
        for (mode, mirror) in MIRRORS {
            prop_assert_eq!(s.blend(d, mode), d.blend(s, mirror), "{:?} / {:?}", mode, mirror);
        }
    });
}

/// A float channel rounds to the nearest of the 256 steps, as the GPU's
/// float-to-unorm8 conversion does; truncating would land up to one step low.
pub(crate) fn channels_round_to_the_nearest_step() {
    let red = Color::rgb(255, 0, 0);
    // 127.5 and 30.6.
    assert_eq!(red.with_opacity(0.5).a, 128);
    assert_eq!(red.with_opacity(0.12).a, 31);
}

/// Oklab's lightness of an achromatic colour is the cube root of its relative
/// luminance Y, so the black-to-white midpoint L = 0.5 has Y = 0.125, which the
/// sRGB transfer function encodes as 0.3886 — channel 99 (a gamma-sRGB lerp
/// gives 128).
pub(crate) fn black_to_white_midpoint_is_oklab_mid_grey() {
    let grey = Color::rgb(99, 99, 99);
    assert_eq!(Color::lerp(Color::BLACK, Color::WHITE, 0.5), grey);
    assert_eq!(Color::BLACK.lerp_to(&Color::WHITE, 0.5), grey);
}

/// The lerp is premultiplied: fading to transparent black keeps the hue
/// instead of darkening on the way (Oklab with straight alpha mixes in black's
/// lightness and lands on a dark red).
pub(crate) fn lerp_to_transparent_keeps_the_hue() {
    let half = Color::lerp(Color::rgb(255, 0, 0), Color::TRANSPARENT, 0.5);
    assert_eq!(half, Color::rgba(255, 0, 0, 128));
    let tweened = Color::rgb(255, 0, 0).lerp_to(&Color::TRANSPARENT, 0.5);
    assert_eq!(tweened, Color::rgba(255, 0, 0, 128));
}

/// `t = 0` and `t = 1` return the endpoints exactly, through both the clamping
/// `Color::lerp` and the extrapolating `Lerp` impl.
pub(crate) fn lerp_endpoints_are_exact() {
    proptest!(|(a in arb_color(), b in arb_color())| {
        prop_assert_eq!(Color::lerp(a, b, 0.0), a);
        prop_assert_eq!(Color::lerp(a, b, 1.0), b);
        prop_assert_eq!(a.lerp_to(&b, 0.0), a);
        prop_assert_eq!(a.lerp_to(&b, 1.0), b);
    });
}

/// Overshoot extrapolates and saturates the channels; a NaN `t` keeps `begin`.
pub(crate) fn lerp_outside_the_segment_saturates_and_nan_keeps_begin() {
    let (opaque, clear) = (Color::rgba(255, 0, 0, 255), Color::rgba(0, 0, 255, 0));
    assert_eq!(opaque.lerp_to(&clear, 1.5).a, 0);
    assert_eq!(opaque.lerp_to(&clear, -0.5).a, 255);
    assert_eq!(Color::lerp(opaque, clear, 1.5), clear);
    assert_eq!(Color::lerp(opaque, clear, -0.5), opaque);
    let (black, white) = (Color::BLACK, Color::WHITE);
    assert_eq!(black.lerp_to(&white, 1.5), white);
    assert_eq!(white.lerp_to(&black, 1.5), black);
    assert_eq!(Color::lerp(opaque, clear, f64::NAN), opaque);
    assert_eq!(opaque.lerp_to(&clear, f64::NAN), opaque);
}
