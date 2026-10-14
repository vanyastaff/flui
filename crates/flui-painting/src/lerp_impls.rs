//! [`Lerp`] implementations for the painting value types.
//!
//! Orphan-rule legal: `Lerp` is defined in `flui_foundation::geometry`, and `Color`/`Alignment` are local to this crate. These let one
//! generic `Tween<V: Lerp>` in flui-animation interpolate them without a
//! bespoke per-type tween struct.
//!
//! `BorderRadius` is **not** here: it is `flui_foundation::geometry::Corners<Radius>`,
//! a `flui_foundation::geometry` type, so its `Lerp` lives there — the `Lerp for Corners<T>`
//! blanket also makes `Tween<BorderRadius>` work without another tween type.

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

/// Interpolates what has a magnitude and switches the rest half way.
///
/// A size, spacing, line height or colour set on both ends interpolates; a
/// weight interpolates through its CSS number to the nearest weight; shadow
/// lists of equal length interpolate pairwise. Anything set on one end only,
/// and every discrete field (family, style, features, variations, sizing),
/// takes the start before `t = 0.5` and the end from there. Font size and
/// line height never go below zero, even where `t` overshoots.
impl Lerp for crate::typography::TextStyle {
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        fn both<V: Clone>(
            a: &Option<V>,
            b: &Option<V>,
            t: f64,
            mix: impl Fn(&V, &V) -> V,
        ) -> Option<V> {
            match (a, b) {
                (Some(a), Some(b)) => Some(mix(a, b)),
                _ if t < 0.5 => a.clone(),
                _ => b.clone(),
            }
        }
        let scalar = |a: &f64, b: &f64| a + (b - a) * t;
        let colour = |a: &Color, b: &Color| a.lerp_to(b, t);
        let near = if t < 0.5 { self } else { other };
        let shadows = if self.shadows.len() == other.shadows.len() {
            self.shadows
                .iter()
                .zip(&other.shadows)
                .map(|(a, b)| crate::typography::TextShadow {
                    color: a.color.lerp_to(&b.color, t),
                    offset_x: scalar(&a.offset_x, &b.offset_x),
                    offset_y: scalar(&a.offset_y, &b.offset_y),
                    blur_radius: scalar(&a.blur_radius, &b.blur_radius).max(0.0),
                    ..a.clone()
                })
                .collect()
        } else {
            near.shadows.clone()
        };
        Self {
            color: both(&self.color, &other.color, t, colour),
            background_color: both(&self.background_color, &other.background_color, t, colour),
            foreground: both(&self.foreground, &other.foreground, t, colour),
            background: both(&self.background, &other.background, t, colour),
            font_size: both(&self.font_size, &other.font_size, t, |a, b| {
                scalar(a, b).max(0.0)
            }),
            letter_spacing: both(&self.letter_spacing, &other.letter_spacing, t, scalar),
            word_spacing: both(&self.word_spacing, &other.word_spacing, t, scalar),
            height: both(&self.height, &other.height, t, |a, b| scalar(a, b).max(0.0)),
            font_weight: both(&self.font_weight, &other.font_weight, t, |a, b| {
                let weight = scalar(&f64::from(a.value()), &f64::from(b.value()));
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a weight between two CSS weights fits an i32"
                )]
                crate::typography::FontWeight::from_css(weight.round() as i32)
            }),
            shadows,
            ..near.clone()
        }
    }
}
