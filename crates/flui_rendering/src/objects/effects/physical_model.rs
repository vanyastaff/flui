//! RenderPhysicalModel - Material Design elevation with shadow

use flui_core::render::{BoxProtocol, LayoutContext, PaintContext};
use flui_core::render::{Optional, RenderBox};
use flui_painting::{Canvas, Paint};
use flui_types::{painting::Path, Color, Point, RRect, Rect, Size};

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
/// # Without Child
///
/// When no child is present, still renders the physical shape with shadow (decorative).
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

impl RenderBox<Optional> for RenderPhysicalModel {
    fn layout(&mut self, ctx: LayoutContext<'_, Optional, BoxProtocol>) -> Size {
        let constraints = ctx.constraints;

        let size = if let Some(child_id) = ctx.children.get() {
            // Layout child and use its size
            ctx.layout_child(child_id, constraints)
        } else {
            // No child - use max constraints for shape size
            Size::new(constraints.max_width, constraints.max_height)
        };

        // Store size for paint
        self.size = size;

        size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Optional>) {
        let offset = ctx.offset;

        let size = self.size;

        // Draw shadow if elevation > 0
        // Note: For proper shadow rendering, we would need to use a more sophisticated
        // shadow algorithm. For now, we use Canvas::draw_shadow which provides basic support.
        if self.elevation > 0.0 {
            let shadow_path = match self.shape {
                PhysicalShape::Rectangle => {
                    let mut path = Path::new();
                    let rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);
                    path.add_rect(rect);
                    path
                }
                PhysicalShape::RoundedRectangle | PhysicalShape::Circle => {
                    // For rounded shapes, approximate with a simple rect for shadow
                    // A full implementation would use Path::add_rrect() when available
                    let mut path = Path::new();
                    let rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);
                    path.add_rect(rect);
                    path
                }
            };

            ctx.canvas()
                .draw_shadow(&shadow_path, self.shadow_color, self.elevation);
        }

        // Paint background shape at the offset position
        let paint = Paint::fill(self.color);

        match self.shape {
            PhysicalShape::Rectangle => {
                let rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);
                ctx.canvas().draw_rect(rect, &paint);
            }
            PhysicalShape::RoundedRectangle => {
                let radius = flui_types::styling::Radius::circular(self.border_radius);
                let rrect = RRect::from_rect_and_radius(
                    Rect::from_xywh(offset.dx, offset.dy, size.width, size.height),
                    radius,
                );
                ctx.canvas().draw_rrect(rrect, &paint);
            }
            PhysicalShape::Circle => {
                let radius = size.width.min(size.height) / 2.0;
                let center =
                    Point::new(offset.dx + size.width / 2.0, offset.dy + size.height / 2.0);
                ctx.canvas().draw_circle(center, radius, &paint);
            }
        }

        // Paint child on top at same offset if present
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(child_id, offset);
        }
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
    }
}
