//! PaintingContext for recording paint commands.

use flui_types::{Offset, Rect};

use crate::traits::{RenderBox, RenderSliver};

// ============================================================================
// PaintingContext
// ============================================================================

/// A context for painting render objects.
///
/// Provides a canvas for recording paint commands and methods for
/// painting child render objects with proper layer management.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `PaintingContext` class in
/// `rendering/object.dart`.
///
/// # Usage
///
/// ```ignore
/// fn paint(&self, context: &mut PaintingContext, offset: Offset) {
///     // Paint background
///     context.canvas().draw_rect(Rect::from_size(self.size()), paint);
///
///     // Paint child
///     if let Some(child) = self.child() {
///         context.paint_child(child, offset + child_offset);
///     }
/// }
/// ```
#[derive(Debug)]
pub struct PaintingContext {
    /// Estimated bounds for painting.
    estimated_bounds: Rect,

    /// Whether recording has started.
    is_recording: bool,
}

impl PaintingContext {
    /// Creates a new painting context.
    pub fn new(estimated_bounds: Rect) -> Self {
        Self {
            estimated_bounds,
            is_recording: false,
        }
    }

    /// Returns the estimated bounds for this context.
    pub fn estimated_bounds(&self) -> Rect {
        self.estimated_bounds
    }

    // ========================================================================
    // Child Painting
    // ========================================================================

    /// Paints a child box render object.
    ///
    /// This handles layer management - if the child is a repaint boundary,
    /// it will be painted into its own layer.
    pub fn paint_child(&mut self, child: &dyn RenderBox, offset: Offset) {
        // TODO: Handle repaint boundaries and layers
        let _ = (child, offset);
    }

    /// Paints a child sliver render object.
    ///
    /// Similar to `paint_child` but for sliver protocol.
    pub fn paint_sliver_child(&mut self, child: &dyn RenderSliver, offset: Offset) {
        // TODO: Handle repaint boundaries and layers for slivers
        let _ = (child, offset);
    }

    // ========================================================================
    // Layer Operations
    // ========================================================================

    /// Pushes an opacity layer.
    ///
    /// All painting within the callback will be rendered with the given opacity.
    pub fn push_opacity<F>(&mut self, offset: Offset, alpha: u8, painter: F)
    where
        F: FnOnce(&mut PaintingContext),
    {
        let _ = (offset, alpha);
        // TODO: Create opacity layer
        painter(self);
    }

    /// Pushes a clip rect layer.
    ///
    /// All painting within the callback will be clipped to the given rect.
    pub fn push_clip_rect<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        clip_rect: Rect,
        painter: F,
    ) where
        F: FnOnce(&mut PaintingContext),
    {
        let _ = (needs_compositing, offset, clip_rect);
        // TODO: Create clip layer
        painter(self);
    }

    /// Pushes a transform layer.
    ///
    /// All painting within the callback will have the transform applied.
    pub fn push_transform<F>(
        &mut self,
        needs_compositing: bool,
        offset: Offset,
        transform: &[f32; 16],
        painter: F,
    ) where
        F: FnOnce(&mut PaintingContext),
    {
        let _ = (needs_compositing, offset, transform);
        // TODO: Create transform layer
        painter(self);
    }

    // ========================================================================
    // Canvas Access
    // ========================================================================

    /// Returns a canvas for direct drawing.
    ///
    /// # Warning
    ///
    /// The canvas may change after painting children (due to layer creation).
    /// Do not cache the canvas reference across child paint calls.
    pub fn canvas(&mut self) -> Canvas {
        self.is_recording = true;
        Canvas::new()
    }

    /// Stops recording if needed.
    pub fn stop_recording_if_needed(&mut self) {
        if self.is_recording {
            self.is_recording = false;
            // TODO: Finalize current picture
        }
    }
}

// ============================================================================
// Canvas
// ============================================================================

/// A canvas for recording drawing commands.
///
/// This is a placeholder that will be connected to the actual
/// rendering backend (wgpu, etc.).
#[derive(Debug)]
pub struct Canvas {
    // TODO: Connect to actual rendering backend
}

impl Canvas {
    /// Creates a new canvas.
    pub fn new() -> Self {
        Self {}
    }

    /// Draws a rectangle.
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        let _ = (rect, paint);
        // TODO: Record draw command
    }

    /// Draws a rounded rectangle.
    pub fn draw_rrect(&mut self, rrect: flui_types::RRect, paint: &Paint) {
        let _ = (rrect, paint);
        // TODO: Record draw command
    }

    /// Draws a circle.
    pub fn draw_circle(&mut self, center: Offset, radius: f32, paint: &Paint) {
        let _ = (center, radius, paint);
        // TODO: Record draw command
    }

    /// Draws a line.
    pub fn draw_line(&mut self, p1: Offset, p2: Offset, paint: &Paint) {
        let _ = (p1, p2, paint);
        // TODO: Record draw command
    }

    /// Saves the current canvas state.
    pub fn save(&mut self) {
        // TODO: Save state
    }

    /// Restores the previously saved canvas state.
    pub fn restore(&mut self) {
        // TODO: Restore state
    }

    /// Translates the canvas.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let _ = (dx, dy);
        // TODO: Apply translation
    }

    /// Scales the canvas.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        let _ = (sx, sy);
        // TODO: Apply scale
    }

    /// Rotates the canvas.
    pub fn rotate(&mut self, radians: f32) {
        let _ = radians;
        // TODO: Apply rotation
    }

    /// Clips to a rectangle.
    pub fn clip_rect(&mut self, rect: Rect) {
        let _ = rect;
        // TODO: Apply clip
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Paint
// ============================================================================

/// Paint style for drawing operations.
#[derive(Debug, Clone)]
pub struct Paint {
    /// The color to paint with.
    pub color: u32,

    /// The paint style (fill, stroke, etc.).
    pub style: PaintStyle,

    /// The stroke width (for stroke style).
    pub stroke_width: f32,
}

impl Paint {
    /// Creates a new paint with the given color.
    pub fn new(color: u32) -> Self {
        Self {
            color,
            style: PaintStyle::Fill,
            stroke_width: 1.0,
        }
    }

    /// Sets the paint style.
    pub fn with_style(mut self, style: PaintStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the stroke width.
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }
}

impl Default for Paint {
    fn default() -> Self {
        Self::new(0xFF000000) // Black
    }
}

/// The style of painting (fill vs stroke).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaintStyle {
    /// Fill the shape.
    #[default]
    Fill,

    /// Stroke the shape outline.
    Stroke,

    /// Fill and stroke the shape.
    FillAndStroke,
}
