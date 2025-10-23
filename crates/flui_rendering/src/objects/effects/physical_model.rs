//! RenderPhysicalModel - Material Design elevation with shadow

use flui_types::{Offset, Size, constraints::BoxConstraints, Color};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Shape for physical model
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalShape {
    /// Rectangle
    Rectangle,
    /// Rounded rectangle with border radius
    RoundedRectangle,
    /// Circle
    Circle,
}

/// Data for RenderPhysicalModel
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalModelData {
    /// Shape of the physical model
    pub shape: PhysicalShape,
    /// Border radius (for rounded rectangle)
    pub border_radius: f32,
    /// Elevation above parent (affects shadow)
    pub elevation: f32,
    /// Color of the model
    pub color: Color,
    /// Shadow color
    pub shadow_color: Color,
}

impl PhysicalModelData {
    /// Create new physical model data
    pub fn new(shape: PhysicalShape, elevation: f32, color: Color) -> Self {
        Self {
            shape,
            border_radius: 0.0,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
        }
    }

    /// Create rectangular model
    pub fn rectangle(elevation: f32, color: Color) -> Self {
        Self::new(PhysicalShape::Rectangle, elevation, color)
    }

    /// Create rounded rectangle model
    pub fn rounded_rectangle(elevation: f32, border_radius: f32, color: Color) -> Self {
        Self {
            shape: PhysicalShape::RoundedRectangle,
            border_radius,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
        }
    }

    /// Create circular model
    pub fn circle(elevation: f32, color: Color) -> Self {
        Self::new(PhysicalShape::Circle, elevation, color)
    }

    /// Set shadow color
    pub fn with_shadow_color(mut self, shadow_color: Color) -> Self {
        self.shadow_color = shadow_color;
        self
    }
}

impl Default for PhysicalModelData {
    fn default() -> Self {
        Self::rectangle(0.0, Color::WHITE)
    }
}

/// RenderObject that renders Material Design elevation with shadow
///
/// Creates a physical layer effect with shadow based on elevation.
/// Higher elevation values create larger, softer shadows.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::PhysicalModelData};
/// use flui_types::Color;
///
/// // Create elevated card with rounded corners
/// let mut card = SingleRenderBox::new(
///     PhysicalModelData::rounded_rectangle(4.0, 8.0, Color::WHITE)
/// );
/// ```
pub type RenderPhysicalModel = SingleRenderBox<PhysicalModelData>;

// ===== Public API =====

impl RenderPhysicalModel {
    /// Get shape
    pub fn shape(&self) -> PhysicalShape {
        self.data().shape
    }

    /// Get elevation
    pub fn elevation(&self) -> f32 {
        self.data().elevation
    }

    /// Get color
    pub fn color(&self) -> Color {
        self.data().color
    }

    /// Get shadow color
    pub fn shadow_color(&self) -> Color {
        self.data().shadow_color
    }

    /// Set shape
    pub fn set_shape(&mut self, shape: PhysicalShape) {
        if self.data().shape != shape {
            self.data_mut().shape = shape;
            self.mark_needs_paint();
        }
    }

    /// Set elevation
    pub fn set_elevation(&mut self, elevation: f32) {
        if self.data().elevation != elevation {
            self.data_mut().elevation = elevation;
            self.mark_needs_paint();
        }
    }

