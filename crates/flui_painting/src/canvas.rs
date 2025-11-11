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
//! 6. **Thread-safe**: Canvas and DisplayList are Send (can be sent across threads)
//!
//! # Thread Safety
//!
//! Canvas is designed for **single-threaded recording** but **multi-threaded execution**:
//!
//! - Each RenderObject creates its own Canvas during `paint()`
//! - Canvases are composed via `append_canvas()` (zero-copy move)
//! - Final DisplayList can be sent to GPU thread for execution
//! - All types are `Send` but not `Sync` (no shared mutable state)
//!
//! This design enables efficient parallel painting in FLUI's parallel build pipeline.

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

    /// Applies a transform to the current coordinate system
    ///
    /// This method accepts both `Transform` and `Matrix4` types via the `Into` trait,
    /// allowing for idiomatic Rust usage with the high-level Transform API.
    ///
    /// # Arguments
    ///
    /// * `transform` - A Transform or Matrix4 to apply
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    /// use flui_types::geometry::Transform;
    /// use std::f32::consts::PI;
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Using Transform (high-level API)
    /// canvas.transform(Transform::rotate(PI / 4.0));
    ///
    /// // Using Matrix4 (low-level API)
    /// let matrix = Matrix4::rotation_z(PI / 4.0);
    /// canvas.transform(matrix);
    ///
    /// // Composing transforms
    /// let composed = Transform::translate(50.0, 50.0)
    ///     .then(Transform::rotate(PI / 4.0))
    ///     .then(Transform::scale(2.0));
    /// canvas.transform(composed);
    /// ```
    pub fn transform<T: Into<Matrix4>>(&mut self, transform: T) {
        let matrix = transform.into();
        self.transform *= matrix;
    }

    /// Sets the transform matrix directly
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::geometry::Transform;
    ///
    /// // With Transform
    /// canvas.set_transform(Transform::rotate(PI / 4.0));
    ///
    /// // With Matrix4
    /// canvas.set_transform(Matrix4::identity());
    /// ```
    pub fn set_transform<T: Into<Matrix4>>(&mut self, transform: T) {
        self.transform = transform.into();
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

    // ===== Clip Query Methods =====

    /// Returns the local-space bounds of the current clip, if available.
    ///
    /// This returns the bounds of the most recent clip operation, without
    /// applying transformations. Returns `None` if:
    /// - No clip is active (clip stack is empty)
    /// - The current clip is a Path (bounds require mutable access)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let mut canvas = Canvas::new();
    /// canvas.clip_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(canvas.get_local_clip_bounds(), Some(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0)));
    /// ```
    #[inline]
    #[must_use]
    pub fn get_local_clip_bounds(&self) -> Option<Rect> {
        self.clip_stack.last().and_then(|clip| match clip {
            ClipOp::Rect(rect) => Some(*rect),
            ClipOp::RRect(rrect) => Some(rrect.bounding_rect()),
            ClipOp::Path(_) => None, // Path bounds require &mut Path
        })
    }

    /// Returns the device-space (transformed) bounds of the current clip, if available.
    ///
    /// This applies the current transformation matrix to the clip bounds.
    /// Returns `None` if:
    /// - No clip is active (clip stack is empty)
    /// - The current clip is a Path (bounds require mutable access)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let mut canvas = Canvas::new();
    /// canvas.translate(10.0, 20.0);
    /// canvas.clip_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0));
    /// // Returns transformed bounds: (10, 20, 110, 120)
    /// let bounds = canvas.get_device_clip_bounds();
    /// ```
    #[inline]
    #[must_use]
    pub fn get_device_clip_bounds(&self) -> Option<Rect> {
        self.get_local_clip_bounds()
            .map(|local_bounds| self.transform.transform_rect(&local_bounds))
    }

    /// Returns true if the given rectangle is completely outside the current clip bounds.
    ///
    /// This can be used for culling optimizations - if this returns `true`, you can
    /// skip drawing operations that would be completely clipped anyway.
    ///
    /// # Returns
    ///
    /// - `Some(true)` - The rect is definitely outside the clip (can skip drawing)
    /// - `Some(false)` - The rect may be visible (should draw)
    /// - `None` - Cannot determine (no clip active or clip is a Path)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let mut canvas = Canvas::new();
    /// canvas.clip_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0));
    ///
    /// // This rect is outside the clip
    /// let outside_rect = Rect::from_ltrb(200.0, 200.0, 300.0, 300.0);
    /// assert_eq!(canvas.is_rect_outside_clip(&outside_rect), Some(true));
    ///
    /// // This rect overlaps the clip
    /// let inside_rect = Rect::from_ltrb(50.0, 50.0, 150.0, 150.0);
    /// assert_eq!(canvas.is_rect_outside_clip(&inside_rect), Some(false));
    /// ```
    #[inline]
    #[must_use]
    pub fn is_rect_outside_clip(&self, rect: &Rect) -> Option<bool> {
        self.get_local_clip_bounds()
            .map(|clip_bounds| !clip_bounds.intersects(rect))
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

    // ===== Advanced Drawing Methods =====

    /// Draws an arc segment
    ///
    /// # Arguments
    ///
    /// * `rect` - Bounding rectangle for the ellipse
    /// * `start_angle` - Start angle in radians
    /// * `sweep_angle` - Sweep angle in radians
    /// * `use_center` - Whether to draw from center (pie slice) or just the arc
    /// * `paint` - Paint style
    pub fn draw_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        self.display_list.push(DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws difference between two rounded rectangles (ring/border)
    ///
    /// # Arguments
    ///
    /// * `outer` - Outer rounded rectangle
    /// * `inner` - Inner rounded rectangle
    /// * `paint` - Paint style
    pub fn draw_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) {
        self.display_list.push(DrawCommand::DrawDRRect {
            outer,
            inner,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws a sequence of points with the specified mode
    ///
    /// # Arguments
    ///
    /// * `mode` - Point drawing mode (points, lines, or polygon)
    /// * `points` - Points to draw
    /// * `paint` - Paint style
    pub fn draw_points_mode(
        &mut self,
        mode: crate::display_list::PointMode,
        points: Vec<Point>,
        paint: &Paint,
    ) {
        self.display_list.push(DrawCommand::DrawPoints {
            mode,
            points,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Draws custom vertices with optional colors and texture coordinates
    ///
    /// # Arguments
    ///
    /// * `vertices` - Vertex positions
    /// * `colors` - Optional vertex colors (must match vertices length)
    /// * `tex_coords` - Optional texture coordinates (must match vertices length)
    /// * `indices` - Triangle indices (groups of 3)
    /// * `paint` - Paint style
    pub fn draw_vertices(
        &mut self,
        vertices: Vec<Point>,
        colors: Option<Vec<Color>>,
        tex_coords: Option<Vec<Point>>,
        indices: Vec<u16>,
        paint: &Paint,
    ) {
        self.display_list.push(DrawCommand::DrawVertices {
            vertices,
            colors,
            tex_coords,
            indices,
            paint: paint.clone(),
            transform: self.transform,
        });
    }

    /// Fills entire canvas with a color (respects clipping)
    ///
    /// # Arguments
    ///
    /// * `color` - Color to fill with
    /// * `blend_mode` - Blend mode
    pub fn draw_color(&mut self, color: Color, blend_mode: crate::display_list::BlendMode) {
        self.display_list.push(DrawCommand::DrawColor {
            color,
            blend_mode,
            transform: self.transform,
        });
    }

    /// Draws multiple sprites from a texture atlas
    ///
    /// # Arguments
    ///
    /// * `image` - Source image (atlas texture)
    /// * `sprites` - Source rectangles in atlas (sprite locations)
    /// * `transforms` - Destination transforms for each sprite
    /// * `colors` - Optional colors to blend with each sprite
    /// * `blend_mode` - Blend mode
    /// * `paint` - Optional paint for additional effects
    pub fn draw_atlas(
        &mut self,
        image: Image,
        sprites: Vec<Rect>,
        transforms: Vec<Matrix4>,
        colors: Option<Vec<Color>>,
        blend_mode: crate::display_list::BlendMode,
        paint: Option<&Paint>,
    ) {
        self.display_list.push(DrawCommand::DrawAtlas {
            image,
            sprites,
            transforms,
            colors,
            blend_mode,
            paint: paint.cloned(),
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

    // ===== Canvas Composition =====

    /// Appends all drawing commands from another canvas to this canvas
    ///
    /// This is useful for parent RenderObjects that need to draw their own content
    /// and then draw their children on top.
    ///
    /// # Performance
    ///
    /// This method uses **zero-copy move semantics**:
    /// - Commands are moved, not cloned (O(N) pointer moves, not O(N) deep copies)
    /// - If parent canvas is empty, this is O(1) (vector swap)
    /// - No allocations if capacity is sufficient
    ///
    /// This is **much faster** than cloning commands individually, especially
    /// for complex scenes with many children.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut parent_canvas = Canvas::new();
    /// parent_canvas.draw_rect(background_rect, &background_paint);
    ///
    /// let child_canvas = child.paint(ctx);
    /// parent_canvas.append_canvas(child_canvas);  // Zero-copy move
    /// ```
    pub fn append_canvas(&mut self, other: Canvas) {
        // Zero-copy move of commands via DisplayList::append
        self.display_list.append(other.display_list);
    }

    /// Appends all drawing commands from another canvas with opacity applied
    ///
    /// This is useful for implementing opacity effects on entire subtrees of rendering.
    /// All drawing commands from the child canvas will have the specified opacity
    /// multiplied with their existing paint opacity.
    ///
    /// # Arguments
    ///
    /// * `other` - Child canvas to append
    /// * `opacity` - Opacity to apply (0.0 = fully transparent, 1.0 = fully opaque)
    ///
    /// # Performance
    ///
    /// This method creates a new DisplayList with modified Paint objects, which
    /// involves cloning commands. For opacity of 1.0, prefer using `append_canvas()`
    /// directly which uses zero-copy move semantics.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut parent_canvas = Canvas::new();
    /// parent_canvas.draw_rect(background_rect, &background_paint);
    ///
    /// let child_canvas = child.paint(ctx);
    /// parent_canvas.append_canvas_with_opacity(child_canvas, 0.5);  // 50% transparent
    /// ```
    pub fn append_canvas_with_opacity(&mut self, other: Canvas, opacity: f32) {
        if opacity >= 1.0 {
            // Fast path: no opacity change needed
            self.append_canvas(other);
        } else {
            // Apply opacity to all commands
            let modified_display_list = other.display_list.with_opacity(opacity);
            self.display_list.append(modified_display_list);
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

/// Clip operation stored in the clip stack.
///
/// Currently used for tracking clip depth in save/restore operations.
/// The clip geometry (Rect/RRect/Path) is stored for future optimizations:
/// - Culling: Skip drawing commands outside the clip bounds
/// - Clip bounds queries: `canvas.get_local_clip_bounds()`
/// - Render optimization: Merge adjacent clips
///
/// TODO: Add methods to query clip bounds and use for culling optimization
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields stored for future optimization features
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
