//! Decoration types for styling

// Re-export painting types that are commonly used with decorations
pub use crate::painting::{BlendMode, BoxFit, ColorFilter, ImageRepeat};
use crate::{
    geometry::traits::{NumericUnit, Unit},
    layout::{Alignment, BoxShape},
    painting::Image,
    styling::{Border, BorderRadius, BorderRadiusExt, BoxShadow, Color, Gradient},
};

/// An image to paint as part of a decoration.
///
/// Used within `BoxDecoration` to display images with specific fit, alignment,
/// repeat, and opacity settings.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecorationImage {
    /// The image to paint.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub image: Image,

    /// How to inscribe the image into the space allocated during layout.
    pub fit: Option<BoxFit>,

    /// How to align the image within its bounds.
    pub alignment: Alignment,

    /// How to repeat the image.
    pub repeat: ImageRepeat,

    /// The opacity to apply to the image.
    ///
    /// 0.0 = fully transparent, 1.0 = fully opaque.
    pub opacity: f32,

    /// A color filter to apply to the image before painting it.
    pub color_filter: Option<ColorFilter>,
}

impl DecorationImage {
    /// Creates a decoration image with default settings: no fit, centered,
    /// not repeated, fully opaque, and no color filter.
    #[must_use]
    #[inline]
    pub fn new(image: Image) -> Self {
        Self {
            image,
            fit: None,
            alignment: Alignment::CENTER,
            repeat: ImageRepeat::NoRepeat,
            opacity: 1.0,
            color_filter: None,
        }
    }

    /// Sets how to inscribe the image into the space allocated during layout.
    #[must_use]
    #[inline]
    pub fn with_fit(mut self, fit: BoxFit) -> Self {
        self.fit = Some(fit);
        self
    }

    /// Sets how to align the image within its bounds.
    #[must_use]
    #[inline]
    pub const fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets how to repeat the image.
    #[must_use]
    #[inline]
    pub const fn with_repeat(mut self, repeat: ImageRepeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Sets the opacity to apply to the image (0.0 = fully transparent,
    /// 1.0 = fully opaque).
    #[must_use]
    #[inline]
    pub const fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// Sets a color filter to apply to the image before painting it.
    #[must_use]
    #[inline]
    pub const fn with_color_filter(mut self, color_filter: ColorFilter) -> Self {
        self.color_filter = Some(color_filter);
        self
    }
}

/// Base trait for decorations.
///
/// Similar to Flutter's `Decoration`.
pub trait Decoration: std::fmt::Debug {
    /// Returns true if this decoration is complex enough that it might
    /// change its appearance when the size changes.
    #[inline]
    fn is_complex(&self) -> bool {
        false
    }

    /// Linearly interpolate between two decorations.
    fn lerp_decoration(a: &Self, b: &Self, t: f32) -> Option<Self>
    where
        Self: Sized;
}

/// Box decoration with borders, shadows, and gradients.
///
/// Generic over unit type `T` for full type safety.
///
/// # Construction
///
/// This type is `#[non_exhaustive]`: build it with [`BoxDecoration::new`] (or
/// one of the `with_*` constructors) and the `set_*` chain, never with a
/// struct literal. Flutter's `BoxDecoration` has grown fields steadily —
/// `shape`, `backgroundBlendMode`, `image` all arrived after the type
/// existed — and each one would otherwise be a source-breaking change for
/// every caller that spelled out the braces. Keeping construction on the
/// constructors is what lets a new field be additive.
///
/// # Examples
///
/// ```
/// use flui_types::{
///     geometry::{Pixels, px},
///     styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color},
/// };
///
/// // Simple colored box
/// let decoration = BoxDecoration::<Pixels>::with_color(Color::RED);
///
/// // Box with border and shadow
/// let decoration = BoxDecoration::<Pixels>::new()
///     .set_color(Some(Color::WHITE))
///     .set_border(Some(Border::all(BorderSide::new(
///         Color::BLACK,
///         px(2.0),
///         BorderStyle::Solid,
///     ))));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct BoxDecoration<T: Unit> {
    /// The color to fill the box with.
    pub color: Option<Color>,

    /// An image to paint above the background color or gradient.
    pub image: Option<DecorationImage>,

    /// A border to draw above the background.
    pub border: Option<Border<T>>,

    /// The border radius of the box.
    pub border_radius: Option<BorderRadius>,

    /// A list of shadows cast by the box.
    pub box_shadow: Option<Vec<BoxShadow<T>>>,

    /// A gradient to use when filling the box.
    ///
    /// If this is specified, `color` has no effect.
    pub gradient: Option<Gradient>,

    /// The shape to fill the background, gradient, and image into, and
    /// to cast as the box shadow.
    ///
    /// If this is [`BoxShape::Circle`], `border_radius` is ignored (a
    /// warning is logged — at most once per process, since resolving the
    /// silhouette runs on every paint and every hit test — rather than
    /// asserting, so the fallback is observable in every build profile).
    /// The circle is inscribed in the shorter of the paint rect's two
    /// sides.
    ///
    /// The shape cannot be interpolated: `lerp` switches discretely at
    /// `t == 0.5` (Flutter parity, `box_decoration.dart:209-211,314`).
    ///
    /// `serde(default)` is load-bearing, not decoration: this field was
    /// added after the type was already serializable, so a payload
    /// written without it would otherwise fail to deserialize with a
    /// missing-field error. Deriving `Default` on [`BoxShape`] does not
    /// give serde that behaviour on its own.
    #[cfg_attr(feature = "serde", serde(default))]
    pub shape: BoxShape,
}

impl<T: Unit> BoxDecoration<T> {
    /// Creates a new box decoration.
    #[inline]
    pub const fn new() -> Self {
        Self {
            color: None,
            image: None,
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            shape: BoxShape::Rectangle,
        }
    }

