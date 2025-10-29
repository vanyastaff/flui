//! PathLayer - renders arbitrary vector paths
//!
//! This module provides a specialized layer for rendering vector paths with
//! advanced stroke and fill options.

use flui_types::{Rect, Offset, Event, HitTestResult, Point};
use flui_types::painting::path::Path;

use crate::layer::Layer;
use crate::painter::{Painter, Paint};
use flui_types::painting::effects::{StrokeOptions, PathPaintMode};
use std::sync::Arc;



/// A layer that renders an arbitrary vector path.
///
/// Similar to Flutter's custom painting with Path objects. Supports both
/// fill and stroke modes with advanced stroke options.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_engine::layer::PathLayer;
/// use flui_types::painting::Path;
/// use flui_types::geometry::Point;
/// use flui_engine::painter::Paint;
///
/// // Create a star path
/// let mut path = Path::new();
/// path.move_to(Point::new(50.0, 0.0));
/// path.line_to(Point::new(60.0, 40.0));
/// path.line_to(Point::new(100.0, 40.0));
/// path.line_to(Point::new(70.0, 60.0));
/// path.line_to(Point::new(80.0, 100.0));
/// path.line_to(Point::new(50.0, 75.0));
/// path.line_to(Point::new(20.0, 100.0));
/// path.line_to(Point::new(30.0, 60.0));
/// path.line_to(Point::new(0.0, 40.0));
/// path.line_to(Point::new(40.0, 40.0));
/// path.close();
///
/// // Create layer
/// let layer = PathLayer::new(path)
///     .with_paint(Paint::fill([1.0, 0.8, 0.0, 1.0])) // Gold fill
///     .with_stroke_paint(Paint::stroke(2.0, [0.0, 0.0, 0.0, 1.0])); // Black outline
/// ```
pub struct PathLayer {
    /// The path to render
    path: Arc<Path>,

    /// Paint for filling/stroking
    paint: Paint,

    /// Paint mode (fill, stroke, or both)
    paint_mode: PathPaintMode,

    /// Stroke options (used only when paint_mode includes stroke)
    stroke_options: Option<StrokeOptions>,

    /// Optional separate paint for stroke (when using FillAndStroke mode)
    stroke_paint: Option<Paint>,

    /// Cached bounds
    cached_bounds: Option<Rect>,

    /// Whether this layer has been disposed
    disposed: bool,
}

