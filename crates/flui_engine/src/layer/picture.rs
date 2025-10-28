//! Picture layer - leaf layer with actual drawing commands

use flui_types::{Rect, Point};
use flui_types::painting::path::Path;
use flui_types::painting::Image;
use flui_types::typography::TextStyle;
use crate::layer::Layer;
use crate::painter::{Painter, Paint, RRect};
use std::sync::Arc;

/// A drawing command to be executed
#[derive(Debug, Clone)]
pub enum DrawCommand {
    /// Draw a rectangle
    Rect {
        rect: Rect,
        paint: Paint,
    },

    /// Draw a rounded rectangle
    RRect {
        rrect: RRect,
        paint: Paint,
    },

    /// Draw a circle
    Circle {
        center: Point,
        radius: f32,
        paint: Paint,
    },

    /// Draw a line
    Line {
        p1: Point,
        p2: Point,
        paint: Paint,
    },

    /// Draw text
    Text {
        text: String,
        position: Point,
        style: TextStyle,
    },

    /// Draw an image
    Image {
        image: Arc<Image>,
        src_rect: Rect,
        dst_rect: Rect,
        paint: Paint,
    },

    /// Draw a path
    Path {
        path: Arc<Path>,
        paint: Paint,
    },

    /// Draw an arc or pie slice
    Arc {
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        paint: Paint,
    },

    /// Draw a polygon
    Polygon {
        points: Arc<Vec<Point>>,
        paint: Paint,
    },
}

/// Picture layer - a leaf layer that contains drawing commands
///
/// This is where actual rendering happens. All other layers are just
/// containers or effects - only PictureLayer does real drawing.
///
/// # Example
///
/// ```text
/// PictureLayer
///   commands: [
///     DrawCommand::Rect { ... },
///     DrawCommand::Circle { ... },
///   ]
/// ```
pub struct PictureLayer {
    /// The drawing commands to execute
    commands: Vec<DrawCommand>,

    /// Cached bounds of all drawing commands
    bounds: Rect,
}

