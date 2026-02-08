//! Decoration types for styling

use crate::geometry::traits::{NumericUnit, Unit};
use crate::layout::Alignment;
use crate::painting::Image;
use crate::styling::{Border, BorderRadius, BorderRadiusExt, BoxShadow, Color, Gradient};

// Re-export painting types that are commonly used with decorations
pub use crate::painting::{BlendMode, BoxFit, ColorFilter, ImageRepeat};

/// An image to paint as part of a decoration.
///
/// Used within `BoxDecoration` to display images with specific fit, alignment,
/// repeat, and opacity settings.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecorationImage {
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

    #[must_use]
    #[inline]
    pub fn with_fit(mut self, fit: BoxFit) -> Self {
        self.fit = Some(fit);
        self
    }

    #[must_use]
    #[inline]
    pub const fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    #[must_use]
    #[inline]
    pub const fn with_repeat(mut self, repeat: ImageRepeat) -> Self {
        self.repeat = repeat;
        self
    }

    #[must_use]
    #[inline]
    pub const fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

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
    #[inline]
    fn lerp_decoration(a: &Self, b: &Self, t: f32) -> Option<Self>
    where
        Self: Sized;
}

/// Box decoration with borders, shadows, and gradients.
///
/// Generic over unit type `T` for full type safety.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{BoxDecoration, Color, Border, BorderSide, BorderStyle};
/// use flui_types::geometry::{Pixels, px};
///
/// // Simple colored box
/// let decoration = BoxDecoration::<Pixels>::with_color(Color::RED);
///
/// // Box with border and shadow
/// let decoration = BoxDecoration::<Pixels>::new()
///     .set_color(Some(Color::WHITE))
///     .set_border(Some(Border::all(
///         BorderSide::new(Color::BLACK, px(2.0), BorderStyle::Solid)
///     )));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
}

impl<T: NumericUnit> BoxDecoration<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    /// Linearly interpolate between two box decorations.
    #[inline]
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);

        let color = match (a.color, b.color) {
            (Some(a_color), Some(b_color)) => Some(Color::lerp(a_color, b_color, t)),
            (Some(color), None) | (None, Some(color)) => {
                let alpha_f32 = color.a as f32 / 255.0;
                let new_alpha = (alpha_f32
                    * if t < 0.5 {
                        1.0 - t * 2.0
                    } else {
                        (t - 0.5) * 2.0
                    })
                .clamp(0.0, 1.0);
                Some(color.with_alpha((new_alpha * 255.0) as u8))
            }
            (None, None) => None,
        };

        let border = match (&a.border, &b.border) {
            (Some(a_border), Some(b_border)) => Some(Border::lerp(*a_border, *b_border, t)),
            _ => None,
        };

        let border_radius = match (a.border_radius, b.border_radius) {
            (Some(a_radius), Some(b_radius)) => Some(BorderRadius::lerp(a_radius, b_radius, t)),
            (Some(radius), None) | (None, Some(radius)) => Some(radius),
            (None, None) => None,
        };

        let box_shadow = match (&a.box_shadow, &b.box_shadow) {
            (Some(a_shadows), Some(b_shadows)) => {
                Some(BoxShadow::lerp_list(a_shadows, b_shadows, t))
            }
            (Some(shadows), None) | (None, Some(shadows)) => Some(shadows.clone()),
            (None, None) => None,
        };

        let gradient = match (&a.gradient, &b.gradient) {
            (Some(a_grad), Some(b_grad)) => Gradient::lerp(a_grad, b_grad, t),
            _ => None,
        };

        // Image interpolation: crossfade between images (at t=0.5, switch)
        let image = if t < 0.5 {
            a.image.clone()
        } else {
            b.image.clone()
        };

        Self {
            color,
            image,
            border,
            border_radius,
            box_shadow,
            gradient,
        }
    }
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
