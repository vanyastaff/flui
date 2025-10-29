//! Path types for vector drawing.
//!
//! Provides Path structure for creating complex shapes with lines, curves, and arcs.

use crate::geometry::{Offset, Point, Rect};
use crate::painting::PathFillType;

/// A command in a path.
///
/// Similar to SVG path commands and Flutter's Path operations.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathCommand {
    /// Move to a point without drawing.
    MoveTo(Point),

    /// Draw a line to a point.
    LineTo(Point),

    /// Draw a quadratic Bézier curve.
    ///
    /// Arguments: control point, end point
    QuadraticTo(Point, Point),

    /// Draw a cubic Bézier curve.
    ///
    /// Arguments: control point 1, control point 2, end point
    CubicTo(Point, Point, Point),

    /// Close the current subpath by drawing a line to the starting point.
    Close,

    /// Add a rectangle.
    AddRect(Rect),

    /// Add a circle.
    ///
    /// Arguments: center, radius
    AddCircle(Point, f32),

    /// Add an oval (ellipse).
    ///
    /// Arguments: bounding rectangle
    AddOval(Rect),

    /// Add an arc.
    ///
    /// Arguments: bounding rectangle, start angle (radians), sweep angle (radians)
    AddArc(Rect, f32, f32),
}

/// A complex, one-dimensional subset of a plane.
///
/// Similar to Flutter's `ui.Path` and HTML Canvas Path2D.
///
/// A path consists of a number of sub-paths, and a current point.
/// Sub-paths consist of segments of various types (lines, arcs, cubic Bézier curves).
///
/// # Examples
///
/// ```rust
/// use flui_types::painting::Path;
/// use flui_types::geometry::Point;
///
/// let mut path = Path::new();
/// path.move_to(Point::new(0.0, 0.0));
/// path.line_to(Point::new(100.0, 0.0));
/// path.line_to(Point::new(100.0, 100.0));
/// path.close();
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Path {
    /// The list of path commands.
    commands: Vec<PathCommand>,

    /// The fill type for this path.
    fill_type: PathFillType,

    /// Cached bounding box (invalidated when commands change).
    bounds: Option<Rect>,
}

