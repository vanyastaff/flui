//! Decoration types for styling

// Re-export painting types that are commonly used with decorations
pub use crate::paint::{BlendMode, BoxFit, ColorFilter, ImageRepeat};
use crate::{
    Alignment, BoxShape,
    paint::Image,
    styling::{Border, BorderRadius, BorderRadiusExt, BoxShadow, Color, Gradient},
};
use flui_foundation::geometry::traits::{NumericUnit, Unit};

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
    pub opacity: f64,

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
    pub const fn with_opacity(mut self, opacity: f64) -> Self {
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
pub trait Decoration: std::fmt::Debug {
    /// Returns true if this decoration is complex enough that it might
    /// change its appearance when the size changes.
    #[inline]
    fn is_complex(&self) -> bool {
        false
    }

    /// Linearly interpolate between two decorations.
    fn lerp_decoration(a: &Self, b: &Self, t: f64) -> Option<Self>
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
/// struct literal. A decoration type tends to grow fields over time
/// (`shape`, a background blend mode and `image` all arrive after the
/// basics), and each one would otherwise be a source-breaking change for
/// every caller that spelled out the braces. Keeping construction on the
/// constructors is what lets a new field be additive.
///
/// # Examples
///
/// ```
/// use flui_painting::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
///
/// // Simple colored box
/// let decoration = BoxDecoration::<f64>::with_color(Color::RED);
///
/// // Box with border and shadow
/// let decoration = BoxDecoration::<f64>::new()
///     .set_color(Some(Color::WHITE))
///     .set_border(Some(Border::all(BorderSide::new(
///         Color::BLACK,
///         2.0,
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

    // Retain the bounded endpoint until paint bounds are available. The raw
    // gradient is also retained so direct writes to the public gradient field
    // cannot accidentally use a stale fallback. Box this rare state rather
    // than adding two full gradient values to every decoration.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub(crate) gradient_fallback: Option<Box<(Gradient, Gradient)>>,

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
    /// `t == 0.5`.
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
            gradient_fallback: None,
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
            gradient_fallback: None,
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
            gradient_fallback: None,
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
            gradient_fallback: None,
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
        self.gradient_fallback = None;
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
    T: std::ops::Mul<f64, Output = T>,
{
    /// Linearly interpolate between two box decorations.
    ///
    /// The exact endpoints return `a` and `b`. A paired gradient's geometry
    /// extrapolates with `t`; other fields clamp `t` to `0..=1`.
    /// Extrapolation retains a bounded endpoint for painting when the resolved
    /// geometry cannot be represented at the actual box size.
    /// A field set on only one side fades toward nothing: a lone
    /// color or gradient scales its alpha, a lone border, radius or shadow
    /// list scales its geometry (by `1 - t` for `a`'s, `t` for `b`'s). The
    /// image and shape are not interpolated and switch at `t = 0.5`.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Self {
        if t == 0.0 {
            return a.clone();
        }
        if t == 1.0 {
            return b.clone();
        }
        let gradient_t = t;
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 || t == 1.0 {
            let mut endpoint = if t == 0.0 { a.clone() } else { b.clone() };
            if let (Some(a_gradient), Some(b_gradient)) = (&a.gradient, &b.gradient)
                && let Some(mut gradient) = Gradient::lerp(a_gradient, b_gradient, gradient_t)
            {
                if let Some(bounded) = &endpoint.gradient {
                    // Only geometry extrapolates. Keep the saturated endpoint's
                    // ramp instead of merging disjoint stop positions again.
                    match (&mut gradient, bounded) {
                        (Gradient::Linear(raw), Gradient::Linear(end)) => {
                            raw.colors.clone_from(&end.colors);
                            raw.stops.clone_from(&end.stops);
                        }
                        (Gradient::Radial(raw), Gradient::Radial(end)) => {
                            raw.colors.clone_from(&end.colors);
                            raw.stops.clone_from(&end.stops);
                        }
                        (Gradient::Sweep(raw), Gradient::Sweep(end)) => {
                            raw.colors.clone_from(&end.colors);
                            raw.stops.clone_from(&end.stops);
                        }
                        _ => {}
                    }
                    if &gradient != bounded {
                        let terminal = endpoint.terminal_gradient().unwrap_or(bounded);
                        endpoint.gradient_fallback =
                            Some(Box::new((gradient.clone(), terminal.clone())));
                    }
                }
                endpoint.gradient = Some(gradient);
            }
            return endpoint;
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
        let scale_shadows = |shadows: &[BoxShadow<T>], factor: f64| {
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
        let mix_gradient = |a: Option<&Gradient>, b: Option<&Gradient>| match (a, b) {
            (Some(a_grad), Some(b_grad)) => Gradient::lerp(a_grad, b_grad, gradient_t)
                .or_else(|| Gradient::lerp(a_grad, b_grad, t)),
            (Some(gradient), None) => Some(scale_gradient(gradient, fade_a)),
            (None, Some(gradient)) => Some(scale_gradient(gradient, fade_b)),
            (None, None) => None,
        };
        let gradient = mix_gradient(a.gradient.as_ref(), b.gradient.as_ref());
        let bounded = if a.gradient_fallback.is_some() || b.gradient_fallback.is_some() {
            mix_gradient(a.terminal_gradient(), b.terminal_gradient())
        } else {
            None
        };
        let (gradient, gradient_fallback) = match (gradient, bounded) {
            (Some(raw), Some(bounded)) if raw != bounded => {
                (Some(raw.clone()), Some(Box::new((raw, bounded))))
            }
            (raw, bounded) => (raw.or(bounded), None),
        };

        // Images are not crossfaded; they switch at the midpoint.
        let image = if t < 0.5 {
            a.image.clone()
        } else {
            b.image.clone()
        };
        // Not interpolatable:
        // discrete switch at the midpoint, same as the image above.
        let shape = if t < 0.5 { a.shape } else { b.shape };

        Self {
            color,
            image,
            border,
            border_radius,
            box_shadow,
            gradient,
            gradient_fallback,
            shape,
        }
    }

    /// Ignore retained state after direct replacement of the public gradient.
    fn terminal_gradient(&self) -> Option<&Gradient> {
        self.gradient.as_ref().map(|gradient| {
            self.gradient_fallback
                .as_deref()
                .filter(|(raw, _)| raw == gradient)
                .map_or(gradient, |(_, bounded)| bounded)
        })
    }
}

/// The alpha of `color` scaled by `factor`, rounded to 8 bits.
fn scale_alpha(color: Color, factor: f64) -> Color {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 first"
    )]
    let alpha = (f64::from(color.a) * factor).clamp(0.0, 255.0).round() as u8;
    color.with_alpha(alpha)
}

/// Every side's width scaled by `factor`.
fn scale_border<T>(border: Border<T>, factor: f64) -> Border<T>
where
    T: NumericUnit + std::ops::Mul<f64, Output = T>,
{
    let side = |side: Option<crate::styling::BorderSide<T>>| side.map(|s| s.scale(factor));
    Border::new(
        side(border.top),
        side(border.right),
        side(border.bottom),
        side(border.left),
    )
}

/// Every color's alpha scaled by `factor`.
fn scale_gradient(gradient: &Gradient, factor: f64) -> Gradient {
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
    T: std::ops::Mul<f64, Output = T>,
{
    #[inline]
    fn is_complex(&self) -> bool {
        self.gradient.is_some() || self.box_shadow.is_some()
    }

    #[inline]
    fn lerp_decoration(a: &Self, b: &Self, t: f64) -> Option<Self> {
        Some(BoxDecoration::lerp(a, b, t))
    }
}
