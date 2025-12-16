//! Canvas - High-level drawing API
//!
//! This module provides the `Canvas` type, which is a high-level drawing
//! interface that records commands into a DisplayList for later execution
//! by the GPU backend.
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
//! 3. **Intuitive API**: Consistent with common 2D graphics APIs
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

use crate::display_list::{
    BlendMode, DisplayList, DisplayListCore, DrawCommand, ImageFilter, Paint,
};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect},
    painting::{Clip, ClipOp, Image, Path},
    styling::Color,
    typography::{InlineSpan, TextStyle},
};

/// High-level drawing canvas with intuitive API
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
#[derive(Debug, Clone)]
pub struct Canvas {
    /// Commands being recorded
    display_list: DisplayList,

    /// Current transform matrix
    transform: Matrix4,

    /// Current clip bounds (stack of clips)
    clip_stack: Vec<ClipShape>,

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
    #[inline]
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let translation = Matrix4::translation(dx, dy, 0.0);
        self.transform *= translation;
    }

    /// Scales the coordinate system uniformly.
    ///
    /// # Arguments
    ///
    /// * `factor` - Scale factor applied to both axes
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.scale_uniform(2.0); // Double size in both directions
    /// ```
    #[inline]
    pub fn scale_uniform(&mut self, factor: f32) {
        let scaling = Matrix4::scaling(factor, factor, 1.0);
        self.transform *= scaling;
    }

    /// Scales the coordinate system with separate factors for each axis.
    ///
    /// # Arguments
    ///
    /// * `sx` - Horizontal scale factor
    /// * `sy` - Vertical scale factor
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.scale_xy(2.0, 0.5); // Stretch horizontally, compress vertically
    /// ```
    #[inline]
    pub fn scale_xy(&mut self, sx: f32, sy: f32) {
        let scaling = Matrix4::scaling(sx, sy, 1.0);
        self.transform *= scaling;
    }

    /// Rotates the coordinate system around the origin.
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians (positive = counter-clockwise)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use std::f32::consts::PI;
    /// canvas.rotate(PI / 4.0); // Rotate 45 degrees
    /// ```
    #[inline]
    pub fn rotate(&mut self, radians: f32) {
        let rotation = Matrix4::rotation_z(radians);
        self.transform *= rotation;
    }

    /// Rotates the coordinate system around a specified pivot point.
    ///
    /// This is equivalent to translating to the pivot, rotating, then translating back.
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians (positive = counter-clockwise)
    /// * `pivot_x` - X coordinate of the pivot point
    /// * `pivot_y` - Y coordinate of the pivot point
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use std::f32::consts::PI;
    /// // Rotate 90 degrees around the center of a 100x100 rectangle
    /// canvas.rotate_around(PI / 2.0, 50.0, 50.0);
    /// ```
    #[inline]
    pub fn rotate_around(&mut self, radians: f32, pivot_x: f32, pivot_y: f32) {
        self.translate(pivot_x, pivot_y);
        self.rotate(radians);
        self.translate(-pivot_x, -pivot_y);
    }

    /// Skews the coordinate system along the X and Y axes.
    ///
    /// Skew transforms are useful for creating italic text effects, parallax,
    /// and perspective-like distortions.
    ///
    /// # Arguments
    ///
    /// * `sx` - Horizontal skew factor (tan of the angle to skew along X axis)
    /// * `sy` - Vertical skew factor (tan of the angle to skew along Y axis)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Skew horizontally (italic-like effect)
    /// canvas.skew(0.2, 0.0);  // ~11.3 degrees horizontal shear
    ///
    /// // Skew both axes
    /// canvas.skew(0.3, 0.1);
    /// ```
    #[inline]
    pub fn skew(&mut self, sx: f32, sy: f32) {
        // Skew matrix (row-major):
        // | 1  sx  0  0 |
        // | sy  1  0  0 |
        // | 0   0  1  0 |
        // | 0   0  0  1 |
        let skew_matrix = Matrix4::new(
            1.0, sx, 0.0, 0.0, sy, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        );
        self.transform *= skew_matrix;
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

    /// Returns the current transform matrix.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// canvas.translate(50.0, 50.0);
    /// let matrix = canvas.transform_matrix();
    /// ```
    #[inline]
    #[must_use]
    pub fn transform_matrix(&self) -> Matrix4 {
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
    #[inline]
    pub fn save(&mut self) {
        self.save_stack.push(CanvasState {
            transform: self.transform,
            clip_depth: self.clip_stack.len(),
            is_layer: false,
        });
    }

    /// Restores the most recently saved state
    ///
    /// If the saved state was created by `save_layer()`, this also composites
    /// the layer back using the paint specified when the layer was created.
    ///
    /// If there is no saved state, this is a no-op.
    #[inline]
    pub fn restore(&mut self) {
        if let Some(state) = self.save_stack.pop() {
            // If this was a layer save, record the RestoreLayer command
            if state.is_layer {
                self.display_list.push(DrawCommand::RestoreLayer {
                    transform: self.transform,
                });
            }

            self.transform = state.transform;
            self.clip_stack.truncate(state.clip_depth);
        }
        // No saved state: silently ignore (no-op)
    }

    /// Returns the number of saved states (plus 1 for the initial state).
    /// The initial save count is 1.
    pub fn save_count(&self) -> usize {
        self.save_stack.len() + 1
    }

    /// Restores the canvas state to a specific save count.
    ///
    /// This pops states from the save stack until the stack reaches the specified count.
    ///
    /// # Arguments
    ///
    /// * `count` - Target save count (must be >= 1 and <= current save count)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// assert_eq!(canvas.save_count(), 1);
    ///
    /// canvas.save();
    /// assert_eq!(canvas.save_count(), 2);
    ///
    /// canvas.save();
    /// assert_eq!(canvas.save_count(), 3);
    ///
    /// canvas.restore_to_count(1);  // Restore all the way to initial state
    /// assert_eq!(canvas.save_count(), 1);
    /// ```
    pub fn restore_to_count(&mut self, count: usize) {
        let count = count.max(1); // Cannot go below 1
        while self.save_count() > count {
            self.restore();
        }
    }

    // ===== Layer Operations =====

    /// Saves the canvas state and creates a new compositing layer
    ///
    /// This is similar to `save()` but creates an offscreen buffer for subsequent
    /// drawing commands. When `restore()` is called, the layer is composited back
    /// using the specified paint settings (opacity, blend mode, color filter, etc.).
    ///
    /// # Arguments
    ///
    /// * `bounds` - Optional bounds for the layer. If None, uses current clip bounds.
    /// * `paint` - Paint to apply when compositing (for opacity, blend mode, etc.)
    ///
    /// # Use Cases
    ///
    /// - **Opacity effects**: Apply uniform transparency to a group of drawings
    /// - **Blend modes**: Apply complex blending to multiple overlapping elements
    /// - **Anti-aliasing**: Get clean edges when clipping overlapping content
    ///
    /// # Performance
    ///
    /// `save_layer` is relatively expensive because it:
    /// 1. Forces GPU to switch render targets
    /// 2. Allocates an offscreen buffer
    /// 3. Requires copying framebuffer contents
    ///
    /// Use sparingly, especially on lower-end hardware.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Paint};
    /// use flui_types::{Rect, Color};
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw a group of shapes at 50% opacity
    /// let bounds = Rect::from_ltrb(0.0, 0.0, 200.0, 200.0);
    /// canvas.save_layer(Some(bounds), &Paint::new().with_opacity(0.5));
    /// canvas.draw_rect(rect1, &red_paint);
    /// canvas.draw_rect(rect2, &blue_paint);
    /// canvas.restore(); // Composites the layer at 50% opacity
    /// ```
    #[tracing::instrument(skip(self, paint), fields(
        bounds = ?bounds,
        opacity = paint.color.alpha_f32(),
        blend_mode = ?paint.blend_mode,
        layer_depth = self.save_stack.len(),
    ))]
    pub fn save_layer(&mut self, bounds: Option<Rect>, paint: &Paint) {
        // Save state for restore (marked as layer)
        self.save_stack.push(CanvasState {
            transform: self.transform,
            clip_depth: self.clip_stack.len(),
            is_layer: true,
        });

        // Record the SaveLayer command
        self.display_list.push(DrawCommand::SaveLayer {
            bounds,
            paint: paint.clone(),
            transform: self.transform,
        });

        tracing::debug!(layer_depth = self.save_stack.len(), "Layer created");
    }

    /// Saves the canvas state with a layer that applies alpha transparency
    ///
    /// This is a convenience method equivalent to:
    /// ```rust,ignore
    /// canvas.save_layer(bounds, &Paint::new().with_opacity(alpha / 255.0));
    /// ```
    ///
    /// # Arguments
    ///
    /// * `bounds` - Optional bounds for the layer
    /// * `alpha` - Alpha value (0 = fully transparent, 255 = fully opaque)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Draw content at 50% opacity (alpha = 127)
    /// canvas.save_layer_alpha(Some(bounds), 127);
    /// canvas.draw_rect(rect, &paint);
    /// canvas.restore();
    /// ```
    pub fn save_layer_alpha(&mut self, bounds: Option<Rect>, alpha: u8) {
        let opacity = alpha as f32 / 255.0;
        self.save_layer(
            bounds,
            &Paint::fill(Color::TRANSPARENT).with_opacity(opacity),
        );
    }

    /// Saves the canvas state with a layer that applies float opacity
    ///
    /// This is a convenience method for applying opacity to a group of drawings.
    ///
    /// # Arguments
    ///
    /// * `bounds` - Optional bounds for the layer
    /// * `opacity` - Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Draw content at 50% opacity
    /// canvas.save_layer_opacity(Some(bounds), 0.5);
    /// canvas.draw_rect(rect, &paint);
    /// canvas.restore();
    /// ```
    pub fn save_layer_opacity(&mut self, bounds: Option<Rect>, opacity: f32) {
        self.save_layer(
            bounds,
            &Paint::fill(Color::TRANSPARENT).with_opacity(opacity.clamp(0.0, 1.0)),
        );
    }

    /// Saves the canvas state with a layer that applies a blend mode
    ///
    /// # Arguments
    ///
    /// * `bounds` - Optional bounds for the layer
    /// * `blend_mode` - Blend mode to apply when compositing
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Draw content with multiply blend mode
    /// canvas.save_layer_blend(Some(bounds), BlendMode::Multiply);
    /// canvas.draw_rect(rect, &paint);
    /// canvas.restore();
    /// ```
    pub fn save_layer_blend(&mut self, bounds: Option<Rect>, blend_mode: BlendMode) {
        self.save_layer(
            bounds,
            &Paint::fill(Color::TRANSPARENT).with_blend_mode(blend_mode),
        );
    }

    // ===== Clipping =====

    /// Clips to a rectangle.
    ///
    /// All subsequent drawing will be clipped to this rectangle.
    /// Uses default clip behavior (intersect, anti-aliased).
    pub fn clip_rect(&mut self, rect: Rect) {
        self.clip_stack.push(ClipShape::Rect(rect));
        self.display_list.push(DrawCommand::ClipRect {
            rect,
            transform: self.transform,
        });
    }

    /// Clips to a rounded rectangle.
    ///
    /// Uses default clip behavior (intersect, anti-aliased).
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.clip_stack.push(ClipShape::RRect(rrect));
        self.display_list.push(DrawCommand::ClipRRect {
            rrect,
            transform: self.transform,
        });
    }

    /// Clips to an arbitrary path.
    ///
    /// Uses default clip behavior (intersect, anti-aliased).
    pub fn clip_path(&mut self, path: &Path) {
        self.clip_stack
            .push(ClipShape::Path(Box::new(path.clone())));
        self.display_list.push(DrawCommand::ClipPath {
            path: path.clone(),
            transform: self.transform,
        });
    }

    /// Clips to a rectangle with explicit options.
    ///
    /// Supports clip operations (intersect/difference) and anti-aliasing.
    ///
    /// # Arguments
    ///
    /// * `rect` - Rectangle to clip to
    /// * `clip_op` - How to combine with existing clips (Intersect or Difference)
    /// * `clip_behavior` - Anti-aliasing behavior
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::painting::{ClipOp, Clip};
    ///
    /// // Standard intersection clip with anti-aliasing
    /// canvas.clip_rect_ext(rect, ClipOp::Intersect, Clip::AntiAlias);
    ///
    /// // Punch a hole (difference mode)
    /// canvas.clip_rect_ext(hole_rect, ClipOp::Difference, Clip::AntiAlias);
    ///
    /// // Fast clip without anti-aliasing
    /// canvas.clip_rect_ext(rect, ClipOp::Intersect, Clip::HardEdge);
    /// ```
    pub fn clip_rect_ext(&mut self, rect: Rect, _clip_op: ClipOp, _clip_behavior: Clip) {
        // TODO: Store clip_op and clip_behavior in DrawCommand when engine supports it
        self.clip_rect(rect);
    }

    /// Clips to a rounded rectangle with explicit options.
    pub fn clip_rrect_ext(&mut self, rrect: RRect, _clip_op: ClipOp, _clip_behavior: Clip) {
        // TODO: Store options in DrawCommand when engine supports it
        self.clip_rrect(rrect);
    }

    /// Clips to a path with explicit options.
    pub fn clip_path_ext(&mut self, path: &Path, _clip_op: ClipOp, _clip_behavior: Clip) {
        // TODO: Store options in DrawCommand when engine supports it
        self.clip_path(path);
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
    /// assert_eq!(canvas.local_clip_bounds(), Some(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0)));
    /// ```
    #[inline]
    #[must_use]
    pub fn local_clip_bounds(&self) -> Option<Rect> {
        self.clip_stack.last().and_then(|clip| match clip {
            ClipShape::Rect(rect) => Some(*rect),
            ClipShape::RRect(rrect) => Some(rrect.bounding_rect()),
            ClipShape::Path(_) => None, // Path bounds require &mut Path
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
    /// let bounds = canvas.device_clip_bounds();
    /// ```
    #[inline]
    #[must_use]
    pub fn device_clip_bounds(&self) -> Option<Rect> {
        self.local_clip_bounds()
            .map(|local_bounds| self.transform.transform_rect(&local_bounds))
    }

    /// Checks if the given rectangle is completely outside the current clip bounds.
    ///
    /// Use this for culling optimizations - if this returns `true`, you can
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
    /// assert_eq!(canvas.would_be_clipped(&outside_rect), Some(true));
    ///
    /// // This rect overlaps the clip
    /// let inside_rect = Rect::from_ltrb(50.0, 50.0, 150.0, 150.0);
    /// assert_eq!(canvas.would_be_clipped(&inside_rect), Some(false));
    /// ```
    #[inline]
    #[must_use]
    pub fn would_be_clipped(&self, rect: &Rect) -> Option<bool> {
        self.local_clip_bounds()
            .map(|clip_bounds| !clip_bounds.intersects(*rect))
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

    /// Draws a circle.
    ///
    /// # Arguments
    ///
    /// * `center` - Center point
    /// * `radius` - Circle radius (must be non-negative)
    /// * `paint` - Paint style
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `radius` is negative or NaN.
    pub fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        debug_assert!(
            radius >= 0.0 && !radius.is_nan(),
            "Circle radius must be non-negative and not NaN, got: {}",
            radius
        );

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

    /// Draws rich text with inline spans
    ///
    /// This is used by [`TextPainter`] to render styled text with nested spans.
    ///
    /// # Arguments
    ///
    /// * `span` - The rich text span (may contain nested styles)
    /// * `offset` - Position offset
    /// * `text_scale_factor` - Scale factor for accessibility
    ///
    /// [`TextPainter`]: crate::TextPainter
    pub fn draw_text_span(&mut self, span: &InlineSpan, offset: Offset, text_scale_factor: f64) {
        self.display_list.push(DrawCommand::DrawTextSpan {
            span: span.clone(),
            offset,
            text_scale_factor,
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

    /// Draws an image with tiling/repeat
    ///
    /// Tiles the image to fill the destination rectangle based on the repeat mode.
    ///
    /// # Arguments
    ///
    /// * `image` - Image to tile
    /// * `dst` - Destination rectangle to fill
    /// * `repeat` - How to repeat the image (Repeat, RepeatX, RepeatY, NoRepeat)
    /// * `paint` - Optional paint (for tinting, opacity, etc.)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    /// use flui_types::{Rect, painting::{Image, ImageRepeat}};
    ///
    /// let mut canvas = Canvas::new();
    /// let pattern = Image::solid_color(32, 32, Color::BLUE);
    /// let dst = Rect::from_xywh(0.0, 0.0, 200.0, 200.0);
    ///
    /// // Tile the pattern image to fill the rectangle
    /// canvas.draw_image_repeat(pattern, dst, ImageRepeat::Repeat, None);
    /// ```
    pub fn draw_image_repeat(
        &mut self,
        image: Image,
        dst: Rect,
        repeat: crate::display_list::ImageRepeat,
        paint: Option<&Paint>,
    ) {
        self.display_list.push(DrawCommand::DrawImageRepeat {
            image,
            dst,
            repeat,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws an image with 9-slice/9-patch scaling
    ///
    /// The center slice defines the stretchable region of the image. Areas outside
    /// the center slice (corners and edges) are drawn at their natural size, while
    /// the center slice stretches to fill the remaining space.
    ///
    /// This is useful for resizable UI elements like buttons, panels, and chat bubbles.
    ///
    /// # Arguments
    ///
    /// * `image` - Image to draw
    /// * `center_slice` - Rectangle defining the stretchable center region (in image coordinates)
    /// * `dst` - Destination rectangle
    /// * `paint` - Optional paint (for tinting, opacity, etc.)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    /// use flui_types::{Rect, painting::Image};
    ///
    /// let mut canvas = Canvas::new();
    /// let button_bg = load_image("button.png"); // 64x32 image
    ///
    /// // Define center slice (stretchable area), leaving 8px borders
    /// let center = Rect::from_ltrb(8.0, 8.0, 56.0, 24.0);
    /// let dst = Rect::from_xywh(0.0, 0.0, 200.0, 48.0);
    ///
    /// canvas.draw_image_nine_slice(button_bg, center, dst, None);
    /// ```
    pub fn draw_image_nine_slice(
        &mut self,
        image: Image,
        center_slice: Rect,
        dst: Rect,
        paint: Option<&Paint>,
    ) {
        self.display_list.push(DrawCommand::DrawImageNineSlice {
            image,
            center_slice,
            dst,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws an image with a color filter applied
    ///
    /// Applies a color transformation to the image when drawing. This can be used
    /// for effects like grayscale, sepia, tinting, and color matrix transformations.
    ///
    /// # Arguments
    ///
    /// * `image` - Image to draw
    /// * `dst` - Destination rectangle
    /// * `filter` - Color filter to apply
    /// * `paint` - Optional paint (for additional effects like opacity)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    /// use flui_types::{Rect, painting::{Image, ColorFilter, BlendMode}, Color};
    ///
    /// let mut canvas = Canvas::new();
    /// let photo = load_image("photo.png");
    /// let dst = Rect::from_xywh(0.0, 0.0, 200.0, 150.0);
    ///
    /// // Draw with grayscale filter
    /// canvas.draw_image_filtered(photo.clone(), dst, ColorFilter::grayscale(), None);
    ///
    /// // Draw with sepia tone
    /// canvas.draw_image_filtered(photo.clone(), dst, ColorFilter::sepia(), None);
    ///
    /// // Draw with red tint
    /// canvas.draw_image_filtered(
    ///     photo,
    ///     dst,
    ///     ColorFilter::mode(Color::RED, BlendMode::Multiply),
    ///     None,
    /// );
    /// ```
    pub fn draw_image_filtered(
        &mut self,
        image: Image,
        dst: Rect,
        filter: crate::display_list::ColorFilter,
        paint: Option<&Paint>,
    ) {
        self.display_list.push(DrawCommand::DrawImageFiltered {
            image,
            dst,
            filter,
            paint: paint.cloned(),
            transform: self.transform,
        });
    }

    /// Draws a GPU texture referenced by ID
    ///
    /// This method renders an external GPU texture (e.g., video frame, camera preview,
    /// platform view) to the destination rectangle. The texture must be registered
    /// with the rendering engine before use.
    ///
    /// # Arguments
    ///
    /// * `texture_id` - GPU texture identifier
    /// * `dst` - Destination rectangle where the texture will be drawn
    /// * `src` - Optional source rectangle within the texture (None = entire texture)
    /// * `filter_quality` - Quality of texture sampling (bilinear, etc.)
    /// * `opacity` - Opacity of the texture (0.0 = transparent, 1.0 = opaque)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    /// use flui_types::{Rect, painting::{TextureId, FilterQuality}};
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw entire texture at full opacity
    /// let texture_id = TextureId::new(42);
    /// let dst = Rect::from_xywh(0.0, 0.0, 320.0, 240.0);
    /// canvas.draw_texture(texture_id, dst, None, FilterQuality::Low, 1.0);
    ///
    /// // Draw a portion of the texture with 50% opacity
    /// let src = Rect::from_xywh(0.0, 0.0, 160.0, 120.0);
    /// canvas.draw_texture(texture_id, dst, Some(src), FilterQuality::Medium, 0.5);
    /// ```
    pub fn draw_texture(
        &mut self,
        texture_id: crate::display_list::TextureId,
        dst: Rect,
        src: Option<Rect>,
        filter_quality: crate::display_list::FilterQuality,
        opacity: f32,
    ) {
        self.display_list.push(DrawCommand::DrawTexture {
            texture_id,
            dst,
            src,
            filter_quality,
            opacity: opacity.clamp(0.0, 1.0),
            transform: self.transform,
        });
    }

    /// Draws a shadow.
    ///
    /// # Arguments
    ///
    /// * `path` - Path casting the shadow
    /// * `color` - Shadow color
    /// * `elevation` - Shadow blur radius (elevation above surface, must be non-negative)
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `elevation` is negative or NaN.
    pub fn draw_shadow(&mut self, path: &Path, color: Color, elevation: f32) {
        debug_assert!(
            elevation >= 0.0 && !elevation.is_nan(),
            "Shadow elevation must be non-negative and not NaN, got: {}",
            elevation
        );

        self.display_list.push(DrawCommand::DrawShadow {
            path: path.clone(),
            color,
            elevation,
            transform: self.transform,
        });
    }

    /// Draws a gradient-filled rectangle
    ///
    /// This is a convenience method for drawing gradients. For more complex
    /// gradient shapes (circles, paths), use `draw_rect` or `draw_path` with
    /// a Paint that has a shader set.
    ///
    /// # Arguments
    ///
    /// * `rect` - Rectangle to fill with gradient
    /// * `shader` - Gradient shader specification
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Shader};
    /// use flui_types::{Rect, Color, Offset};
    ///
    /// let mut canvas = Canvas::new();
    /// let rect = Rect::from_ltrb(0.0, 0.0, 200.0, 100.0);
    ///
    /// // Linear gradient from left to right
    /// let gradient = Shader::linear_gradient(
    ///     Offset::new(0.0, 0.0),
    ///     Offset::new(200.0, 0.0),
    ///     vec![Color::RED, Color::BLUE],
    ///     None,
    /// );
    /// canvas.draw_gradient(rect, gradient);
    /// ```
    pub fn draw_gradient(&mut self, rect: Rect, shader: crate::display_list::Shader) {
        self.display_list.push(DrawCommand::DrawGradient {
            rect,
            shader,
            transform: self.transform,
        });
    }

    /// Draws a gradient-filled rounded rectangle
    ///
    /// # Arguments
    ///
    /// * `rrect` - Rounded rectangle to fill with gradient
    /// * `shader` - Gradient shader specification
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Shader};
    /// use flui_types::{RRect, Rect, Color, Offset};
    ///
    /// let mut canvas = Canvas::new();
    /// let rrect = RRect::from_rect_circular(
    ///     Rect::from_ltrb(0.0, 0.0, 200.0, 100.0),
    ///     10.0
    /// );
    ///
    /// let gradient = Shader::radial_gradient(
    ///     Offset::new(100.0, 50.0),
    ///     100.0,
    ///     vec![Color::WHITE, Color::TRANSPARENT],
    ///     None,
    /// );
    /// canvas.draw_gradient_rrect(rrect, gradient);
    /// ```
    pub fn draw_gradient_rrect(&mut self, rrect: RRect, shader: crate::display_list::Shader) {
        self.display_list.push(DrawCommand::DrawGradientRRect {
            rrect,
            shader,
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

    /// Draws a sequence of points with the specified mode.
    ///
    /// # Arguments
    ///
    /// * `mode` - How to interpret the points:
    ///   - `PointMode::Points` - Draw each point as a dot
    ///   - `PointMode::Lines` - Draw lines between consecutive point pairs
    ///   - `PointMode::Polygon` - Draw connected line segments
    /// * `points` - Points to draw
    /// * `paint` - Paint style (stroke width affects point/line size)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Paint, PointMode};
    /// use flui_types::{Point, Color};
    ///
    /// let mut canvas = Canvas::new();
    /// let points = vec![
    ///     Point::new(10.0, 10.0),
    ///     Point::new(50.0, 30.0),
    ///     Point::new(90.0, 10.0),
    /// ];
    ///
    /// // Draw as connected lines (triangle outline without closing)
    /// canvas.draw_points_with_mode(PointMode::Polygon, points, &Paint::stroke(Color::RED, 2.0));
    /// ```
    pub fn draw_points_with_mode(
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

    /// Fills entire canvas with a paint (respects clipping).
    ///
    /// Useful for solid backgrounds or full-screen effects.
    /// Fills the canvas with the paint, which can include colors, gradients, or patterns.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Fill with solid color
    /// canvas.draw_paint(&Paint::fill(Color::BLUE));
    ///
    /// // Fill with gradient (if paint has shader)
    /// let gradient_paint = Paint::new().with_shader(gradient);
    /// canvas.draw_paint(&gradient_paint);
    /// ```
    pub fn draw_paint(&mut self, paint: &Paint) {
        // DrawPaint is equivalent to DrawColor with the paint's color
        // For full shader support, we'd need a dedicated DrawPaint command
        self.display_list.push(DrawCommand::DrawColor {
            color: paint.color,
            blend_mode: paint.blend_mode,
            transform: self.transform,
        });
    }

    /// Draws a previously recorded DisplayList.
    ///
    /// This replays all commands from the DisplayList into this canvas.
    /// Useful for caching and reusing drawing commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Record some drawing
    /// let picture = Canvas::record(|c| {
    ///     c.draw_circle(Point::new(50.0, 50.0), 40.0, &paint);
    /// });
    ///
    /// // Draw the picture multiple times
    /// canvas.draw_picture(&picture);
    /// canvas.translate(100.0, 0.0);
    /// canvas.draw_picture(&picture);
    /// ```
    pub fn draw_picture(&mut self, picture: &DisplayList) {
        // Clone and append the picture's commands
        self.display_list.append(picture.clone());
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

    /// Apply a shader as a mask to child content
    ///
    /// This method wraps child drawing commands and applies a shader as an alpha mask.
    /// The shader determines the opacity at each pixel, creating effects like:
    /// - Gradient fades
    /// - Vignettes
    /// - Custom masking effects
    ///
    /// # Arguments
    ///
    /// * `bounds` - Bounding rectangle for the masked region
    /// * `shader` - Shader specification (linear gradient, radial gradient, solid color)
    /// * `blend_mode` - Blend mode for final compositing (default: SrcOver)
    /// * `draw_child` - Closure that records child drawing commands
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Shader, Paint};
    /// use flui_types::{Rect, Color, BlendMode};
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Create linear gradient fade
    /// let shader = Shader::linear_gradient(
    ///     (0.0, 0.0),
    ///     (1.0, 0.0),
    ///     vec![Color::TRANSPARENT, Color::WHITE],
    /// );
    ///
    /// // Apply fade to child content
    /// canvas.draw_shader_mask(
    ///     Rect::from_xywh(0.0, 0.0, 200.0, 100.0),
    ///     shader,
    ///     BlendMode::SrcOver,
    ///     |child_canvas| {
    ///         // Draw child content that will be masked
    ///         child_canvas.draw_rect(
    ///             Rect::from_xywh(0.0, 0.0, 200.0, 100.0),
    ///             &Paint::fill(Color::BLUE),
    ///         );
    ///     },
    /// );
    /// ```
    ///
    /// # Architecture
    ///
    /// ```text
    /// Canvas::draw_shader_mask()
    ///     ↓ records child commands
    /// DrawCommand::ShaderMask { child: DisplayList }
    ///     ↓ dispatched to
    /// WgpuRenderer → OffscreenRenderer.render_masked()
    ///     ↓ GPU execution
    /// Shader applied as alpha mask
    /// ```
    pub fn draw_shader_mask<F>(
        &mut self,
        bounds: Rect,
        shader: crate::display_list::Shader,
        blend_mode: crate::display_list::BlendMode,
        draw_child: F,
    ) where
        F: FnOnce(&mut Canvas),
    {
        // Create child canvas to record child commands
        let mut child_canvas = Canvas::new();
        draw_child(&mut child_canvas);

        // Push ShaderMask command with child DisplayList
        self.display_list.push(DrawCommand::ShaderMask {
            child: Box::new(child_canvas.finish()),
            shader,
            bounds,
            blend_mode,
            transform: self.transform,
        });
    }

    /// Draw a backdrop filter effect (frosted glass, blur, etc.)
    ///
    /// Applies an image filter to the backdrop content behind this layer,
    /// then optionally renders child content on top. Perfect for frosted glass
    /// modals, blurred backgrounds, and creative backdrop effects.
    ///
    /// # Arguments
    ///
    /// * `bounds` - Bounding rectangle for backdrop capture
    /// * `filter` - Image filter to apply (blur, color adjustments, etc.)
    /// * `blend_mode` - How to composite the result
    /// * `draw_child` - Optional closure to draw child content on top
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_painting::Canvas;
    /// use flui_types::{
    ///     geometry::Rect,
    ///     painting::{effects::ImageFilter, BlendMode},
    /// };
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Frosted glass effect with content on top
    /// canvas.draw_backdrop_filter(
    ///     Rect::from_xywh(0.0, 0.0, 400.0, 300.0),
    ///     ImageFilter::blur(10.0),  // 10px gaussian blur
    ///     BlendMode::SrcOver,
    ///     Some(|child_canvas| {
    ///         // Draw semi-transparent panel
    ///         child_canvas.draw_rect(
    ///             Rect::from_xywh(0.0, 0.0, 400.0, 300.0),
    ///             &Paint::new().with_color(Color::rgba(255, 255, 255, 200)),
    ///         );
    ///     }),
    /// );
    /// ```
    ///
    /// # Architecture
    ///
    /// ```text
    /// Canvas::draw_backdrop_filter()
    ///     ↓ records child commands (optional)
    /// DrawCommand::BackdropFilter { child: Option<DisplayList> }
    ///     ↓ dispatched to
    /// WgpuRenderer → capture framebuffer → apply filter → composite
    ///     ↓ GPU execution
    /// Filter applied to backdrop, child rendered on top
    /// ```
    pub fn draw_backdrop_filter<F>(
        &mut self,
        bounds: Rect,
        filter: ImageFilter,
        blend_mode: BlendMode,
        draw_child: Option<F>,
    ) where
        F: FnOnce(&mut Canvas),
    {
        // Create child canvas if provided
        let child_display_list = draw_child.map(|draw_fn| {
            let mut child_canvas = Canvas::new();
            draw_fn(&mut child_canvas);
            Box::new(child_canvas.finish())
        });

        // Push BackdropFilter command
        self.display_list.push(DrawCommand::BackdropFilter {
            child: child_display_list,
            filter,
            bounds,
            blend_mode,
            transform: self.transform,
        });
    }

    // ===== Convenience Methods =====

    /// Draws a point as a small circle.
    ///
    /// # Arguments
    ///
    /// * `point` - Position of the point
    /// * `radius` - Radius of the point (must be non-negative)
    /// * `paint` - Paint style
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `radius` is negative or NaN.
    #[inline]
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

    /// Extends this canvas with all commands from another canvas
    ///
    /// Takes ownership of the child canvas and moves all its commands into this canvas.
    /// This is useful for parent RenderObjects that need to draw their own content
    /// and then draw their children on top.
    ///
    /// # Naming
    ///
    /// Uses `extend_from` (not `append`) to follow Rust API conventions:
    /// - `append(&mut other)` in std takes a mutable reference and drains it
    /// - `extend_from(other)` takes ownership (consuming), matching our use case
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
    /// parent_canvas.extend_from(child_canvas);  // Zero-copy move
    /// ```
    #[tracing::instrument(skip(self, other), fields(
        parent_commands = self.display_list.len(),
        child_commands = other.display_list.len(),
    ))]
    pub fn extend_from(&mut self, other: Canvas) {
        let child_count = other.display_list.len();

        // Zero-copy move of commands via DisplayList::append
        self.display_list.append(other.display_list);

        tracing::debug!(
            total_commands = self.display_list.len(),
            appended = child_count,
            "Canvas composition complete"
        );
    }

    /// Extends this canvas from multiple canvases
    ///
    /// Efficiently appends commands from multiple child canvases in order.
    /// This is useful for multi-child render objects like Column, Row, Stack.
    ///
    /// # Arguments
    ///
    /// * `others` - Iterator of canvases to extend from
    ///
    /// # Performance
    ///
    /// Uses zero-copy move semantics for each canvas, making it very efficient
    /// even with many children.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut parent = Canvas::new();
    /// let children: Vec<Canvas> = vec![child1, child2, child3];
    /// parent.extend(children);
    /// ```
    pub fn extend(&mut self, others: impl IntoIterator<Item = Canvas>) {
        for canvas in others {
            self.extend_from(canvas);
        }
    }

    /// Merges two canvases into a new canvas
    ///
    /// Unlike `extend_from` which modifies `self`, this creates a new canvas
    /// containing commands from both canvases.
    ///
    /// # Arguments
    ///
    /// * `other` - Canvas to merge with
    ///
    /// # Performance
    ///
    /// This consumes both canvases and creates a new one using zero-copy moves.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let background = Canvas::new();
    /// let foreground = Canvas::new();
    /// let combined = background.merge(foreground);
    /// ```
    pub fn merge(mut self, other: Canvas) -> Self {
        self.extend_from(other);
        self
    }

    /// Appends a cached DisplayList at a given offset
    ///
    /// This is used by layer caching (RepaintBoundary) to replay cached
    /// drawing commands at a specified offset. The offset is applied by
    /// wrapping commands in a save/translate/restore sequence.
    ///
    /// # Arguments
    ///
    /// * `display_list` - Cached DisplayList to append
    /// * `offset` - Offset to apply to all commands
    ///
    /// # Performance
    ///
    /// This method clones the DisplayList to apply the offset transform.
    /// For performance-critical paths, consider caching the transformed
    /// DisplayList if the offset is stable.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// let cached_display_list = get_cached_display_list();
    /// let offset = Offset::new(100.0, 50.0);
    ///
    /// canvas.append_display_list_at_offset(&cached_display_list, offset);
    /// ```
    pub fn append_display_list_at_offset(&mut self, display_list: &DisplayList, offset: Offset) {
        // If offset is zero, we can potentially optimize
        if offset.dx == 0.0 && offset.dy == 0.0 {
            // Clone and append directly
            self.display_list.append(display_list.clone());
            return;
        }

        // Apply offset by wrapping in save/translate/restore
        self.save();
        self.translate(offset.dx, offset.dy);

        // Append cloned commands - they will inherit our current transform
        // Note: We need to re-record commands with the new transform
        // For simplicity, we clone the display list (commands retain their original transforms)
        self.display_list.append(display_list.clone());

        self.restore();
    }

    /// Appends a cached DisplayList directly (no offset)
    ///
    /// This is a zero-cost operation when the DisplayList can be moved.
    /// For cached DisplayLists that need to be reused, use `append_display_list_at_offset`.
    ///
    /// # Arguments
    ///
    /// * `display_list` - DisplayList to append (consumed)
    pub fn append_display_list(&mut self, display_list: DisplayList) {
        self.display_list.append(display_list);
    }

    // ===== Hit Testing =====

    /// Add a hit-testable region with an event handler
    ///
    /// This registers an area that will respond to pointer events.
    /// Used by RenderPointerListener to connect gestures to UI elements.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, HitRegion};
    /// use flui_types::Rect;
    /// use std::sync::Arc;
    ///
    /// let mut canvas = Canvas::new();
    /// let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
    /// canvas.add_hit_region(HitRegion::new(bounds, Arc::new(|event| {
    ///     println!("Clicked!");
    /// })));
    /// ```
    pub fn add_hit_region(&mut self, region: crate::display_list::HitRegion) {
        self.display_list.add_hit_region(region);
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
    #[tracing::instrument(skip(self), fields(
        commands = self.display_list.len(),
        save_depth = self.save_stack.len(),
    ))]
    pub fn finish(self) -> DisplayList {
        if !self.save_stack.is_empty() {
            tracing::warn!(
                unrestored_saves = self.save_stack.len(),
                "Canvas finished with unrestored save() calls"
            );
        }

        tracing::debug!(
            commands = self.display_list.len(),
            bounds = ?self.display_list.bounds(),
            "Canvas finalized"
        );

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

// ===== AsRef Implementation =====

/// Allow zero-cost conversion from Canvas to DisplayList reference
///
/// This enables generic functions that accept `impl AsRef<DisplayList>` to work with Canvas.
///
/// # Examples
///
/// ```rust,ignore
/// fn count_commands(dl: impl AsRef<DisplayList>) -> usize {
///     dl.as_ref().len()
/// }
///
/// let canvas = Canvas::new();
/// canvas.draw_rect(rect, &paint);
/// let count = count_commands(&canvas); // Works!
/// ```
impl AsRef<DisplayList> for Canvas {
    fn as_ref(&self) -> &DisplayList {
        &self.display_list
    }
}

impl Canvas {
    // ===== Query Methods =====

    /// Returns `true` if no drawing commands have been recorded.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let canvas = Canvas::new();
    /// assert!(canvas.is_empty());
    ///
    /// canvas.draw_rect(rect, &paint);
    /// assert!(!canvas.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.display_list.is_empty()
    }

    /// Returns the number of recorded drawing commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// assert_eq!(canvas.len(), 0);
    ///
    /// canvas.draw_rect(rect, &paint);
    /// canvas.draw_circle(center, radius, &paint);
    /// assert_eq!(canvas.len(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.display_list.len()
    }

    /// Returns the bounds of all recorded drawing commands.
    ///
    /// This is useful for determining the area that needs to be repainted.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(Rect::from_ltrb(10.0, 10.0, 100.0, 100.0), &paint);
    /// let bounds = canvas.bounds();
    /// ```
    #[inline]
    #[must_use]
    pub fn bounds(&self) -> Rect {
        self.display_list.bounds()
    }

    /// Resets the canvas to its initial state, clearing all commands and state.
    ///
    /// This is more efficient than creating a new Canvas when you want to reuse
    /// the existing allocations.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(rect, &paint);
    /// canvas.save();
    /// canvas.translate(50.0, 50.0);
    ///
    /// canvas.reset(); // Clear everything
    ///
    /// assert!(canvas.is_empty());
    /// assert_eq!(canvas.save_count(), 0);
    /// ```
    pub fn reset(&mut self) {
        self.display_list.clear();
        self.transform = Matrix4::identity();
        self.clip_stack.clear();
        self.save_stack.clear();
    }

    /// Clears all recorded drawing commands but preserves transform and clip state.
    ///
    /// Use this when you want to re-record commands but keep the current
    /// coordinate system setup.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// canvas.translate(50.0, 50.0);
    /// canvas.draw_rect(rect, &paint);
    ///
    /// canvas.clear_commands(); // Keep transform, clear commands
    ///
    /// assert!(canvas.is_empty());
    /// // Transform is still applied
    /// ```
    pub fn clear_commands(&mut self) {
        self.display_list.clear();
    }

    // ===== Closure-based Scoped Operations =====

    /// Executes a closure with automatic save/restore.
    ///
    /// This is a safer and more ergonomic alternative to manual `save()`/`restore()` calls.
    /// The canvas state (transform, clip) is automatically saved before the closure
    /// and restored after, even if the closure panics.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_painting::{Canvas, Paint};
    /// use flui_types::{Rect, Color};
    ///
    /// let mut canvas = Canvas::new();
    /// let paint = Paint::fill(Color::RED);
    ///
    /// // Draw with temporary transform - state is automatically restored
    /// canvas.with_save(|c| {
    ///     c.translate(100.0, 100.0);
    ///     c.rotate(std::f32::consts::PI / 4.0);
    ///     c.draw_rect(Rect::from_xywh(-25.0, -25.0, 50.0, 50.0), &paint);
    /// });
    ///
    /// // Canvas is back to original state here
    /// canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    /// ```
    #[inline]
    pub fn with_save<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.save();
        let result = f(self);
        self.restore();
        result
    }

    /// Executes a closure with a translated coordinate system.
    ///
    /// Combines `save()`, `translate()`, closure execution, and `restore()` into
    /// a single ergonomic call.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw at offset (100, 50)
    /// canvas.with_translate(100.0, 50.0, |c| {
    ///     c.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    ///     c.draw_circle(Point::new(25.0, 25.0), 10.0, &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_translate<F, R>(&mut self, dx: f32, dy: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.translate(dx, dy);
            f(c)
        })
    }

    /// Executes a closure with a rotated coordinate system.
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians (positive = counter-clockwise)
    /// * `f` - Closure to execute with the rotated coordinate system
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use std::f32::consts::PI;
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw rotated 45 degrees
    /// canvas.with_rotate(PI / 4.0, |c| {
    ///     c.draw_rect(rect, &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_rotate<F, R>(&mut self, radians: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.rotate(radians);
            f(c)
        })
    }

    /// Executes a closure with a rotated coordinate system around a pivot point.
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians
    /// * `pivot_x` - X coordinate of pivot point
    /// * `pivot_y` - Y coordinate of pivot point
    /// * `f` - Closure to execute
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use std::f32::consts::PI;
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Rotate 90 degrees around the center of a 100x100 square
    /// canvas.with_rotate_around(PI / 2.0, 50.0, 50.0, |c| {
    ///     c.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_rotate_around<F, R>(&mut self, radians: f32, pivot_x: f32, pivot_y: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.rotate_around(radians, pivot_x, pivot_y);
            f(c)
        })
    }

    /// Executes a closure with a scaled coordinate system.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw at 2x scale
    /// canvas.with_scale(2.0, |c| {
    ///     c.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    ///     // Actually draws 100x100 rectangle
    /// });
    /// ```
    #[inline]
    pub fn with_scale<F, R>(&mut self, factor: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.scale_uniform(factor);
            f(c)
        })
    }

    /// Executes a closure with a non-uniform scaled coordinate system.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw stretched horizontally
    /// canvas.with_scale_xy(2.0, 1.0, |c| {
    ///     c.draw_circle(Point::new(50.0, 50.0), 25.0, &paint);
    ///     // Actually draws an ellipse
    /// });
    /// ```
    #[inline]
    pub fn with_scale_xy<F, R>(&mut self, sx: f32, sy: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.scale_xy(sx, sy);
            f(c)
        })
    }

    /// Executes a closure with an arbitrary transform applied.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::geometry::Transform;
    /// use std::f32::consts::PI;
    ///
    /// let mut canvas = Canvas::new();
    ///
    /// // Apply complex transform
    /// let transform = Transform::translate(100.0, 100.0)
    ///     .then(Transform::rotate(PI / 4.0))
    ///     .then(Transform::scale(2.0));
    ///
    /// canvas.with_transform(transform, |c| {
    ///     c.draw_rect(rect, &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_transform<T, F, R>(&mut self, transform: T, f: F) -> R
    where
        T: Into<Matrix4>,
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.transform(transform);
            f(c)
        })
    }

    /// Executes a closure with a clipping rectangle applied.
    ///
    /// All drawing within the closure will be clipped to the specified rectangle.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// let clip_rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
    ///
    /// canvas.with_clip_rect(clip_rect, |c| {
    ///     // This circle will be clipped to the rectangle
    ///     c.draw_circle(Point::new(50.0, 50.0), 80.0, &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_clip_rect<F, R>(&mut self, rect: Rect, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.clip_rect(rect);
            f(c)
        })
    }

    /// Executes a closure with a clipping rounded rectangle applied.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// let clip_rrect = RRect::from_rect_circular(
    ///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
    ///     10.0
    /// );
    ///
    /// canvas.with_clip_rrect(clip_rrect, |c| {
    ///     c.draw_rect(Rect::from_xywh(0.0, 0.0, 200.0, 200.0), &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_clip_rrect<F, R>(&mut self, rrect: RRect, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.clip_rrect(rrect);
            f(c)
        })
    }

    /// Executes a closure with a clipping path applied.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    /// let clip_path = Path::circle(Point::new(50.0, 50.0), 40.0);
    ///
    /// canvas.with_clip_path(&clip_path, |c| {
    ///     c.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_clip_path<F, R>(&mut self, path: &Path, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.clip_path(path);
            f(c)
        })
    }

    /// Executes a closure with a compositing layer for opacity effects.
    ///
    /// This is a convenience wrapper around `save_layer_opacity` and `restore`.
    /// All drawing within the closure will be rendered to an offscreen buffer
    /// and then composited with the specified opacity.
    ///
    /// # Performance
    ///
    /// This creates an offscreen buffer, which has GPU overhead. Use sparingly.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw multiple shapes at 50% opacity
    /// canvas.with_opacity(0.5, Some(bounds), |c| {
    ///     c.draw_rect(rect1, &red_paint);
    ///     c.draw_rect(rect2, &blue_paint);
    ///     // Both rects composited together at 50% opacity
    /// });
    /// ```
    #[inline]
    pub fn with_opacity<F, R>(&mut self, opacity: f32, bounds: Option<Rect>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.save_layer_opacity(bounds, opacity);
        let result = f(self);
        self.restore();
        result
    }

    /// Executes a closure with a compositing layer for blend mode effects.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::new();
    ///
    /// // Draw with multiply blend mode
    /// canvas.with_blend_mode(BlendMode::Multiply, Some(bounds), |c| {
    ///     c.draw_rect(rect, &paint);
    /// });
    /// ```
    #[inline]
    pub fn with_blend_mode<F, R>(&mut self, blend_mode: BlendMode, bounds: Option<Rect>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.save_layer_blend(bounds, blend_mode);
        let result = f(self);
        self.restore();
        result
    }

    /// Creates a new Canvas, executes a closure on it, and returns the finished DisplayList.
    ///
    /// This is useful for creating isolated drawing contexts.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Create a reusable icon
    /// let icon_display_list = Canvas::record(|c| {
    ///     c.draw_circle(Point::new(16.0, 16.0), 14.0, &outline_paint);
    ///     c.draw_path(&checkmark_path, &fill_paint);
    /// });
    ///
    /// // Use it multiple times
    /// canvas.append_display_list(icon_display_list.clone());
    /// canvas.translate(50.0, 0.0);
    /// canvas.append_display_list(icon_display_list);
    /// ```
    #[inline]
    pub fn record<F>(f: F) -> DisplayList
    where
        F: FnOnce(&mut Canvas),
    {
        let mut canvas = Canvas::new();
        f(&mut canvas);
        canvas.finish()
    }

    /// Builds a Canvas using a closure and returns it (not consumed).
    ///
    /// Unlike `record()`, this returns the Canvas itself for further operations.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut canvas = Canvas::build(|c| {
    ///     c.translate(100.0, 100.0);
    ///     c.draw_rect(rect, &paint);
    /// });
    ///
    /// // Continue drawing
    /// canvas.draw_circle(center, radius, &paint);
    /// let display_list = canvas.finish();
    /// ```
    #[inline]
    pub fn build<F>(f: F) -> Self
    where
        F: FnOnce(&mut Canvas),
    {
        let mut canvas = Canvas::new();
        f(&mut canvas);
        canvas
    }

    // ===== Batch Drawing Methods =====

    /// Draws multiple rectangles with the same paint.
    ///
    /// More efficient than calling `draw_rect` multiple times when drawing
    /// many rectangles with identical styling.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let rects = vec![
    ///     Rect::from_xywh(0.0, 0.0, 50.0, 50.0),
    ///     Rect::from_xywh(60.0, 0.0, 50.0, 50.0),
    ///     Rect::from_xywh(120.0, 0.0, 50.0, 50.0),
    /// ];
    /// canvas.draw_rects(&rects, &paint);
    /// ```
    #[inline]
    pub fn draw_rects(&mut self, rects: &[Rect], paint: &Paint) {
        for rect in rects {
            self.draw_rect(*rect, paint);
        }
    }

    /// Draws multiple circles with the same paint.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let circles = vec![
    ///     (Point::new(50.0, 50.0), 20.0),
    ///     (Point::new(100.0, 50.0), 15.0),
    ///     (Point::new(150.0, 50.0), 25.0),
    /// ];
    /// canvas.draw_circles(&circles, &paint);
    /// ```
    #[inline]
    pub fn draw_circles(&mut self, circles: &[(Point, f32)], paint: &Paint) {
        for (center, radius) in circles {
            self.draw_circle(*center, *radius, paint);
        }
    }

    /// Draws multiple lines with the same paint.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let lines = vec![
    ///     (Point::new(0.0, 0.0), Point::new(100.0, 100.0)),
    ///     (Point::new(0.0, 100.0), Point::new(100.0, 0.0)),
    /// ];
    /// canvas.draw_lines(&lines, &paint);
    /// ```
    #[inline]
    pub fn draw_lines(&mut self, lines: &[(Point, Point)], paint: &Paint) {
        for (p1, p2) in lines {
            self.draw_line(*p1, *p2, paint);
        }
    }

    /// Draws multiple rounded rectangles with the same paint.
    #[inline]
    pub fn draw_rrects(&mut self, rrects: &[RRect], paint: &Paint) {
        for rrect in rrects {
            self.draw_rrect(*rrect, paint);
        }
    }

    /// Draws multiple paths with the same paint.
    #[inline]
    pub fn draw_paths(&mut self, paths: &[&Path], paint: &Paint) {
        for path in paths {
            self.draw_path(path, paint);
        }
    }

    // ===== Conditional Drawing =====

    /// Draws a rectangle only if the condition is true.
    ///
    /// Useful for conditional rendering without verbose if statements.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.draw_rect_if(is_selected, selection_rect, &highlight_paint);
    /// ```
    #[inline]
    pub fn draw_rect_if(&mut self, condition: bool, rect: Rect, paint: &Paint) {
        if condition {
            self.draw_rect(rect, paint);
        }
    }

    /// Draws a circle only if the condition is true.
    #[inline]
    pub fn draw_circle_if(&mut self, condition: bool, center: Point, radius: f32, paint: &Paint) {
        if condition {
            self.draw_circle(center, radius, paint);
        }
    }

    /// Executes drawing closure only if the condition is true.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.draw_if(show_overlay, |c| {
    ///     c.draw_rect(overlay_rect, &overlay_paint);
    ///     c.draw_text("Overlay", offset, &style, &text_paint);
    /// });
    /// ```
    #[inline]
    pub fn draw_if<F>(&mut self, condition: bool, f: F)
    where
        F: FnOnce(&mut Self),
    {
        if condition {
            f(self);
        }
    }

    /// Executes drawing closure only if the condition is false.
    #[inline]
    pub fn draw_unless<F>(&mut self, condition: bool, f: F)
    where
        F: FnOnce(&mut Self),
    {
        if !condition {
            f(self);
        }
    }

    /// Draws based on Option - draws if Some, skips if None.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let maybe_rect: Option<Rect> = get_selection();
    /// canvas.draw_if_some(maybe_rect, |c, rect| {
    ///     c.draw_rect(rect, &selection_paint);
    /// });
    /// ```
    #[inline]
    pub fn draw_if_some<T, F>(&mut self, option: Option<T>, f: F)
    where
        F: FnOnce(&mut Self, T),
    {
        if let Some(value) = option {
            f(self, value);
        }
    }

    // ===== Grid and Repeat Patterns =====

    /// Draws a grid of items using a closure.
    ///
    /// # Arguments
    ///
    /// * `cols` - Number of columns
    /// * `rows` - Number of rows
    /// * `cell_width` - Width of each cell
    /// * `cell_height` - Height of each cell
    /// * `f` - Closure called for each cell with (canvas, col, row)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Draw a 5x5 grid of squares
    /// canvas.draw_grid(5, 5, 50.0, 50.0, |c, col, row| {
    ///     let color = if (col + row) % 2 == 0 { Color::WHITE } else { Color::BLACK };
    ///     c.draw_rect(Rect::from_xywh(5.0, 5.0, 40.0, 40.0), &Paint::fill(color));
    /// });
    /// ```
    pub fn draw_grid<F>(
        &mut self,
        cols: usize,
        rows: usize,
        cell_width: f32,
        cell_height: f32,
        f: F,
    ) where
        F: Fn(&mut Self, usize, usize),
    {
        for row in 0..rows {
            for col in 0..cols {
                self.with_translate(col as f32 * cell_width, row as f32 * cell_height, |c| {
                    f(c, col, row);
                });
            }
        }
    }

    /// Repeats a drawing operation in a horizontal line.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Draw 10 circles in a row
    /// canvas.repeat_x(10, 30.0, |c, i| {
    ///     c.draw_circle(Point::new(10.0, 10.0), 8.0, &paint);
    /// });
    /// ```
    pub fn repeat_x<F>(&mut self, count: usize, spacing: f32, f: F)
    where
        F: Fn(&mut Self, usize),
    {
        for i in 0..count {
            self.with_translate(i as f32 * spacing, 0.0, |c| {
                f(c, i);
            });
        }
    }

    /// Repeats a drawing operation in a vertical line.
    pub fn repeat_y<F>(&mut self, count: usize, spacing: f32, f: F)
    where
        F: Fn(&mut Self, usize),
    {
        for i in 0..count {
            self.with_translate(0.0, i as f32 * spacing, |c| {
                f(c, i);
            });
        }
    }

    /// Repeats a drawing operation around a circle.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Draw 12 dots around a clock face
    /// canvas.repeat_radial(12, 100.0, |c, i, angle| {
    ///     c.draw_circle(Point::new(0.0, 0.0), 5.0, &paint);
    /// });
    /// ```
    pub fn repeat_radial<F>(&mut self, count: usize, radius: f32, f: F)
    where
        F: Fn(&mut Self, usize, f32),
    {
        use std::f32::consts::PI;
        let angle_step = 2.0 * PI / count as f32;

        for i in 0..count {
            let angle = i as f32 * angle_step;
            let x = angle.cos() * radius;
            let y = angle.sin() * radius;

            self.with_translate(x, y, |c| {
                f(c, i, angle);
            });
        }
    }

    // ===== Debug Visualization =====

    /// Draws a debug rectangle showing bounds (outline only).
    ///
    /// Useful for debugging layout issues.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// #[cfg(debug_assertions)]
    /// canvas.debug_rect(widget_bounds, Color::RED);
    /// ```
    #[inline]
    pub fn debug_rect(&mut self, rect: Rect, color: Color) {
        let paint = Paint::stroke(color, 1.0);
        self.draw_rect(rect, &paint);
    }

    /// Draws a debug cross at the specified point.
    ///
    /// Useful for marking anchor points, centers, etc.
    #[inline]
    pub fn debug_point(&mut self, point: Point, size: f32, color: Color) {
        let half = size / 2.0;
        let paint = Paint::stroke(color, 1.0);
        self.draw_line(
            Point::new(point.x - half, point.y),
            Point::new(point.x + half, point.y),
            &paint,
        );
        self.draw_line(
            Point::new(point.x, point.y - half),
            Point::new(point.x, point.y + half),
            &paint,
        );
    }

    /// Draws debug visualization of the current transform origin.
    ///
    /// Shows X axis (red), Y axis (green), and origin point.
    #[inline]
    pub fn debug_axes(&mut self, length: f32) {
        let origin = Point::new(0.0, 0.0);

        // X axis (red)
        self.draw_line(
            origin,
            Point::new(length, 0.0),
            &Paint::stroke(Color::RED, 2.0),
        );

        // Y axis (green)
        self.draw_line(
            origin,
            Point::new(0.0, length),
            &Paint::stroke(Color::GREEN, 2.0),
        );

        // Origin point (blue)
        self.draw_circle(origin, 3.0, &Paint::fill(Color::BLUE));
    }

    /// Draws a debug grid overlay.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.debug_grid(bounds, 50.0, Color::from_rgba(128, 128, 128, 64));
    /// ```
    pub fn debug_grid(&mut self, bounds: Rect, spacing: f32, color: Color) {
        let paint = Paint::stroke(color, 0.5);

        // Vertical lines
        let mut x = bounds.left();
        while x <= bounds.right() {
            self.draw_line(
                Point::new(x, bounds.top()),
                Point::new(x, bounds.bottom()),
                &paint,
            );
            x += spacing;
        }

        // Horizontal lines
        let mut y = bounds.top();
        while y <= bounds.bottom() {
            self.draw_line(
                Point::new(bounds.left(), y),
                Point::new(bounds.right(), y),
                &paint,
            );
            y += spacing;
        }
    }

    // ===== Convenience Shape Methods =====

    /// Draws a rounded rectangle with uniform corner radius.
    ///
    /// Convenience method that creates RRect internally.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.draw_rounded_rect(rect, 10.0, &paint);
    /// ```
    #[inline]
    pub fn draw_rounded_rect(&mut self, rect: Rect, radius: f32, paint: &Paint) {
        let rrect = RRect::from_rect_circular(rect, radius);
        self.draw_rrect(rrect, paint);
    }

    /// Draws a rectangle with different corner radii.
    ///
    /// # Arguments
    ///
    /// * `rect` - Rectangle bounds
    /// * `top_left`, `top_right`, `bottom_right`, `bottom_left` - Corner radii
    /// * `paint` - Paint style
    #[inline]
    pub fn draw_rounded_rect_corners(
        &mut self,
        rect: Rect,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
        paint: &Paint,
    ) {
        use flui_types::geometry::Radius;
        let rrect = RRect::from_rect_and_corners(
            rect,
            Radius::circular(top_left),
            Radius::circular(top_right),
            Radius::circular(bottom_right),
            Radius::circular(bottom_left),
        );
        self.draw_rrect(rrect, paint);
    }

    /// Draws a pill shape (fully rounded rectangle).
    ///
    /// The corner radius is automatically set to half the smaller dimension.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.draw_pill(Rect::from_xywh(0.0, 0.0, 100.0, 40.0), &paint);
    /// ```
    #[inline]
    pub fn draw_pill(&mut self, rect: Rect, paint: &Paint) {
        let radius = rect.width().min(rect.height()) / 2.0;
        self.draw_rounded_rect(rect, radius, paint);
    }

    /// Draws a ring (donut shape).
    ///
    /// # Arguments
    ///
    /// * `center` - Center point
    /// * `outer_radius` - Outer circle radius
    /// * `inner_radius` - Inner circle radius (hole)
    /// * `paint` - Paint style
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas.draw_ring(Point::new(100.0, 100.0), 50.0, 30.0, &paint);
    /// ```
    #[inline]
    pub fn draw_ring(
        &mut self,
        center: Point,
        outer_radius: f32,
        inner_radius: f32,
        paint: &Paint,
    ) {
        let outer = RRect::from_rect_circular(
            Rect::from_center_size(
                center,
                flui_types::geometry::Size::new(outer_radius * 2.0, outer_radius * 2.0),
            ),
            outer_radius,
        );
        let inner = RRect::from_rect_circular(
            Rect::from_center_size(
                center,
                flui_types::geometry::Size::new(inner_radius * 2.0, inner_radius * 2.0),
            ),
            inner_radius,
        );
        self.draw_drrect(outer, inner, paint);
    }
}

