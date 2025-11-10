//! Canvas - High-level drawing API
//!
//! This module provides the `Canvas` type, which is a Flutter-compatible
//! drawing interface that records commands into a DisplayList for later
//! execution by the GPU backend.
//!
//! # Architecture
//!
//! ```text
//! RenderObject → Canvas (records) → DisplayList → PictureLayer → WgpuPainter (executes)
//! ```
//!
//! # Design Principles
//!
//! 1. **Recording only**: Canvas does NOT perform actual rendering
//! 2. **Immutable commands**: Once recorded, DisplayList is immutable
//! 3. **Flutter-compatible API**: Same methods as Flutter's Canvas
//! 4. **Transform tracking**: Maintains current transform matrix
//! 5. **Save/restore stack**: Supports save() and restore() for state management

use crate::display_list::{DisplayList, DrawCommand, Paint};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

/// High-level drawing canvas (Flutter-compatible API)
///
/// Canvas records drawing commands into a DisplayList without performing
/// any actual rendering. Rendering happens later in flui_engine via WgpuPainter.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::{Canvas, Paint};
/// use flui_types::{Rect, Color};
///
/// let mut canvas = Canvas::new();
///
/// // Draw a red rectangle
/// let rect = Rect::from_ltrb(10.0, 10.0, 100.0, 100.0);
/// let paint = Paint::fill(Color::RED);
/// canvas.draw_rect(rect, &paint);
///
/// // Finish and get display list
/// let display_list = canvas.finish();
/// ```
///
/// # Transform and State Management
///
/// ```rust,ignore
/// let mut canvas = Canvas::new();
///
/// canvas.save();
/// canvas.translate(50.0, 50.0);
/// canvas.rotate(std::f32::consts::PI / 4.0);
/// canvas.draw_rect(rect, &paint);
/// canvas.restore();
/// ```
pub struct Canvas {
    /// Commands being recorded
    display_list: DisplayList,

    /// Current transform matrix
    transform: Matrix4,

    /// Current clip bounds (stack of clips)
    clip_stack: Vec<ClipOp>,

    /// Save/restore stack (stores previous states)
    save_stack: Vec<CanvasState>,
}

impl Canvas {
    /// Creates a new canvas
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    ///
    /// let canvas = Canvas::new();
    /// ```
    pub fn new() -> Self {
        Self {
            display_list: DisplayList::new(),
            transform: Matrix4::identity(),
            clip_stack: Vec::new(),
            save_stack: Vec::new(),
        }
    }

    // ===== Transform Operations =====

