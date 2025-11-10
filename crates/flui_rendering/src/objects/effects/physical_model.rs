//! RenderPhysicalModel - Material Design elevation with shadow

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::{BoxedLayer, Paint, PictureLayer};
use flui_types::{Color, Size};

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

/// RenderObject that renders Material Design elevation with shadow
///
/// Creates a physical layer effect with shadow based on elevation.
/// Higher elevation values create larger, softer shadows.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPhysicalModel;
/// use flui_types::Color;
///
/// // Create elevated card with rounded corners
/// let card = RenderPhysicalModel::rounded_rectangle(4.0, 8.0, Color::WHITE);
/// ```
#[derive(Debug)]
pub struct RenderPhysicalModel {
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

    // Cache for paint
    size: Size,
}

impl RenderPhysicalModel {
    /// Create new RenderPhysicalModel
    pub fn new(shape: PhysicalShape, elevation: f32, color: Color) -> Self {
        Self {
            shape,
            border_radius: 0.0,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            size: Size::ZERO,
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
            size: Size::ZERO,
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

    /// Set shape
    pub fn set_shape(&mut self, shape: PhysicalShape) {
        self.shape = shape;
    }

    /// Set elevation
    pub fn set_elevation(&mut self, elevation: f32) {
        self.elevation = elevation;
    }

    /// Set color
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }
}

impl Default for RenderPhysicalModel {
    fn default() -> Self {
        Self::rectangle(0.0, Color::WHITE)
    }
}

impl Render for RenderPhysicalModel {

    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // SingleArity always has exactly one child_id
        let size = tree.layout_child(child_id, constraints);

        // Store size for paint
        self.size = size;

        size
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Use pool for allocation efficiency
        let mut container = flui_engine::layer::pool::acquire_container();

        // TODO: Add shadow layer when BoxShadow layer is implemented
        // For now, skip shadow painting
        // A full implementation would:
        // 1. Calculate shadow parameters from elevation
        // 2. Create a BoxShadowLayer with appropriate blur and offset
        // 3. Add it before the background shape

        // Paint background shape at the offset position
        let mut picture = PictureLayer::new();
        let size = self.size;

        let paint = Paint::builder()
            .color(self.color)
            .build();

        match self.shape {
            PhysicalShape::Rectangle => {
                picture.draw_rect(
                    flui_types::Rect::from_xywh(offset.dx, offset.dy, size.width, size.height),
                    paint,
                );
            }
            PhysicalShape::RoundedRectangle => {
                let radius = flui_types::styling::Radius::circular(self.border_radius);
                let rrect = flui_engine::painter::RRect::from_rect_and_radius(
                    flui_types::Rect::from_xywh(offset.dx, offset.dy, size.width, size.height),
                    radius,
                );
                picture.draw_rrect(rrect, paint);
            }
            PhysicalShape::Circle => {
                let radius = size.width.min(size.height) / 2.0;
                let center = flui_types::Point::new(
                    offset.dx + size.width / 2.0,
                    offset.dy + size.height / 2.0,
                );
                picture.draw_circle(center, radius, paint);
            }
        }

        container.add_child(Box::new(picture));

        // Paint child_id on top at same offset
        let child_layer = tree.paint_child(child_id, offset);
        container.add_child(child_layer);

        Box::new(container)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable  // Default - update if needed
    }

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
    fn test_render_physical_model_new() {
        let model = RenderPhysicalModel::new(PhysicalShape::Rectangle, 4.0, Color::WHITE);
        assert_eq!(model.shape, PhysicalShape::Rectangle);
        assert_eq!(model.elevation, 4.0);
        assert_eq!(model.color, Color::WHITE);
    }

    #[test]
    fn test_render_physical_model_rectangle() {
        let model = RenderPhysicalModel::rectangle(2.0, Color::rgb(255, 0, 0));
        assert_eq!(model.shape, PhysicalShape::Rectangle);
        assert_eq!(model.elevation, 2.0);
    }

    #[test]
    fn test_render_physical_model_rounded_rectangle() {
        let model = RenderPhysicalModel::rounded_rectangle(4.0, 8.0, Color::WHITE);
        assert_eq!(model.shape, PhysicalShape::RoundedRectangle);
        assert_eq!(model.border_radius, 8.0);
        assert_eq!(model.elevation, 4.0);
    }

    #[test]
    fn test_render_physical_model_circle() {
        let model = RenderPhysicalModel::circle(6.0, Color::BLUE);
        assert_eq!(model.shape, PhysicalShape::Circle);
        assert_eq!(model.elevation, 6.0);
    }

    #[test]
    fn test_render_physical_model_with_shadow_color() {
        let model = RenderPhysicalModel::rectangle(4.0, Color::WHITE)
            .with_shadow_color(Color::rgba(0, 0, 0, 64));
        assert_eq!(model.shadow_color, Color::rgba(0, 0, 0, 64));
    }

    #[test]
    fn test_render_physical_model_set_elevation() {
        let mut model = RenderPhysicalModel::rectangle(2.0, Color::WHITE);
        model.set_elevation(8.0);
        assert_eq!(model.elevation, 8.0);
    }

    #[test]
    fn test_render_physical_model_set_color() {
        let mut model = RenderPhysicalModel::rectangle(4.0, Color::WHITE);
        model.set_color(Color::RED);
        assert_eq!(model.color, Color::RED);

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
    }
}