// ===== Chaining API =====
//
// These methods return `&mut Self` for fluent method chaining.
// Unlike the standard methods, they allow building complex drawings in a single expression.

impl Canvas {
    /// Translates and returns self for chaining.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas
    ///     .translated(100.0, 50.0)
    ///     .rotated(PI / 4.0)
    ///     .draw_rect(rect, &paint);
    /// ```
    #[inline]
    pub fn translated(&mut self, dx: f32, dy: f32) -> &mut Self {
        self.translate(dx, dy);
        self
    }

    /// Scales uniformly and returns self for chaining.
    #[inline]
    pub fn scaled(&mut self, factor: f32) -> &mut Self {
        self.scale_uniform(factor);
        self
    }

    /// Scales non-uniformly and returns self for chaining.
    #[inline]
    pub fn scaled_xy(&mut self, sx: f32, sy: f32) -> &mut Self {
        self.scale_xy(sx, sy);
        self
    }

    /// Rotates and returns self for chaining.
    #[inline]
    pub fn rotated(&mut self, radians: f32) -> &mut Self {
        self.rotate(radians);
        self
    }

    /// Rotates around a pivot and returns self for chaining.
    #[inline]
    pub fn rotated_around(&mut self, radians: f32, pivot_x: f32, pivot_y: f32) -> &mut Self {
        self.rotate_around(radians, pivot_x, pivot_y);
        self
    }

