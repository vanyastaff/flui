//! Picture layer - leaf layer with actual drawing commands

use crate::layer::Layer;
use crate::painter::{Paint as EnginePaint, Painter, RRect, Stroke};
use flui_painting::{Canvas, DisplayList, DrawCommand, Paint as PaintingPaint, PaintStyle};
use flui_types::painting::path::Path;
use flui_types::painting::Image;
use flui_types::typography::TextStyle;
use flui_types::{Offset, Point, Rect};
use std::sync::Arc;

/// Picture layer - a leaf layer that contains drawing commands
///
/// This is where actual rendering happens. All other layers are just
/// containers or effects - only PictureLayer does real drawing.
///
/// # Architecture
///
/// ```text
/// Canvas (flui_painting) → DisplayList → PictureLayer (flui_engine) → WgpuPainter
/// ```
///
/// PictureLayer now uses Canvas from flui_painting for recording commands,
/// which are stored in a DisplayList for execution.
#[derive(Default)]
pub struct PictureLayer {
    /// Canvas for recording drawing commands
    canvas: Canvas,
}

impl PictureLayer {
    /// Create a new picture layer
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a picture layer from a Canvas
    ///
    /// This is the primary way to create a PictureLayer from RenderObjects.
    /// RenderObjects create a Canvas, record drawing commands, and return it.
    /// The pipeline then converts Canvas → PictureLayer for GPU rendering.
    pub fn from_canvas(canvas: Canvas) -> Self {
        Self { canvas }
    }

    /// Create a picture layer from a display list
    pub fn from_display_list(display_list: DisplayList) -> Self {
        // Can't convert DisplayList back to Canvas (DisplayList is immutable)
        // So we'll just create a new canvas
        // TODO: Consider alternative API if this is commonly needed
        let _ = display_list; // Silence unused warning
        Self::new()
    }

    /// Clear all drawing commands
    ///
    /// Used by the pool to reset layers before reuse.
    pub fn clear(&mut self) {
        self.canvas = Canvas::new();
    }

    /// Get the display list (finishes recording)
    pub fn display_list(&self) -> &DisplayList {
        self.canvas.display_list()
    }

    // ===== Backward-compatible drawing methods =====

    /// Draw a rectangle
    pub fn draw_rect(&mut self, rect: Rect, paint: EnginePaint) {
        self.canvas.draw_rect(rect, &Self::convert_paint_to_painting(&paint));
    }

    /// Draw a rounded rectangle
    pub fn draw_rrect(&mut self, rrect: RRect, paint: EnginePaint) {
        self.canvas.draw_rrect(rrect, &Self::convert_paint_to_painting(&paint));
    }

    /// Draw a circle
    pub fn draw_circle(&mut self, center: Point, radius: f32, paint: EnginePaint) {
        self.canvas.draw_circle(center, radius, &Self::convert_paint_to_painting(&paint));
    }

    /// Draw a line
    pub fn draw_line(&mut self, p1: Point, p2: Point, paint: EnginePaint) {
        self.canvas.draw_line(p1, p2, &Self::convert_paint_to_painting(&paint));
    }

    /// Draw text
    pub fn draw_text(&mut self, text: impl Into<String>, position: Point, style: TextStyle) {
        let text = text.into();
        let paint = PaintingPaint::fill(style.color.unwrap_or(flui_types::styling::Color::BLACK));
        let offset = Offset::new(position.x, position.y);
        self.canvas.draw_text(&text, offset, &style, &paint);
    }

    /// Draw an image
    pub fn draw_image(&mut self, image: Arc<Image>, _src_rect: Rect, dst_rect: Rect, _paint: EnginePaint) {
        self.canvas.draw_image((*image).clone(), dst_rect, None);
    }

    /// Draw a path
    pub fn draw_path(&mut self, path: Arc<Path>, paint: EnginePaint) {
        self.canvas.draw_path(&path, &Self::convert_paint_to_painting(&paint));
    }

    // ===== Paint conversion helpers =====

    /// Helper to convert flui_painting::Paint to flui_engine::Paint
    fn convert_paint_to_engine(paint: &PaintingPaint) -> EnginePaint {
        match paint.style {
            PaintStyle::Fill => EnginePaint::fill(paint.color),
            PaintStyle::Stroke => EnginePaint::builder()
                .color(paint.color)
                .stroke(Stroke::new(paint.stroke_width))
                .build(),
        }
    }

    /// Helper to convert flui_engine::Paint to flui_painting::Paint
    fn convert_paint_to_painting(paint: &EnginePaint) -> PaintingPaint {
        if paint.is_stroke() {
            if let Some(stroke) = paint.get_stroke() {
                PaintingPaint::stroke(paint.get_color(), stroke.width())
            } else {
                PaintingPaint::stroke(paint.get_color(), 1.0)
            }
        } else {
            PaintingPaint::fill(paint.get_color())
        }
    }
}


