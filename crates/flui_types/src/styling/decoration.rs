//! Decoration types for styling

use crate::geometry::Rect;
use crate::styling::{Border, BorderRadius, BoxShadow, Color, Gradient};

// Re-export painting types that are commonly used with decorations
pub use crate::painting::{BlendMode, BoxFit, ColorFilter, ImageRepeat};

/// Base trait for decorations.
///
/// Similar to Flutter's `Decoration`.
pub trait Decoration: std::fmt::Debug {
    /// Returns true if this decoration is complex enough that it might
    /// change its appearance when the size changes.
    fn is_complex(&self) -> bool {
        false
    }

    /// Linearly interpolate between two decorations.
    fn lerp_decoration(a: &Self, b: &Self, t: f32) -> Option<Self>
    where
        Self: Sized;
}

/// A decoration for a box.
///
/// Similar to Flutter's `BoxDecoration`.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{BoxDecoration, Color, BorderRadius};
///
/// let decoration = BoxDecoration {
///     color: Some(Color::WHITE),
///     border: None,
///     border_radius: Some(BorderRadius::circular(10.0)),
///     box_shadow: None,
///     gradient: None,
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoxDecoration {
    /// The color to fill the box with.
    pub color: Option<Color>,

    /// A border to draw above the background.
    pub border: Option<Border>,

    /// The border radius of the box.
    pub border_radius: Option<BorderRadius>,

    /// A list of shadows cast by the box.
    pub box_shadow: Option<Vec<BoxShadow>>,

    /// A gradient to use when filling the box.
    ///
    /// If this is specified, `color` has no effect.
    pub gradient: Option<Gradient>,
}

impl BoxDecoration {
    /// Creates a new box decoration.
    pub const fn new() -> Self {
        Self {
            color: None,
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        }
    }

    /// Creates a box decoration with a color.
    pub const fn with_color(color: Color) -> Self {
        Self {
            color: Some(color),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        }
    }

    /// Creates a box decoration with a gradient.
    pub fn with_gradient(gradient: Gradient) -> Self {
        Self {
            color: None,
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: Some(gradient),
        }
    }

    /// Creates a copy of this decoration with the given color.
    pub const fn set_color(mut self, color: Option<Color>) -> Self {
        self.color = color;
        self
    }

    /// Creates a copy of this decoration with the given border.
    pub const fn set_border(mut self, border: Option<Border>) -> Self {
        self.border = border;
        self
    }

    /// Creates a copy of this decoration with the given border radius.
    pub const fn set_border_radius(mut self, border_radius: Option<BorderRadius>) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Creates a copy of this decoration with the given box shadow.
    pub fn set_box_shadow(mut self, box_shadow: Option<Vec<BoxShadow>>) -> Self {
        self.box_shadow = box_shadow;
        self
    }

    /// Creates a copy of this decoration with the given gradient.
    pub fn set_gradient(mut self, gradient: Option<Gradient>) -> Self {
        self.gradient = gradient;
        self
    }

    /// Linearly interpolate between two box decorations.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);

        let color = match (a.color, b.color) {
            (Some(a_color), Some(b_color)) => Some(Color::lerp(a_color, b_color, t)),
            (Some(color), None) | (None, Some(color)) => {
                let alpha_f32 = color.alpha() as f32 / 255.0;
                let new_alpha = (alpha_f32 * if t < 0.5 { 1.0 - t * 2.0 } else { (t - 0.5) * 2.0 }).clamp(0.0, 1.0);
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

        Self {
            color,
            border,
            border_radius,
            box_shadow,
            gradient,
        }
    }
}

impl Default for BoxDecoration {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoration for BoxDecoration {
    fn is_complex(&self) -> bool {
        self.gradient.is_some() || self.box_shadow.is_some()
    }

    fn lerp_decoration(a: &Self, b: &Self, t: f32) -> Option<Self> {
        Some(BoxDecoration::lerp(a, b, t))
    }
}

/// An image for a decoration.
///
/// Similar to Flutter's `DecorationImage`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecorationImage {
    /// How the image should be inscribed into the box.
    pub fit: BoxFit,

    /// How to align the image within its bounds.
    pub alignment: crate::layout::Alignment,

    /// The center slice for 9-slice scaling.
    ///
    /// If specified, the image will be scaled using 9-slice scaling,
    /// where the center rectangle is stretched while the edges and
    /// corners maintain their original dimensions.
    pub center_slice: Option<Rect>,

    /// Whether to repeat the image to fill the box.
    pub repeat: ImageRepeat,

