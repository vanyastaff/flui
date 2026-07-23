//! Path types for vector drawing.
//!
//! Provides Path structure for creating complex shapes with lines, curves, and
//! arcs.

use smallvec::SmallVec;

use crate::{
    geometry::{FloatUnit, NumericUnit, Offset, Pixels, Point, Rect, Vec2, px},
    painting::PathFillType,
};

/// A single drawing command within a [`Path`].
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathCommand {
    /// Move to a point without drawing.
    MoveTo(Point<Pixels>),

    /// Draw a line to a point.
    LineTo(Point<Pixels>),

    /// Draw a quadratic Bézier curve.
    ///
    /// Arguments: control point, end point
    QuadraticTo(Point<Pixels>, Point<Pixels>),

    /// Draw a cubic Bézier curve.
    ///
    /// Arguments: control point 1, control point 2, end point
    CubicTo(Point<Pixels>, Point<Pixels>, Point<Pixels>),

    /// Close the current subpath by drawing a line to the starting point.
    Close,

    /// Add a rectangle.
    AddRect(Rect<Pixels>),

    /// Add a circle.
    ///
    /// Arguments: center, radius
    AddCircle(Point<Pixels>, f32),

    /// Add an oval (ellipse).
    ///
    /// Arguments: bounding rectangle
    AddOval(Rect<Pixels>),

    /// Add an arc.
    ///
    /// Arguments: bounding rectangle, start angle (radians), sweep angle
    /// (radians)
    AddArc(Rect<Pixels>, f32, f32),
}

/// A sequence of drawing commands describing a vector shape.
///
/// A path is an ordered list of [`PathCommand`]s (lines, Bézier curves, and
/// whole shapes) plus a [`PathFillType`] that determines which regions count
/// as inside when filling or hit-testing.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Path {
    /// The list of path commands.
    /// Uses SmallVec to avoid heap allocation for simple paths (<16 commands).
    commands: SmallVec<[PathCommand; 16]>,

    /// The fill type for this path.
    fill_type: PathFillType,

    /// Cached bounding box (invalidated when commands change).
    bounds: Option<Rect<Pixels>>,
}