impl Layer for PictureLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Execute all drawing commands from DisplayList
        for command in self.canvas.display_list().commands() {
            match command {
                DrawCommand::DrawRect { rect, paint, .. } => {
                    painter.rect(*rect, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawRRect { rrect, paint, .. } => {
                    painter.rrect(*rrect, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawCircle {
                    center,
                    radius,
                    paint,
                    ..
                } => {
                    painter.circle(*center, *radius, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawLine { p1, p2, paint, .. } => {
                    painter.line(*p1, *p2, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawOval { rect, paint, .. } => {
                    // Render oval/ellipse using proper ellipse rendering
                    painter.oval(*rect, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawPath { path, paint, .. } => {
                    // Render path using tessellation
                    painter.draw_flui_path(path, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawText {
                    text,
                    offset,
                    style,
                    ..
                } => {
                    // Extract font size and create paint from style
                    let font_size = style.font_size.unwrap_or(14.0) as f32;
                    let paint =
                        EnginePaint::fill(style.color.unwrap_or(flui_types::styling::Color::BLACK));
                    let position = Point::new(offset.dx, offset.dy);
                    painter.text_styled(text, position, font_size, &paint);
                }

                DrawCommand::DrawImage { image, dst, .. } => {
                    // Image rendering using painter's draw_image method
                    painter.draw_image(image, *dst);
                }

                DrawCommand::DrawShadow { path, color, elevation, .. } => {
                    // Multi-layer shadow rendering with blur approximation
                    painter.draw_shadow(path, *color, *elevation);
                }

                DrawCommand::DrawArc { rect, start_angle, sweep_angle, use_center, paint, .. } => {
                    // Full arc rendering with angles and use_center
                    painter.draw_arc(*rect, *start_angle, *sweep_angle, *use_center, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawDRRect { outer, inner, paint, .. } => {
                    // Draw ring (outer - inner) with proper inner cutout
                    painter.draw_drrect(*outer, *inner, &Self::convert_paint_to_engine(paint));
                }

                DrawCommand::DrawPoints { mode, points, paint, .. } => {
                    // Draw points with specified mode
                    use flui_painting::PointMode;
                    match mode {
                        PointMode::Points => {
                            // Draw individual points as small circles
                            let radius = paint.stroke_width / 2.0;
                            for point in points {
                                painter.circle(*point, radius, &Self::convert_paint_to_engine(paint));
                            }
                        }
                        PointMode::Lines => {
                            // Draw lines between consecutive points
                            for i in (0..points.len()).step_by(2) {
                                if i + 1 < points.len() {
                                    painter.line(points[i], points[i + 1], &Self::convert_paint_to_engine(paint));
                                }
                            }
                        }
                        PointMode::Polygon => {
                            // Draw connected lines (polygon)
                            for i in 0..points.len().saturating_sub(1) {
                                painter.line(points[i], points[i + 1], &Self::convert_paint_to_engine(paint));
                            }
                            // Close the polygon
                            if points.len() > 2 {
                                painter.line(points[points.len() - 1], points[0], &Self::convert_paint_to_engine(paint));
                            }
                        }
                    }
                }

                DrawCommand::DrawVertices { vertices, colors, tex_coords, indices, paint, .. } => {
                    // Custom vertex rendering with optional per-vertex colors
                    painter.draw_vertices(
                        vertices,
                        colors.as_deref(),
                        tex_coords.as_deref(),
                        indices,
                        &Self::convert_paint_to_engine(paint),
                    );
                }

                DrawCommand::DrawColor { color, .. } => {
                    // Fill entire canvas with color
                    // Get the canvas bounds from display list
                    let bounds = self.canvas.display_list().bounds();
                    if !bounds.is_empty() {
                        let paint = flui_painting::Paint::fill(*color);
                        painter.rect(bounds, &Self::convert_paint_to_engine(&paint));
                    }
                }

                DrawCommand::DrawAtlas { image, sprites, transforms, colors, .. } => {
                    // Efficient sprite atlas rendering with GPU instancing
                    // Note: BlendMode is currently ignored in the implementation
                    painter.draw_atlas(
                        image,
                        sprites,
                        transforms,
                        colors.as_deref(),
                    );
                }

                DrawCommand::ClipRect { .. }
                | DrawCommand::ClipRRect { .. }
                | DrawCommand::ClipPath { .. } => {
                    // Clipping commands are handled separately via Painter trait
                    // They don't map directly to PictureLayer paint operations
                    #[cfg(debug_assertions)]
                    tracing::debug!("PictureLayer: Clipping commands handled by Painter trait");
                }
            }
        }
    }

    fn bounds(&self) -> Rect {
        self.canvas.display_list().bounds()
    }

    fn is_visible(&self) -> bool {
        !self.canvas.display_list().is_empty()
    }
}
