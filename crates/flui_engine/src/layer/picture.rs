//! Picture layer - leaf layer with actual drawing commands

use flui_types::{Rect, Point};
use crate::layer::Layer;
use crate::painter::{Painter, Paint, RRect};

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

    // TODO: Add more drawing commands:
    // - Text
    // - Image
    // - Path
    // - Arc
    // - etc.
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
