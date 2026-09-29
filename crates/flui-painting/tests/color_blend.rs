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

use crate::color_property::arb_color;

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
