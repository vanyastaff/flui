//! PhysicalModel widget - Material Design elevation with shadow
//!
//! A widget that renders Material Design elevation effects with shadows.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, RenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::{PhysicalShape, RenderPhysicalModel};
use flui_types::Color;

/// A widget that renders Material Design elevation with shadow.
///
/// PhysicalModel creates a physical layer effect with shadow based on elevation.
/// Higher elevation values create larger, softer shadows, following Material Design
/// guidelines.
///
/// ## Key Properties
///
/// - **shape**: Shape of the physical model (Rectangle, RoundedRectangle, Circle)
/// - **elevation**: Elevation above parent (affects shadow depth)
/// - **color**: Color of the model
/// - **border_radius**: Border radius for rounded rectangles (default: 0.0)
/// - **shadow_color**: Shadow color (default: semi-transparent black)
/// - **child**: The child widget
///
/// ## Common Use Cases
///
/// ### Elevated card
/// ```rust,ignore
/// PhysicalModel::rounded_rectangle(8.0, 4.0, Color::WHITE, child)
/// ```
///
/// ### Circular elevated button
/// ```rust,ignore
/// PhysicalModel::circle(4.0, Color::BLUE, icon)
/// ```
///
/// ### Custom shadow color
/// ```rust,ignore
/// PhysicalModel::builder()
///     .shape(PhysicalShape::Rectangle)
///     .elevation(12.0)
///     .color(Color::WHITE)
///     .shadow_color(Color::rgba(0, 0, 255, 100))
///     .child(content)
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Elevated card
/// PhysicalModel::rounded_rectangle(8.0, 4.0, Color::WHITE, child)
///
/// // Flat surface
/// PhysicalModel::rectangle(0.0, Color::WHITE, child)
///
/// // Custom configuration
/// PhysicalModel::builder()
///     .shape(PhysicalShape::RoundedRectangle)
///     .elevation(16.0)
///     .border_radius(8.0)
///     .color(Color::rgb(240, 240, 240))
///     .child(widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct PhysicalModel {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Shape of the physical model
    /// Default: PhysicalShape::Rectangle
    #[builder(default = PhysicalShape::Rectangle)]
    pub shape: PhysicalShape,

    /// Border radius (for rounded rectangle)
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub border_radius: f32,

    /// Elevation above parent (affects shadow)
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub elevation: f32,

    /// Color of the model
    /// Default: Color::WHITE
    #[builder(default = Color::WHITE)]
    pub color: Color,

    /// Shadow color
    /// Default: semi-transparent black
    #[builder(default = Color::rgba(0, 0, 0, 128))]
    pub shadow_color: Color,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for PhysicalModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicalModel")
            .field("key", &self.key)
            .field("shape", &self.shape)
            .field("border_radius", &self.border_radius)
            .field("elevation", &self.elevation)
            .field("color", &self.color)
            .field("shadow_color", &self.shadow_color)
            .field(
                "child",
                &if self.child.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for PhysicalModel {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            shape: self.shape,
            border_radius: self.border_radius,
            elevation: self.elevation,
            color: self.color,
            shadow_color: self.shadow_color,
            child: self.child.clone(),
        }
    }
}

impl PhysicalModel {
    /// Creates a new rectangular PhysicalModel.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let model = PhysicalModel::rectangle(8.0, Color::WHITE, child);
    /// ```
    pub fn rectangle(elevation: f32, color: Color, child: impl View + 'static) -> Self {
        Self {
            key: None,
            shape: PhysicalShape::Rectangle,
            border_radius: 0.0,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            child: Some(Box::new(child)),
        }
    }

    /// Creates a new rounded rectangle PhysicalModel.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let model = PhysicalModel::rounded_rectangle(8.0, 4.0, Color::WHITE, child);
    /// ```
    pub fn rounded_rectangle(
        elevation: f32,
        border_radius: f32,
        color: Color,
        child: impl View + 'static,
    ) -> Self {
        Self {
            key: None,
            shape: PhysicalShape::RoundedRectangle,
            border_radius,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            child: Some(Box::new(child)),
        }
    }

    /// Creates a new circular PhysicalModel.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let model = PhysicalModel::circle(4.0, Color::BLUE, icon);
    /// ```
    pub fn circle(elevation: f32, color: Color, child: impl View + 'static) -> Self {
        Self {
            key: None,
            shape: PhysicalShape::Circle,
            border_radius: 0.0,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            child: Some(Box::new(child)),
        }
    }
}

impl Default for PhysicalModel {
    fn default() -> Self {
        Self {
            key: None,
            shape: PhysicalShape::Rectangle,
            border_radius: 0.0,
            elevation: 0.0,
            color: Color::WHITE,
            shadow_color: Color::rgba(0, 0, 0, 128),
            child: None,
        }
    }
}

// bon Builder Extensions
use physical_model_builder::{IsUnset, SetChild, State};

impl<S: State> PhysicalModelBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> PhysicalModelBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper
impl<S: State> PhysicalModelBuilder<S> {
    /// Builds the PhysicalModel widget.
    pub fn build(self) -> PhysicalModel {
        self.build_internal()
    }
}

// Implement View trait
impl View for PhysicalModel {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create RenderPhysicalModel with custom properties
        let mut render = RenderPhysicalModel::new(self.shape, self.elevation, self.color);
        render.border_radius = self.border_radius;
        render.shadow_color = self.shadow_color;

        RenderBuilder::single(render).with_optional_child(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_model_rectangle() {
        let model = PhysicalModel::rectangle(8.0, Color::WHITE, crate::SizedBox::new());
        assert_eq!(model.shape, PhysicalShape::Rectangle);
        assert_eq!(model.elevation, 8.0);
    }

    #[test]
    fn test_physical_model_rounded_rectangle() {
        let model =
            PhysicalModel::rounded_rectangle(8.0, 4.0, Color::WHITE, crate::SizedBox::new());
        assert_eq!(model.shape, PhysicalShape::RoundedRectangle);
        assert_eq!(model.border_radius, 4.0);
    }

    #[test]
    fn test_physical_model_circle() {
        let model = PhysicalModel::circle(4.0, Color::BLUE, crate::SizedBox::new());
        assert_eq!(model.shape, PhysicalShape::Circle);
    }

    #[test]
    fn test_physical_model_builder() {
        let model = PhysicalModel::builder()
            .shape(PhysicalShape::RoundedRectangle)
            .elevation(12.0)
            .border_radius(8.0)
            .build();
        assert_eq!(model.elevation, 12.0);
    }

    #[test]
    fn test_physical_model_default() {
        let model = PhysicalModel::default();
        assert_eq!(model.shape, PhysicalShape::Rectangle);
        assert_eq!(model.elevation, 0.0);
    }
}
