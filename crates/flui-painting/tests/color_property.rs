//! `Color` contracts: the `to_hex`/`from_hex` roundtrip and the Porter-Duff
//! mirror algebra of `blend` checked for arbitrary colors, `blend_over`
//! against hand-computed values, multi-stop lerp, channel rounding and the
//! premultiplied lerp.

use flui_foundation::geometry::Lerp;
use flui_painting::paint::{BlendMode, BlendMode::*};
use flui_painting::styling::Color;
use proptest::prelude::*;

pub(crate) fn normalized_admission_and_hashing_are_canonical() {
    use flui_foundation::geometry::ApproxEq;
    for channel in 0..255 {
        assert!(
            Color::rgb(channel, 0, 0).approx_eq(&Color::rgb(channel + 1, 0, 0)),
            "adjacent byte channels {channel} and {}",
            channel + 1
        );
    }
    for (input, expected) in [
        (f32::NAN, 0.0),
        (f32::NEG_INFINITY, 0.0),
        (f32::INFINITY, 1.0),
        (-1.0, 0.0),
        (2.0, 1.0),
        (-0.0, 0.0),
    ] {
        let color = Color::from_rgba_f32_array([input; 4]);
        assert_eq!(color.to_rgba_f32_array(), [expected; 4]);
        assert!(
            color
                .to_rgba_f32_array()
                .into_iter()
                .all(|v| v.is_finite() && !v.is_sign_negative())
        );
    }
    let mut colors = std::collections::HashSet::new();
    colors.insert(Color::from_rgba_f32_array([-0.0; 4]));
    assert!(colors.contains(&Color::TRANSPARENT));
    assert!(!colors.insert(Color::TRANSPARENT));
    let first = Color::WHITE.with_opacity(0.0025);
    let second = Color::WHITE.with_opacity(0.005);
    assert_eq!(first.to_rgba8(), second.to_rgba8());
    assert!(colors.insert(first));
    assert!(colors.insert(second));
    for (input, expected) in [
        (0.0, 0),
        (0.001_960_784_2, 0),
        (0.001_960_784_4, 1),
        (0.5, 128),
        (1.0, 255),
    ] {
        assert_eq!(
            Color::from_rgba_f32_array([input; 4]).to_rgba8(),
            [expected; 4]
        );
    }
}