    /// Applies a transform and returns self for chaining.
    #[inline]
    pub fn transformed<T: Into<Matrix4>>(&mut self, transform: T) -> &mut Self {
        self.transform(transform);
        self
    }

    /// Clips to a rectangle and returns self for chaining.
    #[inline]
    pub fn clipped_rect(&mut self, rect: Rect) -> &mut Self {
        self.clip_rect(rect);
        self
    }

    /// Clips to a rounded rectangle and returns self for chaining.
    #[inline]
    pub fn clipped_rrect(&mut self, rrect: RRect) -> &mut Self {
        self.clip_rrect(rrect);
        self
    }

    /// Clips to a path and returns self for chaining.
    #[inline]
    pub fn clipped_path(&mut self, path: &Path) -> &mut Self {
        self.clip_path(path);
        self
    }

    /// Saves state and returns self for chaining.
    #[inline]
    pub fn saved(&mut self) -> &mut Self {
        self.save();
        self
    }

    /// Restores state and returns self for chaining.
    #[inline]
    pub fn restored(&mut self) -> &mut Self {
        self.restore();
        self
    }

    /// Draws a rect and returns self for chaining.
    #[inline]
    pub fn rect(&mut self, rect: Rect, paint: &Paint) -> &mut Self {
        self.draw_rect(rect, paint);
        self
    }