    /// The opacity to apply to the image.
    pub opacity: f32,

    /// A color filter to apply to the image.
    pub color_filter: Option<ColorFilter>,

    /// Whether to invert the colors of the image.
    pub invert_colors: bool,

    /// Whether the image should be flipped horizontally in right-to-left contexts.
    pub match_text_direction: bool,
}

impl DecorationImage {
    /// Creates a new decoration image.
    pub fn new(fit: BoxFit, alignment: crate::layout::Alignment) -> Self {
        Self {
            fit,
            alignment,
            center_slice: None,
            repeat: ImageRepeat::NoRepeat,
            opacity: 1.0,
            color_filter: None,
            invert_colors: false,
            match_text_direction: false,
        }
    }
}

impl Default for DecorationImage {
    fn default() -> Self {
        Self {
            fit: BoxFit::Contain,
            alignment: crate::layout::Alignment::CENTER,
            center_slice: None,
            repeat: ImageRepeat::NoRepeat,
            opacity: 1.0,
            color_filter: None,
            invert_colors: false,
            match_text_direction: false,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::styling::LinearGradient;

    #[test]
    fn test_box_decoration_new() {
        let decoration = BoxDecoration::new();
        assert!(decoration.color.is_none());
        assert!(decoration.border.is_none());
        assert!(decoration.gradient.is_none());
    }

    #[test]
    fn test_box_decoration_with_color() {
        let decoration = BoxDecoration::with_color(Color::RED);
        assert_eq!(decoration.color, Some(Color::RED));
    }

    #[test]
    fn test_box_decoration_with_gradient() {
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![
            Color::RED,
            Color::BLUE,
        ]));
        let decoration = BoxDecoration::with_gradient(gradient.clone());
        assert_eq!(decoration.gradient, Some(gradient));
    }

    #[test]
    fn test_box_decoration_builder_pattern() {
        let decoration = BoxDecoration::new()
            .set_color(Some(Color::WHITE))
            .set_border_radius(Some(BorderRadius::circular(10.0)));

        assert_eq!(decoration.color, Some(Color::WHITE));
        assert_eq!(decoration.border_radius, Some(BorderRadius::circular(10.0)));
    }

    #[test]
    fn test_box_decoration_lerp_colors() {
        let a = BoxDecoration::with_color(Color::BLACK);
        let b = BoxDecoration::with_color(Color::WHITE);

        let mid = BoxDecoration::lerp(&a, &b, 0.5);
        assert!(mid.color.is_some());
    }

    #[test]
    fn test_box_decoration_is_complex() {
        let simple = BoxDecoration::with_color(Color::RED);
        assert!(!simple.is_complex());

        let gradient = BoxDecoration::with_gradient(Gradient::Linear(
            LinearGradient::horizontal(vec![Color::RED, Color::BLUE]),
        ));
        assert!(gradient.is_complex());

        let with_shadow = BoxDecoration::new()
            .set_box_shadow(Some(vec![BoxShadow::default()]));
        assert!(with_shadow.is_complex());
    }

    #[test]
    fn test_decoration_image_new() {
        let image = DecorationImage::new(BoxFit::Cover, crate::layout::Alignment::CENTER);
        assert_eq!(image.fit, BoxFit::Cover);
        assert_eq!(image.opacity, 1.0);
    }

    #[test]
    fn test_decoration_image_default() {
        let image = DecorationImage::default();
        assert_eq!(image.fit, BoxFit::Contain);
        assert_eq!(image.repeat, ImageRepeat::NoRepeat);
    }

    #[test]
    fn test_box_fit_variants() {
        assert_eq!(BoxFit::default(), BoxFit::Contain);
        assert_ne!(BoxFit::Fill, BoxFit::Cover);
    }

    #[test]
    fn test_image_repeat_variants() {
        assert_eq!(ImageRepeat::default(), ImageRepeat::NoRepeat);
        assert_ne!(ImageRepeat::Repeat, ImageRepeat::RepeatX);
    }

    #[test]
    fn test_color_filter_mode() {
        let filter = ColorFilter::Mode {
            color: Color::RED,
            blend_mode: BlendMode::Multiply,
        };

        match filter {
            ColorFilter::Mode { color, blend_mode } => {
                assert_eq!(color, Color::RED);
                assert_eq!(blend_mode, BlendMode::Multiply);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_blend_mode_variants() {
        assert_ne!(BlendMode::SrcOver, BlendMode::DstOver);
        assert_ne!(BlendMode::Multiply, BlendMode::Screen);
    }
}