impl PictureLayer {
    /// Create a new picture layer
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: Rect::ZERO,
        }
    }

    /// Add a drawing command
    pub fn add_command(&mut self, command: DrawCommand) {
        // Update bounds
        let command_bounds = Self::command_bounds(&command);
        if self.commands.is_empty() {
            self.bounds = command_bounds;
        } else {
            self.bounds = self.bounds.union(&command_bounds);
        }

        self.commands.push(command);
    }

    /// Draw a rectangle
    pub fn draw_rect(&mut self, rect: Rect, paint: Paint) {
        self.add_command(DrawCommand::Rect { rect, paint });
    }

    /// Draw a rounded rectangle
    pub fn draw_rrect(&mut self, rrect: RRect, paint: Paint) {
        self.add_command(DrawCommand::RRect { rrect, paint });
    }

    /// Draw a circle
    pub fn draw_circle(&mut self, center: Point, radius: f32, paint: Paint) {
        self.add_command(DrawCommand::Circle { center, radius, paint });
    }

    /// Draw a line
    pub fn draw_line(&mut self, p1: Point, p2: Point, paint: Paint) {
        self.add_command(DrawCommand::Line { p1, p2, paint });
    }

    /// Draw text
    ///
    /// # Arguments
    /// * `text` - The text to draw
    /// * `position` - Top-left position of the text
    /// * `style` - Text style (font, size, color, etc.)
    pub fn draw_text(&mut self, text: impl Into<String>, position: Point, style: TextStyle) {
        self.add_command(DrawCommand::Text {
            text: text.into(),
            position,
            style,
        });
    }

    /// Draw an image
    ///
    /// # Arguments
    /// * `image` - The image to draw (Arc for cheap cloning)
    /// * `src_rect` - Source rectangle in image coordinates
    /// * `dst_rect` - Destination rectangle on canvas
    /// * `paint` - Paint settings (opacity, blend mode, etc.)
    pub fn draw_image(
        &mut self,
        image: Arc<Image>,
        src_rect: Rect,
        dst_rect: Rect,
        paint: Paint,
    ) {
        self.add_command(DrawCommand::Image {
            image,
            src_rect,
            dst_rect,
            paint,
        });
    }

    /// Draw a path
    ///
    /// # Arguments
    /// * `path` - The path to draw (Arc for cheap cloning)
    /// * `paint` - Paint settings
    pub fn draw_path(&mut self, path: Arc<Path>, paint: Paint) {
        self.add_command(DrawCommand::Path { path, paint });
    }

    /// Draw an arc or pie slice
    ///
    /// # Arguments
    /// * `rect` - Bounding rectangle of the ellipse
    /// * `start_angle` - Starting angle in radians
    /// * `sweep_angle` - Angle to sweep in radians
    /// * `paint` - Paint settings
    pub fn draw_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        paint: Paint,
    ) {
        self.add_command(DrawCommand::Arc {
            rect,
            start_angle,
            sweep_angle,
            paint,
        });
    }

    /// Draw a polygon
    ///
    /// # Arguments
    /// * `points` - The vertices of the polygon (Arc for cheap cloning)
    /// * `paint` - Paint settings
    pub fn draw_polygon(&mut self, points: Arc<Vec<Point>>, paint: Paint) {
        self.add_command(DrawCommand::Polygon { points, paint });
    }

    /// Get all drawing commands
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Calculate bounds of a single drawing command
    fn command_bounds(command: &DrawCommand) -> Rect {
        match command {
            DrawCommand::Rect { rect, paint } => {
                if paint.stroke_width > 0.0 {
                    // Add stroke width to bounds
                    rect.expand(paint.stroke_width / 2.0)
                } else {
                    *rect
                }
            }
            DrawCommand::RRect { rrect, paint } => {
                if paint.stroke_width > 0.0 {
                    rrect.rect.expand(paint.stroke_width / 2.0)
                } else {
                    rrect.rect
                }
            }
            DrawCommand::Circle { center, radius, paint } => {
                let r = if paint.stroke_width > 0.0 {
                    radius + paint.stroke_width / 2.0
                } else {
                    *radius
                };
                // Create rect from center and radius
                let size = flui_types::Size::new(r * 2.0, r * 2.0);
                Rect::from_center_size(*center, size)
            }
            DrawCommand::Line { p1, p2, paint } => {
                let min_x = p1.x.min(p2.x);
                let min_y = p1.y.min(p2.y);
                let max_x = p1.x.max(p2.x);
                let max_y = p1.y.max(p2.y);

                let stroke = paint.stroke_width / 2.0;
                Rect::from_min_max(
                    Point::new(min_x - stroke, min_y - stroke),
                    Point::new(max_x + stroke, max_y + stroke),
                )
            }
            DrawCommand::Text { text, position, style } => {
                // Approximate text bounds
                // TODO: Use proper text measurement when available
                let font_size = style.font_size.unwrap_or(14.0) as f32;
                let width = text.len() as f32 * font_size * 0.6;
                let height = font_size * 1.2; // Include line height
                Rect::from_xywh(position.x, position.y, width, height)
            }
            DrawCommand::Image { dst_rect, paint, .. } => {
                if paint.stroke_width > 0.0 {
                    dst_rect.expand(paint.stroke_width / 2.0)
                } else {
                    *dst_rect
                }
            }
            DrawCommand::Path { path, paint } => {
                // Compute approximate bounds from path commands
                let bounds = path.commands().iter().fold(Rect::ZERO, |acc, cmd| {
                    use flui_types::painting::path::PathCommand;
                    let cmd_rect = match cmd {
                        PathCommand::MoveTo(p) | PathCommand::LineTo(p) => {
                            Rect::from_xywh(p.x, p.y, 0.0, 0.0)
                        }
                        PathCommand::AddRect(r) => *r,
                        PathCommand::AddCircle(center, radius) => {
                            let size = flui_types::Size::new(radius * 2.0, radius * 2.0);
                            Rect::from_center_size(*center, size)
                        }
                        PathCommand::AddOval(r) | PathCommand::AddArc(r, _, _) => *r,
                        _ => Rect::ZERO,
                    };
                    if acc == Rect::ZERO { cmd_rect } else { acc.union(&cmd_rect) }
                });

                if paint.stroke_width > 0.0 {
                    bounds.expand(paint.stroke_width / 2.0)
                } else {
                    bounds
                }
            }
            DrawCommand::Arc { rect, paint, .. } => {
                if paint.stroke_width > 0.0 {
                    rect.expand(paint.stroke_width / 2.0)
                } else {
                    *rect
                }
            }
            DrawCommand::Polygon { points, paint } => {
                if points.is_empty() {
                    return Rect::ZERO;
                }

                let mut min_x = points[0].x;
                let mut min_y = points[0].y;
                let mut max_x = points[0].x;
                let mut max_y = points[0].y;

                for p in points.iter().skip(1) {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }

                let stroke = paint.stroke_width / 2.0;
                Rect::from_min_max(
                    Point::new(min_x - stroke, min_y - stroke),
                    Point::new(max_x + stroke, max_y + stroke),
                )
            }
        }
    }
}

impl Default for PictureLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl Layer for PictureLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Execute all drawing commands
        for command in &self.commands {
            match command {
                DrawCommand::Rect { rect, paint } => {
                    painter.rect(*rect, paint);
                }
                DrawCommand::RRect { rrect, paint } => {
                    painter.rrect(*rrect, paint);
                }
                DrawCommand::Circle { center, radius, paint } => {
                    painter.circle(*center, *radius, paint);
                }
                DrawCommand::Line { p1, p2, paint } => {
                    painter.line(*p1, *p2, paint);
                }
                DrawCommand::Text { text, position, style } => {
                    painter.text_styled(text, *position, style);
                }
                DrawCommand::Image { image, src_rect, dst_rect, paint } => {
                    painter.image(image, *src_rect, *dst_rect, paint);
                }
                DrawCommand::Path { path, paint } => {
                    painter.path(path, paint);
                }
                DrawCommand::Arc { rect, start_angle, sweep_angle, paint } => {
                    // Convert rect-based arc to center/radius arc
                    let center = rect.center();
                    let radius = rect.width().min(rect.height()) / 2.0;
                    let end_angle = start_angle + sweep_angle;
                    painter.arc(center, radius, *start_angle, end_angle, paint);
                }
                DrawCommand::Polygon { points, paint } => {
                    painter.polygon(points, paint);
                }
            }
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn is_visible(&self) -> bool {
        !self.commands.is_empty()
    }
}