    /// Creates a box decoration with a color.
    #[inline]
    pub const fn with_color(color: Color) -> Self {
        Self {
            color: Some(color),
            image: None,
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            shape: BoxShape::Rectangle,
        }
    }

    /// Creates a box decoration with a gradient.
    #[inline]
    pub fn with_gradient(gradient: Gradient) -> Self {
        Self {
            color: None,
            image: None,
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: Some(gradient),
            shape: BoxShape::Rectangle,
        }
    }

    /// Creates a box decoration with an image.
    #[inline]
    pub fn with_image(image: DecorationImage) -> Self {
        Self {
            color: None,
            image: Some(image),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            shape: BoxShape::Rectangle,
        }
    }

    /// Creates a copy of this decoration with the given color.
    #[inline]
    pub const fn set_color(mut self, color: Option<Color>) -> Self {
        self.color = color;
        self
    }

    /// Creates a copy of this decoration with the given border.
    #[inline]
    pub const fn set_border(mut self, border: Option<Border<T>>) -> Self {
        self.border = border;
        self
    }

    /// Creates a copy of this decoration with the given border radius.
    #[inline]
    pub const fn set_border_radius(mut self, border_radius: Option<BorderRadius>) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Creates a copy of this decoration with the given box shadow.
    #[inline]
    pub fn set_box_shadow(mut self, box_shadow: Option<Vec<BoxShadow<T>>>) -> Self {
        self.box_shadow = box_shadow;
        self
    }

    /// Creates a copy of this decoration with the given gradient.
    #[inline]
    pub fn set_gradient(mut self, gradient: Option<Gradient>) -> Self {
        self.gradient = gradient;
        self
    }