    /// Set color
    pub fn set_color(&mut self, color: Color) {
        if self.data().color != color {
            self.data_mut().color = color;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderPhysicalModel {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        let size = self.state().size.unwrap_or(Size::ZERO);
        let elevation = self.data().elevation;
        let color = self.data().color;

        // Create rect for background
        let rect = egui::Rect::from_min_size(
            egui::pos2(offset.dx, offset.dy),
            egui::vec2(size.width, size.height),
        );

        // Paint shadow if elevation > 0
        if elevation > 0.0 {
            // TODO: Paint Material Design shadow
            // Shadow properties based on elevation:
            // - blur_radius = elevation * 0.5
            // - spread_radius = elevation * 0.25
            // - offset = (0, elevation * 0.5)
            //
            // For now, we skip shadow painting
            // A real implementation would:
            // 1. Calculate shadow parameters from elevation
            // 2. Paint shadow using BoxShadow or custom shape
            // 3. Support different shadow styles (key light, ambient)
        }

        // Paint background shape
        let egui_color = egui::Color32::from_rgba_unmultiplied(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        );

        match self.data().shape {
            PhysicalShape::Rectangle => {
                painter.rect_filled(rect, 0.0, egui_color);
            }
            PhysicalShape::RoundedRectangle => {
                let radius = self.data().border_radius;
                painter.rect_filled(rect, radius, egui_color);
            }
            PhysicalShape::Circle => {
                let center = rect.center();
                let radius = size.width.min(size.height) / 2.0;
                painter.circle_filled(center, radius, egui_color);
            }
        }

        // Paint child on top
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_shape_variants() {
        assert_ne!(PhysicalShape::Rectangle, PhysicalShape::Circle);
        assert_ne!(PhysicalShape::RoundedRectangle, PhysicalShape::Circle);
    }

    #[test]
    fn test_physical_model_data_new() {
        let data = PhysicalModelData::new(
            PhysicalShape::Rectangle,
            4.0,
            Color::WHITE,
        );
        assert_eq!(data.shape, PhysicalShape::Rectangle);
        assert_eq!(data.elevation, 4.0);
        assert_eq!(data.color, Color::WHITE);
    }

    #[test]
    fn test_physical_model_data_rectangle() {
        let data = PhysicalModelData::rectangle(2.0, Color::rgb(255, 0, 0));
        assert_eq!(data.shape, PhysicalShape::Rectangle);
        assert_eq!(data.elevation, 2.0);
    }

    #[test]
    fn test_physical_model_data_rounded_rectangle() {
        let data = PhysicalModelData::rounded_rectangle(4.0, 8.0, Color::WHITE);
        assert_eq!(data.shape, PhysicalShape::RoundedRectangle);
        assert_eq!(data.border_radius, 8.0);
        assert_eq!(data.elevation, 4.0);
    }

    #[test]
    fn test_physical_model_data_circle() {
        let data = PhysicalModelData::circle(6.0, Color::BLUE);
        assert_eq!(data.shape, PhysicalShape::Circle);
        assert_eq!(data.elevation, 6.0);
    }

    #[test]
    fn test_physical_model_data_with_shadow_color() {
        let data = PhysicalModelData::rectangle(4.0, Color::WHITE)
            .with_shadow_color(Color::rgba(0, 0, 0, 64));
        assert_eq!(data.shadow_color, Color::rgba(0, 0, 0, 64));
    }

    #[test]
    fn test_render_physical_model_new() {
        let model = SingleRenderBox::new(PhysicalModelData::rectangle(4.0, Color::WHITE));
        assert_eq!(model.shape(), PhysicalShape::Rectangle);
        assert_eq!(model.elevation(), 4.0);
        assert_eq!(model.color(), Color::WHITE);
    }

    #[test]
    fn test_render_physical_model_set_elevation() {
        let mut model = SingleRenderBox::new(PhysicalModelData::rectangle(2.0, Color::WHITE));

        model.set_elevation(8.0);
        assert_eq!(model.elevation(), 8.0);
        assert!(model.needs_paint());
    }

    #[test]
    fn test_render_physical_model_set_color() {
        let mut model = SingleRenderBox::new(PhysicalModelData::rectangle(4.0, Color::WHITE));

        model.set_color(Color::RED);
        assert_eq!(model.color(), Color::RED);
        assert!(model.needs_paint());
    }

    #[test]
    fn test_render_physical_model_layout() {
        let mut model = SingleRenderBox::new(PhysicalModelData::rectangle(4.0, Color::WHITE));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = model.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