impl Path {
    /// Creates a new empty path.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            fill_type: PathFillType::default(),
            bounds: None,
        }
    }

    /// Creates a path with a specific fill type.
    #[inline]
    #[must_use]
    pub fn with_fill_type(fill_type: PathFillType) -> Self {
        Self {
            commands: Vec::new(),
            fill_type,
            bounds: None,
        }
    }

    /// Sets the fill type for this path.
    #[inline]
    pub fn set_fill_type(&mut self, fill_type: PathFillType) {
        self.fill_type = fill_type;
    }

    /// Gets the fill type for this path.
    #[inline]
    #[must_use]
    pub const fn fill_type(&self) -> PathFillType {
        self.fill_type
    }

    /// Starts a new subpath at the given point.
    #[inline]
    pub fn move_to(&mut self, point: Point) {
        self.commands.push(PathCommand::MoveTo(point));
        self.bounds = None;
    }

    /// Adds a line from the current point to the given point.
    #[inline]
    pub fn line_to(&mut self, point: Point) {
        self.commands.push(PathCommand::LineTo(point));
        self.bounds = None;
    }

    /// Adds a quadratic Bézier curve from the current point.
    ///
    /// # Arguments
    ///
    /// * `control` - The control point
    /// * `end` - The end point
    #[inline]
    pub fn quadratic_to(&mut self, control: Point, end: Point) {
        self.commands.push(PathCommand::QuadraticTo(control, end));
        self.bounds = None;
    }

    /// Adds a cubic Bézier curve from the current point.
    ///
    /// # Arguments
    ///
    /// * `control1` - The first control point
    /// * `control2` - The second control point
    /// * `end` - The end point
    #[inline]
    pub fn cubic_to(&mut self, control1: Point, control2: Point, end: Point) {
        self.commands
            .push(PathCommand::CubicTo(control1, control2, end));
        self.bounds = None;
    }

    /// Closes the current subpath by adding a line back to the starting point.
    #[inline]
    pub fn close(&mut self) {
        self.commands.push(PathCommand::Close);
    }

    /// Adds a rectangle to the path.
    #[inline]
    pub fn add_rect(&mut self, rect: Rect) {
        self.commands.push(PathCommand::AddRect(rect));
        self.bounds = None;
    }

    /// Adds a circle to the path.
    ///
    /// # Arguments
    ///
    /// * `center` - The center of the circle
    /// * `radius` - The radius of the circle
    #[inline]
    pub fn add_circle(&mut self, center: Point, radius: f32) {
        self.commands.push(PathCommand::AddCircle(center, radius));
        self.bounds = None;
    }

    /// Adds an oval (ellipse) to the path.
    ///
    /// # Arguments
    ///
    /// * `rect` - The bounding rectangle of the oval
    #[inline]
    pub fn add_oval(&mut self, rect: Rect) {
        self.commands.push(PathCommand::AddOval(rect));
        self.bounds = None;
    }

    /// Adds an arc to the path.
    ///
    /// # Arguments
    ///
    /// * `rect` - The bounding rectangle of the arc
    /// * `start_angle` - The starting angle in radians
    /// * `sweep_angle` - The sweep angle in radians
    #[inline]
    pub fn add_arc(&mut self, rect: Rect, start_angle: f32, sweep_angle: f32) {
        self.commands
            .push(PathCommand::AddArc(rect, start_angle, sweep_angle));
        self.bounds = None;
    }

    /// Returns an iterator over the path commands.
    #[inline]
    #[must_use]
    pub fn commands(&self) -> &[PathCommand] {
        &self.commands
    }

    /// Returns true if the path contains no commands.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Clears all commands from the path.
    #[inline]
    pub fn reset(&mut self) {
        self.commands.clear();
        self.bounds = None;
    }

    /// Computes and returns the bounding box of the path.
    ///
    /// This is cached after the first computation.
    #[must_use]
    pub fn bounds(&mut self) -> Rect {
        if let Some(bounds) = self.bounds {
            return bounds;
        }

        // Compute bounds from commands
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for cmd in &self.commands {
            match cmd {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                PathCommand::QuadraticTo(c, e) => {
                    min_x = min_x.min(c.x).min(e.x);
                    min_y = min_y.min(c.y).min(e.y);
                    max_x = max_x.max(c.x).max(e.x);
                    max_y = max_y.max(c.y).max(e.y);
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    min_x = min_x.min(c1.x).min(c2.x).min(e.x);
                    min_y = min_y.min(c1.y).min(c2.y).min(e.y);
                    max_x = max_x.max(c1.x).max(c2.x).max(e.x);
                    max_y = max_y.max(c1.y).max(c2.y).max(e.y);
                }
                PathCommand::AddRect(r)
                | PathCommand::AddOval(r)
                | PathCommand::AddArc(r, _, _) => {
                    min_x = min_x.min(r.left());
                    min_y = min_y.min(r.top());
                    max_x = max_x.max(r.right());
                    max_y = max_y.max(r.bottom());
                }
                PathCommand::AddCircle(center, radius) => {
                    min_x = min_x.min(center.x - radius);
                    min_y = min_y.min(center.y - radius);
                    max_x = max_x.max(center.x + radius);
                    max_y = max_y.max(center.y + radius);
                }
                PathCommand::Close => {}
            }
        }

        let bounds = if min_x.is_finite() && max_x.is_finite() {
            Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
        } else {
            Rect::ZERO
        };

        self.bounds = Some(bounds);
        bounds
    }

    /// Transforms the path by translating it.
    #[must_use]
    pub fn translate(&self, offset: Offset) -> Self {
        let commands = self
            .commands
            .iter()
            .map(|cmd| match *cmd {
                PathCommand::MoveTo(p) => {
                    PathCommand::MoveTo(Point::new(p.x + offset.dx, p.y + offset.dy))
                }
                PathCommand::LineTo(p) => {
                    PathCommand::LineTo(Point::new(p.x + offset.dx, p.y + offset.dy))
                }
                PathCommand::QuadraticTo(c, e) => PathCommand::QuadraticTo(
                    Point::new(c.x + offset.dx, c.y + offset.dy),
                    Point::new(e.x + offset.dx, e.y + offset.dy),
                ),
                PathCommand::CubicTo(c1, c2, e) => PathCommand::CubicTo(
                    Point::new(c1.x + offset.dx, c1.y + offset.dy),
                    Point::new(c2.x + offset.dx, c2.y + offset.dy),
                    Point::new(e.x + offset.dx, e.y + offset.dy),
                ),
                PathCommand::AddRect(r) => PathCommand::AddRect(r.translate(offset.dx, offset.dy)),
                PathCommand::AddCircle(center, radius) => PathCommand::AddCircle(
                    Point::new(center.x + offset.dx, center.y + offset.dy),
                    radius,
                ),
                PathCommand::AddOval(r) => PathCommand::AddOval(r.translate(offset.dx, offset.dy)),
                PathCommand::AddArc(r, start, sweep) => {
                    PathCommand::AddArc(r.translate(offset.dx, offset.dy), start, sweep)
                }
                PathCommand::Close => PathCommand::Close,
            })
            .collect();

        Self {
            commands,
            fill_type: self.fill_type,
            bounds: None,
        }
    }
}

impl Default for Path {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_new() {
        let path = Path::new();
        assert!(path.is_empty());
        assert_eq!(path.fill_type(), PathFillType::NonZero);
    }

    #[test]
    fn test_path_move_line() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));

        assert_eq!(path.commands().len(), 2);
        assert!(!path.is_empty());
    }

    #[test]
    fn test_path_close() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));
        path.close();

        assert_eq!(path.commands().len(), 4);
    }

    #[test]
    fn test_path_add_rect() {
        let mut path = Path::new();
        let rect = Rect::from_xywh(10.0, 10.0, 100.0, 100.0);
        path.add_rect(rect);

        assert_eq!(path.commands().len(), 1);
    }

    #[test]
    fn test_path_add_circle() {
        let mut path = Path::new();
        path.add_circle(Point::new(50.0, 50.0), 25.0);

        assert_eq!(path.commands().len(), 1);
    }

    #[test]
    fn test_path_bounds() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 10.0));
        path.line_to(Point::new(100.0, 100.0));

        let bounds = path.bounds();
        assert_eq!(bounds.left(), 10.0);
        assert_eq!(bounds.top(), 10.0);
        assert_eq!(bounds.right(), 100.0);
        assert_eq!(bounds.bottom(), 100.0);
    }

    #[test]
    fn test_path_reset() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));

        assert!(!path.is_empty());

        path.reset();
        assert!(path.is_empty());
    }

    #[test]
    fn test_path_translate() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 10.0));
        path.line_to(Point::new(20.0, 20.0));

        let translated = path.translate(Offset::new(5.0, 5.0));

        match translated.commands()[0] {
            PathCommand::MoveTo(p) => {
                assert_eq!(p.x, 15.0);
                assert_eq!(p.y, 15.0);
            }
            _ => panic!("Expected MoveTo"),
        }
    }
}
