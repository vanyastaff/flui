//! [`Lerp`] implementations for flui-types value types.
//!
//! Orphan-rule legal: `Lerp` is defined in flui-geometry (which flui-types
//! depends on), and `Color`/`Alignment` are local to this crate. These let one
//! generic `Tween<V: Lerp>` in flui-animation interpolate them without a
//! bespoke per-type tween struct.
//!
//! `BorderRadius` is **not** here: it is `flui_geometry::Corners<Radius<Pixels>>`,
//! a flui-geometry type, so a `Lerp for Corners<T>` blanket belongs in
//! flui-geometry (added alongside the matrix `Lerp` work); `BorderRadiusTween`
//! stays a dedicated struct until then.

use flui_geometry::Lerp;

use crate::layout::Alignment;
use crate::styling::Color;

impl Lerp for Color {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f32) -> Self {
        // Delegates to the rounding (non-truncating) channel lerp. Color clamps
        // `t` internally — overshoot in a u8 channel space is meaningless and
        // saturates regardless, matching Flutter's `Color.lerp`.
        Color::lerp(*self, *other, t)
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
}