    /// Creates a copy of this decoration with the given shape.
    #[inline]
    pub const fn set_shape(mut self, shape: BoxShape) -> Self {
        self.shape = shape;
        self
    }
}

impl<T: NumericUnit> BoxDecoration<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    /// Linearly interpolate between two box decorations, following
    /// Flutter's `BoxDecoration.lerp`.
    ///
    /// `t` is clamped to `0..=1`, and the endpoints return `a` and `b`
    /// exactly. A field set on only one side fades toward nothing: a lone
    /// color or gradient scales its alpha, a lone border, radius or shadow
    /// list scales its geometry (by `1 - t` for `a`'s, `t` for `b`'s). The
    /// image and shape are not interpolated and switch at `t = 0.5`.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 {
            return a.clone();
        }
        if t == 1.0 {
            return b.clone();
        }
        let (fade_a, fade_b) = (1.0 - t, t);

        let color = match (a.color, b.color) {
            (Some(a_color), Some(b_color)) => Some(Color::lerp(a_color, b_color, t)),
            (Some(color), None) => Some(scale_alpha(color, fade_a)),
            (None, Some(color)) => Some(scale_alpha(color, fade_b)),
            (None, None) => None,
        };
        let border = match (a.border, b.border) {
            (Some(a_border), Some(b_border)) => Some(Border::lerp(a_border, b_border, t)),
            (Some(border), None) => Some(scale_border(border, fade_a)),
            (None, Some(border)) => Some(scale_border(border, fade_b)),
            (None, None) => None,
        };
        let zero = <BorderRadius as BorderRadiusExt>::ZERO;
        let border_radius = match (a.border_radius, b.border_radius) {
            (Some(a_radius), Some(b_radius)) => Some(BorderRadius::lerp(a_radius, b_radius, t)),
            (Some(radius), None) => Some(BorderRadius::lerp(radius, zero, t)),
            (None, Some(radius)) => Some(BorderRadius::lerp(zero, radius, t)),
            (None, None) => None,
        };
        let scale_shadows = |shadows: &[BoxShadow<T>], factor: f32| {
            shadows.iter().map(|shadow| shadow.scale(factor)).collect()
        };
        let box_shadow = match (&a.box_shadow, &b.box_shadow) {
            (Some(a_shadows), Some(b_shadows)) => {
                Some(BoxShadow::lerp_list(a_shadows, b_shadows, t))
            }
            (Some(shadows), None) => Some(scale_shadows(shadows, fade_a)),
            (None, Some(shadows)) => Some(scale_shadows(shadows, fade_b)),
            (None, None) => None,
        };
        let gradient = match (&a.gradient, &b.gradient) {
            (Some(a_grad), Some(b_grad)) => Gradient::lerp(a_grad, b_grad, t),
            (Some(gradient), None) => Some(scale_gradient(gradient, fade_a)),
            (None, Some(gradient)) => Some(scale_gradient(gradient, fade_b)),
            (None, None) => None,
        };

        // Images crossfade in Flutter; here they switch at the midpoint.
        let image = if t < 0.5 {
            a.image.clone()
        } else {
            b.image.clone()
        };
        // Not interpolatable (Flutter parity, box_decoration.dart:209-211):
        // discrete switch at the midpoint, same as the image above.
        let shape = if t < 0.5 { a.shape } else { b.shape };

        Self {
            color,
            image,
            border,
            border_radius,
            box_shadow,
            gradient,
            shape,
        }
    }
}

/// Flutter's `Color.lerp(null, color, factor)`: the alpha scaled by
/// `factor`, rounded to 8 bits.
fn scale_alpha(color: Color, factor: f32) -> Color {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 first"
    )]
    let alpha = (f32::from(color.a) * factor).clamp(0.0, 255.0).round() as u8;
    color.with_alpha(alpha)
}

/// Flutter's `Border.scale`: every side's width scaled by `factor`.
fn scale_border<T>(border: Border<T>, factor: f32) -> Border<T>
where
    T: NumericUnit + std::ops::Mul<f32, Output = T>,
{
    let side = |side: Option<crate::styling::BorderSide<T>>| side.map(|s| s.scale(factor));
    Border::new(
        side(border.top),
        side(border.right),
        side(border.bottom),
        side(border.left),
    )
}

/// Flutter's `Gradient.scale`: every color's alpha scaled by `factor`.
fn scale_gradient(gradient: &Gradient, factor: f32) -> Gradient {
    let mut scaled = gradient.clone();
    let colors = match &mut scaled {
        Gradient::Linear(g) => &mut g.colors,
        Gradient::Radial(g) => &mut g.colors,
        Gradient::Sweep(g) => &mut g.colors,
    };
    for color in colors {
        *color = scale_alpha(*color, factor);
    }
    scaled
}