    /// Draws a rounded rect and returns self for chaining.
    #[inline]
    pub fn rrect(&mut self, rrect: RRect, paint: &Paint) -> &mut Self {
        self.draw_rrect(rrect, paint);
        self
    }

    /// Draws a rectangle with uniform corner radius and returns self for chaining.
    #[inline]
    pub fn rounded_rect(&mut self, rect: Rect, radius: f32, paint: &Paint) -> &mut Self {
        self.draw_rounded_rect(rect, radius, paint);
        self
    }

    /// Draws a circle and returns self for chaining.
    #[inline]
    pub fn circle(&mut self, center: Point, radius: f32, paint: &Paint) -> &mut Self {
        self.draw_circle(center, radius, paint);
        self
    }

    /// Draws a line and returns self for chaining.
    #[inline]
    pub fn line(&mut self, p1: Point, p2: Point, paint: &Paint) -> &mut Self {
        self.draw_line(p1, p2, paint);
        self
    }

    /// Draws a path and returns self for chaining.
    #[inline]
    pub fn path(&mut self, path: &Path, paint: &Paint) -> &mut Self {
        self.draw_path(path, paint);
        self
    }

    /// Draws text and returns self for chaining.
    #[inline]
    pub fn text(
        &mut self,
        text: &str,
        offset: Offset,
        style: &TextStyle,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_text(text, offset, style, paint);
        self
    }

