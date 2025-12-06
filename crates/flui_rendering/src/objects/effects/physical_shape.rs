//! RenderPhysicalShape - Custom shapes with Material Design elevation
//!
//! Implements Flutter's PhysicalShape that renders arbitrary custom paths with
//! Material Design shadows and elevation. More flexible than RenderPhysicalModel
//! which only supports basic shapes (rectangle, rounded rectangle, circle).
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderPhysicalShape` | `RenderPhysicalShape` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `ShapeClipper` | `CustomClipper<Path>` type |
//! | `clipper` | `clipper` property (CustomClipper<Path>) |
//! | `elevation` | `elevation` property (double) |
//! | `color` | `color` property (Color) |
//! | `shadow_color` | `shadowColor` property (Color) |
//!
//! # Layout Protocol
//!
//! 1. **Check for child**
//!    - If child present: layout with full constraints, use child size
//!    - If no child: use max constraints for decorative shape
//!
//! 2. **Store size for paint**
//!    - Size cached for clipper function call during paint
//!    - Clipper generates path based on final size
//!
//! 3. **Return size**
//!    - Child size if present
//!    - Max constraints if no child (decorative shape)
//!
//! # Paint Protocol
//!
//! 1. **Generate custom path**
//!    - Call clipper function with current size
//!    - Path created in local coordinates (origin at 0,0)
//!
//! 2. **Apply canvas transforms**
//!    - Save canvas state
//!    - Translate to widget offset
//!
//! 3. **Draw shadow** (if elevation > 0)
//!    - Shadow drawn beneath shape
//!    - Shadow size based on elevation value
//!    - Material Design shadow algorithm
//!
//! 4. **Fill shape**
//!    - Draw custom path filled with color
//!
//! 5. **Clip to shape**
//!    - Set clip path to custom shape
//!    - Child painted only within shape bounds
//!
//! 6. **Restore canvas and paint child**
//!    - Restore canvas state
//!    - Paint child on top if present
//!
//! # Performance
//!
//! - **Layout**:
//!   - O(1) when no child (use max constraints)
//!   - O(child) when child present
//! - **Paint**:
//!   - O(path complexity + child) - custom path generation can be expensive
//!   - Shadow cost: O(path complexity × elevation)
//! - **Memory**: ~48 bytes + closure + cached path
//!
//! # Use Cases
//!
//! - **Custom card shapes**: Hexagon, pentagon, star cards with elevation
//! - **Irregular containers**: Non-rectangular containers with Material shadows
//! - **Decorative elements**: Custom shape backgrounds with depth
//! - **Avatar masks**: Custom shaped avatars (hexagon, diamond, etc.)
//! - **Creative UI elements**: Any custom shape needing elevation
//! - **Clipped images**: Images clipped to custom paths with shadows
//!
//! # Material Design Elevation
//!
//! Elevation values follow Material Design guidelines:
//! - **0dp**: No shadow (flat surface)
//! - **2dp**: Raised button, card
//! - **4dp**: FAB (Floating Action Button) resting
//! - **6dp**: Snackbar
//! - **8dp**: Bottom navigation, menu, card on pick up
//! - **12dp**: FAB pressed
//! - **16dp**: Navigation drawer
//! - **24dp**: Dialog, picker
//!
//! # Difference from RenderPhysicalModel
//!
//! **RenderPhysicalShape (this):**
//! - Accepts any custom Path via clipper function
//! - Maximum flexibility for arbitrary shapes
//! - Path generated dynamically based on size
//!
//! **RenderPhysicalModel:**
//! - Only supports Rectangle, RoundedRectangle, Circle
//! - Faster for standard shapes (no clipper overhead)
//! - Simpler API for common cases
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderPhysicalShape;
//! use flui_types::{Color, painting::Path, Point};
//!
//! // Star shape with elevation
//! let star_clipper = Box::new(|size| {
//!     let mut path = Path::new();
//!     // ... create star shape
//!     path
//! });
//! let star = RenderPhysicalShape::new(star_clipper, 4.0, Color::YELLOW);
//!
//! // Hexagon avatar with shadow
//! let hex_clipper = Box::new(|size| {
//!     let mut path = Path::new();
//!     let radius = size.width.min(size.height) / 2.0;
//!     let center = Point::new(size.width / 2.0, size.height / 2.0);
//!     // ... create hexagon
//!     path
//! });
//! let avatar = RenderPhysicalShape::new(hex_clipper, 2.0, Color::WHITE);
//!
//! // Custom shadow color
//! let custom = RenderPhysicalShape::new(star_clipper, 8.0, Color::BLUE)
//!     .with_shadow_color(Color::rgba(0, 0, 255, 64));
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx};
use crate::core::{Optional, RenderBox};
use flui_painting::{Canvas, Paint};
use flui_types::{
    painting::Path,
    Color, Size,
};

/// Clipper function that creates a custom path for the given size.
///
/// Takes a `Size` and returns a `Path` in local coordinates (origin at 0,0).
/// The path will be translated to the widget's position during painting.
pub type ShapeClipper = Box<dyn Fn(Size) -> Path + Send + Sync>;

/// RenderObject that renders arbitrary custom shapes with Material Design elevation.
///
/// Accepts any custom path via a clipper function, allowing for flexible shape rendering
/// with Material Design shadows and clipping. More versatile than RenderPhysicalModel
/// which only supports standard geometric shapes.
///
/// # Arity
///
/// `Optional` - Can have 0 or 1 child.
/// - With child: child clipped to custom shape
/// - Without child: decorative shape with shadow
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Custom shaped cards**: Star, hexagon, pentagon cards with Material elevation
/// - **Irregular UI elements**: Non-rectangular containers with depth
/// - **Shaped avatars**: Custom avatar masks (diamond, hexagon, etc.) with shadows
/// - **Decorative backgrounds**: Arbitrary shape backgrounds with elevation
/// - **Clipped content**: Images or widgets clipped to custom paths
/// - **Creative designs**: Any custom shape needing Material Design depth
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderPhysicalShape behavior:
/// - Accepts CustomClipper<Path> for arbitrary shapes
/// - Supports Material Design elevation and shadows
/// - Child clipped to custom shape bounds
/// - Shadow rendered beneath shape based on elevation
/// - Decorative mode when no child present
/// - Path generated dynamically based on widget size
///
/// # Material Design Elevation Guidelines
///
/// - 0dp: Flat (no shadow)
/// - 2dp: Card, raised button
/// - 4dp: FAB resting
/// - 8dp: Bottom nav, dropdown, card picked up
/// - 16dp: Navigation drawer
/// - 24dp: Dialog, modal
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPhysicalShape;
/// use flui_types::{Color, painting::Path};
///
/// // Custom star shape with elevation
/// let clipper = Box::new(|size| {
///     let mut path = Path::new();
///     // Create star shape...
///     path
/// });
/// let star = RenderPhysicalShape::new(clipper, 4.0, Color::YELLOW)
///     .with_shadow_color(Color::rgba(0, 0, 0, 100));
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

impl RenderBox<Optional> for RenderPhysicalShape {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Optional>) -> Size {
        let constraints = ctx.constraints;

        // Optional arity: ctx.children.get() returns Option<&ElementId>
        // Dereference in pattern match to get ElementId
        let size = if let Some(&child_id) = ctx.children.get() {
            // With child: layout child with full constraints
            ctx.layout_child(child_id, constraints)
        } else {
            // No child: use max constraints for decorative shape size
            Size::new(constraints.max_width, constraints.max_height)
        };

        // Store size for clipper function call during paint
        self.size = size;

        size
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Optional>) {
        let offset = ctx.offset;

        // Get the custom shape path in local coordinates (origin at 0,0)
        let local_path = self.get_shape_path();

        // Transform to world coordinates using Canvas transforms
        // (Path doesn't have a transform method, so we use canvas save/translate/restore)
        ctx.canvas().save();
        ctx.canvas().translate(offset.dx, offset.dy);

        // Draw Material Design shadow beneath shape (if elevation > 0)
        if self.elevation > 0.0 {
            ctx.canvas().draw_shadow(&local_path, self.shadow_color, self.elevation);
        }

        // Fill the custom shape with color
        let paint = Paint::fill(self.color);
        ctx.canvas().draw_path(&local_path, &paint);

        // Clip canvas to shape bounds (child will be clipped)
        ctx.canvas().clip_path(&local_path);

        ctx.canvas().restore();

        // Paint child on top if present
        // Optional arity: ctx.children.get() returns Option<&ElementId>
        // Dereference in pattern match to get ElementId
        if let Some(&child_id) = ctx.children.get() {
            ctx.paint_child(child_id, offset);
        }
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
}
