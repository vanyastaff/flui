//! `Color::blend_over` against hand-computed expected values.

use flui_painting::styling::Color;

/// Source-over blend, computed by hand. Over an opaque
/// background the result alpha is exactly 1: `r = 255 · 128/255 = 128`,
/// `b = 255 · 127/255 = 127`. Over a translucent one the background keeps
/// `56/255 · 245/255` of its alpha: `10/255 + 0.2110 = 0.2502 → 63.8 → 64`.
pub(crate) fn blend_over_is_source_over_alpha_blend() {
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
