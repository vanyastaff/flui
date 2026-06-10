//! [`Lerp`] implementations for flui-types value types.
//!
//! Orphan-rule legal: `Lerp` is defined in flui-geometry (which flui-types
//! depends on), and `Color`/`Alignment` are local to this crate. These let one
//! generic `Tween<V: Lerp>` in flui-animation interpolate them without a
//! bespoke per-type tween struct.
//!
//! `BorderRadius` is **not** here: it is `flui_geometry::Corners<Radius<Pixels>>`,
//! a flui-geometry type, so its `Lerp` lives there — the `Lerp for Corners<T>`
//! blanket added alongside the matrix `Lerp` work — and `BorderRadiusTween` is
//! now simply an alias for `Tween<BorderRadius>`.

use flui_geometry::Lerp;

use crate::layout::Alignment;
use crate::styling::Color;

impl Lerp for Color {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        // The `Lerp` contract is no-clamp: `t` may fall outside [0, 1] so
        // overshoot curves (elastic/back) propagate through `Tween<Color>`.
        // Delegating to `Color::lerp` (which clamps `t`) would flatten that
        // overshoot — the very thing the no-clamp tween path restores — so the
        // channels are interpolated directly here. `t` is NOT clamped; the
        // channel *values* still saturate into [0, 255] (the `f32 as u8` cast
        // saturates). Round, not truncate, to avoid biasing each channel down.
        let lerp_channel =
            |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Color::rgba(
            lerp_channel(self.r, other.r),
            lerp_channel(self.g, other.g),
            lerp_channel(self.b, other.b),
            lerp_channel(self.a, other.a),
        )
    }
}

impl Lerp for Alignment {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        Alignment::lerp(*self, *other, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_lerp_rounds_not_truncates() {
        let a = Color::rgba(0, 0, 0, 255);
        let b = Color::rgba(3, 3, 3, 255);
        // Midpoint of 0..3 is 1.5 -> rounds to 2 (truncation would give 1).
        let mid = a.lerp_to(&b, 0.5);
        assert_eq!(mid.r, 2, "channel must round, not truncate");
    }

    #[test]
    fn color_lerp_endpoints() {
        let a = Color::rgba(10, 20, 30, 40);
        let b = Color::rgba(200, 100, 50, 255);
        assert_eq!(a.lerp_to(&b, 0.0), a);
        assert_eq!(a.lerp_to(&b, 1.0), b);
    }

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