impl Path {
    /// Creates an empty path with the default fill type (non-zero).
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            commands: SmallVec::new(),
            fill_type: PathFillType::default(),
            bounds: None,
        }
    }

    /// Creates an empty path with the given fill type.
    #[must_use]
    #[inline]
    pub fn with_fill_type(fill_type: PathFillType) -> Self {
        Self {
            commands: SmallVec::new(),
            fill_type,
            bounds: None,
        }
    }

    /// Creates a path consisting of a single rectangle.
    #[must_use]
    #[inline]
    pub fn rectangle(rect: Rect<Pixels>) -> Self {
        let mut path = Self::new();
        path.add_rect(rect);
        path
    }

    /// Creates a path consisting of a single oval inscribed in `rect`.
    #[must_use]
    #[inline]
    pub fn oval(rect: Rect<Pixels>) -> Self {
        let mut path = Self::new();
        path.add_oval(rect);
        path
    }

    /// Creates a circular path.
    ///
    /// # Arguments
    ///
    /// * `center` - Center point of the circle
    /// * `radius` - Radius of the circle
    #[must_use]
    #[inline]
    pub fn circle(center: Point<Pixels>, radius: f32) -> Self {
        use crate::geometry::px;
        let rect = Rect::from_xywh(
            px(center.x.0 - radius),
            px(center.y.0 - radius),
            px(radius * 2.0),
            px(radius * 2.0),
        );
        Self::oval(rect)
    }

    /// Creates a polygon path from a slice of points.
    ///
    /// # Arguments
    ///
    /// * `points` - Vertices of the polygon
    #[must_use]
    #[inline]
    pub fn polygon(points: &[Point<Pixels>]) -> Self {
        let mut path = Self::new();
        if let Some((first, rest)) = points.split_first() {
            path.move_to(*first);
            for point in rest {
                path.line_to(*point);
            }
            path.close();
        }
        path
    }

    /// Creates a path consisting of a single arc.
    ///
    /// The arc lies on the oval inscribed in `rect`, starting at
    /// `start_angle` and sweeping by `sweep_angle` (both in radians).
    #[must_use]
    #[inline]
    pub fn arc(rect: Rect<Pixels>, start_angle: f32, sweep_angle: f32) -> Self {
        let mut path = Self::new();
        path.add_arc(rect, start_angle, sweep_angle);
        path
    }

    /// Creates a path outlining a rounded rectangle.
    ///
    /// Corners are approximated with quarter-circle arcs; a rounded rectangle
    /// with no rounding degenerates to a plain rectangle.
    #[must_use]
    #[inline]
    pub fn from_rrect(rrect: crate::geometry::RRect) -> Self {
        let mut path = Self::new();

        // If no rounding, just add a rectangle
        if rrect.is_rect() {
            path.add_rect(rrect.bounding_rect());
            return path;
        }

        let rect = rrect.bounding_rect();
        let tl_x = rrect.top_left.x;
        let tl_y = rrect.top_left.y;
        let tr_x = rrect.top_right.x;
        let tr_y = rrect.top_right.y;
        let br_x = rrect.bottom_right.x;
        let br_y = rrect.bottom_right.y;
        let bl_x = rrect.bottom_left.x;
        let bl_y = rrect.bottom_left.y;

        // Start at top-left, after the corner radius
        path.move_to(Point::new(rect.left() + tl_x, rect.top()));

        // Top edge
        path.line_to(Point::new(rect.right() - tr_x, rect.top()));

        // Top-right corner
        if tr_x > px(0.0) || tr_y > px(0.0) {
            let corner_rect = Rect::from_xywh(
                rect.right() - tr_x * 2.0,
                rect.top(),
                tr_x * 2.0,
                tr_y * 2.0,
            );
            path.add_arc(
                corner_rect,
                -std::f32::consts::FRAC_PI_2,
                std::f32::consts::FRAC_PI_2,
            );
        }

        // Right edge
        path.line_to(Point::new(rect.right(), rect.bottom() - br_y));

        // Bottom-right corner
        if br_x > px(0.0) || br_y > px(0.0) {
            let corner_rect = Rect::from_xywh(
                rect.right() - br_x * 2.0,
                rect.bottom() - br_y * 2.0,
                br_x * 2.0,
                br_y * 2.0,
            );
            path.add_arc(corner_rect, 0.0, std::f32::consts::FRAC_PI_2);
        }

        // Bottom edge
        path.line_to(Point::new(rect.left() + bl_x, rect.bottom()));

        // Bottom-left corner
        if bl_x > px(0.0) || bl_y > px(0.0) {
            let corner_rect = Rect::from_xywh(
                rect.left(),
                rect.bottom() - bl_y * 2.0,
                bl_x * 2.0,
                bl_y * 2.0,
            );
            path.add_arc(
                corner_rect,
                std::f32::consts::FRAC_PI_2,
                std::f32::consts::FRAC_PI_2,
            );
        }

        // Left edge
        path.line_to(Point::new(rect.left(), rect.top() + tl_y));

        // Top-left corner
        if tl_x > px(0.0) || tl_y > px(0.0) {
            let corner_rect = Rect::from_xywh(rect.left(), rect.top(), tl_x * 2.0, tl_y * 2.0);
            path.add_arc(
                corner_rect,
                std::f32::consts::PI,
                std::f32::consts::FRAC_PI_2,
            );
        }

        path.close();
        path
    }

    /// Recovers the originating [`RRect`] when this path is a fully-rounded
    /// rectangle emitted by [`Self::from_rrect`].
    ///
    /// The GPU shadow path uses this to route rounded-rectangle shadows
    /// (Material `Card` / `FloatingActionButton` / `Dialog` / `Chip`, …) through
    /// the analytical single-pass Gaussian shadow instead of the multi-layer
    /// path-fill approximation. It recognizes only the "all four corners
    /// rounded" command shape `from_rrect` emits — the common Material case;
    /// a plain rectangle, a partially-rounded rect, or any hand-built path
    /// returns `None` and keeps the fallback shadow.
    #[must_use]
    pub fn rrect_hint(&self) -> Option<crate::geometry::RRect> {
        use crate::geometry::{RRect, Radius};

        // `from_rrect`'s all-corners-rounded emission, in order:
        //   MoveTo, LineTo, AddArc(TR), LineTo, AddArc(BR),
        //   LineTo, AddArc(BL), LineTo, AddArc(TL), Close.
        // Each corner arc's bounding rect encodes that corner's box: its half
        // extents are the elliptical radii, and its edges pin the rectangle.
        let [
            PathCommand::MoveTo(_),
            PathCommand::LineTo(_),
            PathCommand::AddArc(tr, _, _),
            PathCommand::LineTo(_),
            PathCommand::AddArc(br, _, _),
            PathCommand::LineTo(_),
            PathCommand::AddArc(bl, _, _),
            PathCommand::LineTo(_),
            PathCommand::AddArc(tl, _, _),
            PathCommand::Close,
        ] = self.commands.as_slice()
        else {
            return None;
        };

        let rect = Rect::from_ltrb(tl.left(), tl.top(), tr.right(), br.bottom());
        let half = |corner: &Rect<Pixels>| {
            Radius::new(px(corner.width().0 / 2.0), px(corner.height().0 / 2.0))
        };
        Some(RRect::new(rect, half(tl), half(tr), half(br), half(bl)))
    }

    /// Sets the fill type used for filling and containment tests.
    #[inline]
    pub fn set_fill_type(&mut self, fill_type: PathFillType) {
        self.fill_type = fill_type;
    }

    /// Returns the path's current fill type.
    #[must_use]
    #[inline]
    pub const fn fill_type(&self) -> PathFillType {
        self.fill_type
    }

    /// Returns `true` if `point` lies inside this path, respecting the
    /// path's [`PathFillType`] (non-zero winding or even-odd).
    ///
    /// Uses a ray-casting algorithm for even-odd fill and a winding-number
    /// algorithm for non-zero fill.
    ///
    /// Note: `AddArc` commands are currently ignored (conservative miss);
    /// only line/quadratic/cubic segments, rects, circles, and ovals are
    /// evaluated.
    #[must_use]
    #[inline]
    pub fn contains(&self, point: Point<Pixels>) -> bool {
        match self.fill_type {
            PathFillType::EvenOdd => self.contains_even_odd(point),
            PathFillType::NonZero => self.contains_non_zero(point),
        }
    }

    /// Starts a new subpath at `point` without drawing.
    #[inline]
    pub fn move_to(&mut self, point: Point<Pixels>) {
        self.commands.push(PathCommand::MoveTo(point));
        self.bounds = None;
    }

    /// Adds a straight line from the current position to `point`.
    #[inline]
    pub fn line_to(&mut self, point: Point<Pixels>) {
        self.commands.push(PathCommand::LineTo(point));
        self.bounds = None;
    }

    /// Closes the current subpath with a line back to its starting point.
    #[inline]
    pub fn close(&mut self) {
        self.commands.push(PathCommand::Close);
    }

    /// Adds a rectangle as a separate subpath.
    #[inline]
    pub fn add_rect(&mut self, rect: Rect<Pixels>) {
        self.commands.push(PathCommand::AddRect(rect));
        self.bounds = None;
    }

    /// Adds an oval inscribed in `rect` as a separate subpath.
    #[inline]
    pub fn add_oval(&mut self, rect: Rect<Pixels>) {
        self.commands.push(PathCommand::AddOval(rect));
        self.bounds = None;
    }

    /// Adds an arc on the oval inscribed in `rect`, starting at `start_angle`
    /// and sweeping by `sweep_angle` (both in radians).
    ///
    /// # Divergence from Flutter's `Path.addArc`
    ///
    /// Flutter's `Path.addArc` always starts a **new** sub-path — it behaves
    /// like `arcTo(rect, startAngle, sweepAngle, forceMoveTo: true)`,
    /// regardless of what the path was doing before the call.
    ///
    /// FLUI's `add_arc` does not: when called while a sub-path is already
    /// open (a preceding `move_to`/`line_to`/`add_arc` with no `close`), the
    /// tessellator (`flui-engine`'s `wgpu::tessellator`) draws a line from
    /// the current position to the arc's start and *continues* that
    /// contour — chord-connected, not a new sub-path. This is deliberate:
    /// [`Self::from_rrect`] builds a fully-rounded rectangle as one
    /// continuous contour (edge, corner arc, edge, corner arc, …), and
    /// [`Self::rrect_hint`] depends on recognizing that exact single-contour
    /// command shape to route rounded-rectangle shadows through the
    /// analytical fast path. Starting a fresh sub-path per corner arc would
    /// fragment the contour into four open pieces, each rendering an
    /// unwanted diagonal fill-closure chord across its corner.
    ///
    /// A caller that wants Flutter's "always a new sub-path" semantics must
    /// call [`Self::move_to`] (to the arc's start point) immediately before
    /// `add_arc`, or call `add_arc` as the first command on a fresh `Path`.
    #[inline]
    pub fn add_arc(&mut self, rect: Rect<Pixels>, start_angle: f32, sweep_angle: f32) {
        self.commands
            .push(PathCommand::AddArc(rect, start_angle, sweep_angle));
        self.bounds = None;
    }

    /// Returns the path's commands in insertion order.
    #[must_use]
    #[inline]
    pub fn commands(&self) -> &[PathCommand] {
        &self.commands
    }

    /// Returns `true` if the path contains no commands.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Removes all commands, leaving the path empty.
    ///
    /// The fill type is preserved; the cached bounds are invalidated.
    #[inline]
    pub fn reset(&mut self) {
        self.commands.clear();
        self.bounds = None;
    }

    /// Returns the cached bounding box, if one has been computed via
    /// `bounds` and not invalidated by a later mutation.
    #[must_use]
    #[inline]
    pub fn cached_bounds(&self) -> Option<Rect<Pixels>> {
        self.bounds
    }

    /// Computes the bounding box of the path without caching the result.
    ///
    /// Uses the cached value when available. Curve bounds are conservative
    /// (control points are included). Returns `Rect::ZERO` for an empty path.
    #[must_use]
    #[inline]
    pub fn compute_bounds(&self) -> Rect<Pixels> {
        // Quick return if cached
        if let Some(bounds) = self.bounds {
            return bounds;
        }

        self.compute_bounds_internal()
    }

    /// Internal bounds computation (shared between bounds() and
    /// compute_bounds())
    #[inline]
    fn compute_bounds_internal(&self) -> Rect<Pixels> {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for cmd in &self.commands {
            match cmd {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => {
                    min_x = min_x.min(p.x.0);
                    min_y = min_y.min(p.y.0);
                    max_x = max_x.max(p.x.0);
                    max_y = max_y.max(p.y.0);
                }
                PathCommand::QuadraticTo(c, e) => {
                    min_x = min_x.min(c.x.0).min(e.x.0);
                    min_y = min_y.min(c.y.0).min(e.y.0);
                    max_x = max_x.max(c.x.0).max(e.x.0);
                    max_y = max_y.max(c.y.0).max(e.y.0);
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    min_x = min_x.min(c1.x.0).min(c2.x.0).min(e.x.0);
                    min_y = min_y.min(c1.y.0).min(c2.y.0).min(e.y.0);
                    max_x = max_x.max(c1.x.0).max(c2.x.0).max(e.x.0);
                    max_y = max_y.max(c1.y.0).max(c2.y.0).max(e.y.0);
                }
                PathCommand::AddRect(r) | PathCommand::AddOval(r) | PathCommand::AddArc(r, ..) => {
                    min_x = min_x.min(r.left().0);
                    min_y = min_y.min(r.top().0);
                    max_x = max_x.max(r.right().0);
                    max_y = max_y.max(r.bottom().0);
                }
                PathCommand::AddCircle(center, radius) => {
                    min_x = min_x.min(center.x.0 - radius);
                    min_y = min_y.min(center.y.0 - radius);
                    max_x = max_x.max(center.x.0 + radius);
                    max_y = max_y.max(center.y.0 + radius);
                }
                PathCommand::Close => {}
            }
        }

        if min_x.is_finite() && max_x.is_finite() {
            Rect::from_min_max(
                Point::new(Pixels(min_x), Pixels(min_y)),
                Point::new(Pixels(max_x), Pixels(max_y)),
            )
        } else {
            Rect::ZERO
        }
    }

    /// Returns the bounding box of the path, computing and caching it if
    /// necessary.
    ///
    /// Same semantics as `compute_bounds`, but stores the result so later
    /// calls are free until the path is mutated.
    #[must_use]
    #[inline]
    pub fn bounds(&mut self) -> Rect<Pixels> {
        if let Some(bounds) = self.bounds {
            return bounds;
        }

        let bounds = self.compute_bounds_internal();
        self.bounds = Some(bounds);
        bounds
    }

    /// Returns a copy of this path with every command translated by `offset`.
    #[must_use]
    #[inline]
    pub fn translate(&self, offset: Offset<Pixels>) -> Self {
        let delta = Vec2::new(offset.dx, offset.dy);
        let commands = self
            .commands
            .iter()
            .map(|cmd| match *cmd {
                PathCommand::MoveTo(p) => PathCommand::MoveTo(p + delta),
                PathCommand::LineTo(p) => PathCommand::LineTo(p + delta),
                PathCommand::QuadraticTo(c, e) => PathCommand::QuadraticTo(c + delta, e + delta),
                PathCommand::CubicTo(c1, c2, e) => {
                    PathCommand::CubicTo(c1 + delta, c2 + delta, e + delta)
                }
                PathCommand::AddRect(r) => PathCommand::AddRect(r.translate(delta)),
                PathCommand::AddCircle(center, radius) => {
                    PathCommand::AddCircle(center + delta, radius)
                }
                PathCommand::AddOval(r) => PathCommand::AddOval(r.translate(delta)),
                PathCommand::AddArc(r, start, sweep) => {
                    PathCommand::AddArc(r.translate(delta), start, sweep)
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

    /// Ray casting algorithm for even-odd fill rule.
    #[must_use]
    #[inline]
    fn contains_even_odd(&self, point: Point<Pixels>) -> bool {
        let mut crossings = 0;
        let mut current_pos = Point::new(px(0.0), px(0.0));
        let mut subpath_start = Point::new(px(0.0), px(0.0));

        for cmd in &self.commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    // Fill semantics: each subpath is implicitly closed for
                    // containment even without an explicit `Close` (Skia/Flutter
                    // fill an open contour as if closed). Count the closing edge
                    // of the subpath being left. Degenerate (contributes 0) when
                    // the subpath already ended with an explicit `Close`
                    // (`current_pos == subpath_start`), so there is no double-count.
                    if Self::ray_intersects_segment(point, current_pos, subpath_start) {
                        crossings += 1;
                    }
                    current_pos = *p;
                    subpath_start = *p;
                }
                PathCommand::LineTo(p) => {
                    if Self::ray_intersects_segment(point, current_pos, *p) {
                        crossings += 1;
                    }
                    current_pos = *p;
                }
                PathCommand::Close => {
                    if Self::ray_intersects_segment(point, current_pos, subpath_start) {
                        crossings += 1;
                    }
                    current_pos = subpath_start;
                }
                PathCommand::QuadraticTo(c, e) => {
                    // Approximate with line segments
                    crossings += Self::count_curve_crossings_quad(point, current_pos, *c, *e);
                    current_pos = *e;
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    // Approximate with line segments
                    crossings +=
                        Self::count_curve_crossings_cubic(point, current_pos, *c1, *c2, *e);
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
                    if dx.0 * dx.0 + dy.0 * dy.0 <= radius * radius {
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
                PathCommand::AddArc(..) => {
                    // TODO: Implement arc containment
                    // For now, skip arcs (conservative - may miss some points)
                }
            }
        }

        // Implicitly close the final subpath (fill semantics — see MoveTo arm).
        if Self::ray_intersects_segment(point, current_pos, subpath_start) {
            crossings += 1;
        }

        crossings % 2 == 1
    }

    /// Winding number algorithm for non-zero fill rule.
    #[inline]
    fn contains_non_zero(&self, point: Point<Pixels>) -> bool {
        let mut winding = 0;
        let mut current_pos = Point::new(px(0.0), px(0.0));
        let mut subpath_start = Point::new(px(0.0), px(0.0));

        for cmd in &self.commands {
            match cmd {
                PathCommand::MoveTo(p) => {
                    // Fill semantics: implicitly close the subpath being left
                    // (see the even-odd variant). Degenerate after an explicit
                    // `Close`, so no double-count.
                    winding += Self::segment_winding(point, current_pos, subpath_start);
                    current_pos = *p;
                    subpath_start = *p;
                }
                PathCommand::LineTo(p) => {
                    winding += Self::segment_winding(point, current_pos, *p);
                    current_pos = *p;
                }
                PathCommand::Close => {
                    winding += Self::segment_winding(point, current_pos, subpath_start);
                    current_pos = subpath_start;
                }
                PathCommand::QuadraticTo(c, e) => {
                    winding += Self::curve_winding_quad(point, current_pos, *c, *e);
                    current_pos = *e;
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    winding += Self::curve_winding_cubic(point, current_pos, *c1, *c2, *e);
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
                    if dx.0 * dx.0 + dy.0 * dy.0 <= radius * radius {
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
                PathCommand::AddArc(..) => {
                    // TODO: Implement arc winding
                }
            }
        }

        // Implicitly close the final subpath (fill semantics — see MoveTo arm).
        winding += Self::segment_winding(point, current_pos, subpath_start);

        winding != 0
    }

    /// Tests if a horizontal ray from point intersects a line segment.
    #[inline]
    fn ray_intersects_segment(point: Point<Pixels>, p1: Point<Pixels>, p2: Point<Pixels>) -> bool {
        // Ray extends to the right from point
        if (p1.y > point.y) == (p2.y > point.y) {
            return false; // Both endpoints on same side of ray
        }

        // Calculate x coordinate of intersection
        let x_intersect = p1.x + (point.y - p1.y) / (p2.y - p1.y) * (p2.x - p1.x);
        x_intersect > point.x
    }

    /// Compute winding contribution of a line segment.
    #[inline]
    fn segment_winding(point: Point<Pixels>, p1: Point<Pixels>, p2: Point<Pixels>) -> i32 {
        if p1.y <= point.y {
            if p2.y > point.y {
                // Upward crossing
                if Self::is_left(p1, p2, point) > 0.0 {
                    return 1;
                }
            }
        } else if p2.y <= point.y {
            // Downward crossing
            if Self::is_left(p1, p2, point) < 0.0 {
                return -1;
            }
        }
        0
    }

    /// Test if point is left of line segment (p1 -> p2).
    /// Returns > 0 for left, < 0 for right, 0 for on line.
    #[inline]
    fn is_left(p1: Point<Pixels>, p2: Point<Pixels>, point: Point<Pixels>) -> f32 {
        (p2.x - p1.x).get() * (point.y - p1.y).get() - (point.x - p1.x).get() * (p2.y - p1.y).get()
    }

    /// Count crossings for quadratic bezier curve (approximated).
    #[inline]
    fn count_curve_crossings_quad(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
    ) -> usize {
        // Simple approximation: subdivide into 4 line segments
        let t_values: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut crossings = 0;

        for i in 0..4 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = Self::eval_quadratic(p0, p1, p2, t1);
            let end = Self::eval_quadratic(p0, p1, p2, t2);

            if Self::ray_intersects_segment(point, start, end) {
                crossings += 1;
            }
        }

        crossings
    }

    /// Count crossings for cubic bezier curve (approximated).
    #[inline]
    fn count_curve_crossings_cubic(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        p3: Point<Pixels>,
    ) -> usize {
        // Simple approximation: subdivide into 8 line segments
        let t_values: [f32; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
        let mut crossings = 0;

        for i in 0..8 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = Self::eval_cubic(p0, p1, p2, p3, t1);
            let end = Self::eval_cubic(p0, p1, p2, p3, t2);

            if Self::ray_intersects_segment(point, start, end) {
                crossings += 1;
            }
        }

        crossings
    }

    /// Winding number for quadratic curve.
    #[inline]
    fn curve_winding_quad(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
    ) -> i32 {
        let t_values: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut winding = 0;

        for i in 0..4 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = Self::eval_quadratic(p0, p1, p2, t1);
            let end = Self::eval_quadratic(p0, p1, p2, t2);

            winding += Self::segment_winding(point, start, end);
        }

        winding
    }

    /// Winding number for cubic curve.
    #[inline]
    fn curve_winding_cubic(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        p3: Point<Pixels>,
    ) -> i32 {
        let t_values: [f32; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
        let mut winding = 0;

        for i in 0..8 {
            let t1 = t_values[i];
            let t2 = t_values[i + 1];

            let start = Self::eval_cubic(p0, p1, p2, p3, t1);
            let end = Self::eval_cubic(p0, p1, p2, p3, t2);

            winding += Self::segment_winding(point, start, end);
        }

        winding
    }

    /// Evaluate quadratic bezier at parameter t.
    #[inline]
    fn eval_quadratic<T>(p0: Point<T>, p1: Point<T>, p2: Point<T>, t: f32) -> Point<T>
    where
        T: NumericUnit + Into<f32> + FloatUnit,
    {
        let t2 = t * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;

        Point::new(
            T::from_f32(mt2 * p0.x.into() + 2.0 * mt * t * p1.x.into() + t2 * p2.x.into()),
            T::from_f32(mt2 * p0.y.into() + 2.0 * mt * t * p1.y.into() + t2 * p2.y.into()),
        )
    }

    /// Evaluate cubic bezier at parameter t.
    #[inline]
    fn eval_cubic<T>(p0: Point<T>, p1: Point<T>, p2: Point<T>, p3: Point<T>, t: f32) -> Point<T>
    where
        T: NumericUnit + Into<f32> + FloatUnit,
    {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        Point::new(
            T::from_f32(
                mt3 * p0.x.into()
                    + 3.0 * mt2 * t * p1.x.into()
                    + 3.0 * mt * t2 * p2.x.into()
                    + t3 * p3.x.into(),
            ),
            T::from_f32(
                mt3 * p0.y.into()
                    + 3.0 * mt2 * t * p1.y.into()
                    + 3.0 * mt * t2 * p2.y.into()
                    + t3 * p3.y.into(),
            ),
        )
    }
}

impl Default for Path {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Point, px};

    /// Builds the triangle (0,0)→(100,0)→(50,100) without an explicit `close()`.
    fn open_triangle(fill_type: PathFillType) -> Path {
        let mut path = Path::with_fill_type(fill_type);
        path.move_to(Point::new(px(0.0), px(0.0)));
        path.line_to(Point::new(px(100.0), px(0.0)));
        path.line_to(Point::new(px(50.0), px(100.0)));
        path
    }

    /// Regression (Codex review of PR #307): a filled path implicitly closes
    /// each subpath, so containment must honor the closing edge even without an
    /// explicit `Close`. Point (10,50) is left of the implicit closing edge —
    /// OUTSIDE the triangle — yet was wrongly reported inside before the fix
    /// (the missing left edge dropped one crossing). Holds for both fill rules.
    #[test]
    fn open_filled_contour_is_implicitly_closed_for_containment() {
        for fill_type in [PathFillType::EvenOdd, PathFillType::NonZero] {
            let path = open_triangle(fill_type);
            assert!(
                !path.contains(Point::new(px(10.0), px(50.0))),
                "{fill_type:?}: point outside an open-but-filled triangle must not be contained",
            );
            assert!(
                path.contains(Point::new(px(50.0), px(30.0))),
                "{fill_type:?}: point inside the triangle must be contained",
            );
        }
    }

    /// The implicit closure must not double-count when the contour already ends
    /// with an explicit `Close`: a closed triangle agrees with the open one.
    #[test]
    fn explicit_close_does_not_double_count() {
        for fill_type in [PathFillType::EvenOdd, PathFillType::NonZero] {
            let mut closed = open_triangle(fill_type);
            closed.close();
            let open = open_triangle(fill_type);
            for p in [
                Point::new(px(10.0), px(50.0)), // outside
                Point::new(px(50.0), px(30.0)), // inside
                Point::new(px(50.0), px(5.0)),  // inside, near base
            ] {
                assert_eq!(
                    closed.contains(p),
                    open.contains(p),
                    "{fill_type:?}: explicit close must match implicit close at {p:?}",
                );
            }
        }
    }

    /// `rrect_hint` must recover the exact `RRect` a `from_rrect` path was built
    /// from, so the analytical Gaussian shadow uses the true rect and radii.
    #[test]
    fn rrect_hint_round_trips_a_rounded_rectangle() {
        use crate::geometry::{RRect, Radius, Rect};

        let rrect = RRect::from_rect_and_corners(
            Rect::from_xywh(px(10.0), px(20.0), px(120.0), px(80.0)),
            Radius::new(px(4.0), px(6.0)),
            Radius::new(px(8.0), px(8.0)),
            Radius::new(px(12.0), px(10.0)),
            Radius::new(px(16.0), px(14.0)),
        );

        let recovered = Path::from_rrect(rrect)
            .rrect_hint()
            .expect("a fully-rounded rectangle path must be recognized");

        assert_eq!(recovered.rect, rrect.rect);
        assert_eq!(recovered.top_left, rrect.top_left);
        assert_eq!(recovered.top_right, rrect.top_right);
        assert_eq!(recovered.bottom_right, rrect.bottom_right);
        assert_eq!(recovered.bottom_left, rrect.bottom_left);
    }

    /// A plain rectangle carries no rounding, so it is not a shadow candidate
    /// for the analytical path and must return `None` (keeps the fallback).
    #[test]
    fn rrect_hint_is_none_for_a_plain_rectangle() {
        use crate::geometry::Rect;

        let path = Path::rectangle(Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)));
        assert!(path.rrect_hint().is_none());
    }
}
