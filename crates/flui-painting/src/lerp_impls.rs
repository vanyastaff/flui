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
        // takes the same premultiplied Oklab interpolation without the clamp.
        Color::lerp_unclamped(*self, *other, t)
    }
}

impl Lerp for Alignment {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Alignment::lerp(*self, *other, t)
    }
}
