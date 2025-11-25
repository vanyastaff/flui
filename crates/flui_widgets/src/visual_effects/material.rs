//! Material widget - Material Design surface with elevation
//!
//! A widget that provides Material Design visual structure with elevation,
//! shadows, and shape.

use bon::Builder;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_types::styling::BorderRadius;
use flui_types::Color;

use crate::PhysicalModel;

/// Material Design surface widget.
///
/// Material provides the visual foundation for Material Design components.
/// It handles elevation (shadows), shape (rounded corners), and color.
///
/// ## Key Properties
///
/// - **elevation**: Shadow depth following Material Design guidelines
/// - **color**: Surface color
/// - **border_radius**: Corner radius for rounded shapes
/// - **shadow_color**: Custom shadow color (optional)
/// - **child**: Content widget
///
/// ## Common Use Cases
///
/// ### Basic elevated surface
/// ```rust,ignore
/// Material::new(4.0, Color::WHITE, child)
/// ```
///
/// ### Custom shape and shadow
/// ```rust,ignore
/// Material::builder()
///     .elevation(8.0)
///     .color(Color::WHITE)
///     .border_radius(BorderRadius::circular(12.0))
///     .shadow_color(Color::rgba(0, 0, 255, 100))
///     .child(content)
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Standard Material surface
/// Material::new(2.0, Color::WHITE, child)
///
/// // Elevated card surface
/// Material::builder()
///     .elevation(4.0)
///     .color(Color::WHITE)
///     .border_radius(BorderRadius::circular(8.0))
///     .child(content)
///     .build()
///
/// // Flat surface (no elevation)
/// Material::new(0.0, Color::WHITE, child)
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Material {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Elevation above parent (affects shadow depth)
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub elevation: f32,

    /// Surface color
    /// Default: Color::WHITE
    #[builder(default = Color::WHITE)]
    pub color: Color,

    /// Border radius for rounded corners
    /// Default: BorderRadius::ZERO (sharp corners)
    #[builder(default = BorderRadius::ZERO)]
    pub border_radius: BorderRadius,

    /// Shadow color (optional, uses default if None)
    /// Default: None (uses semi-transparent black)
    pub shadow_color: Option<Color>,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn >>,
}

impl std::fmt::Debug for Material {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Material")
            .field("key", &self.key)
            .field("elevation", &self.elevation)
            .field("color", &self.color)
            .field("border_radius", &self.border_radius)
            .field("shadow_color", &self.shadow_color)
            .field(
                "child",
                &if self.child.is_some() {
                    "<>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for Material {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            elevation: self.elevation,
            color: self.color,
            border_radius: self.border_radius,
            shadow_color: self.shadow_color,
            child: self.child.clone(),
        }
    }
}

impl Material {
    /// Creates a new Material surface.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let material = Material::new(4.0, Color::WHITE, child);
    /// ```
    pub fn new(elevation: f32, color: Color, child: impl IntoElement) -> Self {
        Self {
            key: None,
            elevation,
            color,
            border_radius: BorderRadius::ZERO,
            shadow_color: None,
            child: Some(Box::new(child)),
        }
    }

    /// Creates a Material surface with rounded corners.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let material = Material::rounded(8.0, 4.0, Color::WHITE, child);
    /// ```
    pub fn rounded(
        elevation: f32,
        border_radius: f32,
        color: Color,
        child: impl IntoElement,
    ) -> Self {
        Self {
            key: None,
            elevation,
            color,
            border_radius: BorderRadius::circular(border_radius),
            shadow_color: None,
            child: Some(Box::new(child)),
        }
    }

    /// Creates a flat Material surface (no elevation).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let material = Material::flat(Color::WHITE, child);
    /// ```
    pub fn flat(color: Color, child: impl IntoElement) -> Self {
        Self {
            key: None,
            elevation: 0.0,
            color,
            border_radius: BorderRadius::ZERO,
            shadow_color: None,
            child: Some(Box::new(child)),
        }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self {
            key: None,
            elevation: 0.0,
            color: Color::WHITE,
            border_radius: BorderRadius::ZERO,
            shadow_color: None,
            child: None,
        }
    }
}

// bon Builder Extensions
use material_builder::{IsUnset, SetChild, State};

impl<S: State> MaterialBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl IntoElement) -> MaterialBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper
impl<S: State> MaterialBuilder<S> {
    /// Builds the Material widget.
    pub fn build(self) -> Material {
        self.build_internal()
    }
}

// Implement View trait
impl StatelessView for Material {
    fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
        // Get uniform border radius (use top_left as uniform)
        let radius = self.border_radius.top_left.x;

        // Create child widget or default
        let child = self
            .child
            .unwrap_or_else(|| Box::new(crate::SizedBox::new()));

        // Create PhysicalModel directly based on shape
        if radius > 0.0 {
            // Rounded rectangle shape
            PhysicalModel {
                key: None,
                shape: flui_rendering::PhysicalShape::RoundedRectangle,
                border_radius: radius,
                elevation: self.elevation,
                color: self.color,
                shadow_color: self
                    .shadow_color
                    .unwrap_or(flui_types::Color::rgba(0, 0, 0, 128)),
                child: Some(child),
            }
        } else {
            // Rectangle shape
            PhysicalModel {
                key: None,
                shape: flui_rendering::PhysicalShape::Rectangle,
                border_radius: 0.0,
                elevation: self.elevation,
                color: self.color,
                shadow_color: self
                    .shadow_color
                    .unwrap_or(flui_types::Color::rgba(0, 0, 0, 128)),
                child: Some(child),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_material_new() {
        let material = Material::new(4.0, Color::WHITE, crate::SizedBox::new());
        assert_eq!(material.elevation, 4.0);
        assert_eq!(material.color, Color::WHITE);
        assert!(material.child.is_some());
    }

    #[test]
    fn test_material_rounded() {
        let material = Material::rounded(8.0, 12.0, Color::WHITE, crate::SizedBox::new());
        assert_eq!(material.elevation, 8.0);
        assert_eq!(material.border_radius.top_left.x, 12.0);
    }

    #[test]
    fn test_material_flat() {
        let material = Material::flat(Color::WHITE, crate::SizedBox::new());
        assert_eq!(material.elevation, 0.0);
    }

    #[test]
    fn test_material_builder() {
        let material = Material::builder()
            .elevation(4.0)
            .color(Color::BLUE)
            .border_radius(BorderRadius::circular(8.0))
            .build();
        assert_eq!(material.elevation, 4.0);
        assert_eq!(material.color, Color::BLUE);
    }

    #[test]
    fn test_material_default() {
        let material = Material::default();
        assert_eq!(material.elevation, 0.0);
        assert_eq!(material.color, Color::WHITE);
        assert!(material.child.is_none());
    }

    #[test]
    fn test_material_with_shadow_color() {
        let material = Material::builder()
            .elevation(4.0)
            .shadow_color(Color::rgba(0, 0, 255, 100))
            .build();
        assert!(material.shadow_color.is_some());
    }
}
