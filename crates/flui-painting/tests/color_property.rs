//! `Color` contracts: the `to_hex`/`from_hex` roundtrip and the Porter-Duff
//! mirror algebra of `blend` checked for arbitrary colors, `blend_over`
//! against hand-computed values, multi-stop lerp, channel rounding and the
//! premultiplied lerp.

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
    let grey = Color::lerp(Color::rgb(0, 0, 0), Color::rgb(255, 255, 255), 0.5);
    assert_eq!(grey, Color::rgb(128, 128, 128));
}

/// The lerp is premultiplied: fading to transparent black keeps the hue
/// instead of darkening on the way.
pub(crate) fn lerp_to_transparent_keeps_the_hue() {
    let half = Color::lerp(Color::rgb(255, 0, 0), Color::TRANSPARENT, 0.5);
    assert_eq!(half, Color::rgba(255, 0, 0, 128));
}