impl<T: Unit> Default for BoxDecoration<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: NumericUnit> Decoration for BoxDecoration<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    #[inline]
    fn is_complex(&self) -> bool {
        self.gradient.is_some() || self.box_shadow.is_some()
    }

    #[inline]
    fn lerp_decoration(a: &Self, b: &Self, t: f32) -> Option<Self> {
        Some(BoxDecoration::lerp(a, b, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Pixels;

    /// `BoxDecoration::lerp`'s `shape` field is NOT interpolated (Flutter
    /// parity, `box_decoration.dart:209-211,314`): it switches discretely
    /// at the midpoint (`shape = if t < 0.5 { a.shape } else { b.shape }`
    /// above). This branch had zero coverage before -- it could regress to
    /// always picking `a`, always picking `b`, or an actual interpolation
    /// without any test failing.
    #[test]
    fn lerp_shape_switches_discretely_at_the_midpoint() {
        let a = BoxDecoration::<Pixels>::new().set_shape(BoxShape::Rectangle);
        let b = BoxDecoration::<Pixels>::new().set_shape(BoxShape::Circle);

        // t < 0.5: `a`'s shape.
        assert_eq!(BoxDecoration::lerp(&a, &b, 0.0).shape, BoxShape::Rectangle);
        assert_eq!(BoxDecoration::lerp(&a, &b, 0.25).shape, BoxShape::Rectangle);
        assert_eq!(
            BoxDecoration::lerp(&a, &b, 0.499).shape,
            BoxShape::Rectangle
        );

        // t == 0.5 exactly: the switch already happened (`t < 0.5` is
        // false at 0.5), so this lands on `b`'s shape, not `a`'s.
        assert_eq!(BoxDecoration::lerp(&a, &b, 0.5).shape, BoxShape::Circle);

        // t > 0.5: `b`'s shape.
        assert_eq!(BoxDecoration::lerp(&a, &b, 0.75).shape, BoxShape::Circle);
        assert_eq!(BoxDecoration::lerp(&a, &b, 1.0).shape, BoxShape::Circle);

        // And the reverse direction, to catch an accidental `a`/`b` swap.
        assert_eq!(BoxDecoration::lerp(&b, &a, 0.0).shape, BoxShape::Circle);
        assert_eq!(BoxDecoration::lerp(&b, &a, 0.5).shape, BoxShape::Rectangle);
        assert_eq!(BoxDecoration::lerp(&b, &a, 1.0).shape, BoxShape::Rectangle);
    }

    /// A payload written before `shape` existed must still load, as the
    /// rectangle it was. Without `serde(default)` on the field, serde
    /// rejects it outright — `BoxShape: Default` does not reach the
    /// derive on its own.
    #[cfg(feature = "serde")]
    #[test]
    fn a_decoration_serialized_before_shape_existed_still_deserializes() {
        let legacy = r#"{
            "color": null,
            "image": null,
            "border": null,
            "border_radius": null,
            "box_shadow": null,
            "gradient": null
        }"#;

        let decoration: BoxDecoration<crate::geometry::Pixels> =
            serde_json::from_str(legacy).expect("a pre-shape payload must still deserialize");
        assert_eq!(decoration.shape, BoxShape::Rectangle);
    }

    use crate::geometry::{Offset, px};
    use crate::styling::{BorderSide, BorderStyle, LinearGradient};

    type Deco = BoxDecoration<Pixels>;

    fn side(width: f32) -> BorderSide<Pixels> {
        BorderSide::new(Color::RED, px(width), BorderStyle::Solid)
    }

    fn shadow(v: f32) -> BoxShadow<Pixels> {
        BoxShadow::new(Color::BLACK, Offset::new(px(v), px(v)), px(v), px(v))
    }

    fn gradient() -> Gradient {
        Gradient::Linear(LinearGradient::horizontal(vec![Color::RED, Color::BLUE]))
    }

    /// Everything set, so a one-sided lerp exercises every field.
    fn full() -> Deco {
        Deco::with_color(Color::rgb(200, 100, 50))
            .set_border(Some(crate::styling::Border::all(side(4.0))))
            .set_border_radius(Some(BorderRadius::circular(px(8.0))))
            .set_box_shadow(Some(vec![shadow(4.0)]))
            .set_gradient(Some(gradient()))
    }

    #[test]
    fn endpoints_are_exact() {
        let (a, b) = (full(), Deco::new());
        assert_eq!(Deco::lerp(&a, &b, 0.0), a);
        assert_eq!(Deco::lerp(&a, &b, 1.0), b);
        assert_eq!(Deco::lerp(&a, &b, -1.0), a);
        assert_eq!(Deco::lerp(&a, &b, 2.0), b);
    }

    /// A field present on one side only fades toward nothing, monotonically:
    /// by `1 - t` for `a`'s fields and by `t` for `b`'s. (The alpha was once
    /// faded out and back in, so a decoration lerping to one with no color
    /// ended at its own color, fully opaque.)
    #[test]
    fn one_sided_fields_fade_toward_nothing() {
        for (a, b, t, factor) in [
            (full(), Deco::new(), 0.5_f32, 0.5_f32),
            (full(), Deco::new(), 0.25, 0.75),
            (full(), Deco::new(), 0.75, 0.25),
            (Deco::new(), full(), 0.25, 0.25),
            (Deco::new(), full(), 0.75, 0.75),
        ] {
            let mid = Deco::lerp(&a, &b, t);
            let alpha = (255.0_f32 * factor).round() as u8;
            assert_eq!(mid.color, Some(Color::rgba(200, 100, 50, alpha)), "t = {t}");
            let border = mid.border.expect("scaled border");
            assert_eq!(border.top, Some(side(4.0 * factor)), "t = {t}");
            assert_eq!(border.left, Some(side(4.0 * factor)), "t = {t}");
            assert_eq!(
                mid.border_radius,
                Some(BorderRadius::circular(px(8.0 * factor)))
            );
            assert_eq!(mid.box_shadow, Some(vec![shadow(4.0 * factor)]));
            let colors = mid.gradient.expect("scaled gradient").colors().to_vec();
            assert_eq!(
                colors,
                vec![Color::RED.with_alpha(alpha), Color::BLUE.with_alpha(alpha)]
            );
        }
    }

    #[test]
    fn two_sided_fields_lerp() {
        let a = Deco::with_color(Color::rgb(0, 0, 0))
            .set_border(Some(crate::styling::Border::all(side(2.0))))
            .set_border_radius(Some(BorderRadius::circular(px(2.0))))
            .set_box_shadow(Some(vec![shadow(2.0)]))
            .set_gradient(Some(gradient()));
        let b = full();
        let mid = Deco::lerp(&a, &b, 0.5);
        assert_eq!(mid.color, Some(Color::rgb(100, 50, 25)));
        assert_eq!(mid.border.and_then(|b| b.top), Some(side(3.0)));
        assert_eq!(mid.border_radius, Some(BorderRadius::circular(px(5.0))));
        assert_eq!(mid.box_shadow, Some(vec![shadow(3.0)]));
        assert_eq!(mid.gradient, Some(gradient()));
        assert_eq!(Deco::lerp(&Deco::new(), &Deco::new(), 0.5), Deco::new());
    }

    #[test]
    fn image_switches_at_the_midpoint() {
        let image = DecorationImage::new(Image::solid_color(1, 1, Color::RED));
        let a = Deco::with_image(image.clone());
        let b = Deco::new();
        assert_eq!(Deco::lerp(&a, &b, 0.25).image, Some(image.clone()));
        assert_eq!(Deco::lerp(&a, &b, 0.5).image, None);
        assert_eq!(Deco::lerp(&b, &a, 0.5).image, Some(image));
    }

    #[test]
    fn constructors_setters_and_complexity() {
        assert_eq!(Deco::default(), Deco::new());
        assert_eq!(Deco::with_color(Color::RED).color, Some(Color::RED));
        assert_eq!(
            Deco::with_gradient(gradient()),
            Deco::new().set_gradient(Some(gradient()))
        );
        let image = DecorationImage::new(Image::solid_color(1, 1, Color::RED));
        assert_eq!(
            Deco::with_image(image.clone()),
            Deco {
                image: Some(image),
                ..Deco::new()
            }
        );
        let d = Deco::new()
            .set_color(Some(Color::BLUE))
            .set_shape(BoxShape::Circle);
        assert_eq!((d.color, d.shape), (Some(Color::BLUE), BoxShape::Circle));

        assert!(!Deco::with_color(Color::RED).is_complex());
        assert!(Deco::with_gradient(gradient()).is_complex());
        assert!(Deco::new().set_box_shadow(Some(vec![])).is_complex());
        assert_eq!(
            Deco::lerp_decoration(&full(), &Deco::new(), 0.5),
            Some(Deco::lerp(&full(), &Deco::new(), 0.5))
        );
    }

    #[test]
    fn decoration_image_builders() {
        let filter = ColorFilter::mode(Color::RED, BlendMode::Multiply);
        let image = DecorationImage::new(Image::solid_color(1, 1, Color::RED))
            .with_fit(BoxFit::Cover)
            .with_alignment(Alignment::TOP_LEFT)
            .with_repeat(ImageRepeat::Repeat)
            .with_opacity(0.5)
            .with_color_filter(filter);
        assert_eq!(image.fit, Some(BoxFit::Cover));
        assert_eq!(image.alignment, Alignment::TOP_LEFT);
        assert_eq!(image.repeat, ImageRepeat::Repeat);
        assert_eq!(image.opacity, 0.5);
        assert_eq!(image.color_filter, Some(filter));
        let plain = DecorationImage::new(Image::solid_color(1, 1, Color::RED));
        assert_eq!(
            (
                plain.fit,
                plain.alignment,
                plain.repeat,
                plain.opacity,
                plain.color_filter
            ),
            (None, Alignment::CENTER, ImageRepeat::NoRepeat, 1.0, None)
        );
    }
}
