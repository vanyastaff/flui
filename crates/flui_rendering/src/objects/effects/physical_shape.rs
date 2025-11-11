//! RenderPhysicalShape - Custom shape with Material Design elevation

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::{Canvas, Paint};
use flui_types::{
    painting::Path,
    Color, Size,
};

/// Clipper function that creates a custom path for the given size
pub type ShapeClipper = Box<dyn Fn(Size) -> Path + Send + Sync>;

/// RenderObject that renders arbitrary shapes with Material Design elevation
///
/// Unlike RenderPhysicalModel which supports only Rectangle/RoundedRectangle/Circle,
/// RenderPhysicalShape accepts any custom Path via a clipper function.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPhysicalShape;
/// use flui_types::{Color, painting::Path};
///
/// // Create a star shape with elevation
/// let clipper = Box::new(|size| {
///     let mut path = Path::new();
///     // ... create star shape
///     path
/// });
/// let star = RenderPhysicalShape::new(clipper, 4.0, Color::YELLOW);
/// ```
pub struct RenderPhysicalShape {
    /// Function that creates the shape path for a given size
    clipper: ShapeClipper,
    /// Elevation above parent (affects shadow)
    pub elevation: f32,
    /// Color of the shape
    pub color: Color,
    /// Shadow color
    pub shadow_color: Color,

    // Cache for paint
    size: Size,
}

impl RenderPhysicalShape {
    /// Create new RenderPhysicalShape with custom clipper
    pub fn new(clipper: ShapeClipper, elevation: f32, color: Color) -> Self {
        Self {
            clipper,
            elevation,
            color,
            shadow_color: Color::rgba(0, 0, 0, 128),
            size: Size::ZERO,
        }
    }

    /// Create with custom shadow color
    pub fn with_shadow_color(mut self, shadow_color: Color) -> Self {
        self.shadow_color = shadow_color;
        self
    }

    /// Set elevation
    pub fn set_elevation(&mut self, elevation: f32) {
        self.elevation = elevation;
    }

    /// Set color
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    /// Set shadow color
    pub fn set_shadow_color(&mut self, shadow_color: Color) {
        self.shadow_color = shadow_color;
    }

    /// Get the current shape path for the stored size
    /// Note: The clipper should return paths in local coordinates (0,0 origin)
    /// The caller is responsible for applying the offset when drawing
    fn get_shape_path(&self) -> Path {
        (self.clipper)(self.size)
    }
}

// Manual Debug implementation since closures don't implement Debug
impl std::fmt::Debug for RenderPhysicalShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderPhysicalShape")
            .field("clipper", &"<closure>")
            .field("elevation", &self.elevation)
            .field("color", &self.color)
            .field("shadow_color", &self.shadow_color)
            .field("size", &self.size)
            .finish()
    }
}

impl Render for RenderPhysicalShape {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;

        // Layout child with full constraints
        let size = tree.layout_child(child_id, constraints);

        // Store size for paint
        self.size = size;

        size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        let mut canvas = Canvas::new();

        // Get the custom shape path in local coordinates
        let local_path = self.get_shape_path();

        // Transform the path to world coordinates by applying offset translation
        // Since Path doesn't have a transform method, we use Canvas transforms instead
        canvas.save();
        canvas.translate(offset.dx, offset.dy);

        // Draw shadow if elevation > 0
        if self.elevation > 0.0 {
            canvas.draw_shadow(&local_path, self.shadow_color, self.elevation);
        }

        // Fill the shape with color
        let paint = Paint::fill(self.color);
        canvas.draw_path(&local_path, &paint);

        // Clip to shape for child
        canvas.clip_path(&local_path);

        canvas.restore();

        // Paint child on top
        let child_canvas = tree.paint_child(child_id, offset);
        canvas.append_canvas(child_canvas);

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Rect;

    fn create_rect_clipper() -> ShapeClipper {
        Box::new(|size| {
            let mut path = Path::new();
            path.add_rect(Rect::from_xywh(0.0, 0.0, size.width, size.height));
            path
        })
    }

    fn create_circle_clipper() -> ShapeClipper {
        Box::new(|size| {
            let mut path = Path::new();
            let radius = size.width.min(size.height) / 2.0;
            let center = flui_types::Point::new(size.width / 2.0, size.height / 2.0);
            path.add_circle(center, radius);
            path
        })
    }

    #[test]
    fn test_render_physical_shape_new() {
        let clipper = create_rect_clipper();
        let shape = RenderPhysicalShape::new(clipper, 4.0, Color::WHITE);

        assert_eq!(shape.elevation, 4.0);
        assert_eq!(shape.color, Color::WHITE);
        assert_eq!(shape.shadow_color, Color::rgba(0, 0, 0, 128));
    }

    #[test]
    fn test_render_physical_shape_with_shadow_color() {
        let clipper = create_rect_clipper();
        let shape = RenderPhysicalShape::new(clipper, 4.0, Color::WHITE)
            .with_shadow_color(Color::rgba(0, 0, 0, 64));

        assert_eq!(shape.shadow_color, Color::rgba(0, 0, 0, 64));
    }

    #[test]
    fn test_render_physical_shape_set_elevation() {
        let clipper = create_rect_clipper();
        let mut shape = RenderPhysicalShape::new(clipper, 4.0, Color::WHITE);

        shape.set_elevation(8.0);
        assert_eq!(shape.elevation, 8.0);
    }

    #[test]
    fn test_render_physical_shape_set_color() {
        let clipper = create_rect_clipper();
        let mut shape = RenderPhysicalShape::new(clipper, 4.0, Color::WHITE);

        shape.set_color(Color::RED);
        assert_eq!(shape.color, Color::RED);
    }

    #[test]
    fn test_render_physical_shape_set_shadow_color() {
        let clipper = create_rect_clipper();
        let mut shape = RenderPhysicalShape::new(clipper, 4.0, Color::WHITE);

        shape.set_shadow_color(Color::rgba(255, 0, 0, 100));
        assert_eq!(shape.shadow_color, Color::rgba(255, 0, 0, 100));
    }

    #[test]
    fn test_clipper_creates_rect_path() {
        let clipper = create_rect_clipper();
        let size = Size::new(100.0, 50.0);
        let mut path = clipper(size);

        // Path should be created (we can't easily inspect its contents,
        // but we can verify it doesn't panic)
        assert!(path.bounds().width() > 0.0);
    }

    #[test]
    fn test_clipper_creates_circle_path() {
        let clipper = create_circle_clipper();
        let size = Size::new(100.0, 100.0);
        let mut path = clipper(size);

        // Circle path should have bounds
        let bounds = path.bounds();
        assert!(bounds.width() > 0.0);
        assert!(bounds.height() > 0.0);
    }

    #[test]
    fn test_get_shape_path_returns_local_coords() {
        let clipper = create_rect_clipper();
        let mut shape = RenderPhysicalShape::new(clipper, 4.0, Color::WHITE);

        // Simulate layout phase storing size
        shape.size = Size::new(100.0, 50.0);

        // Get path (should be in local coordinates)
        let mut path = shape.get_shape_path();

        // Path should be created at origin (local coordinates)
        let bounds = path.bounds();
        assert!(bounds.width() > 0.0);
        assert_eq!(bounds.left(), 0.0);
        assert_eq!(bounds.top(), 0.0);
    }

    #[test]
    fn test_arity_is_single_child() {
        let clipper = create_rect_clipper();
        let shape = RenderPhysicalShape::new(clipper, 4.0, Color::WHITE);

        assert_eq!(shape.arity(), Arity::Exact(1));
    }
}