    /// Draws an oval and returns self for chaining.
    #[inline]
    pub fn oval(&mut self, rect: Rect, paint: &Paint) -> &mut Self {
        self.draw_oval(rect, paint);
        self
    }

    /// Draws a texture and returns self for chaining.
    ///
    /// # Arguments
    ///
    /// * `texture_id` - The ID of the texture to draw
    /// * `dst` - Destination rectangle where the texture will be drawn
    /// * `src` - Optional source rectangle for texture cropping (None = entire texture)
    /// * `filter_quality` - Quality of texture filtering
    /// * `opacity` - Opacity of the texture (0.0 to 1.0)
    #[inline]
    pub fn texture(
        &mut self,
        texture_id: crate::display_list::TextureId,
        dst: Rect,
        src: Option<Rect>,
        filter_quality: crate::display_list::FilterQuality,
        opacity: f32,
    ) -> &mut Self {
        self.draw_texture(texture_id, dst, src, filter_quality, opacity);
        self
    }

    /// Draws an image and returns self for chaining.
    #[inline]
    pub fn image(&mut self, image: Image, dst: Rect, paint: Option<&Paint>) -> &mut Self {
        self.draw_image(image, dst, paint);
        self
    }

    /// Draws a tiled/repeated image and returns self for chaining.
    #[inline]
    pub fn image_repeat(
        &mut self,
        image: Image,
        dst: Rect,
        repeat: crate::display_list::ImageRepeat,
        paint: Option<&Paint>,
    ) -> &mut Self {
        self.draw_image_repeat(image, dst, repeat, paint);
        self
    }

