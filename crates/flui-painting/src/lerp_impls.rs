//! [`Lerp`] implementations for the painting value types.
//!
//! Orphan-rule legal: `Lerp` is defined in `flui_foundation::geometry`, and `Color`/`Alignment` are local to this crate. These let one
//! generic `Tween<V: Lerp>` in flui-animation interpolate them without a
//! bespoke per-type tween struct.
//!
//! `BorderRadius` is **not** here: it is `flui_foundation::geometry::Corners<Radius>`,
//! a `flui_foundation::geometry` type, so its `Lerp` lives there — the `Lerp for Corners<T>`
//! blanket added alongside the matrix `Lerp` work — and `BorderRadiusTween` is
//! now simply an alias for `Tween<BorderRadius>`.

use flui_foundation::geometry::Lerp;

use crate::Alignment;
use crate::styling::Color;

impl Lerp for Color {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        // The `Lerp` contract is no-clamp: `t` may fall outside [0, 1] so
        // overshoot curves (elastic/back) propagate through `Tween<Color>`.
        // `Color::lerp` clamps `t`, which would flatten that overshoot, so this
        // takes the same premultiplied interpolation without the clamp. Colour
        // channels interpolate in f32; the animation parameter arrives as f64.
        Color::lerp_unclamped(*self, *other, t as f32)
    }
}

impl Lerp for Alignment {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Alignment::lerp(*self, *other, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_lerp_overshoot_is_not_flattened() {
        // The no-clamp `Lerp` contract: `t` outside [0, 1] extrapolates so
        // overshoot curves are not flattened. Channel values still saturate.
        let a = Color::rgba(0, 0, 0, 255);
        let b = Color::rgba(100, 0, 0, 255);
        // t = 1.5 -> r = 0 + 100 * 1.5 = 150 (clamping t would pin it at 100).
        assert_eq!(a.lerp_to(&b, 1.5).r, 150, "overshoot must not clamp t");
        // t = 2.6 -> r = 260 -> saturates to 255.
        assert_eq!(a.lerp_to(&b, 2.6).r, 255, "channel saturates at 255");
        // t = -0.5 -> r = -50 -> saturates to 0.
        assert_eq!(a.lerp_to(&b, -0.5).r, 0, "undershoot saturates at 0");
    }
}