    /// Translates the coordinate system
    ///
    /// # Arguments
    ///
    /// * `dx` - Horizontal translation
    /// * `dy` - Vertical translation
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.translate(50.0, 100.0);
    /// canvas.draw_rect(rect, &paint); // Drawn at (50, 100) offset
    /// ```
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let translation = Matrix4::translation(dx, dy, 0.0);
        self.transform *= translation;
    }

    /// Scales the coordinate system
    ///
    /// # Arguments
    ///
    /// * `sx` - Horizontal scale factor
    /// * `sy` - Vertical scale factor (defaults to sx if None)
    pub fn scale(&mut self, sx: f32, sy: Option<f32>) {
        let sy = sy.unwrap_or(sx);
        let scaling = Matrix4::scaling(sx, sy, 1.0);
        self.transform *= scaling;
    }

    /// Rotates the coordinate system
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians
    pub fn rotate(&mut self, radians: f32) {
        let rotation = Matrix4::rotation_z(radians);
        self.transform *= rotation;
    }

    /// Sets the transform matrix directly
    pub fn set_transform(&mut self, transform: Matrix4) {
        self.transform = transform;
    }

    /// Gets the current transform matrix
    pub fn get_transform(&self) -> Matrix4 {
        self.transform
    }

    // ===== Save/Restore =====

    /// Saves the current canvas state (transform, clip)
    ///
    /// Must be balanced with restore().
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.save();
    /// canvas.translate(50.0, 50.0);
    /// canvas.draw_rect(rect, &paint);
    /// canvas.restore(); // Back to original state
    /// ```
    pub fn save(&mut self) {
        self.save_stack.push(CanvasState {
            transform: self.transform,
            clip_depth: self.clip_stack.len(),
        });
    }

    /// Restores the most recently saved state
    ///
    /// # Panics
    ///
    /// Panics if there is no saved state (save/restore mismatch)
    pub fn restore(&mut self) {
        if let Some(state) = self.save_stack.pop() {
            self.transform = state.transform;
            self.clip_stack.truncate(state.clip_depth);
        } else {
            #[cfg(debug_assertions)]
            panic!("Canvas::restore() called without matching save()");
        }
    }

    /// Returns the number of saved states
    pub fn save_count(&self) -> usize {
        self.save_stack.len()
    }

    // ===== Clipping =====

    /// Clips to a rectangle
    ///
    /// All subsequent drawing will be clipped to this rectangle.
    pub fn clip_rect(&mut self, rect: Rect) {
        self.clip_stack.push(ClipOp::Rect(rect));
        self.display_list.push(DrawCommand::ClipRect {
            rect,
            transform: self.transform,
        });
    }

    /// Clips to a rounded rectangle
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.clip_stack.push(ClipOp::RRect(rrect));
        self.display_list.push(DrawCommand::ClipRRect {
            rrect,
            transform: self.transform,
        });
    }

    /// Clips to an arbitrary path
    pub fn clip_path(&mut self, path: &Path) {
        self.clip_stack.push(ClipOp::Path(Box::new(path.clone())));
        self.display_list.push(DrawCommand::ClipPath {
            path: path.clone(),
            transform: self.transform,
        });
    }

    // ===== Drawing Primitives =====

    /// Draws a line
    ///
    /// # Arguments
    ///
    /// * `p1` - Start point
    /// * `p2` - End point
    /// * `paint` - Paint style (color, stroke width, etc.)
    pub fn draw_line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawLine {
            p1,
            p2,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a rectangle
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Paint};
    /// use flui_types::{Rect, Color};
    ///
    /// let mut canvas = Canvas::new();
    /// let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 50.0);
    /// let paint = Paint::fill(Color::BLUE);
    /// canvas.draw_rect(rect, &paint);
    /// ```
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawRect {
            rect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a rounded rectangle
    pub fn draw_rrect(&mut self, rrect: RRect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawRRect {
            rrect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a circle
    ///
    /// # Arguments
    ///
    /// * `center` - Center point
    /// * `radius` - Circle radius
    /// * `paint` - Paint style
    pub fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawCircle {
            center,
            radius,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws an oval (ellipse)
    ///
    /// The oval is inscribed in the given rectangle.
    pub fn draw_oval(&mut self, rect: Rect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawOval {
            rect,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws an arbitrary path
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Paint};
    /// use flui_types::{Path, Point, Color};
    ///
    /// let mut canvas = Canvas::new();
    /// let mut path = Path::new();
    /// path.move_to(Point::new(0.0, 0.0));
    /// path.line_to(Point::new(100.0, 0.0));
    /// path.line_to(Point::new(50.0, 100.0));
    /// path.close();
    ///
    /// let paint = Paint::fill(Color::RED);
    /// canvas.draw_path(&path, &paint);
    /// ```
    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawPath {
            path: path.clone(),
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws text
    ///
    /// # Arguments
    ///
    /// * `text` - Text content to draw
    /// * `offset` - Position offset
    /// * `style` - Text style (font, size, etc.)
    /// * `paint` - Paint style (color)
    pub fn draw_text(&mut self, text: &str, offset: Offset, style: &TextStyle, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawText {
            text: text.to_string(),
            offset,
            style: style.clone(),
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws an image
    ///
    /// # Arguments
    ///
    /// * `image` - Image
    /// * `dst` - Destination rectangle
    /// * `paint` - Optional paint (for tinting, opacity, etc.)
    pub fn draw_image(&mut self, image: Image, dst: Rect, paint: Option<&Paint>) {
        self.display_list.push(DrawCommand::DrawImage {
            image,
            dst,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws a shadow
    ///
    /// # Arguments
    ///
    /// * `path` - Path casting the shadow
    /// * `color` - Shadow color
    /// * `elevation` - Shadow blur radius (elevation above surface)
    pub fn draw_shadow(&mut self, path: &Path, color: Color, elevation: f32) {
        self.display_list.push(DrawCommand::DrawShadow {
            path: path.clone(),
            color,
            elevation,
            transform: self.transform,
        });
    }

    // ===== Convenience Methods =====

    /// Draws a point as a small circle
    pub fn draw_point(&mut self, point: Point, radius: f32, paint: &Paint) {
        self.draw_circle(point, radius, paint);
    }

    /// Draws multiple points
    pub fn draw_points(&mut self, points: &[Point], radius: f32, paint: &Paint) {
        for &point in points {
            self.draw_circle(point, radius, paint);
        }
    }

    /// Draws a polyline (connected line segments)
    pub fn draw_polyline(&mut self, points: &[Point], paint: &Paint) {
        if points.len() < 2 {
            return;
        }

        for i in 0..points.len() - 1 {
            self.draw_line(points[i], points[i + 1], paint);
        }
    }

    // ===== Finalization =====

    /// Finishes recording and returns the display list
    ///
    /// This consumes the canvas and returns the recorded commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(rect, &paint);
    /// let display_list = canvas.finish();
    /// ```
    pub fn finish(self) -> DisplayList {
        #[cfg(debug_assertions)]
        {
            if !self.save_stack.is_empty() {
                tracing::warn!(
                    "Canvas::finish() called with {} unrestored save(s)",
                    self.save_stack.len()
                );
            }
        }

        self.display_list
    }

    /// Returns a reference to the display list (without consuming canvas)
    pub fn display_list(&self) -> &DisplayList {
        &self.display_list
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

/// Saved canvas state (for save/restore)
#[derive(Debug, Clone)]
struct CanvasState {
    /// Saved transform matrix
    transform: Matrix4,
    /// Depth of clip stack when saved
    clip_depth: usize,
}

/// Clip operation (for clip stack tracking)
///
/// NOTE: Fields are stored for future use (query clip bounds, optimize rendering)
/// but not currently read. This is intentional for now.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ClipOp {
    Rect(Rect),
    RRect(RRect),
    Path(Box<Path>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::Point;

    #[test]
    fn test_canvas_creation() {
        let canvas = Canvas::new();
        assert_eq!(canvas.save_count(), 0);
        assert_eq!(canvas.display_list().len(), 0);
    }

    #[test]
    fn test_canvas_draw_rect() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let paint = Paint::fill(Color::RED);

        canvas.draw_rect(rect, &paint);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 1);
    }

    #[test]
    fn test_canvas_save_restore() {
        let mut canvas = Canvas::new();

        assert_eq!(canvas.save_count(), 0);

        canvas.save();
        assert_eq!(canvas.save_count(), 1);

        canvas.translate(50.0, 50.0);

        canvas.save();
        assert_eq!(canvas.save_count(), 2);

        canvas.restore();
        assert_eq!(canvas.save_count(), 1);

        canvas.restore();
        assert_eq!(canvas.save_count(), 0);
    }

    #[test]
    fn test_canvas_transform() {
        let mut canvas = Canvas::new();

        let original_transform = canvas.get_transform();
        canvas.translate(100.0, 50.0);
        let translated_transform = canvas.get_transform();

        assert_ne!(original_transform, translated_transform);
    }

    #[test]
    fn test_canvas_clip() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);

        canvas.clip_rect(rect);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 1);
    }

    #[test]
    fn test_canvas_multiple_commands() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let paint = Paint::fill(Color::RED);

        canvas.draw_rect(rect, &paint);
        canvas.draw_circle(Point::new(50.0, 50.0), 25.0, &paint);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 2);
    }

    #[test]
    #[should_panic(expected = "Canvas::restore() called without matching save()")]
    fn test_canvas_restore_without_save() {
        let mut canvas = Canvas::new();
        canvas.restore(); // Should panic
    }
}