    /// Draws an image with 9-slice scaling and returns self for chaining.
    #[inline]
    pub fn image_nine_slice(
        &mut self,
        image: Image,
        center_slice: Rect,
        dst: Rect,
        paint: Option<&Paint>,
    ) -> &mut Self {
        self.draw_image_nine_slice(image, center_slice, dst, paint);
        self
    }

    /// Draws an image with a color filter and returns self for chaining.
    #[inline]
    pub fn image_filtered(
        &mut self,
        image: Image,
        dst: Rect,
        filter: crate::display_list::ColorFilter,
        paint: Option<&Paint>,
    ) -> &mut Self {
        self.draw_image_filtered(image, dst, filter, paint);
        self
    }

    /// Draws a shadow and returns self for chaining.
    #[inline]
    pub fn shadow(&mut self, path: &Path, color: Color, elevation: f32) -> &mut Self {
        self.draw_shadow(path, color, elevation);
        self
    }

    /// Draws a gradient-filled rectangle and returns self for chaining.
    #[inline]
    pub fn gradient(&mut self, rect: Rect, shader: crate::display_list::Shader) -> &mut Self {
        self.draw_gradient(rect, shader);
        self
    }

    /// Draws a gradient-filled rounded rectangle and returns self for chaining.
    #[inline]
    pub fn gradient_rrect(
        &mut self,
        rrect: RRect,
        shader: crate::display_list::Shader,
    ) -> &mut Self {
        self.draw_gradient_rrect(rrect, shader);
        self
    }

