//! [`Lerp`] implementations for the painting value types.
//!
//! Orphan-rule legal: `Lerp` is defined in `flui_foundation::geometry`, and `Color`/`Alignment`/`BoxDecoration` are local to this crate. These let one
//! generic `Tween<V: Lerp>` in flui-animation interpolate them without a
//! bespoke per-type tween struct.
//!
//! `BorderRadius` is **not** here: it is `flui_foundation::geometry::Corners<Radius>`,
//! a `flui_foundation::geometry` type, so its `Lerp` lives there — the `Lerp for Corners<T>`
//! blanket also makes `Tween<BorderRadius>` work without another tween type.

use flui_foundation::geometry::Lerp;

use crate::Alignment;
use crate::styling::{BoxDecoration, Color};

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

/// [`BoxDecoration::lerp`], so `Tween<BoxDecoration<f64>>` drives a
/// decoration animation without a decoration-specific tween type.
///
/// The decoration is the property that owns its domain (see the trait's
/// "Domains belong to the property"): a paired gradient's geometry
/// extrapolates with `t`, while colors, border widths, radii and shadows
/// saturate at the endpoints, because an overshooting blur or border width
/// would leave the range a painter accepts.
impl Lerp for BoxDecoration<f64> {
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        BoxDecoration::lerp(self, other, t)
    }
}

/// Interpolates what has a magnitude and switches the rest half way.
///
/// A size, spacing, line height or colour set on both ends interpolates; a
/// weight interpolates through its CSS number to the nearest weight; shadow
/// lists of equal length interpolate pairwise. Anything set on one end only,
/// and every discrete field (family, style, features, variations, sizing),
/// takes the start before `t = 0.5` and the end from there. Font size and
/// line height never go below zero, and a font size stays positive, even
/// where `t` overshoots; the line a size and a relative height resolve to is
/// no taller than the taller end's, and spacing stays finite in `f32`.
impl Lerp for crate::typography::TextStyle {
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        fn both<V: Clone>(
            a: Option<&V>,
            b: Option<&V>,
            t: f64,
            mix: impl Fn(&V, &V) -> V,
        ) -> Option<V> {
            match (a, b) {
                (Some(a), Some(b)) => Some(mix(a, b)),
                _ if t < 0.5 => a.cloned(),
                _ => b.cloned(),
            }
        }
        // Overflow-aware: finite magnitudes keep a finite interpolation.
        let scalar = |a: &f64, b: &f64| a.lerp_to(b, t);
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
                })
                .collect()
        } else {
            near.shadows.clone()
        };
        let font_size = both(
            self.font_size.as_ref(),
            other.font_size.as_ref(),
            t,
            // Shaping needs a strictly positive size it can represent: an
            // overshoot stops inside the positive finite f32 range.
            |a, b| scalar(a, b).clamp(f64::from(f32::MIN_POSITIVE), f64::from(f32::MAX)),
        );
        let height = both(self.height.as_ref(), other.height.as_ref(), t, |a, b| {
            scalar(a, b).max(0.0)
        });
        let height = match (font_size, height) {
            (Some(size), Some(height)) => Some(bounded_height(size, height, [self, other])),
            // Both ends resolve against the same inherited size, so the
            // taller relative height bounds the blend.
            (None, Some(height)) => {
                Some(height.min(self.height.unwrap_or(0.0).max(other.height.unwrap_or(0.0))))
            }
            (_, None) => None,
        };
        // Shaping narrows spacing to f32: an overshoot stops inside its
        // finite range.
        let spacing =
            |a: &f64, b: &f64| scalar(a, b).clamp(-f64::from(f32::MAX), f64::from(f32::MAX));
        Self {
            color: both(self.color.as_ref(), other.color.as_ref(), t, colour),
            background_color: both(
                self.background_color.as_ref(),
                other.background_color.as_ref(),
                t,
                colour,
            ),
            foreground: both(
                self.foreground.as_ref(),
                other.foreground.as_ref(),
                t,
                colour,
            ),
            background: both(
                self.background.as_ref(),
                other.background.as_ref(),
                t,
                colour,
            ),
            font_size,
            letter_spacing: both(
                self.letter_spacing.as_ref(),
                other.letter_spacing.as_ref(),
                t,
                spacing,
            ),
            word_spacing: both(
                self.word_spacing.as_ref(),
                other.word_spacing.as_ref(),
                t,
                spacing,
            ),
            height,
            font_weight: both(
                self.font_weight.as_ref(),
                other.font_weight.as_ref(),
                t,
                |a, b| {
                    let weight = scalar(&f64::from(a.value()), &f64::from(b.value()));
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "a weight between two CSS weights fits an i32"
                    )]
                    crate::typography::FontWeight::from_css(weight.round() as i32)
                },
            ),
            shadows,
            ..near.clone()
        }
    }
}

/// `height`, lowered where needed so the line it resolves to (`height ×
/// size`) is no taller than the taller end's. Two shapeable ends can trade a
/// huge size against a huge relative height, and their blend multiplies two
/// halves of each. When an end sets no size or height of its own, the bound
/// is what shaping can represent: a line height finite in `f32`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "shaping resolves the line height in f32"
)]
fn bounded_height(size: f64, height: f64, ends: [&crate::typography::TextStyle; 2]) -> f64 {
    let representable = f64::from(f32::MAX);
    let tallest = ends
        .iter()
        .map(|end| {
            end.font_size
                .zip(end.height)
                .map(|(size, height)| size * height)
        })
        .try_fold(0.0_f64, |tallest, line| line.map(|line| tallest.max(line)))
        .map_or(representable, |tallest| tallest.min(representable));
    if size * height <= tallest {
        return height;
    }
    let bounded = tallest / size;
    if (bounded as f32 * size as f32).is_finite() {
        bounded
    } else {
        // The quotient can round up past the bound; one step down cannot.
        f64::from((bounded as f32).next_down())
    }
}
