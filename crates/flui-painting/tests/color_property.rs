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

/// The sRGB decoding of one 8-bit channel, in `f64` (IEC 61966-2-1).
fn decode(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB encoding of linear light, clamped to the gamut, on the `0..=255` scale
/// before rounding.
fn encode(linear: f64) -> f64 {
    let c = linear.clamp(0.0, 1.0);
    let encoded = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    encoded * 255.0
}

/// Ottosson's Oklab, in `f64`, from <https://bottosson.github.io/posts/oklab/>.
#[expect(
    clippy::many_single_char_names,
    reason = "r, g, b and l, m, s are the colour-science names"
)]
fn oklab(color: Color) -> [f64; 3] {
    let (r, g, b) = (decode(color.r), decode(color.g), decode(color.b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// Oklab back to unrounded sRGB channels on the `0..=255` scale.
fn srgb(lab: [f64; 3]) -> [f64; 3] {
    let l = (lab[0] + 0.396_337_777_4 * lab[1] + 0.215_803_757_3 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_345_8 * lab[1] - 0.063_854_172_8 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_177_5 * lab[1] - 1.291_485_548_0 * lab[2]).powi(3);
    [
        encode(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        encode(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        encode(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s),
    ]
}

/// `Color::lerp` against an `f64` evaluation of premultiplied Oklab with the
/// transfer function's own `powf` and `cbrt`: every channel of the 8-bit result lies
/// within one step of the exact, unrounded value, that is, half a step of rounding
/// plus at most half a step (0.5/255) of approximation. Colour channels are compared
/// only where the result is visible (alpha above zero).
pub(crate) fn lerp_matches_the_exact_oklab_evaluation() {
    proptest!(|(a in arb_color(), b in arb_color(), t in 0.0f64..=1.0)| {
        let got = Color::lerp(a, b, t);
        let (alpha_a, alpha_b) = (f64::from(a.a) / 255.0, f64::from(b.a) / 255.0);
        let alpha = alpha_a + (alpha_b - alpha_a) * t;
        prop_assert!((f64::from(got.a) - alpha * 255.0).abs() <= 1.0, "alpha {:?}", got);
        if got.a == 0 || alpha <= 0.0 {
            return Ok(());
        }
        let (la, lb) = (oklab(a), oklab(b));
        let mixed: [f64; 3] = std::array::from_fn(|i| {
            (la[i] * alpha_a + (lb[i] * alpha_b - la[i] * alpha_a) * t) / alpha
        });
        let want = srgb(mixed);
        for (channel, (got, want)) in [got.r, got.g, got.b].into_iter().zip(want).enumerate() {
            prop_assert!(
                (f64::from(got) - want).abs() <= 1.0,
                "channel {} is {}, exact {}: {:?} -> {:?} at t = {}", channel, got, want, a, b, t
            );
        }
    });
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
    // Weights past f32's range still saturate toward the far end.
    for t in [1e30, f64::MAX] {
        assert_eq!(black.lerp_to(&white, t), white, "black to white at t = {t}");
        assert_eq!(white.lerp_to(&black, t), black, "white to black at t = {t}");
        assert_eq!(
            black.lerp_to(&white, -t),
            black,
            "black to white at t = -{t}"
        );
        assert_eq!(opaque.lerp_to(&clear, t).a, 0, "opaque to clear at t = {t}");
    }
    assert_eq!(Color::lerp(opaque, clear, f64::NAN), opaque);
    assert_eq!(opaque.lerp_to(&clear, f64::NAN), opaque);
}