impl PathLayer {
    /// Create a new path layer with default fill mode
    ///
    /// # Arguments
    ///
    /// * `path` - The path to render
    #[must_use]
    pub fn new(path: Path) -> Self {
        Self {
            path: Arc::new(path),
            paint: Paint::default(),
            paint_mode: PathPaintMode::Fill,
            stroke_options: None,
            stroke_paint: None,
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Create a path layer from an Arc path (efficient for sharing)
    #[must_use]
    pub fn from_arc(path: Arc<Path>) -> Self {
        Self {
            path,
            paint: Paint::default(),
            paint_mode: PathPaintMode::Fill,
            stroke_options: None,
            stroke_paint: None,
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Set the paint
    #[inline]
    #[must_use]
    pub fn with_paint(mut self, paint: Paint) -> Self {
        self.paint = paint;
        self
    }

    /// Set the paint mode
    #[inline]
    #[must_use]
    pub fn with_mode(mut self, mode: PathPaintMode) -> Self {
        self.paint_mode = mode;
        self
    }

    /// Enable stroke mode with options
    #[inline]
    #[must_use]
    pub fn with_stroke(mut self, options: StrokeOptions) -> Self {
        self.paint_mode = PathPaintMode::Stroke;
        self.stroke_options = Some(options);
        self
    }

    /// Set stroke paint (for FillAndStroke mode)
    #[inline]
    #[must_use]
    pub fn with_stroke_paint(mut self, paint: Paint) -> Self {
        if self.paint_mode == PathPaintMode::Fill {
            self.paint_mode = PathPaintMode::FillAndStroke;
        }
        self.stroke_paint = Some(paint);
        self
    }

    /// Update the path
    pub fn set_path(&mut self, path: Path) {
        self.path = Arc::new(path);
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Update the paint
    pub fn set_paint(&mut self, paint: Paint) {
        self.paint = paint;
        self.mark_needs_paint();
    }

    /// Update stroke options
    pub fn set_stroke_options(&mut self, options: StrokeOptions) {
        self.stroke_options = Some(options);
        self.mark_needs_paint();
    }

    /// Get the path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the paint
    pub fn paint(&self) -> &Paint {
        &self.paint
    }

    /// Get the paint mode
    pub fn paint_mode(&self) -> PathPaintMode {
        self.paint_mode
    }
}

impl Layer for PathLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot use disposed PathLayer");
        }

        painter.save();

        match self.paint_mode {
            PathPaintMode::Fill => {
                // TODO: Add method to painter to draw path directly
                // For now, we can decompose path into drawing commands
                self.paint_path_fill(painter);
            }
            PathPaintMode::Stroke => {
                self.paint_path_stroke(painter);
            }
            PathPaintMode::FillAndStroke => {
                // Fill first
                self.paint_path_fill(painter);

                // Then stroke
                if let Some(ref stroke_paint) = self.stroke_paint {
                    self.paint_path_stroke_with_paint(painter, stroke_paint);
                } else {
                    self.paint_path_stroke(painter);
                }
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        // Path bounds computation
        let mut path_clone = (*self.path).clone();
        path_clone.bounds()
    }

    fn is_visible(&self) -> bool {
        !self.disposed && !self.path.is_empty()
    }

    fn hit_test(&self, position: Offset, _result: &mut HitTestResult) -> bool {
        // Basic bounds-based hit testing
        // TODO: Implement proper path hit testing
        let point: Point = position.into();
        self.bounds().contains(point)
    }

    fn handle_event(&mut self, _event: &Event) -> bool {
        false
    }

    fn dispose(&mut self) {
        self.disposed = true;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn mark_needs_paint(&mut self) {
        // TODO: Implement dirty flag propagation when we have parent references
    }
}

impl PathLayer {
    /// Helper method to paint path as fill
    fn paint_path_fill(&self, painter: &mut dyn Painter) {
        use flui_types::painting::path::PathCommand;
        use flui_types::geometry::Point;

        let commands = self.path.commands();
        if commands.is_empty() {
            return;
        }

        // Decompose path into basic drawing commands
        // TODO: Add painter.draw_path() method for more efficient rendering
        let mut current_points: Vec<Point> = Vec::new();

        for cmd in commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    // Start new subpath
                    if !current_points.is_empty() {
                        painter.polygon(&current_points, &self.paint);
                        current_points.clear();
                    }
                    current_points.push(*p);
                }
                PathCommand::LineTo(p) => {
                    current_points.push(*p);
                }
                PathCommand::Close => {
                    if !current_points.is_empty() {
                        painter.polygon(&current_points, &self.paint);
                        current_points.clear();
                    }
                }
                PathCommand::AddRect(rect) => {
                    painter.rect(*rect, &self.paint);
                }
                PathCommand::AddCircle(center, radius) => {
                    painter.circle(*center, *radius, &self.paint);
                }
                PathCommand::AddOval(rect) => {
                    painter.ellipse(
                        rect.center(),
                        rect.width() / 2.0,
                        rect.height() / 2.0,
                        &self.paint
                    );
                }
                PathCommand::AddArc(rect, start_angle, sweep_angle) => {
                    painter.arc(
                        rect.center(),
                        rect.width().max(rect.height()) / 2.0,
                        *start_angle,
                        start_angle + sweep_angle,
                        &self.paint
                    );
                }
                // TODO: Implement bezier curve rendering
                PathCommand::QuadraticTo(_, _) | PathCommand::CubicTo(_, _, _) => {
                    // For now, skip bezier curves
                    // TODO: Tessellate bezier curves with lyon
                }
            }
        }

        // Paint remaining points
        if !current_points.is_empty() {
            painter.polygon(&current_points, &self.paint);
        }
    }

    /// Helper method to paint path as stroke
    fn paint_path_stroke(&self, painter: &mut dyn Painter) {
        self.paint_path_stroke_with_paint(painter, &self.paint);
    }

    /// Helper method to paint path stroke with specific paint
    fn paint_path_stroke_with_paint(&self, painter: &mut dyn Painter, paint: &Paint) {
        use flui_types::painting::path::PathCommand;

        let commands = self.path.commands();
        if commands.is_empty() {
            return;
        }

        // Create stroke paint
        let stroke_width = if let Some(ref opts) = self.stroke_options {
            opts.width
        } else {
            paint.stroke_width
        };

        let stroke_paint = Paint {
            color: paint.color,
            stroke_width,
            anti_alias: paint.anti_alias,
        };

        // Decompose path into lines
        let mut last_point: Option<flui_types::geometry::Point> = None;

        for cmd in commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    last_point = Some(*p);
                }
                PathCommand::LineTo(p) => {
                    if let Some(start) = last_point {
                        painter.line(start, *p, &stroke_paint);
                    }
                    last_point = Some(*p);
                }
                PathCommand::Close => {
                    // Close the path by drawing back to start
                    if let Some(start) = commands.iter().find_map(|c| {
                        if let PathCommand::MoveTo(p) = c {
                            Some(*p)
                        } else {
                            None
                        }
                    }) {
                        if let Some(end) = last_point {
                            painter.line(end, start, &stroke_paint);
                        }
                    }
                    last_point = None;
                }
                PathCommand::AddRect(rect) => {
                    painter.rect(*rect, &stroke_paint);
                }
                PathCommand::AddCircle(center, radius) => {
                    painter.circle(*center, *radius, &stroke_paint);
                }
                // TODO: Handle other path commands for stroke
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::Point;
    use flui_types::painting::{StrokeCap, StrokeJoin};

    #[test]
    fn test_path_layer_new() {
        let path = Path::new();
        let layer = PathLayer::new(path);

        assert_eq!(layer.paint_mode(), PathPaintMode::Fill);
        // Empty path should not be visible
        assert!(!layer.is_visible());
    }

    #[test]
    fn test_path_layer_with_stroke() {
        let path = Path::new();
        let layer = PathLayer::new(path)
            .with_stroke(StrokeOptions::new().with_width(2.0));

        assert_eq!(layer.paint_mode(), PathPaintMode::Stroke);
        assert!(layer.stroke_options.is_some());
    }

    #[test]
    fn test_path_layer_fill_and_stroke() {
        let path = Path::new();
        let layer = PathLayer::new(path)
            .with_stroke_paint(Paint::stroke(2.0, [0.0, 0.0, 0.0, 1.0]));

        assert_eq!(layer.paint_mode(), PathPaintMode::FillAndStroke);
        assert!(layer.stroke_paint.is_some());
    }

    #[test]
    fn test_stroke_options() {
        let opts = StrokeOptions::new()
            .with_width(3.0)
            .with_cap(StrokeCap::Round)
            .with_join(StrokeJoin::Round)
            .with_dash_pattern(vec![5.0, 3.0]);

        assert_eq!(opts.width, 3.0);
        assert_eq!(opts.cap, StrokeCap::Round);
        assert_eq!(opts.join, StrokeJoin::Round);
        assert!(opts.dash_pattern.is_some());
    }

    #[test]
    fn test_path_layer_bounds() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 10.0));
        path.line_to(Point::new(100.0, 100.0));

        let layer = PathLayer::new(path);
        let bounds = layer.bounds();

        assert!(bounds.width() > 0.0);
        assert!(bounds.height() > 0.0);
    }
}