#[cfg(feature = "serde")]
pub(crate) fn color_wire_validates_normalized_units_before_narrowing() {
    let color = Color::from_rgba_f32_array([0.125, 0.25, 0.5, 0.0025]);
    let encoded = serde_json::to_value(color).expect("finite Color serializes");
    assert_eq!(
        serde_json::from_value::<Color>(encoded.clone()).expect("Color roundtrip"),
        color
    );
    assert!(encoded.get("srgb").is_some() && encoded.get("alpha").is_some());
    for malformed in [
        r#"{"r":255,"g":0,"b":0,"a":255}"#,
        r#"{"srgb":[1.000000000001,0,0],"alpha":1}"#,
        r#"{"srgb":[0,0,0],"alpha":-1e-100}"#,
        r#"{"srgb":[0,0,0],"alpha":1e100}"#,
        r#"{"srgb":[0,0,0],"alpha":0.5,"extra":true}"#,
    ] {
        assert!(
            serde_json::from_str::<Color>(malformed).is_err(),
            "{malformed}"
        );
    }
    assert_eq!(
        serde_json::from_str::<Color>(r#"{"srgb":[-0.0,0,0],"alpha":-0.0}"#)
            .expect("canonical zero"),
        Color::TRANSPARENT
    );
}

/// Authoring, modulation, blending and Oklab publication retain fractional alpha.
pub(crate) fn fractional_channels_survive_color_operations() {
    let subnormal = Color::from_rgba_f32_array([0.5, 0.25, 0.75, f32::from_bits(1)]);
    assert_eq!(subnormal.blend(Color::BLACK, BlendMode::Src), subnormal);
    assert_eq!(Color::BLACK.blend(subnormal, BlendMode::Dst), subnormal);
    assert_eq!(subnormal.blend_over(Color::TRANSPARENT), subnormal);
    for alpha in [0.002_f32, 0.0025, 0.123_456, 1e-40] {
        let color = Color::from_rgba_f32_array([1.0, 0.0, 0.0, alpha]);
        assert_eq!(color.to_rgba_f32_array(), [1.0, 0.0, 0.0, alpha]);
        assert_eq!(Color::RED.with_opacity(f64::from(alpha)).opacity(), alpha);
        assert_eq!(color.multiply(Color::WHITE), color);
        let restored = Color::from_premultiplied_oklab(color.to_premultiplied_oklab());
        assert_eq!(restored.opacity(), alpha);
        // Subnormal component multiplication has reduced precision, but hue
        // must survive without reciprocal overflow or byte quantization.
        let [r, g, b, _] = restored.to_rgba_f32_array();
        assert!(r > 0.999 && g < 0.001 && b < 0.001, "{alpha}: {restored:?}");
        let [r, _, _, a] = color.blend_over(Color::BLACK).to_rgba_f32_array();
        assert_eq!(a, 1.0);
        assert_eq!(r, alpha);
    }
    for t in [0.002, 0.0025] {
        let sample = Color::lerp(Color::WHITE.with_opacity(0.0), Color::WHITE, t);
        assert!((f64::from(sample.opacity()) - t).abs() < 1e-9);
    }
}

/// Half the generated colors are opaque: `to_hex` has a separate 6-digit
/// branch for `a == 255`, which a uniform alpha would reach 1 time in 256.
pub(crate) fn arb_color() -> impl Strategy<Value = Color> {
    let alpha = prop_oneof![Just(255u8), any::<u8>()];
    (any::<u8>(), any::<u8>(), any::<u8>(), alpha).prop_map(|(r, g, b, a)| Color::rgba(r, g, b, a))
}

fn arb_motion_color() -> impl Strategy<Value = Color> {
    prop_oneof![
        arb_color(),
        (
            0.0_f32..=1.0,
            0.0_f32..=1.0,
            0.0_f32..=1.0,
            0.0001_f32..=1.0
        )
            .prop_map(|(r, g, b, a)| Color::from_rgba_f32_array([r, g, b, a])),
    ]
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
    const PACKED: u32 = Color::rgba(255, 0, 0, 128).to_argb();
    assert_eq!(PACKED, 0x80FF_0000);
    let red = Color::rgb(255, 0, 0);
    // 127.5 and 30.6.
    assert_eq!(red.with_opacity(0.5).to_rgba8()[3], 128);
    assert_eq!(red.with_opacity(0.12).to_rgba8()[3], 31);
}

/// Oklab's lightness of an achromatic colour is the cube root of its relative
/// luminance Y, so the black-to-white midpoint L = 0.5 has Y = 0.125, which the
/// sRGB transfer function encodes as 0.3886 — channel 99 (a gamma-sRGB lerp
/// gives 128).
pub(crate) fn black_to_white_midpoint_is_oklab_mid_grey() {
    let grey = Color::rgb(99, 99, 99);
    let expected = 1.055 * 0.125_f64.powf(1.0 / 2.4) - 0.055;
    for sample in [
        Color::lerp(Color::BLACK, Color::WHITE, 0.5),
        Color::BLACK.lerp_to(&Color::WHITE, 0.5),
    ] {
        assert_eq!(sample.to_rgba8(), grey.to_rgba8());
        for channel in sample.to_rgba_f32_array().into_iter().take(3) {
            assert!((f64::from(channel) - expected).abs() < 2e-6);
        }
    }
}

/// The lerp is premultiplied: fading to transparent black keeps the hue
/// instead of darkening on the way (Oklab with straight alpha mixes in black's
/// lightness and lands on a dark red).
pub(crate) fn lerp_to_transparent_keeps_the_hue() {
    let half = Color::lerp(Color::rgb(255, 0, 0), Color::TRANSPARENT, 0.5);
    assert_eq!(half.to_rgba8(), [255, 0, 0, 128]);
    assert_eq!(half.opacity(), 0.5);
    let tweened = Color::rgb(255, 0, 0).lerp_to(&Color::TRANSPARENT, 0.5);
    assert_eq!(tweened.to_rgba8(), [255, 0, 0, 128]);
    assert_eq!(tweened.opacity(), 0.5);
}

/// The sRGB decoding of one normalized channel, in `f64` (IEC 61966-2-1).
fn decode(channel: f32) -> f64 {
    let c = f64::from(channel);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn contrast_selection_uses_linear_relative_luminance() {
    assert_eq!(
        Color::rgb(127, 127, 127).contrasting_text_color(),
        Color::BLACK
    );
    let backgrounds = (0..=255)
        .map(|channel| Color::rgb(channel, channel, channel))
        .chain([
            Color::RED,
            Color::GREEN,
            Color::BLUE,
            Color::rgb(255, 0, 255),
            Color::from_rgba_f32_array([0.37, 0.52, 0.81, 1.0]),
            Color::WHITE.with_opacity(0.25).blend_over(Color::BLACK),
            Color::BLACK.with_opacity(0.25).blend_over(Color::WHITE),
        ]);
    for background in backgrounds {
        let [red, green, blue, _] = background.to_rgba_f32_array().map(decode);
        let luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
        let white_contrast = 1.05 / (luminance + 0.05);
        let black_contrast = (luminance + 0.05) / 0.05;
        let expected = if white_contrast > black_contrast {
            Color::WHITE
        } else {
            Color::BLACK
        };
        assert_eq!(
            background.contrasting_text_color(),
            expected,
            "{background:?}"
        );
        assert_eq!(background.is_dark(), expected == Color::WHITE);
        assert_eq!(background.is_light(), expected == Color::BLACK);
        assert!((f64::from(background.luminance()) - luminance).abs() < 1e-6);
        assert!(white_contrast.max(black_contrast) >= 4.5);
    }
}

/// The normalized sRGB encoding of linear light, clamped to the gamut.
fn encode(linear: f64) -> f64 {
    let c = linear.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Ottosson's Oklab, in `f64`, from <https://bottosson.github.io/posts/oklab/>.
#[expect(
    clippy::many_single_char_names,
    reason = "r, g, b and l, m, s are the colour-science names"
)]
fn oklab(color: Color) -> [f64; 3] {
    let [r, g, b, _] = color.to_rgba_f32_array().map(decode);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// Oklab back to normalized sRGB channels.
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
/// transfer function's own `powf` and `cbrt`, for both byte-authored and fractional
/// channels. The normalized error tolerance is far below one byte step. Colour
/// channels are compared only where the result is visible (alpha above zero).
pub(crate) fn lerp_matches_the_exact_oklab_evaluation() {
    proptest!(|(a in arb_motion_color(), b in arb_motion_color(), t in 0.0f64..=1.0)| {
        let got = Color::lerp(a, b, t);
        let (alpha_a, alpha_b) = (f64::from(a.opacity()), f64::from(b.opacity()));
        let alpha = alpha_a + (alpha_b - alpha_a) * t;
        prop_assert!((f64::from(got.opacity()) - alpha).abs() <= 2e-7, "alpha {:?}", got);
        if got.is_transparent() || alpha <= 0.0 {
            return Ok(());
        }
        let (la, lb) = (oklab(a), oklab(b));
        let mixed: [f64; 3] = std::array::from_fn(|i| {
            (la[i] * alpha_a + (lb[i] * alpha_b - la[i] * alpha_a) * t) / alpha
        });
        let want = srgb(mixed);
        for (channel, (got, want)) in got.to_rgba_f32_array().into_iter().zip(want).enumerate() {
            prop_assert!(
                (f64::from(got) - want).abs() <= 2e-5,
                "channel {} is {}, exact {}: {:?} -> {:?} at t = {}", channel, got, want, a, b, t
            );
        }
    });
}

/// A fade to transparent keeps the opaque end's colour however little alpha is left:
/// the result is finite, its alpha is the straight mix, and its channels stay on the
/// source colour; once the alpha vanishes it is fully transparent.
pub(crate) fn lerp_to_transparent_keeps_the_hue_at_vanishing_alpha() {
    let faint = Color::from_rgba_f32_array([0.145_098_05, 0.074_509_81, 0.701_960_8, 1.0 / 255.0]);
    for source in [faint, Color::from_rgba_f32_array([0.9, 0.2, 0.4, 0.0001])] {
        let [r, g, b, alpha] = source.to_rgba_f32_array();
        for t in [0.5, 0.9, 0.998_631_274_725_139_5, 0.999_999, 1.0 - 1e-9] {
            let got = Color::lerp(source, Color::TRANSPARENT, t);
            let channels = got.to_rgba_f32_array();
            assert!(channels.iter().all(|c| c.is_finite()), "{got:?} at {t}");
            let want = f64::from(alpha) * (1.0 - t);
            assert!(
                (f64::from(got.opacity()) - want).abs() <= 2e-7,
                "{got:?} at {t}"
            );
            if got.is_transparent() {
                continue;
            }
            for (got, want) in channels.into_iter().zip([r, g, b]) {
                assert!((got - want).abs() <= 2e-5, "{got:?} vs {want} at {t}");
            }
        }
    }
}

/// `t = 0` and `t = 1` return the endpoints exactly, through both the clamping
/// `Color::lerp` and the extrapolating `Lerp` impl.
pub(crate) fn lerp_endpoints_are_exact() {
    proptest!(|(a in arb_motion_color(), b in arb_motion_color())| {
        prop_assert_eq!(Color::lerp(a, b, 0.0), a);
        prop_assert_eq!(Color::lerp(a, b, 1.0), b);
        prop_assert_eq!(a.lerp_to(&b, 0.0), a);
        prop_assert_eq!(a.lerp_to(&b, 1.0), b);
    });
}

/// Overshoot extrapolates and saturates the channels; a NaN `t` keeps `begin`.
pub(crate) fn lerp_outside_the_segment_saturates_and_nan_keeps_begin() {
    let (opaque, clear) = (Color::rgba(255, 0, 0, 255), Color::rgba(0, 0, 255, 0));
    for color in [
        opaque,
        clear,
        Color::from_rgba_f32_array([0.25, 0.5, 0.75, 0.125]),
    ] {
        for t in [f64::MAX, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(color.lerp_to(&color, t), color, "unchanged endpoint at {t}");
        }
    }
    assert_eq!(opaque.lerp_to(&clear, 1.5).opacity(), 0.0);
    assert_eq!(opaque.lerp_to(&clear, -0.5).opacity(), 1.0);
    assert_eq!(Color::lerp(opaque, clear, 1.5), clear);
    assert_eq!(Color::lerp(opaque, clear, -0.5), opaque);
    let (black, white) = (Color::BLACK, Color::WHITE);
    // Fractional endpoints can be much closer than adjacent authored bytes.
    // Bounding the parameter itself loses their extrapolated motion.
    for channel in [1e-20_f32, 1e-30] {
        let near_black = Color::from_rgba_f32_array([channel, channel, channel, 1.0]);
        for t in [1e20, 1e30, f64::MAX] {
            assert_eq!(
                black.lerp_to(&near_black, t),
                white,
                "fractional endpoint {channel} at t={t}"
            );
        }
    }
    for (begin, end, t) in [
        (
            black,
            Color::from_rgba_f32_array([1e-24, 1e-24, 1e-24, 1.0]),
            1e7,
        ),
        (
            black.with_opacity(0.25),
            Color::from_rgba_f32_array([1e-24, 1e-24, 1e-24, 0.250_000_03]),
            2e6,
        ),
        (white, black, f64::INFINITY),
        (black, white, f64::NEG_INFINITY),
    ] {
        let actual = begin.lerp_to(&end, t);
        if t.is_finite() {
            let (alpha_a, alpha_b) = (f64::from(begin.opacity()), f64::from(end.opacity()));
            let alpha = alpha_a + (alpha_b - alpha_a) * t;
            let (la, lb) = (oklab(begin), oklab(end));
            let mixed = std::array::from_fn(|i| {
                (la[i] * alpha_a + (lb[i] * alpha_b - la[i] * alpha_a) * t) / alpha
            });
            let expected = srgb(mixed);
            for (actual, expected) in actual.to_rgba_f32_array().into_iter().zip(expected) {
                assert!((f64::from(actual) - expected).abs() < 2e-5);
            }
            assert!((f64::from(actual.opacity()) - alpha.clamp(0.0, 1.0)).abs() < 2e-7);
        } else {
            assert_eq!(actual, black);
        }
    }
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
        assert_eq!(
            opaque.lerp_to(&clear, t).opacity(),
            0.0,
            "opaque to clear at t = {t}"
        );
    }
    assert_eq!(Color::lerp(opaque, clear, f64::NAN), opaque);
    assert_eq!(opaque.lerp_to(&clear, f64::NAN), opaque);
}