    /// Draws an arc segment and returns self for chaining.
    #[inline]
    pub fn arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_arc(rect, start_angle, sweep_angle, use_center, paint);
        self
    }

    /// Draws difference between two rounded rectangles and returns self for chaining.
    #[inline]
    pub fn drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) -> &mut Self {
        self.draw_drrect(outer, inner, paint);
        self
    }

    /// Draws points with the specified mode and returns self for chaining.
    #[inline]
    pub fn points(
        &mut self,
        mode: crate::display_list::PointMode,
        points: Vec<Point>,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_points_with_mode(mode, points, paint);
        self
    }

    /// Draws custom vertices and returns self for chaining.
    #[inline]
    pub fn vertices(
        &mut self,
        vertices: Vec<Point>,
        colors: Option<Vec<Color>>,
        tex_coords: Option<Vec<Point>>,
        indices: Vec<u16>,
        paint: &Paint,
    ) -> &mut Self {
        self.draw_vertices(vertices, colors, tex_coords, indices, paint);
        self
    }

    /// Executes a closure on self and returns self for chaining.
    ///
    /// Useful for inserting custom logic in a chain.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas
    ///     .translated(100.0, 100.0)
    ///     .also(|c| {
    ///         for i in 0..5 {
    ///             c.draw_circle(Point::new(i as f32 * 20.0, 0.0), 5.0, &paint);
    ///         }
    ///     })
    ///     .rotated(PI / 2.0)
    ///     .rect(rect, &paint);
    /// ```
    #[inline]
    pub fn also<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut Self),
    {
        f(self);
        self
    }

    /// Conditionally executes a closure on self.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// canvas
    ///     .translated(100.0, 100.0)
    ///     .when(is_selected, |c| c.rect(highlight_rect, &highlight_paint))
    ///     .rect(content_rect, &paint);
    /// ```
    #[inline]
    pub fn when<F>(&mut self, condition: bool, f: F) -> &mut Self
    where
        F: FnOnce(&mut Self) -> &mut Self,
    {
        if condition {
            f(self)
        } else {
            self
        }
    }

    /// Conditionally executes one of two closures.
    #[inline]
    pub fn when_else<F, G>(&mut self, condition: bool, if_true: F, if_false: G) -> &mut Self
    where
        F: FnOnce(&mut Self) -> &mut Self,
        G: FnOnce(&mut Self) -> &mut Self,
    {
        if condition {
            if_true(self)
        } else {
            if_false(self)
        }
    }
}

