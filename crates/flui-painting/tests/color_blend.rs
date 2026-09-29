//! `Color::blend` checked through the algebra of its modes rather than
//! per-mode sample pixels: identities, the source/destination swap that pairs
//! each Porter-Duff operator with its mirror, commutativity of the symmetric
//! separable modes, and the luminosity/chroma split that defines the
//! non-separable ones. Every equality below holds exactly in `f32` (the two
//! sides perform the same operations, only in swapped order), so none needs
//! a tolerance except the luminosity checks, which go through u8 rounding.

use flui_painting::paint::{BlendMode, BlendMode::*};
use flui_painting::styling::Color;
use proptest::prelude::*;
use rstest::rstest;

use crate::color_property::arb_color;

fn opaque() -> impl Strategy<Value = Color> {
    (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| Color::rgb(r, g, b))
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

proptest! {

    /// Swapping source and destination turns each Porter-Duff operator into
    /// its mirror (`Xor`, `Plus` and `Clear` are their own).
    #[test]
    fn porter_duff_mirrors(s in arb_color(), d in arb_color()) {
        for (mode, mirror) in MIRRORS {
            prop_assert_eq!(s.blend(d, mode), d.blend(s, mirror), "{:?} / {:?}", mode, mirror);
        }
    }

    /// With both sides opaque the coverage operators reduce to picking one
    /// side or nothing.
    #[test]
    fn porter_duff_between_opaque_colors(s in opaque(), d in opaque()) {
        for (mode, expected) in [
            (SrcIn, s), (SrcATop, s), (DstIn, d), (DstATop, d),
            (SrcOut, Color::TRANSPARENT), (DstOut, Color::TRANSPARENT), (Xor, Color::TRANSPARENT),
        ] {
            prop_assert_eq!(s.blend(d, mode), expected, "{:?}", mode);
        }
    }

    /// `Plus` is a saturating per-channel add.
    #[test]
    fn plus_saturates(s in opaque(), d in opaque()) {
        let add = |a: u8, b: u8| a.saturating_add(b);
        prop_assert_eq!(s.blend(d, Plus), Color::rgb(add(s.r, d.r), add(s.g, d.g), add(s.b, d.b)));
    }

}

/// One opaque gray pixel per separable mode at mid-range inputs, where the
/// branches and coefficients of the W3C formulas actually show (the 0/1
/// inputs of the golden test in `color.rs` can't tell most formulas apart).
/// `cs` is the source channel, `cb` the backdrop, as `n / 255`.
#[rstest]
#[case::multiply(Multiply, 51, 153, 31)] // .2 * .6 = .12
#[case::screen(Screen, 51, 153, 173)] // .2 + .6 - .12 = .68
#[case::exclusion(Exclusion, 51, 153, 143)] // .8 - 2 * .12 = .56
#[case::difference(Difference, 51, 153, 102)] // |.6 - .2|
#[case::darken(Darken, 51, 153, 51)]
#[case::lighten(Lighten, 51, 153, 153)]
#[case::hard_light_dark_source(HardLight, 51, 153, 61)] // 2 * .6 * .2 = .24
#[case::hard_light_light_source(HardLight, 204, 153, 214)] // 1 - 2 * .4 * .2 = .84
#[case::color_dodge(ColorDodge, 102, 51, 85)] // .2 / (1 - .4) = .333
#[case::color_dodge_saturates(ColorDodge, 204, 51, 255)] // .2 / .2 = 1
#[case::color_burn(ColorBurn, 153, 204, 170)] // 1 - (1 - .8) / .6 = .667
#[case::soft_light_dark_source(SoftLight, 51, 153, 116)] // .6 - .6 * .6 * .4 = .456
#[case::soft_light_light_source(SoftLight, 204, 153, 180)] // .6 + .6 * (sqrt .6 - .6) = .7048
#[case::soft_light_dark_backdrop(SoftLight, 204, 26, 56)] // .102 + .6 * (.3001 - .102) = .2208, where sqrt would give .2324
fn separable_mode_at_mid_range(
    #[case] mode: BlendMode,
    #[case] cs: u8,
    #[case] cb: u8,
    #[case] expected: u8,
) {
    let src = Color::rgb(cs, cs, cs);
    let dst = Color::rgb(cb, cb, cb);
    assert_eq!(
        src.blend(dst, mode),
        Color::rgb(expected, expected, expected)
    );
}

/// Flutter's `Color.alphaBlend`, computed by hand. Over an opaque
/// background the result alpha is exactly 1: `r = 255 · 128/255 = 128`,
/// `b = 255 · 127/255 = 127`. Over a translucent one the background keeps
/// `56/255 · 245/255` of its alpha: `10/255 + 0.2110 = 0.2502 → 63.8 → 64`.
#[test]
fn blend_over_matches_flutter_alpha_blend() {
    assert_eq!(
        Color::rgba(255, 0, 0, 128).blend_over(Color::rgb(0, 0, 255)),
        Color::rgba(128, 0, 127, 255)
    );
    assert_eq!(
        Color::rgba(3, 3, 3, 10).blend_over(Color::rgba(3, 3, 3, 56)),
        Color::rgba(3, 3, 3, 64)
    );
    // One step from each fast path: a nearly opaque source still lets the
    // background through, and only a fully opaque background makes the
    // result opaque.
    let white = |a| Color::rgba(255, 255, 255, a);
    let black = |a| Color::rgba(0, 0, 0, a);
    assert_eq!(black(254).blend_over(white(255)), Color::rgba(1, 1, 1, 255));
    assert_eq!(
        black(1).blend_over(white(255)),
        Color::rgba(254, 254, 254, 255)
    );
    assert_eq!(
        black(64).blend_over(white(254)),
        Color::rgba(191, 191, 191, 254)
    );
    assert_eq!(black(64).blend_over(white(1)), Color::rgba(3, 3, 3, 65));
    assert_eq!(black(64).blend_over(white(0)), black(64));
}

/// The non-separable modes at pixels where the chroma matters, derived
/// from the W3C `SetSat`/`SetLum`/`ClipColor` definitions. The properties
/// above only look at gray in/gray out and at luminosity, which a mode that
/// collapsed every result to gray would also satisfy.
///
/// - Hue, orange (1, .502, 0) over (.302, .4, .502): `SetSat` gives
///   (.2, .1004, 0), `SetLum` to .3818 gives (.4626, .3630, .2626).
/// - Saturation, same pair: `SetSat(backdrop, 1)` gives (0, .4902, 1),
///   `SetLum` dips below 0 and `ClipColor` pulls it back to
///   (0, .4688, .9564).
/// - Color, blue over light gray (.902): `SetLum` overshoots to 1.792 and
///   `ClipColor` scales it down to (.8898, .8898, 1).
#[test]
fn non_separable_modes_at_chromatic_pixels() {
    let orange = Color::rgb(255, 128, 0);
    let slate = Color::rgb(77, 102, 128);
    assert_eq!(orange.blend(slate, Hue), Color::rgb(118, 93, 67));
    // Green is the backdrop's smallest channel here, so its saturation
    // (.502 - .302 = .2) needs all three: SetSat gives (.2, .1004, 0),
    // SetLum to .3534 gives (.4341, .3345, .2341).
    assert_eq!(
        orange.blend(Color::rgb(102, 77, 128), Hue),
        Color::rgb(111, 85, 60)
    );
    assert_eq!(orange.blend(slate, Saturation), Color::rgb(0, 120, 244));
    assert_eq!(
        Color::BLUE.blend(Color::rgb(230, 230, 230), BlendMode::Color),
        Color::rgb(227, 227, 255)
    );
}
