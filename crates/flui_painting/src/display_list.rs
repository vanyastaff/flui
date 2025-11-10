//! DisplayList - Recorded sequence of drawing commands
//!
//! This module provides the `DisplayList` type which records drawing commands
//! from a Canvas for later execution by the GPU backend. This follows the
//! Command Pattern - record now, execute later.
//!
//! # Architecture
//!
//! ```text
//! Canvas::draw_rect() → DisplayList::push(DrawRect) → PictureLayer → WgpuPainter
//! ```

use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

/// A recorded sequence of drawing commands
///
/// DisplayList is immutable after recording and can be replayed multiple times
/// by the engine. It's the output of Canvas and the input to PictureLayer.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::{Canvas, DisplayList};
///
/// let mut canvas = Canvas::new();
/// canvas.draw_rect(rect, &paint);
/// let display_list: DisplayList = canvas.finish();
///
/// // Later, in engine:
/// for cmd in display_list.commands() {
///     match cmd {
///         DrawCommand::DrawRect { rect, paint, .. } => {
///             painter.rect(*rect, paint);
///         }
///         // ... other commands
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct DisplayList {
    /// Drawing commands in order
    commands: Vec<DrawCommand>,

    /// Cached bounds of all drawing
    bounds: Rect,
}

impl DisplayList {
    /// Creates a new empty display list
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: Rect::ZERO,
        }
    }

    /// Adds a command to the display list (internal)
    pub(crate) fn push(&mut self, command: DrawCommand) {
        // Update bounds based on command
        if let Some(cmd_bounds) = command.bounds() {
            if self.commands.is_empty() {
                self.bounds = cmd_bounds;
            } else {
                self.bounds = self.bounds.union(&cmd_bounds);
            }
        }
        self.commands.push(command);
    }

    /// Returns an iterator over commands
    pub fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands.iter()
    }

    /// Returns the bounds of all drawing
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Returns the number of commands
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Clears all commands (for pooling/reuse)
    pub fn clear(&mut self) {
        self.commands.clear();
        self.bounds = Rect::ZERO;
    }
}

impl Default for DisplayList {
    fn default() -> Self {
        Self::new()
    }
}

