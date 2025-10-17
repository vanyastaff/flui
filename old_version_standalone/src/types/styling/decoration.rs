//! Decoration types for styling widgets
//!
//! This module contains types for decorating boxes with colors, gradients,
//! borders, shadows, and more - similar to Flutter's BoxDecoration.

use crate::types::core::Color;

use super::border::{Border, BorderSide};
use super::border_radius::BorderRadius;
use super::gradient::Gradient;
use super::shadow::BoxShadow;

/// How to paint a box.
///
/// A decoration describes how to draw a box, including its color, gradient,
/// border, border radius, and shadows.
///
/// Similar to Flutter's `BoxDecoration`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoxDecoration {
    /// The color to fill the box with.
    ///
    /// If both color and gradient are specified, the gradient takes precedence.
    pub color: Option<Color>,

    /// A gradient to use for filling the box.
    ///
    /// If both color and gradient are specified, the gradient takes precedence.
    pub gradient: Option<Gradient>,

    /// A border to draw around the box.
    pub border: Option<Border>,

    /// The border radius for the box.
    pub border_radius: Option<BorderRadius>,

    /// A list of shadows cast by the box.
    pub box_shadows: Vec<BoxShadow>,

    // TODO: Re-enable when BoxFit is re-added
    // /// An image to paint inside the box.
    // ///
    // /// This is represented as a texture ID for egui integration.
    // pub image: Option<egui::TextureId>,
    //
    // /// How the image should be inscribed into the box.
    // pub image_fit: crate::types::layout::layout::BoxFit,
}

impl BoxDecoration {
    /// Create a new empty decoration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a decoration with just a color.
    pub fn from_color(color: impl Into<Color>) -> Self {
        Self {
            color: Some(color.into()),
            ..Default::default()
        }
    }

    /// Create a decoration with a gradient.
    pub fn from_gradient(gradient: Gradient) -> Self {
        Self {
            gradient: Some(gradient),
            ..Default::default()
        }
    }

    /// Create a decoration with a border.
    pub fn from_border(border: Border) -> Self {
        Self {
            border: Some(border),
            ..Default::default()
        }
    }

    /// Set the background color.
    pub fn with_color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set the background gradient.
    pub fn with_gradient(mut self, gradient: Gradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// Set the border.
    pub fn with_border(mut self, border: Border) -> Self {
        self.border = Some(border);
        self
    }

    /// Set the border radius.
    pub fn with_border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = Some(border_radius);
        self
    }

    /// Add a single box shadow.
    pub fn with_shadow(mut self, shadow: BoxShadow) -> Self {
        self.box_shadows.push(shadow);
        self
    }

    /// Set multiple box shadows.
    pub fn with_shadows(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.box_shadows = shadows;
        self
    }

    // TODO: Re-enable when BoxFit is re-added
    // /// Set the background image.
    // pub fn with_image(mut self, image: egui::TextureId, fit: crate::types::layout::layout::BoxFit) -> Self {
    //     self.image = Some(image);
    //     self.image_fit = fit;
    //     self
    // }

    /// Check if this decoration has any visible effects.
    pub fn is_visible(&self) -> bool {
        self.color.is_some()
            || self.gradient.is_some()
            || self.border.is_some()
            || !self.box_shadows.is_empty()
            // || self.image.is_some()  // TODO: Re-enable when BoxFit is re-added
    }

    /// Check if this decoration has a background (color or gradient).
    pub fn has_background(&self) -> bool {
        self.color.is_some() || self.gradient.is_some()
    }

    /// Check if this decoration has a border.
    pub fn has_border(&self) -> bool {
        self.border.is_some()
    }

    /// Check if this decoration has shadows.
    pub fn has_shadows(&self) -> bool {
        !self.box_shadows.is_empty()
    }

    /// Get the background color if set (gradient takes precedence).
    pub fn background_color(&self) -> Option<Color> {
        if self.gradient.is_some() {
            None
        } else {
            self.color
        }
    }

    /// Calculate the total padding needed for the decoration's border.
    pub fn border_padding(&self) -> crate::types::layout::edge_insets::EdgeInsets {
        self.border
            .as_ref()
            .map(|b| {
                crate::types::layout::edge_insets::EdgeInsets::new(
                    b.left.width,
                    b.top.width,
                    b.right.width,
                    b.bottom.width,
                )
            })
            .unwrap_or(crate::types::layout::edge_insets::EdgeInsets::ZERO)
    }

    /// Scale the decoration (useful for animations).
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            color: self.color,
            gradient: self.gradient.clone(),
            border: self.border.map(|b| b * factor),
            border_radius: self.border_radius.map(|r| r * factor),
            box_shadows: self.box_shadows.iter().map(|s| *s * factor).collect(),
            // TODO: Re-enable when BoxFit is re-added
            // image: self.image,
            // image_fit: self.image_fit,
        }
    }
}

