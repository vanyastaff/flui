//! Path types for vector drawing.
//!
//! Provides Path structure for creating complex shapes with lines, curves, and arcs.

use crate::geometry::{Offset, Point, Rect};
use crate::painting::PathFillType;
use smallvec::SmallVec;

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
    /// Uses SmallVec to avoid heap allocation for simple paths (<16 commands).
    commands: SmallVec<[PathCommand; 16]>,

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
            commands: SmallVec::new(),
            fill_type: PathFillType::default(),
            bounds: None,
        }
    }

    /// Creates a path with a specific fill type.
    #[inline]
    #[must_use]
    pub fn with_fill_type(fill_type: PathFillType) -> Self {
        Self {
            commands: SmallVec::new(),
            fill_type,
            bounds: None,
        }
    }

    /// Creates a path containing a rectangle.
    ///
    /// Common pattern for drawing rectangular shapes. More concise than creating
    /// an empty path and calling add_rect.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::Rect;
    ///
    /// let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 50.0);
    /// let path = Path::rectangle(rect);
    /// ```
    #[inline]
    #[must_use]
    pub fn rectangle(rect: Rect) -> Self {
        let mut path = Self::new();
        path.add_rect(rect);
        path
    }

    /// Creates a path containing a circle.
    ///
    /// Common pattern for drawing circular shapes.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::Point;
    ///
    /// let path = Path::circle(Point::new(50.0, 50.0), 25.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn circle(center: Point, radius: f32) -> Self {
        let mut path = Self::new();
        path.add_circle(center, radius);
        path
    }

    /// Creates a path containing a single line.
    ///
    /// Useful for drawing simple lines without manual move_to/line_to calls.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::Point;
    ///
    /// let path = Path::line(Point::new(0.0, 0.0), Point::new(100.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn line(from: Point, to: Point) -> Self {
        let mut path = Self::new();
        path.move_to(from);
        path.line_to(to);
        path
    }

    /// Creates a path containing an oval (ellipse).
    ///
    /// The oval is inscribed within the given rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::Rect;
    ///
    /// let path = Path::oval(Rect::from_xywh(0.0, 0.0, 100.0, 50.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn oval(rect: Rect) -> Self {
        let mut path = Self::new();
        path.add_oval(rect);
        path
    }

    /// Creates a path containing an arc.
    ///
    /// The arc is part of an oval inscribed in the given rectangle.
    ///
    /// # Arguments
    ///
    /// * `rect` - The bounding rectangle of the oval
    /// * `start_angle` - The starting angle in radians
    /// * `sweep_angle` - The sweep angle in radians
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::Rect;
    /// use std::f32::consts::PI;
    ///
    /// // Create a quarter circle arc
    /// let path = Path::arc(
    ///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
    ///     0.0,
    ///     PI / 2.0,
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn arc(rect: Rect, start_angle: f32, sweep_angle: f32) -> Self {
        let mut path = Self::new();
        path.add_arc(rect, start_angle, sweep_angle);
        path
    }

    /// Creates a path containing a polygon from a sequence of points.
    ///
    /// The path will automatically close by connecting the last point to the first.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::Point;
    ///
    /// // Create a triangle
    /// let path = Path::polygon(&[
    ///     Point::new(50.0, 0.0),
    ///     Point::new(100.0, 100.0),
    ///     Point::new(0.0, 100.0),
    /// ]);
    /// ```
    #[must_use]
    pub fn polygon(points: &[Point]) -> Self {
        let mut path = Self::new();

        if points.is_empty() {
            return path;
        }

        path.move_to(points[0]);
        for point in &points[1..] {
            path.line_to(*point);
        }
        path.close();

        path
    }

    /// Creates a path from a rounded rectangle (RRect).
    ///
    /// This creates a path that traces the outline of a rounded rectangle,
    /// properly handling the elliptical corners.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::{RRect, Rect};
    /// use flui_types::styling::Radius;
    ///
    /// let rrect = RRect::from_rect_circular(
    ///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
    ///     10.0
    /// );
    /// let path = Path::from_rrect(rrect);
    /// ```
    #[must_use]
    pub fn from_rrect(rrect: crate::geometry::RRect) -> Self {
        let mut path = Self::new();

        // If no rounding, just add a rectangle
        if rrect.is_rect() {
            path.add_rect(rrect.bounding_rect());
            return path;
        }

        let rect = rrect.bounding_rect();
        let tl = rrect.top_left;
        let tr = rrect.top_right;
        let br = rrect.bottom_right;
        let bl = rrect.bottom_left;

        // Start at top-left, after the corner radius
        path.move_to(Point::new(rect.left() + tl.x, rect.top()));

        // Top edge
        path.line_to(Point::new(rect.right() - tr.x, rect.top()));

        // Top-right corner
        if tr.x > 0.0 || tr.y > 0.0 {
            let corner_rect = Rect::from_xywh(
                rect.right() - tr.x * 2.0,
                rect.top(),
                tr.x * 2.0,
                tr.y * 2.0,
            );
            path.add_arc(
                corner_rect,
                -std::f32::consts::FRAC_PI_2,
                std::f32::consts::FRAC_PI_2,
            );
        }

        // Right edge
        path.line_to(Point::new(rect.right(), rect.bottom() - br.y));

        // Bottom-right corner
        if br.x > 0.0 || br.y > 0.0 {
            let corner_rect = Rect::from_xywh(
                rect.right() - br.x * 2.0,
                rect.bottom() - br.y * 2.0,
                br.x * 2.0,
                br.y * 2.0,
            );
            path.add_arc(corner_rect, 0.0, std::f32::consts::FRAC_PI_2);
        }

        // Bottom edge
        path.line_to(Point::new(rect.left() + bl.x, rect.bottom()));

        // Bottom-left corner
        if bl.x > 0.0 || bl.y > 0.0 {
            let corner_rect = Rect::from_xywh(
                rect.left(),
                rect.bottom() - bl.y * 2.0,
                bl.x * 2.0,
                bl.y * 2.0,
            );
            path.add_arc(
                corner_rect,
                std::f32::consts::FRAC_PI_2,
                std::f32::consts::FRAC_PI_2,
            );
        }

        // Left edge
        path.line_to(Point::new(rect.left(), rect.top() + tl.y));

        // Top-left corner
        if tl.x > 0.0 || tl.y > 0.0 {
            let corner_rect = Rect::from_xywh(rect.left(), rect.top(), tl.x * 2.0, tl.y * 2.0);
            path.add_arc(
                corner_rect,
                std::f32::consts::PI,
                std::f32::consts::FRAC_PI_2,
            );
        }

        path.close();
        path
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

    /// Returns the cached bounding box if available, without computing.
    ///
    /// This is useful when you have an immutable reference to the path and need
    /// the bounds if they were already computed. Returns `None` if bounds haven't
    /// been computed yet.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::Point;
    ///
    /// let mut path = Path::new();
    /// path.move_to(Point::new(0.0, 0.0));
    /// path.line_to(Point::new(100.0, 100.0));
    ///
    /// // Before computing
    /// assert!(path.cached_bounds().is_none());
    ///
    /// // After computing
    /// let _ = path.bounds();
    /// assert!(path.cached_bounds().is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn cached_bounds(&self) -> Option<Rect> {
        self.bounds
    }

    /// Computes the bounding box without caching.
    ///
    /// Use this when you have an immutable reference and need bounds computed.
    /// For repeated access, prefer `bounds()` which caches the result.
    #[must_use]
    pub fn compute_bounds(&self) -> Rect {
        // Quick return if cached
        if let Some(bounds) = self.bounds {
            return bounds;
        }

        self.compute_bounds_internal()
    }

    /// Internal bounds computation (shared between bounds() and compute_bounds())
    fn compute_bounds_internal(&self) -> Rect {
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

        if min_x.is_finite() && max_x.is_finite() {
            Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
        } else {
            Rect::ZERO
        }
    }

    /// Computes and returns the bounding box of the path.
    ///
    /// This is cached after the first computation.
    #[must_use]
    pub fn bounds(&mut self) -> Rect {
        if let Some(bounds) = self.bounds {
            return bounds;
        }

        let bounds = self.compute_bounds_internal();
        self.bounds = Some(bounds);
        bounds
    }

    /// Computes and returns the bounding box of the path (legacy).
    ///
    /// This is cached after the first computation.
    #[deprecated(
        since = "0.2.0",
        note = "Use `bounds()` for mutable or `compute_bounds()` for immutable access"
    )]
    #[must_use]
    pub fn bounds_mut(&mut self) -> Rect {
        self.bounds()
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

    /// Tests whether a point is inside the path using the path's fill rule.
    ///
    /// This implements both even-odd and non-zero winding number algorithms
    /// for path containment testing.
    ///
    /// # Algorithm
    ///
    /// - **EvenOdd**: Counts the number of times a ray from the point crosses
    ///   path edges. Point is inside if count is odd.
    /// - **NonZero**: Computes the winding number by considering edge direction.
    ///   Point is inside if winding number is non-zero.
    ///
    /// # Performance
    ///
    /// This method processes all path commands and may be expensive for complex paths.
    /// Consider caching results if testing many points against the same path.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::painting::Path;
    /// use flui_types::geometry::{Point, Rect};
    ///
    /// let mut path = Path::rectangle(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
    /// assert!(path.contains(Point::new(50.0, 50.0)));
    /// assert!(!path.contains(Point::new(150.0, 50.0)));
    /// ```
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        // Quick bounds check using compute_bounds() (no mutation needed)
        let bounds = self.compute_bounds();
        if !bounds.contains(point) {
            return false;
        }

        match self.fill_type {
            PathFillType::EvenOdd => self.contains_even_odd(point),
            PathFillType::NonZero => self.contains_non_zero(point),
        }
    }

    /// Ray casting algorithm for even-odd fill rule.
    fn contains_even_odd(&self, point: Point) -> bool {
        let mut crossings = 0;
        let mut current_pos = Point::ZERO;
        let mut subpath_start = Point::ZERO;

        for cmd in &self.commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    current_pos = *p;
                    subpath_start = *p;
                }
                PathCommand::LineTo(p) => {
                    if self.ray_intersects_segment(point, current_pos, *p) {
                        crossings += 1;
                    }
                    current_pos = *p;
                }
                PathCommand::Close => {
                    if self.ray_intersects_segment(point, current_pos, subpath_start) {
                        crossings += 1;
                    }
                    current_pos = subpath_start;
                }
                PathCommand::QuadraticTo(c, e) => {
                    // Approximate with line segments
                    crossings += self.count_curve_crossings_quad(point, current_pos, *c, *e);
                    current_pos = *e;
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    // Approximate with line segments
                    crossings += self.count_curve_crossings_cubic(point, current_pos, *c1, *c2, *e);
                    current_pos = *e;
                }
                PathCommand::AddRect(rect) => {
                    // Simple rectangle test
                    if rect.contains(point) {
                        crossings += 1;
                    }
                }
                PathCommand::AddCircle(center, radius) => {
                    // Simple circle test
                    let dx = point.x - center.x;
                    let dy = point.y - center.y;
                    if dx * dx + dy * dy <= radius * radius {
                        crossings += 1;
                    }
                }
                PathCommand::AddOval(rect) => {
                    // Ellipse test
                    let cx = (rect.left() + rect.right()) * 0.5;
                    let cy = (rect.top() + rect.bottom()) * 0.5;
                    let rx = rect.width() * 0.5;
                    let ry = rect.height() * 0.5;
                    let dx = (point.x - cx) / rx;
                    let dy = (point.y - cy) / ry;
                    if dx * dx + dy * dy <= 1.0 {
                        crossings += 1;
                    }
                }
                PathCommand::AddArc(_, _, _) => {
                    // TODO: Implement arc containment
                    // For now, skip arcs (conservative - may miss some points)
                }
            }
        }

        crossings % 2 == 1
    }

    /// Winding number algorithm for non-zero fill rule.
    fn contains_non_zero(&self, point: Point) -> bool {
        let mut winding = 0;
        let mut current_pos = Point::ZERO;
        let mut subpath_start = Point::ZERO;

        for cmd in &self.commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    current_pos = *p;
                    subpath_start = *p;
                }
                PathCommand::LineTo(p) => {
                    winding += self.segment_winding(point, current_pos, *p);
                    current_pos = *p;
                }
                PathCommand::Close => {
                    winding += self.segment_winding(point, current_pos, subpath_start);
                    current_pos = subpath_start;
                }
                PathCommand::QuadraticTo(c, e) => {
                    winding += self.curve_winding_quad(point, current_pos, *c, *e);
                    current_pos = *e;
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    winding += self.curve_winding_cubic(point, current_pos, *c1, *c2, *e);
                    current_pos = *e;
                }
                PathCommand::AddRect(rect) => {
                    if rect.contains(point) {
                        winding += 1;
                    }
                }
                PathCommand::AddCircle(center, radius) => {
                    let dx = point.x - center.x;
                    let dy = point.y - center.y;
                    if dx * dx + dy * dy <= radius * radius {
                        winding += 1;
                    }
                }
                PathCommand::AddOval(rect) => {
                    let cx = (rect.left() + rect.right()) * 0.5;
                    let cy = (rect.top() + rect.bottom()) * 0.5;
                    let rx = rect.width() * 0.5;
                    let ry = rect.height() * 0.5;
                    let dx = (point.x - cx) / rx;
                    let dy = (point.y - cy) / ry;
                    if dx * dx + dy * dy <= 1.0 {
                        winding += 1;
                    }
                }
                PathCommand::AddArc(_, _, _) => {
                    // TODO: Implement arc winding
                }
            }
        }

        winding != 0
    }

    /// Tests if a horizontal ray from point intersects a line segment.
    fn ray_intersects_segment(&self, point: Point, p1: Point, p2: Point) -> bool {
        // Ray extends to the right from point
        if (p1.y > point.y) == (p2.y > point.y) {
            return false; // Both endpoints on same side of ray
        }

        // Calculate x coordinate of intersection
        let x_intersect = p1.x + (point.y - p1.y) / (p2.y - p1.y) * (p2.x - p1.x);
        x_intersect > point.x
    }

    /// Compute winding contribution of a line segment.
    fn segment_winding(&self, point: Point, p1: Point, p2: Point) -> i32 {
        if p1.y <= point.y {
            if p2.y > point.y {
                // Upward crossing
                if self.is_left(p1, p2, point) > 0.0 {
                    return 1;
                }
            }
        } else if p2.y <= point.y {
            // Downward crossing
            if self.is_left(p1, p2, point) < 0.0 {
                return -1;
            }
        }
        0
    }

    /// Test if point is left of line segment (p1 -> p2).
    /// Returns > 0 for left, < 0 for right, 0 for on line.
    fn is_left(&self, p1: Point, p2: Point, point: Point) -> f32 {
        (p2.x - p1.x) * (point.y - p1.y) - (point.x - p1.x) * (p2.y - p1.y)
    }

    /// Count crossings for quadratic bezier curve (approximated).
    fn count_curve_crossings_quad(&self, point: Point, p0: Point, p1: Point, p2: Point) -> usize {
        // Simple approximation: subdivide into 4 line segments
        let t_values = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut crossings = 0;

        for i in 0..4 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = self.eval_quadratic(p0, p1, p2, t1);
            let end = self.eval_quadratic(p0, p1, p2, t2);

            if self.ray_intersects_segment(point, start, end) {
                crossings += 1;
            }
        }

        crossings
    }

    /// Count crossings for cubic bezier curve (approximated).
    fn count_curve_crossings_cubic(
        &self,
        point: Point,
        p0: Point,
        p1: Point,
        p2: Point,
        p3: Point,
    ) -> usize {
        // Simple approximation: subdivide into 8 line segments
        let t_values = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
        let mut crossings = 0;

        for i in 0..8 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = self.eval_cubic(p0, p1, p2, p3, t1);
            let end = self.eval_cubic(p0, p1, p2, p3, t2);

            if self.ray_intersects_segment(point, start, end) {
                crossings += 1;
            }
        }

        crossings
    }

    /// Winding number for quadratic curve.
    fn curve_winding_quad(&self, point: Point, p0: Point, p1: Point, p2: Point) -> i32 {
        let t_values = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut winding = 0;

        for i in 0..4 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = self.eval_quadratic(p0, p1, p2, t1);
            let end = self.eval_quadratic(p0, p1, p2, t2);

            winding += self.segment_winding(point, start, end);
        }

        winding
    }

    /// Winding number for cubic curve.
    fn curve_winding_cubic(&self, point: Point, p0: Point, p1: Point, p2: Point, p3: Point) -> i32 {
        let t_values = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
        let mut winding = 0;

        for i in 0..8 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = self.eval_cubic(p0, p1, p2, p3, t1);
            let end = self.eval_cubic(p0, p1, p2, p3, t2);

            winding += self.segment_winding(point, start, end);
        }

        winding
    }

    /// Evaluate quadratic bezier at parameter t.
    fn eval_quadratic(&self, p0: Point, p1: Point, p2: Point, t: f32) -> Point {
        let t2 = t * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;

        Point::new(
            mt2 * p0.x + 2.0 * mt * t * p1.x + t2 * p2.x,
            mt2 * p0.y + 2.0 * mt * t * p1.y + t2 * p2.y,
        )
    }

    /// Evaluate cubic bezier at parameter t.
    fn eval_cubic(&self, p0: Point, p1: Point, p2: Point, p3: Point, t: f32) -> Point {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        Point::new(
            mt3 * p0.x + 3.0 * mt2 * t * p1.x + 3.0 * mt * t2 * p2.x + t3 * p3.x,
            mt3 * p0.y + 3.0 * mt2 * t * p1.y + 3.0 * mt * t2 * p2.y + t3 * p3.y,
        )
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

    // Path containment tests

    #[test]
    fn test_contains_rect_even_odd() {
        let mut path = Path::new();
        path.set_fill_type(PathFillType::EvenOdd);
        path.add_rect(Rect::from_xywh(10.0, 10.0, 100.0, 100.0));

        // Points inside
        assert!(path.contains(Point::new(50.0, 50.0)));
        assert!(path.contains(Point::new(20.0, 20.0)));
        assert!(path.contains(Point::new(100.0, 100.0)));

        // Points outside
        assert!(!path.contains(Point::new(0.0, 0.0)));
        assert!(!path.contains(Point::new(150.0, 50.0)));
        assert!(!path.contains(Point::new(50.0, 150.0)));

        // Edge cases (on boundary)
        assert!(path.contains(Point::new(10.0, 50.0)));
        assert!(path.contains(Point::new(110.0, 50.0)));
    }

    #[test]
    fn test_contains_rect_non_zero() {
        let mut path = Path::new();
        path.set_fill_type(PathFillType::NonZero);
        path.add_rect(Rect::from_xywh(10.0, 10.0, 100.0, 100.0));

        // Points inside
        assert!(path.contains(Point::new(50.0, 50.0)));
        assert!(path.contains(Point::new(20.0, 20.0)));
        assert!(path.contains(Point::new(100.0, 100.0)));

        // Points outside
        assert!(!path.contains(Point::new(0.0, 0.0)));
        assert!(!path.contains(Point::new(150.0, 50.0)));
        assert!(!path.contains(Point::new(50.0, 150.0)));
    }

    #[test]
    fn test_contains_circle() {
        let mut path = Path::new();
        let center = Point::new(50.0, 50.0);
        let radius = 25.0;
        path.add_circle(center, radius);

        // Points inside
        assert!(path.contains(center));
        assert!(path.contains(Point::new(60.0, 50.0)));
        assert!(path.contains(Point::new(50.0, 60.0)));

        // Points outside
        assert!(!path.contains(Point::new(0.0, 0.0)));
        assert!(!path.contains(Point::new(100.0, 50.0)));
        assert!(!path.contains(Point::new(50.0, 100.0)));

        // Points near boundary (inside)
        assert!(path.contains(Point::new(50.0 + radius * 0.9, 50.0)));
        assert!(path.contains(Point::new(50.0, 50.0 + radius * 0.9)));

        // Points near boundary (outside)
        assert!(!path.contains(Point::new(50.0 + radius * 1.1, 50.0)));
        assert!(!path.contains(Point::new(50.0, 50.0 + radius * 1.1)));
    }

    #[test]
    fn test_contains_oval() {
        let mut path = Path::new();
        path.add_oval(Rect::from_xywh(10.0, 10.0, 100.0, 50.0));

        // Points inside
        assert!(path.contains(Point::new(60.0, 35.0))); // Center
        assert!(path.contains(Point::new(70.0, 35.0)));

        // Points outside
        assert!(!path.contains(Point::new(0.0, 0.0)));
        assert!(!path.contains(Point::new(150.0, 35.0)));
        assert!(!path.contains(Point::new(60.0, 100.0)));
    }

    #[test]
    fn test_contains_triangle() {
        let mut path = Path::new();
        path.move_to(Point::new(50.0, 10.0));
        path.line_to(Point::new(90.0, 90.0));
        path.line_to(Point::new(10.0, 90.0));
        path.close();

        // Point inside
        assert!(path.contains(Point::new(50.0, 50.0)));
        assert!(path.contains(Point::new(40.0, 60.0)));

        // Points outside
        assert!(!path.contains(Point::new(10.0, 10.0)));
        assert!(!path.contains(Point::new(90.0, 10.0)));
        assert!(!path.contains(Point::new(50.0, 95.0)));
    }

    #[test]
    fn test_contains_quadratic_bezier() {
        let mut path = Path::new();
        // Create a simple closed shape with a quadratic bezier curve
        path.move_to(Point::new(10.0, 50.0));
        path.line_to(Point::new(10.0, 10.0));
        path.line_to(Point::new(90.0, 10.0));
        path.line_to(Point::new(90.0, 50.0));
        // Quadratic curve back (bulging downward)
        path.quadratic_to(Point::new(50.0, 80.0), Point::new(10.0, 50.0));
        path.close();

        // Points inside the shape
        assert!(path.contains(Point::new(50.0, 30.0)));
        assert!(path.contains(Point::new(30.0, 25.0)));
        assert!(path.contains(Point::new(50.0, 50.0)));

        // Points outside (below the curve or far away)
        assert!(!path.contains(Point::new(50.0, 85.0)));
        assert!(!path.contains(Point::new(0.0, 0.0)));
        assert!(!path.contains(Point::new(100.0, 10.0)));
    }

    #[test]
    fn test_contains_cubic_bezier() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 50.0));
        path.cubic_to(
            Point::new(30.0, 10.0),
            Point::new(70.0, 90.0),
            Point::new(90.0, 50.0),
        );
        path.line_to(Point::new(90.0, 80.0));
        path.line_to(Point::new(10.0, 80.0));
        path.close();

        // Points inside (should be inside the closed path)
        assert!(path.contains(Point::new(50.0, 60.0)));

        // Points outside
        assert!(!path.contains(Point::new(0.0, 50.0)));
        assert!(!path.contains(Point::new(100.0, 50.0)));
        assert!(!path.contains(Point::new(50.0, 0.0)));
    }

    #[test]
    fn test_contains_donut_even_odd() {
        // Create a donut shape: outer rect with inner rect hole
        let mut path = Path::new();
        path.set_fill_type(PathFillType::EvenOdd);

        // Outer rectangle
        path.add_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));

        // Inner rectangle (hole)
        path.add_rect(Rect::from_xywh(25.0, 25.0, 50.0, 50.0));

        // Points in the "ring" (between outer and inner)
        assert!(path.contains(Point::new(10.0, 10.0)));
        assert!(path.contains(Point::new(90.0, 90.0)));
        assert!(path.contains(Point::new(10.0, 50.0)));

        // Points in the hole (should be outside with even-odd)
        assert!(!path.contains(Point::new(50.0, 50.0)));
        assert!(!path.contains(Point::new(40.0, 40.0)));
        assert!(!path.contains(Point::new(60.0, 60.0)));

        // Points completely outside
        assert!(!path.contains(Point::new(-10.0, 50.0)));
        assert!(!path.contains(Point::new(110.0, 50.0)));
    }

    #[test]
    fn test_contains_donut_non_zero() {
        // Create a donut with non-zero winding
        let mut path = Path::new();
        path.set_fill_type(PathFillType::NonZero);

        // Outer rectangle (counter-clockwise)
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));
        path.line_to(Point::new(0.0, 100.0));
        path.close();

        // Inner rectangle (clockwise - opposite winding)
        path.move_to(Point::new(25.0, 25.0));
        path.line_to(Point::new(25.0, 75.0));
        path.line_to(Point::new(75.0, 75.0));
        path.line_to(Point::new(75.0, 25.0));
        path.close();

        // Points in the ring
        assert!(path.contains(Point::new(10.0, 10.0)));
        assert!(path.contains(Point::new(90.0, 90.0)));

        // Points in the hole (opposite winding cancels out)
        assert!(!path.contains(Point::new(50.0, 50.0)));
        assert!(!path.contains(Point::new(40.0, 40.0)));
    }

    #[test]
    fn test_contains_complex_path() {
        // Complex path with lines and curves
        let mut path = Path::new();
        path.move_to(Point::new(20.0, 20.0));
        path.line_to(Point::new(80.0, 20.0));
        path.quadratic_to(Point::new(100.0, 50.0), Point::new(80.0, 80.0));
        path.line_to(Point::new(20.0, 80.0));
        path.cubic_to(
            Point::new(10.0, 60.0),
            Point::new(10.0, 40.0),
            Point::new(20.0, 20.0),
        );
        path.close();

        // Point clearly inside
        assert!(path.contains(Point::new(50.0, 50.0)));

        // Points clearly outside
        assert!(!path.contains(Point::new(0.0, 0.0)));
        assert!(!path.contains(Point::new(110.0, 50.0)));
        assert!(!path.contains(Point::new(50.0, 100.0)));
    }

    #[test]
    fn test_contains_empty_path() {
        let path = Path::new();
        assert!(!path.contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn test_contains_point_outside_bounds() {
        let mut path = Path::new();
        path.add_rect(Rect::from_xywh(10.0, 10.0, 100.0, 100.0));

        // Points far outside the bounds should quickly return false
        assert!(!path.contains(Point::new(-100.0, 50.0)));
        assert!(!path.contains(Point::new(200.0, 50.0)));
        assert!(!path.contains(Point::new(50.0, -100.0)));
        assert!(!path.contains(Point::new(50.0, 200.0)));
    }
}