/// A single drawing command recorded by Canvas
///
/// Each variant contains all information needed to execute the command
/// later, including the transform matrix at the time of recording.
#[derive(Debug, Clone)]
pub enum DrawCommand {
    // === Clipping Commands ===
    /// Clip to a rectangle
    ClipRect {
        /// Rectangle to clip to
        rect: Rect,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Clip to a rounded rectangle
    ClipRRect {
        /// Rounded rectangle to clip to
        rrect: RRect,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Clip to an arbitrary path
    ClipPath {
        /// Path to clip to
        path: Path,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Primitive Drawing Commands ===
    /// Draw a line
    DrawLine {
        /// Start point
        p1: Point,
        /// End point
        p2: Point,
        /// Paint style (color, stroke width, etc.)
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a rectangle
    DrawRect {
        /// Rectangle to draw
        rect: Rect,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a rounded rectangle
    DrawRRect {
        /// Rounded rectangle to draw
        rrect: RRect,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw a circle
    DrawCircle {
        /// Center point
        center: Point,
        /// Radius
        radius: f32,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw an oval (ellipse)
    DrawOval {
        /// Bounding rectangle
        rect: Rect,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    /// Draw an arbitrary path
    DrawPath {
        /// Path to draw
        path: Path,
        /// Paint style
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Text ===
    /// Draw text
    DrawText {
        /// Text content
        text: String,
        /// Position offset
        offset: Offset,
        /// Text style (font, size, etc.)
        style: TextStyle,
        /// Paint style (color, etc.)
        paint: Paint,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Image ===
    /// Draw an image
    DrawImage {
        /// Image
        image: Image,
        /// Destination rectangle
        dst: Rect,
        /// Optional paint (for tinting, etc.)
        paint: Option<Paint>,
        /// Transform at recording time
        transform: Matrix4,
    },

    // === Effects ===
    /// Draw a shadow
    DrawShadow {
        /// Path casting shadow
        path: Path,
        /// Shadow color
        color: Color,
        /// Elevation (blur amount)
        elevation: f32,
        /// Transform at recording time
        transform: Matrix4,
    },
}

impl DrawCommand {
    /// Returns the bounding rectangle of this command (if applicable)
    ///
    /// Used to calculate the DisplayList's overall bounds.
    fn bounds(&self) -> Option<Rect> {
        match self {
            DrawCommand::DrawRect { rect, .. } => Some(*rect),
            DrawCommand::DrawRRect { rrect, .. } => Some(rrect.bounding_rect()),
            DrawCommand::DrawCircle {
                center, radius, ..
            } => {
                let size = flui_types::geometry::Size::new(radius * 2.0, radius * 2.0);
                Some(Rect::from_center_size(*center, size))
            }
            DrawCommand::DrawOval { rect, .. } => Some(*rect),
            DrawCommand::DrawImage { dst, .. } => Some(*dst),
            DrawCommand::DrawLine { p1, p2, paint, .. } => {
                // Account for stroke width
                let stroke_half = paint.stroke_width * 0.5;
                let min_x = p1.x.min(p2.x) - stroke_half;
                let min_y = p1.y.min(p2.y) - stroke_half;
                let max_x = p1.x.max(p2.x) + stroke_half;
                let max_y = p1.y.max(p2.y) + stroke_half;
                Some(Rect::from_ltrb(min_x, min_y, max_x, max_y))
            }
            DrawCommand::DrawPath { .. } => {
                // Path bounds calculation requires mutable access
                // We'll compute DisplayList bounds without Path bounds for now
                None
            }
            DrawCommand::DrawShadow { .. } => {
                // Shadow bounds calculation requires path bounds
                // We'll compute DisplayList bounds without Shadow bounds for now
                None
            }
            // Clipping and text don't contribute to bounds directly
            DrawCommand::ClipRect { .. }
            | DrawCommand::ClipRRect { .. }
            | DrawCommand::ClipPath { .. }
            | DrawCommand::DrawText { .. } => None,
        }
    }
}

/// Description of how to paint on a canvas
///
/// Contains color, style (fill/stroke), stroke width, blend mode, etc.
/// This is the painting equivalent of CSS styles.
#[derive(Debug, Clone)]
pub struct Paint {
    /// Paint style (fill or stroke)
    pub style: PaintStyle,

    /// Color (RGBA)
    pub color: Color,

    /// Stroke width (only used for stroke style)
    pub stroke_width: f32,

    /// Stroke cap style
    pub stroke_cap: StrokeCap,

    /// Stroke join style
    pub stroke_join: StrokeJoin,

    /// Blend mode
    pub blend_mode: BlendMode,

    /// Anti-aliasing enabled
    pub anti_alias: bool,

    /// Optional shader (gradient, image pattern, etc.)
    pub shader: Option<Shader>,
}

impl Paint {
    /// Creates a fill paint with the given color
    pub fn fill(color: Color) -> Self {
        Self {
            style: PaintStyle::Fill,
            color,
            stroke_width: 0.0,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            blend_mode: BlendMode::SrcOver,
            anti_alias: true,
            shader: None,
        }
    }

    /// Creates a stroke paint with the given color and width
    pub fn stroke(color: Color, width: f32) -> Self {
        Self {
            style: PaintStyle::Stroke,
            color,
            stroke_width: width,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            blend_mode: BlendMode::SrcOver,
            anti_alias: true,
            shader: None,
        }
    }

    /// Builder for Paint
    pub fn builder() -> PaintBuilder {
        PaintBuilder::default()
    }
}

impl Default for Paint {
    fn default() -> Self {
        Self::fill(Color::BLACK)
    }
}

/// Builder for Paint
#[derive(Debug, Clone)]
pub struct PaintBuilder {
    paint: Paint,
}

impl PaintBuilder {
    /// Sets the paint style
    pub fn style(mut self, style: PaintStyle) -> Self {
        self.paint.style = style;
        self
    }

    /// Sets the color
    pub fn color(mut self, color: Color) -> Self {
        self.paint.color = color;
        self
    }

    /// Sets the stroke width
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.paint.stroke_width = width;
        self
    }

    /// Sets the stroke cap
    pub fn stroke_cap(mut self, cap: StrokeCap) -> Self {
        self.paint.stroke_cap = cap;
        self
    }

    /// Sets the stroke join
    pub fn stroke_join(mut self, join: StrokeJoin) -> Self {
        self.paint.stroke_join = join;
        self
    }

    /// Sets the blend mode
    pub fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.paint.blend_mode = blend_mode;
        self
    }

    /// Sets anti-aliasing
    pub fn anti_alias(mut self, aa: bool) -> Self {
        self.paint.anti_alias = aa;
        self
    }

    /// Sets the shader
    pub fn shader(mut self, shader: Shader) -> Self {
        self.paint.shader = Some(shader);
        self
    }

    /// Builds the Paint
    pub fn build(self) -> Paint {
        self.paint
    }
}

impl Default for PaintBuilder {
    fn default() -> Self {
        Self {
            paint: Paint::default(),
        }
    }
}

/// Paint style (fill or stroke)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintStyle {
    /// Fill the shape
    Fill,
    /// Stroke the outline
    Stroke,
}

/// Stroke cap style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeCap {
    /// Flat edge
    Butt,
    /// Rounded cap
    Round,
    /// Square cap
    Square,
}

/// Stroke join style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeJoin {
    /// Sharp corner
    Miter,
    /// Rounded corner
    Round,
    /// Beveled corner
    Bevel,
}

/// Blend mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// Source over destination (normal blending)
    SrcOver,
    /// Add source to destination
    Plus,
    /// Multiply source and destination
    Multiply,
    /// Screen blend
    Screen,
    /// Overlay blend
    Overlay,
    // ... more blend modes can be added
}

/// Shader (gradient, image pattern, etc.)
#[derive(Debug, Clone)]
pub enum Shader {
    /// Linear gradient
    LinearGradient {
        /// Start point
        start: Point,
        /// End point
        end: Point,
        /// Colors
        colors: Vec<Color>,
        /// Color stops (optional, defaults to evenly spaced)
        stops: Option<Vec<f32>>,
    },
    /// Radial gradient
    RadialGradient {
        /// Center point
        center: Point,
        /// Radius
        radius: f32,
        /// Colors
        colors: Vec<Color>,
        /// Color stops (optional)
        stops: Option<Vec<f32>>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::Point;

    #[test]
    fn test_display_list_creation() {
        let display_list = DisplayList::new();
        assert!(display_list.is_empty());
        assert_eq!(display_list.len(), 0);
        assert_eq!(display_list.bounds(), Rect::ZERO);
    }

    #[test]
    fn test_display_list_push() {
        let mut display_list = DisplayList::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let paint = Paint::fill(Color::RED);

        display_list.push(DrawCommand::DrawRect {
            rect,
            paint,
            transform: Matrix4::identity(),
        });

        assert_eq!(display_list.len(), 1);
        assert_eq!(display_list.bounds(), rect);
    }

    #[test]
    fn test_display_list_clear() {
        let mut display_list = DisplayList::new();
        display_list.push(DrawCommand::DrawRect {
            rect: Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
            paint: Paint::default(),
            transform: Matrix4::identity(),
        });

        assert!(!display_list.is_empty());

        display_list.clear();
        assert!(display_list.is_empty());
        assert_eq!(display_list.bounds(), Rect::ZERO);
    }

    #[test]
    fn test_paint_fill() {
        let paint = Paint::fill(Color::RED);
        assert_eq!(paint.style, PaintStyle::Fill);
        assert_eq!(paint.color, Color::RED);
    }

    #[test]
    fn test_paint_stroke() {
        let paint = Paint::stroke(Color::BLUE, 2.0);
        assert_eq!(paint.style, PaintStyle::Stroke);
        assert_eq!(paint.color, Color::BLUE);
        assert_eq!(paint.stroke_width, 2.0);
    }

    #[test]
    fn test_paint_builder() {
        let paint = Paint::builder()
            .color(Color::GREEN)
            .style(PaintStyle::Stroke)
            .stroke_width(3.0)
            .stroke_cap(StrokeCap::Round)
            .build();

        assert_eq!(paint.color, Color::GREEN);
        assert_eq!(paint.style, PaintStyle::Stroke);
        assert_eq!(paint.stroke_width, 3.0);
        assert_eq!(paint.stroke_cap, StrokeCap::Round);
    }
}