impl From<Color> for BoxDecoration {
    fn from(color: Color) -> Self {
        Self::from_color(color)
    }
}

impl From<Gradient> for BoxDecoration {
    fn from(gradient: Gradient) -> Self {
        Self::from_gradient(gradient)
    }
}

impl From<Border> for BoxDecoration {
    fn from(border: Border) -> Self {
        Self::from_border(border)
    }
}

/// How to paint a shape.
///
/// A simpler alternative to BoxDecoration for shapes.
///
/// Similar to Flutter's `ShapeDecoration`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeDecoration {
    /// The color to fill the shape with.
    pub color: Option<Color>,

    /// A gradient to use for filling the shape.
    pub gradient: Option<Gradient>,

    /// A list of shadows cast by the shape.
    pub shadows: Vec<BoxShadow>,
}

impl ShapeDecoration {
    /// Create a new empty shape decoration.
    pub fn new() -> Self {
        Self {
            color: None,
            gradient: None,
            shadows: Vec::new(),
        }
    }

    /// Create a shape decoration with just a color.
    pub fn from_color(color: impl Into<Color>) -> Self {
        Self {
            color: Some(color.into()),
            gradient: None,
            shadows: Vec::new(),
        }
    }

    /// Create a shape decoration with a gradient.
    pub fn from_gradient(gradient: Gradient) -> Self {
        Self {
            color: None,
            gradient: Some(gradient),
            shadows: Vec::new(),
        }
    }

    /// Set the fill color.
    pub fn with_color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set the fill gradient.
    pub fn with_gradient(mut self, gradient: Gradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// Add a shadow.
    pub fn with_shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    /// Set multiple shadows.
    pub fn with_shadows(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.shadows = shadows;
        self
    }

    /// Check if this decoration has any visible effects.
    pub fn is_visible(&self) -> bool {
        self.color.is_some() || self.gradient.is_some() || !self.shadows.is_empty()
    }

    /// Get the fill color if set (gradient takes precedence).
    pub fn fill_color(&self) -> Option<Color> {
        if self.gradient.is_some() {
            None
        } else {
            self.color
        }
    }
}

impl Default for ShapeDecoration {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Color> for ShapeDecoration {
    fn from(color: Color) -> Self {
        Self::from_color(color)
    }
}

impl From<Gradient> for ShapeDecoration {
    fn from(gradient: Gradient) -> Self {
        Self::from_gradient(gradient)
    }
}

/// Common decoration helpers.
pub struct DecorationPresets;

impl DecorationPresets {
    /// Create a card-like decoration with elevation.
    pub fn card(elevation: f32) -> BoxDecoration {
        let (key_shadow, ambient_shadow) = BoxShadow::elevation_shadows(elevation);
        BoxDecoration::new()
            .with_color(Color::WHITE)
            .with_border_radius(BorderRadius::circular(8.0))
            .with_shadows(vec![key_shadow, ambient_shadow])
    }

    /// Create a button-like decoration.
    pub fn button(color: impl Into<Color>, elevation: f32) -> BoxDecoration {
        let (key_shadow, ambient_shadow) = BoxShadow::elevation_shadows(elevation);
        BoxDecoration::new()
            .with_color(color)
            .with_border_radius(BorderRadius::circular(4.0))
            .with_shadows(vec![key_shadow, ambient_shadow])
    }

    /// Create an outlined decoration.
    pub fn outlined(color: impl Into<Color>, width: f32) -> BoxDecoration {
        BoxDecoration::new()
            .with_border(Border::uniform(color, width))
            .with_border_radius(BorderRadius::circular(4.0))
    }

    /// Create a circular decoration.
    pub fn circle(color: impl Into<Color>) -> BoxDecoration {
        BoxDecoration::new()
            .with_color(color)
            .with_border_radius(BorderRadius::circular(9999.0))
    }

