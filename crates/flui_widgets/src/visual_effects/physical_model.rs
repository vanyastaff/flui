//! PhysicalModel widget - Material Design elevation with shadow
//!
//! A widget that renders Material Design elevation effects with shadows.

use bon::Builder;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::{BuildContext, RenderNode};
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
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_physical_model)]
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
    pub child: Option<Widget>,
}

impl PhysicalModel {
    /// Creates a new rectangular PhysicalModel.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let model = PhysicalModel::rectangle(8.0, Color::WHITE, child);
    /// ```
    pub fn rectangle(elevation: f32, color: Color, child: Widget) -> Self {
        Self {
            key: None,
            shape: PhysicalShape::Rectangle,
            border_radius: 0.0,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            child: Some(child),
        }
    }

    /// Creates a new rounded rectangle PhysicalModel.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let model = PhysicalModel::rounded_rectangle(8.0, 4.0, Color::WHITE, child);
    /// ```
    pub fn rounded_rectangle(elevation: f32, border_radius: f32, color: Color, child: Widget) -> Self {
        Self {
            key: None,
            shape: PhysicalShape::RoundedRectangle,
            border_radius,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            child: Some(child),
        }
    }

    /// Creates a new circular PhysicalModel.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let model = PhysicalModel::circle(4.0, Color::BLUE, icon);
    /// ```
    pub fn circle(elevation: f32, color: Color, child: Widget) -> Self {
        Self {
            key: None,
            shape: PhysicalShape::Circle,
            border_radius: 0.0,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            child: Some(child),
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
    pub fn child(self, child: impl flui_core::IntoWidget) -> PhysicalModelBuilder<SetChild<S>> {
        self.child_internal(child.into_widget())
    }
}

impl<S: State> PhysicalModelBuilder<S> {
    /// Builds the PhysicalModel widget.
    pub fn build(self) -> Widget {
        Widget::render_object(self.build_physical_model())
    }
}

// Implement RenderWidget
impl RenderWidget for PhysicalModel {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let mut render = RenderPhysicalModel::new(self.shape, self.elevation, self.color);
        render.border_radius = self.border_radius;
        render.shadow_color = self.shadow_color;
        RenderNode::single(Box::new(render))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(physical_model) = render.downcast_mut::<RenderPhysicalModel>() {
                physical_model.set_shape(self.shape);
                physical_model.border_radius = self.border_radius;
                physical_model.set_elevation(self.elevation);
                physical_model.set_color(self.color);
                physical_model.shadow_color = self.shadow_color;
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(PhysicalModel, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_model_rectangle() {
        let child = Widget::from(());
        let model = PhysicalModel::rectangle(8.0, Color::WHITE, child);
        assert_eq!(model.shape, PhysicalShape::Rectangle);
        assert_eq!(model.elevation, 8.0);
    }

    #[test]
    fn test_physical_model_rounded_rectangle() {
        let child = Widget::from(());
        let model = PhysicalModel::rounded_rectangle(8.0, 4.0, Color::WHITE, child);
        assert_eq!(model.shape, PhysicalShape::RoundedRectangle);
        assert_eq!(model.border_radius, 4.0);
    }

    #[test]
    fn test_physical_model_circle() {
        let child = Widget::from(());
        let model = PhysicalModel::circle(4.0, Color::BLUE, child);
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