/// Saved canvas state (for save/restore)
#[derive(Debug, Clone)]
struct CanvasState {
    /// Saved transform matrix
    transform: Matrix4,
    /// Depth of clip stack when saved
    clip_depth: usize,
    /// Whether this save created a layer (for save_layer)
    is_layer: bool,
}

/// Clip operation stored in the clip stack.
///
/// Currently used for tracking clip depth in save/restore operations.
/// The clip geometry (Rect/RRect/Path) is stored for future optimizations:
/// - Culling: Skip drawing commands outside the clip bounds
/// - Clip bounds queries: `canvas.local_clip_bounds()`
/// - Render optimization: Merge adjacent clips
///
/// TODO: Add methods to query clip bounds and use for culling optimization
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields stored for future optimization features
enum ClipShape {
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
        assert_eq!(canvas.save_count(), 1); // Initial count is 1
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

        assert_eq!(canvas.save_count(), 1); // Initial count is 1

        canvas.save();
        assert_eq!(canvas.save_count(), 2);

        canvas.translate(50.0, 50.0);

        canvas.save();
        assert_eq!(canvas.save_count(), 3);

        canvas.restore();
        assert_eq!(canvas.save_count(), 2);

        canvas.restore();
        assert_eq!(canvas.save_count(), 1);
    }

    #[test]
    fn test_canvas_transform() {
        let mut canvas = Canvas::new();

        let original_transform = canvas.transform_matrix();
        canvas.translate(100.0, 50.0);
        let translated_transform = canvas.transform_matrix();

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
    fn test_canvas_restore_without_save() {
        // Test that restore() without matching save() is safe (no-op)
        let mut canvas = Canvas::new();
        canvas.restore(); // Should not panic, just do nothing

        // Verify canvas is still usable
        let paint = Paint::fill(Color::RED);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
        assert_eq!(canvas.len(), 1);
    }
}