    /// Create a pill-shaped decoration.
    pub fn pill(color: impl Into<Color>) -> BoxDecoration {
        BoxDecoration::new()
            .with_color(color)
            .with_border_radius(BorderRadius::circular(9999.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{core::Offset, styling::gradient::LinearGradient};

    #[test]
    fn test_box_decoration_creation() {
        let empty = BoxDecoration::new();
        assert!(!empty.is_visible());
        assert!(!empty.has_background());
        assert!(!empty.has_border());
        assert!(!empty.has_shadows());

        let with_color = BoxDecoration::from_color(Color::RED);
        assert!(with_color.is_visible());
        assert!(with_color.has_background());
        assert_eq!(with_color.background_color(), Some(Color::RED));
    }

    #[test]
    fn test_box_decoration_builder() {
        let decoration = BoxDecoration::new()
            .with_color(Color::RED)
            .with_border(Border::uniform(Color::BLACK, 2.0))
            .with_border_radius(BorderRadius::circular(8.0))
            .with_shadow(BoxShadow::elevation(4.0, Color::from_rgba(0, 0, 0, 50)));

        assert!(decoration.is_visible());
        assert!(decoration.has_background());
        assert!(decoration.has_border());
        assert!(decoration.has_shadows());
        assert_eq!(decoration.box_shadows.len(), 1);
    }

    #[test]
    fn test_box_decoration_gradient_precedence() {
        let gradient = Gradient::Linear(LinearGradient::horizontal(Color::RED, Color::BLUE));

        let decoration = BoxDecoration::new()
            .with_color(Color::GREEN) // This should be ignored
            .with_gradient(gradient.clone());

        assert_eq!(decoration.background_color(), None); // Gradient takes precedence
        assert!(decoration.gradient.is_some());
    }

    #[test]
    fn test_box_decoration_border_padding() {
        let decoration = BoxDecoration::new()
            .with_border(Border::new(
                BorderSide::solid(Color::BLACK, 1.0),
                BorderSide::solid(Color::BLACK, 2.0),
                BorderSide::solid(Color::BLACK, 3.0),
                BorderSide::solid(Color::BLACK, 4.0),
            ));

        let padding = decoration.border_padding();
        assert_eq!(padding.top, 1.0);
        assert_eq!(padding.right, 2.0);
        assert_eq!(padding.bottom, 3.0);
        assert_eq!(padding.left, 4.0);

        let no_border = BoxDecoration::new();
        let zero_padding = no_border.border_padding();
        assert!(zero_padding.is_zero());
    }

    #[test]
    fn test_box_decoration_scale() {
        let decoration = BoxDecoration::new()
            .with_border(Border::uniform(Color::BLACK, 2.0))
            .with_border_radius(BorderRadius::circular(4.0))
            .with_shadow(BoxShadow::simple(
                Color::BLACK,
                Offset::new(2.0, 2.0),
                4.0,
            ));

        let scaled = decoration.scale(2.0);

        if let Some(border) = scaled.border {
            assert_eq!(border.top.width, 4.0);
        }

        if let Some(radius) = scaled.border_radius {
            assert_eq!(radius.top_left.x, 8.0);
        }

        assert_eq!(scaled.box_shadows[0].blur_radius, 8.0);
    }

    #[test]
    fn test_box_decoration_conversions() {
        let from_color: BoxDecoration = Color::RED.into();
        assert_eq!(from_color.color, Some(Color::RED));

        let gradient = Gradient::Linear(LinearGradient::horizontal(Color::RED, Color::BLUE));
        let from_gradient: BoxDecoration = gradient.clone().into();
        assert!(from_gradient.gradient.is_some());

        let border = Border::uniform(Color::BLACK, 2.0);
        let from_border: BoxDecoration = border.into();
        assert!(from_border.border.is_some());
    }

    #[test]
    fn test_shape_decoration_creation() {
        let empty = ShapeDecoration::new();
        assert!(!empty.is_visible());

        let with_color = ShapeDecoration::from_color(Color::BLUE);
        assert!(with_color.is_visible());
        assert_eq!(with_color.fill_color(), Some(Color::BLUE));
    }

    #[test]
    fn test_shape_decoration_builder() {
        let decoration = ShapeDecoration::new()
            .with_color(Color::GREEN)
            .with_shadow(BoxShadow::elevation(2.0, Color::from_rgba(0, 0, 0, 30)));

        assert!(decoration.is_visible());
        assert_eq!(decoration.color, Some(Color::GREEN));
        assert_eq!(decoration.shadows.len(), 1);
    }

    #[test]
    fn test_shape_decoration_gradient_precedence() {
        let gradient = Gradient::Linear(LinearGradient::vertical(Color::RED, Color::YELLOW));

        let decoration = ShapeDecoration::new()
            .with_color(Color::GREEN) // This should be ignored
            .with_gradient(gradient.clone());

        assert_eq!(decoration.fill_color(), None); // Gradient takes precedence
        assert!(decoration.gradient.is_some());
    }

    #[test]
    fn test_decoration_presets() {
        let card = DecorationPresets::card(4.0);
        assert!(card.has_background());
        assert!(card.has_shadows());
        assert!(card.border_radius.is_some());

        let button = DecorationPresets::button(Color::BLUE, 2.0);
        assert!(button.has_background());
        assert!(button.has_shadows());

        let outlined = DecorationPresets::outlined(Color::BLACK, 1.0);
        assert!(outlined.has_border());
        assert!(outlined.border_radius.is_some());

        let circle = DecorationPresets::circle(Color::RED);
        assert!(circle.has_background());
        assert!(circle.border_radius.is_some());

        let pill = DecorationPresets::pill(Color::GREEN);
        assert!(pill.has_background());
        assert!(pill.border_radius.is_some());
    }
}
