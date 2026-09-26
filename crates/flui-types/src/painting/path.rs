//! Path types for vector drawing.
//!
//! Provides Path structure for creating complex shapes with lines, curves, and
//! arcs.

use std::sync::Arc;

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
///
/// The command list is copy-on-write (Skia's `SkPath` over `SkPathRef`): a
/// clone shares it, and the first mutation of a shared path copies it. A
/// path recorded into a display list or a clip is therefore a pointer-sized
/// handle plus its fill type and cached bounds, not a copy of every command.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Path {
    /// The commands, shared until mutated.
    commands: Arc<Vec<PathCommand>>,

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
            commands: Arc::default(),
            fill_type: PathFillType::default(),
            bounds: None,
        }
    }

    /// Creates an empty path with the given fill type.
    #[must_use]
    #[inline]
    pub fn with_fill_type(fill_type: PathFillType) -> Self {
        Self {
            commands: Arc::default(),
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

    /// Recovers the originating [`crate::geometry::RRect`] when this path is a
    /// fully-rounded rectangle emitted by [`Self::from_rrect`].
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
    /// Arcs are flattened into chords at the same ellipse parametrization
    /// the tessellator rasterizes them with, so a shape built from corner
    /// arcs (`from_rrect`, a circular `CircleBorder`) is contained as the
    /// curve it draws rather than as the polygon through its arc endpoints.
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
        Arc::make_mut(&mut self.commands).push(PathCommand::MoveTo(point));
        self.bounds = None;
    }

    /// Adds a straight line from the current position to `point`.
    #[inline]
    pub fn line_to(&mut self, point: Point<Pixels>) {
        Arc::make_mut(&mut self.commands).push(PathCommand::LineTo(point));
        self.bounds = None;
    }

    /// Adds a quadratic Bézier curve from the current position to `end`,
    /// pulled toward `control` (Flutter's `quadraticBezierTo`).
    ///
    /// Like [`Self::line_to`], with no contour open it starts from the
    /// current position: the origin on a fresh path, or the start of the
    /// contour just closed.
    #[inline]
    pub fn quadratic_bezier_to(&mut self, control: Point<Pixels>, end: Point<Pixels>) {
        Arc::make_mut(&mut self.commands).push(PathCommand::QuadraticTo(control, end));
        self.bounds = None;
    }

    /// Adds a cubic Bézier curve from the current position to `end`, with
    /// `control1` shaping its start and `control2` its end (Flutter's
    /// `cubicTo`). Starts where [`Self::quadratic_bezier_to`] does.
    #[inline]
    pub fn cubic_to(
        &mut self,
        control1: Point<Pixels>,
        control2: Point<Pixels>,
        end: Point<Pixels>,
    ) {
        Arc::make_mut(&mut self.commands).push(PathCommand::CubicTo(control1, control2, end));
        self.bounds = None;
    }

    /// Closes the current subpath with a line back to its starting point.
    #[inline]
    pub fn close(&mut self) {
        Arc::make_mut(&mut self.commands).push(PathCommand::Close);
    }

    /// Adds a rectangle as a separate subpath.
    #[inline]
    pub fn add_rect(&mut self, rect: Rect<Pixels>) {
        Arc::make_mut(&mut self.commands).push(PathCommand::AddRect(rect));
        self.bounds = None;
    }

    /// Adds an oval inscribed in `rect` as a separate subpath.
    #[inline]
    pub fn add_oval(&mut self, rect: Rect<Pixels>) {
        Arc::make_mut(&mut self.commands).push(PathCommand::AddOval(rect));
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
        Arc::make_mut(&mut self.commands).push(PathCommand::AddArc(rect, start_angle, sweep_angle));
        self.bounds = None;
    }

    /// Returns the path's commands in insertion order.
    #[must_use]
    #[inline]
    pub fn commands(&self) -> &[PathCommand] {
        &self.commands
    }

    /// Whether `self` and `other` share one command list — true for a clone
    /// until either side is mutated.
    #[must_use]
    pub fn shares_commands_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.commands, &other.commands)
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
        Arc::make_mut(&mut self.commands).clear();
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

        for cmd in self.commands.iter() {
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
                PathCommand::AddOval(r) => PathCommand::AddOval(r.translate(delta)),
                PathCommand::AddArc(r, start, sweep) => {
                    PathCommand::AddArc(r.translate(delta), start, sweep)
                }
                PathCommand::Close => PathCommand::Close,
            })
            .collect();

        Self {
            commands: Arc::new(commands),
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
        // Whether a contour is currently open. Only the arc arms read it:
        // `add_arc` continues an open contour but starts a fresh one when
        // nothing is open, and the two cases seed `subpath_start`
        // differently.
        let mut subpath_open = false;

        for cmd in self.commands.iter() {
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
                    subpath_open = true;
                }
                PathCommand::LineTo(p) => {
                    if Self::ray_intersects_segment(point, current_pos, *p) {
                        crossings += 1;
                    }
                    current_pos = *p;
                    subpath_open = true;
                }
                PathCommand::Close => {
                    if Self::ray_intersects_segment(point, current_pos, subpath_start) {
                        crossings += 1;
                    }
                    current_pos = subpath_start;
                    subpath_open = false;
                }
                PathCommand::QuadraticTo(c, e) => {
                    // Approximate with line segments
                    crossings += Self::count_curve_crossings_quad(point, current_pos, *c, *e);
                    current_pos = *e;
                    subpath_open = true;
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    // Approximate with line segments
                    crossings +=
                        Self::count_curve_crossings_cubic(point, current_pos, *c1, *c2, *e);
                    current_pos = *e;
                    subpath_open = true;
                }
                PathCommand::AddRect(rect) => {
                    // A standalone shape ends whatever contour was open, so a
                    // following arc starts fresh instead of chording back
                    // across the shape. The tessellator does exactly this
                    // (`builder.end(false)` then `has_begun = false`), and
                    // containment has to agree with it or the hittable region
                    // grows a wedge nothing paints.
                    Self::end_open_contour_even_odd(
                        point,
                        &mut crossings,
                        &mut current_pos,
                        &mut subpath_start,
                        &mut subpath_open,
                    );
                    // Simple rectangle test
                    if rect.contains(point) {
                        crossings += 1;
                    }
                }
                PathCommand::AddOval(rect) => {
                    // Standalone shape — see the `AddRect` arm.
                    Self::end_open_contour_even_odd(
                        point,
                        &mut crossings,
                        &mut current_pos,
                        &mut subpath_start,
                        &mut subpath_open,
                    );
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
                PathCommand::AddArc(rect, start, sweep) => {
                    let arc_start = Self::eval_arc(*rect, *start);
                    if subpath_open {
                        // An arc appended to an open contour continues it,
                        // chord-connected — the same shape the tessellator
                        // draws (see `add_arc`'s divergence note).
                        if Self::ray_intersects_segment(point, current_pos, arc_start) {
                            crossings += 1;
                        }
                    } else {
                        subpath_start = arc_start;
                        subpath_open = true;
                    }
                    crossings += Self::count_arc_crossings(point, *rect, *start, *sweep);
                    current_pos = Self::eval_arc(*rect, *start + *sweep);
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
        // Whether a contour is currently open. Only the arc arms read it:
        // `add_arc` continues an open contour but starts a fresh one when
        // nothing is open, and the two cases seed `subpath_start`
        // differently.
        let mut subpath_open = false;

        for cmd in self.commands.iter() {
            match cmd {
                PathCommand::MoveTo(p) => {
                    // Fill semantics: implicitly close the subpath being left
                    // (see the even-odd variant). Degenerate after an explicit
                    // `Close`, so no double-count.
                    winding += Self::segment_winding(point, current_pos, subpath_start);
                    current_pos = *p;
                    subpath_start = *p;
                    subpath_open = true;
                }
                PathCommand::LineTo(p) => {
                    winding += Self::segment_winding(point, current_pos, *p);
                    current_pos = *p;
                    subpath_open = true;
                }
                PathCommand::Close => {
                    winding += Self::segment_winding(point, current_pos, subpath_start);
                    current_pos = subpath_start;
                    subpath_open = false;
                }
                PathCommand::QuadraticTo(c, e) => {
                    winding += Self::curve_winding_quad(point, current_pos, *c, *e);
                    current_pos = *e;
                    subpath_open = true;
                }
                PathCommand::CubicTo(c1, c2, e) => {
                    winding += Self::curve_winding_cubic(point, current_pos, *c1, *c2, *e);
                    current_pos = *e;
                    subpath_open = true;
                }
                PathCommand::AddRect(rect) => {
                    // Standalone shape ends the open contour — see the
                    // even-odd walker's `AddRect` arm.
                    Self::end_open_contour_non_zero(
                        point,
                        &mut winding,
                        &mut current_pos,
                        &mut subpath_start,
                        &mut subpath_open,
                    );
                    if rect.contains(point) {
                        winding += 1;
                    }
                }
                PathCommand::AddOval(rect) => {
                    // Standalone shape ends the open contour — see the
                    // even-odd walker's `AddRect` arm.
                    Self::end_open_contour_non_zero(
                        point,
                        &mut winding,
                        &mut current_pos,
                        &mut subpath_start,
                        &mut subpath_open,
                    );
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
                PathCommand::AddArc(rect, start, sweep) => {
                    let arc_start = Self::eval_arc(*rect, *start);
                    if subpath_open {
                        // See the even-odd arm: an arc continues an open
                        // contour through a chord, it does not restart one.
                        winding += Self::segment_winding(point, current_pos, arc_start);
                    } else {
                        subpath_start = arc_start;
                        subpath_open = true;
                    }
                    winding += Self::arc_winding(point, *rect, *start, *sweep);
                    current_pos = Self::eval_arc(*rect, *start + *sweep);
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

    /// Ends an open contour before a standalone shape, for the even-odd
    /// walker.
    ///
    /// Fill semantics close an open contour implicitly, so the closing edge
    /// is counted here rather than being deferred to the walk's tail — the
    /// tail can no longer see it once the position resets. Resetting to the
    /// origin restores the state the walk starts in, which is what a command
    /// arriving with no current position already assumes.
    #[inline]
    fn end_open_contour_even_odd(
        point: Point<Pixels>,
        crossings: &mut usize,
        current_pos: &mut Point<Pixels>,
        subpath_start: &mut Point<Pixels>,
        subpath_open: &mut bool,
    ) {
        if !*subpath_open {
            return;
        }
        if Self::ray_intersects_segment(point, *current_pos, *subpath_start) {
            *crossings += 1;
        }
        *current_pos = Point::new(px(0.0), px(0.0));
        *subpath_start = *current_pos;
        *subpath_open = false;
    }

    /// Non-zero counterpart of [`Self::end_open_contour_even_odd`].
    #[inline]
    fn end_open_contour_non_zero(
        point: Point<Pixels>,
        winding: &mut i32,
        current_pos: &mut Point<Pixels>,
        subpath_start: &mut Point<Pixels>,
        subpath_open: &mut bool,
    ) {
        if !*subpath_open {
            return;
        }
        *winding += Self::segment_winding(point, *current_pos, *subpath_start);
        *current_pos = Point::new(px(0.0), px(0.0));
        *subpath_start = *current_pos;
        *subpath_open = false;
    }

    /// Largest gap, in local units, allowed between an arc and the chords
    /// containment flattens it into.
    ///
    /// Matched to the renderer's own fill-flattening bound
    /// (`DEVICE_FILL_TOLERANCE`, 0.1 device pixels in
    /// `flui-engine`'s `wgpu::tessellator`) so the hittable region tracks
    /// the painted one. The renderer pre-divides that bound by the paint
    /// transform's scale; a local-space containment test cannot see the
    /// transform, so this is the un-scaled figure — which means containment
    /// is at least as fine as the raster wherever the path is not scaled
    /// down.
    const ARC_FLATTENING_TOLERANCE: f32 = 0.1;

    /// Ceiling on chords per arc, so a pathological radius costs a bounded
    /// walk instead of an unbounded one. Generous enough that the tolerance
    /// above is met exactly for a full turn up to a radius near 85,000.
    const MAX_ARC_CHORDS: usize = 2048;

    /// Samples the ellipse inscribed in `rect` at `angle` radians.
    ///
    /// Deliberately the same parametrization the tessellator rasterizes an
    /// arc with (`lyon::geom::Arc` at zero x-rotation: `center + (rx·cosθ,
    /// ry·sinθ)`), so containment and the drawn pixels agree on where the
    /// curve is. Angles are y-down, matching the rest of the geometry
    /// vocabulary.
    #[inline]
    fn eval_arc(rect: Rect<Pixels>, angle: f32) -> Point<Pixels> {
        let cx = (rect.left() + rect.right()) * 0.5;
        let cy = (rect.top() + rect.bottom()) * 0.5;
        let rx = rect.width() * 0.5;
        let ry = rect.height() * 0.5;
        Point::new(cx + rx * angle.cos(), cy + ry * angle.sin())
    }

    /// How many chords to flatten an arc into, chosen from the arc's own
    /// radius so the error stays bounded in *distance* rather than in angle.
    ///
    /// A fixed angular step would under-approximate in proportion to the
    /// radius — at one chord per 11.25° the gap is `r · (1 - cos(5.625°))` ≈
    /// `r / 200`, which is a fifth of a pixel on a button and fifty pixels
    /// on a 10,000-unit circle. `Path` is a general primitive, so the step
    /// is derived from [`Self::ARC_FLATTENING_TOLERANCE`] instead: the sagitta of
    /// a chord spanning `theta` on a circle of radius `r` is
    /// `r · (1 - cos(theta / 2))`, so the widest chord within tolerance is
    /// `theta = 2 · acos(1 - tolerance / r)`. Ellipses use the larger
    /// semi-axis, which is conservative.
    ///
    /// Degenerate input (a zero or non-finite radius or sweep) costs one
    /// chord; a radius large enough that `1 - tolerance / r` rounds to `1`
    /// in `f32` saturates at [`Self::MAX_ARC_CHORDS`] rather than dividing by an
    /// angle of zero.
    #[inline]
    fn arc_chord_count(rect: Rect<Pixels>, sweep: f32) -> usize {
        let radius = (rect.width() * 0.5)
            .get()
            .abs()
            .max((rect.height() * 0.5).get().abs());
        let sweep = sweep.abs();
        if !radius.is_finite() || radius <= 0.0 || !sweep.is_finite() || sweep <= 0.0 {
            return 1;
        }

        let cos_half_chord = (1.0 - Self::ARC_FLATTENING_TOLERANCE / radius).clamp(-1.0, 1.0);
        let chord_angle = 2.0 * cos_half_chord.acos();
        if !chord_angle.is_finite() || chord_angle <= f32::EPSILON {
            return Self::MAX_ARC_CHORDS;
        }

        let wanted = (sweep / chord_angle).ceil();
        if wanted.is_finite() {
            let wanted = wanted as usize;
            wanted.clamp(1, Self::MAX_ARC_CHORDS)
        } else {
            Self::MAX_ARC_CHORDS
        }
    }

    /// Count ray crossings for an arc, flattened into chords. The chord from
    /// the contour's current position to the arc's start is the caller's to
    /// count — whether one exists depends on if a contour is open.
    #[inline]
    fn count_arc_crossings(
        point: Point<Pixels>,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
    ) -> usize {
        let chords = Self::arc_chord_count(rect, sweep_angle);
        let mut crossings = 0;
        let mut from = Self::eval_arc(rect, start_angle);

        for i in 1..=chords {
            let t = i as f32 / chords as f32;
            let to = Self::eval_arc(rect, start_angle + sweep_angle * t);
            if Self::ray_intersects_segment(point, from, to) {
                crossings += 1;
            }
            from = to;
        }

        crossings
    }

    /// Winding-number contribution of an arc, flattened into chords — the
    /// non-zero counterpart of [`Self::count_arc_crossings`].
    #[inline]
    fn arc_winding(
        point: Point<Pixels>,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
    ) -> i32 {
        let chords = Self::arc_chord_count(rect, sweep_angle);
        let mut winding = 0;
        let mut from = Self::eval_arc(rect, start_angle);

        for i in 1..=chords {
            let t = i as f32 / chords as f32;
            let to = Self::eval_arc(rect, start_angle + sweep_angle * t);
            winding += Self::segment_winding(point, from, to);
            from = to;
        }

        winding
    }

    /// How many chords to flatten a Bézier curve into so no point of it lies
    /// farther than [`Self::ARC_FLATTENING_TOLERANCE`] from them.
    ///
    /// Wang's formula: a degree-`d` curve whose largest second difference of
    /// control points has length `M` needs `ceil(sqrt(d(d - 1) M / (8 tol)))`
    /// uniform steps in `t`. It is the bound lyon and Skia flatten by, so the
    /// hittable curve tracks the painted one; a fixed step count would drift
    /// from it in proportion to the curve's size. A straight curve (`M = 0`)
    /// and non-finite input cost one chord; the count saturates at
    /// [`Self::MAX_ARC_CHORDS`].
    fn bezier_chord_count(degree: f32, points: &[Point<Pixels>]) -> usize {
        let largest_second_difference = points
            .windows(3)
            .map(|w| {
                let dx = w[0].x.0 - 2.0 * w[1].x.0 + w[2].x.0;
                let dy = w[0].y.0 - 2.0 * w[1].y.0 + w[2].y.0;
                dx.hypot(dy)
            })
            .fold(0.0_f32, f32::max);
        if !largest_second_difference.is_finite() {
            return 1;
        }
        let wanted = (degree * (degree - 1.0) * largest_second_difference
            / (8.0 * Self::ARC_FLATTENING_TOLERANCE))
            .sqrt()
            .ceil();
        if wanted.is_finite() {
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // non-negative, clamped
            let wanted = wanted as usize;
            wanted.clamp(1, Self::MAX_ARC_CHORDS)
        } else {
            Self::MAX_ARC_CHORDS
        }
    }

    /// The chords of a curve sampled at `chords` uniform steps of `t`.
    fn curve_chords(
        chords: usize,
        eval: impl Fn(f32) -> Point<Pixels>,
    ) -> impl Iterator<Item = (Point<Pixels>, Point<Pixels>)> {
        #[expect(clippy::cast_precision_loss)] // at most MAX_ARC_CHORDS
        let step = move |i: usize| i as f32 / chords as f32;
        (1..=chords).map(move |i| (eval(step(i - 1)), eval(step(i))))
    }

    /// Ray crossings of a quadratic curve, flattened within tolerance.
    #[inline]
    fn count_curve_crossings_quad(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
    ) -> usize {
        let chords = Self::bezier_chord_count(2.0, &[p0, p1, p2]);
        Self::curve_chords(chords, |t| Self::eval_quadratic(p0, p1, p2, t))
            .filter(|&(a, b)| Self::ray_intersects_segment(point, a, b))
            .count()
    }

    /// Ray crossings of a cubic curve, flattened within tolerance.
    #[inline]
    fn count_curve_crossings_cubic(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        p3: Point<Pixels>,
    ) -> usize {
        let chords = Self::bezier_chord_count(3.0, &[p0, p1, p2, p3]);
        Self::curve_chords(chords, |t| Self::eval_cubic(p0, p1, p2, p3, t))
            .filter(|&(a, b)| Self::ray_intersects_segment(point, a, b))
            .count()
    }

    /// Winding contribution of a quadratic curve, flattened within tolerance.
    #[inline]
    fn curve_winding_quad(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
    ) -> i32 {
        let chords = Self::bezier_chord_count(2.0, &[p0, p1, p2]);
        Self::curve_chords(chords, |t| Self::eval_quadratic(p0, p1, p2, t))
            .map(|(a, b)| Self::segment_winding(point, a, b))
            .sum()
    }

    /// Winding contribution of a cubic curve, flattened within tolerance.
    #[inline]
    fn curve_winding_cubic(
        point: Point<Pixels>,
        p0: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        p3: Point<Pixels>,
    ) -> i32 {
        let chords = Self::bezier_chord_count(3.0, &[p0, p1, p2, p3]);
        Self::curve_chords(chords, |t| Self::eval_cubic(p0, p1, p2, p3, t))
            .map(|(a, b)| Self::segment_winding(point, a, b))
            .sum()
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

    /// A clone is a refcount bump: both paths read one command buffer until
    /// either side mutates, at which point only the mutating side copies.
    /// `Canvas::draw_path`/`clip_path` rely on this to record a caller's path
    /// without copying its commands.
    #[test]
    fn path_clone_shares_its_commands_until_mutated() {
        let mut original = open_triangle(PathFillType::NonZero);
        let recorded = original.clone();
        assert!(recorded.shares_commands_with(&original));
        assert_eq!(recorded.commands(), original.commands());

        original.close();
        assert!(
            !recorded.shares_commands_with(&original),
            "a mutation must detach the mutating side"
        );
        assert_eq!(
            recorded.commands().len(),
            3,
            "the clone keeps the buffer it shared"
        );
        assert_eq!(original.commands().len(), 4);

        let untouched = recorded.clone();
        assert!(untouched.shares_commands_with(&recorded));
    }

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

    /// The command shape a circular Material surface (`CircleBorder`, and
    /// `from_rrect` for a fully-rounded box) actually produces: four
    /// quadrant arcs over the same bounding rect, each preceded by a
    /// degenerate `LineTo` to the arc's start.
    fn quadrant_arc_circle(diameter: f32) -> Path {
        use core::f32::consts::FRAC_PI_2;
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(diameter), px(diameter));
        let mid = px(diameter / 2.0);
        let end = px(diameter);

        let mut path = Path::new();
        path.move_to(Point::new(mid, px(0.0)));
        path.line_to(Point::new(mid, px(0.0)));
        path.add_arc(rect, -FRAC_PI_2, FRAC_PI_2);
        path.line_to(Point::new(end, mid));
        path.add_arc(rect, 0.0, FRAC_PI_2);
        path.line_to(Point::new(mid, end));
        path.add_arc(rect, FRAC_PI_2, FRAC_PI_2);
        path.line_to(Point::new(px(0.0), mid));
        path.add_arc(rect, core::f32::consts::PI, FRAC_PI_2);
        path.close();
        path
    }

    /// A circle assembled from quadrant arcs must contain the whole disc,
    /// not just the diamond through the arc endpoints.
    ///
    /// `(9, 9)` on a 40px circle is the case that matters in practice: it is
    /// 15.6px from the centre so it is comfortably inside the 20px radius,
    /// but `|9-20| + |9-20| = 22 > 20` puts it outside the inscribed
    /// diamond. Skipping the arcs left exactly that diamond — roughly 64% of
    /// the disc — so the corner of every circular `IconButton` stopped
    /// hit-testing.
    #[test]
    fn a_circle_built_from_quadrant_arcs_contains_its_whole_disc() {
        for fill_type in [PathFillType::NonZero, PathFillType::EvenOdd] {
            let mut path = quadrant_arc_circle(40.0);
            path.set_fill_type(fill_type);

            assert!(
                path.contains(Point::new(px(9.0), px(9.0))),
                "{fill_type:?}: (9,9) is 15.6px from the centre of a 20px \
                 radius, inside the disc but outside the endpoint diamond"
            );
            assert!(
                path.contains(Point::new(px(20.0), px(20.0))),
                "{fill_type:?}: the centre is inside"
            );
            assert!(
                path.contains(Point::new(px(38.0), px(20.0))),
                "{fill_type:?}: a point just inside the right extreme"
            );
        }
    }

    /// The flattening must not over-claim either: a corner of the bounding
    /// box is outside the disc and stays outside.
    #[test]
    fn a_circle_built_from_quadrant_arcs_rejects_its_bounding_box_corners() {
        for fill_type in [PathFillType::NonZero, PathFillType::EvenOdd] {
            let mut path = quadrant_arc_circle(40.0);
            path.set_fill_type(fill_type);

            for corner in [
                Point::new(px(1.0), px(1.0)),
                Point::new(px(39.0), px(1.0)),
                Point::new(px(1.0), px(39.0)),
                Point::new(px(39.0), px(39.0)),
            ] {
                assert!(
                    !path.contains(corner),
                    "{fill_type:?}: {corner:?} is 26.9px from the centre, \
                     outside the 20px radius"
                );
            }
        }
    }

    /// An arc that opens its own contour seeds that contour at the arc's
    /// start rather than chording back to the origin.
    #[test]
    fn a_leading_arc_starts_its_own_contour() {
        use core::f32::consts::TAU;
        let rect = Rect::from_xywh(px(100.0), px(100.0), px(40.0), px(40.0));

        let mut path = Path::new();
        path.add_arc(rect, 0.0, TAU);
        path.close();

        assert!(
            path.contains(Point::new(px(120.0), px(120.0))),
            "the arc's own centre is inside"
        );
        assert!(
            !path.contains(Point::new(px(50.0), px(50.0))),
            "a point back toward the origin is outside — a leading arc must \
             not chord to (0, 0)"
        );
    }

    /// A standalone shape ends any open contour, so a following arc starts
    /// its own instead of chording back across the shape.
    ///
    /// The renderer already behaves this way — every `AddRect`/
    /// `AddOval` arm in `flui-engine`'s tessellator ends the builder's
    /// contour and clears `has_begun`. If containment kept the contour open
    /// it would chord from the stale pre-shape position to the arc's start,
    /// and the wedge that chord encloses would hit-test as filled while
    /// nothing paints it.
    #[test]
    fn a_standalone_shape_ends_the_open_contour_before_a_following_arc() {
        use core::f32::consts::TAU;
        for fill_type in [PathFillType::NonZero, PathFillType::EvenOdd] {
            let mut path = Path::new();
            path.set_fill_type(fill_type);
            path.move_to(Point::new(px(0.0), px(0.0)));
            path.line_to(Point::new(px(100.0), px(0.0)));
            path.add_rect(Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)));
            path.add_arc(
                Rect::from_xywh(px(200.0), px(200.0), px(40.0), px(40.0)),
                0.0,
                TAU,
            );
            path.close();

            assert!(
                path.contains(Point::new(px(5.0), px(5.0))),
                "{fill_type:?}: the standalone rect is still filled"
            );
            assert!(
                path.contains(Point::new(px(220.0), px(220.0))),
                "{fill_type:?}: the arc's own circle is still filled"
            );
            assert!(
                !path.contains(Point::new(px(150.0), px(100.0))),
                "{fill_type:?}: the gap between the rect and the circle paints \
                 nothing, so it must not hit-test as filled"
            );
        }
    }

    /// Flattening error is bounded in distance, not in angle: a large arc
    /// gets proportionally more chords so containment keeps tracking the
    /// curve the renderer draws.
    ///
    /// A fixed angular step fails here. At one chord per 11.25° the chord
    /// sags `r * (1 - cos(5.625))` below the arc — a fifth of a pixel at
    /// `r = 40`, but roughly 48 units at `r = 10_000`, which would reject a
    /// wide band of pixels that are painted.
    #[test]
    fn a_large_arc_is_flattened_to_the_same_distance_tolerance_as_a_small_one() {
        use core::f32::consts::TAU;
        const RADIUS: f32 = 10_000.0;

        let mut path = Path::new();
        path.add_arc(
            Rect::from_xywh(px(0.0), px(0.0), px(2.0 * RADIUS), px(2.0 * RADIUS)),
            0.0,
            TAU,
        );
        path.close();

        // Probe midway between two chords, where a flattened polygon cuts
        // deepest — at a vertex it touches the circle exactly and proves
        // nothing. 5.625 degrees is the midpoint of the first 11.25-degree
        // chord, so under a fixed angular step the polygon sits
        // `RADIUS * (1 - cos(5.625 deg))`, about 48 units, inside the rim
        // there. A probe 5 units in is painted, comfortably inside the
        // 0.1-unit tolerance, and comfortably outside that 48.
        const PROBE_ANGLE: f32 = core::f32::consts::PI / 32.0;
        let inset = RADIUS - 5.0;
        let probe = Point::new(
            px(RADIUS + inset * PROBE_ANGLE.cos()),
            px(RADIUS + inset * PROBE_ANGLE.sin()),
        );

        assert!(
            path.contains(probe),
            "a point 5 units inside a {RADIUS}-unit circle is painted, so it \
             must hit-test as filled"
        );
        assert!(
            !path.contains(Point::new(
                px(RADIUS + (RADIUS + 5.0) * PROBE_ANGLE.cos()),
                px(RADIUS + (RADIUS + 5.0) * PROBE_ANGLE.sin()),
            )),
            "a point 5 units outside the same rim stays outside"
        );
    }

    mod geometry_oracles {
        use super::super::*;
        use crate::geometry::{Offset, Point, RRect, Radius, px};
        use proptest::prelude::*;

        fn p(x: f32, y: f32) -> Point<Pixels> {
            Point::new(px(x), px(y))
        }

        fn rect(l: f32, t: f32, r: f32, b: f32) -> Rect<Pixels> {
            Rect::from_ltrb(px(l), px(t), px(r), px(b))
        }

        fn with(fill: PathFillType, mut path: Path) -> Path {
            path.set_fill_type(fill);
            path
        }

        const FILLS: [PathFillType; 2] = [PathFillType::NonZero, PathFillType::EvenOdd];

        /// Twice the signed area of `abc`.
        fn cross(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
            (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
        }

        proptest! {
            /// A triangle contains exactly the points on the inner side of
            /// all three edges, under either fill rule and either winding.
            /// Points within a small band of an edge are skipped: which
            /// side of the boundary they land on is not specified.
            #[test]
            fn triangle_containment_matches_barycentric(
                v in proptest::array::uniform3((-50.0f32..50.0, -50.0f32..50.0)),
                q in (-60.0f32..60.0, -60.0f32..60.0),
            ) {
                let area = cross(v[0], v[1], v[2]);
                prop_assume!(area.abs() > 50.0);
                let d = [cross(v[0], v[1], q), cross(v[1], v[2], q), cross(v[2], v[0], q)];
                let edge_len = |a: (f32, f32), b: (f32, f32)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                let lens = [edge_len(v[0], v[1]), edge_len(v[1], v[2]), edge_len(v[2], v[0])];
                prop_assume!(d.iter().zip(lens).all(|(d, l)| d.abs() / l > 0.01));
                let inside = d.iter().all(|x| x.signum() == area.signum());
                let points = v.map(|(x, y)| p(x, y));
                for fill in FILLS {
                    let path = with(fill, Path::polygon(&points));
                    prop_assert_eq!(path.contains(p(q.0, q.1)), inside);
                }
            }
        }

        /// Two squares overlapping in [10, 20]². Wound the same way the
        /// overlap has winding 2: filled by non-zero, a hole for even-odd.
        /// Wound oppositely it has winding 0: a hole for both.
        #[test]
        fn fill_rules_differ_on_the_overlap() {
            let square = |path: &mut Path, l: f32, t: f32, s: f32, clockwise: bool| {
                let mut corners = [p(l, t), p(l + s, t), p(l + s, t + s), p(l, t + s)];
                if !clockwise {
                    corners.reverse();
                }
                path.move_to(corners[0]);
                for c in &corners[1..] {
                    path.line_to(*c);
                }
                path.close();
            };
            for (same_way, overlap_nonzero) in [(true, true), (false, false)] {
                for fill in FILLS {
                    let mut path = Path::with_fill_type(fill);
                    square(&mut path, 0.0, 0.0, 20.0, true);
                    square(&mut path, 10.0, 10.0, 20.0, same_way);
                    let expected_overlap = fill == PathFillType::NonZero && overlap_nonzero;
                    assert_eq!(
                        path.contains(p(15.0, 15.0)),
                        expected_overlap,
                        "{fill:?} same_way={same_way}"
                    );
                    assert!(
                        path.contains(p(5.0, 5.0)) && path.contains(p(25.0, 25.0)),
                        "{fill:?}"
                    );
                    assert!(
                        !path.contains(p(25.0, 5.0)) && !path.contains(p(-1.0, 5.0)),
                        "{fill:?}"
                    );
                }
            }
        }

        /// Standalone shapes add one to the winding inside them.
        #[test]
        fn standalone_shapes_under_both_fill_rules() {
            for fill in FILLS {
                let mut path = Path::with_fill_type(fill);
                path.add_rect(rect(0.0, 0.0, 40.0, 20.0));
                path.add_oval(rect(30.0, 0.0, 70.0, 20.0));
                assert!(path.contains(p(5.0, 10.0)), "{fill:?}"); // rect only
                assert!(path.contains(p(60.0, 10.0)), "{fill:?}"); // oval only
                assert_eq!(path.contains(p(35.0, 10.0)), fill == PathFillType::NonZero); // both
                assert!(!path.contains(p(69.0, 1.0)), "{fill:?}"); // oval's box corner
                assert!(!path.contains(p(80.0, 10.0)), "{fill:?}");
            }
            // An oval is its ellipse: in at the rim's inner side, out beyond it.
            let oval = Path::oval(rect(0.0, 0.0, 40.0, 20.0));
            assert!(oval.contains(p(38.0, 10.0)) && !oval.contains(p(20.0, 21.0)));
            let circle = Path::circle(p(10.0, 10.0), 5.0);
            assert!(circle.contains(p(14.0, 10.0)) && !circle.contains(p(14.0, 14.0)));
            assert!(Path::rectangle(rect(0.0, 0.0, 4.0, 4.0)).contains(p(3.0, 3.0)));
            assert!(!Path::new().contains(p(0.0, 0.0)));
        }

        /// Distinct radii per corner (tl 20, tr 5, br 10, bl 0) on a 100×60
        /// rect: each corner cuts away exactly its own quarter circle.
        #[test]
        fn from_rrect_rounds_each_corner_by_its_own_radius() {
            let r = |v: f32| Radius::circular(px(v));
            let rrect = RRect::from_rect_and_corners(
                rect(0.0, 0.0, 100.0, 60.0),
                r(20.0),
                r(5.0),
                r(10.0),
                r(0.0),
            );
            let path = Path::from_rrect(rrect);
            for (point, inside) in [
                ((50.0, 30.0), true),
                ((4.0, 4.0), false),   // tl: 22.6 from (20, 20)
                ((10.0, 10.0), true),  // tl: 14.1 from (20, 20)
                ((99.0, 1.0), false),  // tr: 5.7 from (95, 5)
                ((96.0, 4.0), true),   // tr: 1.4 from (95, 5)
                ((99.0, 59.0), false), // br: 12.7 from (90, 50)
                ((95.0, 55.0), true),  // br: 7.1 from (90, 50)
                ((1.0, 59.0), true),   // bl: square
                ((1.0, 30.0), true),   // on the left edge's inner side
                ((50.0, 1.0), true),   // on the top edge's inner side
            ] {
                assert_eq!(path.contains(p(point.0, point.1)), inside, "{point:?}");
            }
            assert_eq!(path.compute_bounds(), rect(0.0, 0.0, 100.0, 60.0));
            // No rounding at all is a plain rectangle.
            let square = Path::from_rrect(RRect::from_rect(rect(0.0, 0.0, 10.0, 10.0)));
            assert_eq!(
                square.commands(),
                Path::rectangle(rect(0.0, 0.0, 10.0, 10.0)).commands()
            );
        }

        #[test]
        fn bounds_translate_and_reset() {
            let mut path = Path::polygon(&[p(-5.0, 2.0), p(10.0, -3.0), p(4.0, 8.0)]);
            let bounds = rect(-5.0, -3.0, 10.0, 8.0);
            assert_eq!(path.cached_bounds(), None);
            assert_eq!(path.compute_bounds(), bounds);
            assert_eq!(path.bounds(), bounds);
            assert_eq!(path.cached_bounds(), Some(bounds));

            let moved = path.translate(Offset::new(px(10.0), px(20.0)));
            assert_eq!(moved.compute_bounds(), rect(5.0, 17.0, 20.0, 28.0));
            assert_eq!(moved.cached_bounds(), None);
            assert!(moved.contains(p(13.0, 21.0)) && !path.contains(p(13.0, 21.0)));
            let shapes = {
                let mut s = Path::new();
                s.add_rect(rect(0.0, 0.0, 2.0, 2.0));
                s.add_oval(rect(4.0, 0.0, 6.0, 2.0));
                s.add_arc(rect(8.0, 0.0, 10.0, 2.0), 0.0, 1.0);
                s.translate(Offset::new(px(1.0), px(1.0)))
            };
            assert_eq!(shapes.compute_bounds(), rect(1.0, 1.0, 11.0, 3.0));

            assert!(!path.is_empty());
            path.reset();
            assert!(path.is_empty());
            assert_eq!(path.cached_bounds(), None);
            assert_eq!(path.compute_bounds(), Rect::ZERO);
        }

        #[test]
        fn fill_type_and_constructors() {
            assert_eq!(
                Path::with_fill_type(PathFillType::EvenOdd).fill_type(),
                PathFillType::EvenOdd
            );
            let mut path = Path::new();
            assert_eq!(path.fill_type(), PathFillType::NonZero);
            path.set_fill_type(PathFillType::EvenOdd);
            assert_eq!(path.fill_type(), PathFillType::EvenOdd);
            assert_eq!(
                Path::arc(rect(0.0, 0.0, 4.0, 4.0), 0.0, 1.0).compute_bounds(),
                rect(0.0, 0.0, 4.0, 4.0)
            );
            assert_eq!(
                Path::circle(p(5.0, 5.0), 2.0).compute_bounds(),
                rect(3.0, 3.0, 7.0, 7.0)
            );
            assert!(Path::polygon(&[]).is_empty());
        }

        /// Every mutation drops the cached bounds, so the next query sees
        /// the new command.
        #[test]
        fn mutations_invalidate_the_cached_bounds() {
            /// A named edit and the bounds it leaves.
            type Edit = (&'static str, fn(&mut Path), Rect<Pixels>);
            let edits: [Edit; 8] = [
                // Curve bounds include the control points (conservative).
                (
                    "quadratic_bezier_to",
                    |path| path.quadratic_bezier_to(p(-4.0, 30.0), p(12.0, 1.0)),
                    rect(-4.0, 0.0, 12.0, 30.0),
                ),
                (
                    "cubic_to",
                    |path| path.cubic_to(p(1.0, -6.0), p(15.0, 2.0), p(3.0, 3.0)),
                    rect(0.0, -6.0, 15.0, 10.0),
                ),
                (
                    "move_to",
                    |path| path.move_to(p(20.0, 20.0)),
                    rect(0.0, 0.0, 20.0, 20.0),
                ),
                (
                    "line_to",
                    |path| path.line_to(p(-5.0, 3.0)),
                    rect(-5.0, 0.0, 10.0, 10.0),
                ),
                (
                    "add_rect",
                    |path| path.add_rect(rect(0.0, 0.0, 30.0, 5.0)),
                    rect(0.0, 0.0, 30.0, 10.0),
                ),
                (
                    "add_oval",
                    |path| path.add_oval(rect(-1.0, -1.0, 2.0, 2.0)),
                    rect(-1.0, -1.0, 10.0, 10.0),
                ),
                (
                    "add_arc",
                    |path| path.add_arc(rect(0.0, 0.0, 10.0, 40.0), 0.0, 1.0),
                    rect(0.0, 0.0, 10.0, 40.0),
                ),
                ("reset", Path::reset, Rect::ZERO),
            ];
            for (name, edit, expected) in edits {
                let mut path = Path::rectangle(rect(0.0, 0.0, 10.0, 10.0));
                assert_eq!(path.bounds(), rect(0.0, 0.0, 10.0, 10.0));
                edit(&mut path);
                assert_eq!(path.cached_bounds(), None, "{name}");
                assert_eq!(path.bounds(), expected, "{name}");
            }
            assert_eq!(Path::new().cached_bounds(), None);
            assert_eq!(
                Path::with_fill_type(PathFillType::EvenOdd).cached_bounds(),
                None
            );
            assert_eq!(
                Path::circle(p(5.0, 5.0), 3.0).compute_bounds(),
                rect(2.0, 2.0, 8.0, 8.0)
            );
            let triangle = Path::polygon(&[p(0.0, 0.0), p(4.0, 0.0), p(0.0, 4.0)]);
            assert_eq!(triangle.commands().last(), Some(&PathCommand::Close));
        }

        /// The quarter arc used below: on the circle of radius 50 about
        /// (150, 50), from (200, 50) clockwise (y-down) to (150, 100).
        fn quarter(path: &mut Path) {
            path.add_arc(
                rect(100.0, 0.0, 200.0, 100.0),
                0.0,
                std::f32::consts::FRAC_PI_2,
            );
        }

        fn check(
            name: &str,
            build: impl Fn(&mut Path),
            inside: &[(f32, f32)],
            outside: &[(f32, f32)],
        ) {
            for fill in FILLS {
                let mut path = Path::with_fill_type(fill);
                build(&mut path);
                for &(x, y) in inside {
                    assert!(
                        path.contains(p(x, y)),
                        "{name} {fill:?}: ({x}, {y}) should be inside"
                    );
                }
                for &(x, y) in outside {
                    assert!(
                        !path.contains(p(x, y)),
                        "{name} {fill:?}: ({x}, {y}) should be outside"
                    );
                }
            }
        }

        /// Whether an arc joins the open contour by a chord or starts its
        /// own decides the filled shape: a wedge from the joining point, or
        /// only the segment between the arc and its own chord.
        #[test]
        fn an_arc_joins_an_open_contour_and_starts_a_closed_one_fresh() {
            // Alone, the arc fills the segment cut off by its chord
            // x + y = 250; (150, 70) lies in the wedge an origin-anchored
            // contour would add.
            check(
                "leading arc",
                quarter,
                &[(185.0, 85.0)],
                &[(150.0, 70.0), (160.0, 60.0)],
            );

            // From the centre it is a pie slice: the quadrant of the disc.
            let pie = |path: &mut Path| {
                path.move_to(p(150.0, 50.0));
                quarter(path);
                path.close();
            };
            check(
                "pie",
                pie,
                &[(160.0, 60.0), (185.0, 85.0)],
                &[(140.0, 60.0), (190.0, 95.0)],
            );
            let open_pie = |path: &mut Path| {
                path.move_to(p(150.0, 50.0));
                quarter(path);
            };
            check("open pie", open_pie, &[(160.0, 60.0)], &[(140.0, 60.0)]);

            // After a closed contour the arc starts fresh.
            let after_close = |path: &mut Path| {
                path.move_to(p(0.0, 0.0));
                path.line_to(p(10.0, 0.0));
                path.line_to(p(0.0, 10.0));
                path.close();
                quarter(path);
            };
            check(
                "after close",
                after_close,
                &[(2.0, 2.0), (185.0, 85.0)],
                &[(150.0, 70.0)],
            );

            // A standalone shape closes the open triangle before it and the
            // arc after it starts fresh. The triangle's closing edge is its
            // right side, the only edge (8, 203)'s ray crosses.
            let open_triangle = |path: &mut Path| {
                path.move_to(p(10.0, 200.0));
                path.line_to(p(0.0, 200.0));
                path.line_to(p(10.0, 210.0));
            };
            let shapes: [fn(&mut Path); 2] = [
                |path| path.add_rect(rect(300.0, 300.0, 310.0, 310.0)),
                |path| path.add_oval(rect(300.0, 300.0, 310.0, 310.0)),
            ];
            for shape in shapes {
                let after_shape = |path: &mut Path| {
                    open_triangle(path);
                    shape(path);
                    quarter(path);
                };
                check(
                    "after shape",
                    after_shape,
                    &[(8.0, 203.0), (305.0, 305.0), (185.0, 85.0)],
                    // (42, 180) lies in the wedge a chord from the
                    // triangle to the arc would enclose.
                    &[(2.0, 208.0), (150.0, 70.0), (42.0, 180.0)],
                );
            }

            // So does a move_to: the open triangle is closed before the next
            // contour begins.
            let two_open = |path: &mut Path| {
                open_triangle(path);
                path.move_to(p(0.0, 300.0));
                path.line_to(p(10.0, 300.0));
            };
            check(
                "two open contours",
                two_open,
                &[(8.0, 203.0)],
                &[(2.0, 208.0)],
            );
            // The implicit close at the end can be the only crossing.
            let only_the_close = |path: &mut Path| {
                path.move_to(p(100.0, 0.0));
                path.line_to(p(0.0, 50.0));
                path.line_to(p(100.0, 100.0));
            };
            check(
                "closing edge",
                only_the_close,
                &[(80.0, 60.0)],
                &[(10.0, 60.0)],
            );

            // The chord into the arc counts when it is not level: from
            // (100, 100) up to the arc start (200, 50).
            let chord = |path: &mut Path| {
                path.move_to(p(100.0, 100.0));
                quarter(path);
            };
            check("slanted chord", chord, &[(140.0, 85.0)], &[(105.0, 90.0)]);

            // An arc continues the contour a previous arc left open: the
            // chord from (150, 100) to (100, 50) cuts off the lower left.
            let two_arcs = |path: &mut Path| {
                quarter(path);
                path.add_arc(
                    rect(100.0, 0.0, 200.0, 100.0),
                    std::f32::consts::PI,
                    std::f32::consts::FRAC_PI_2,
                );
            };
            check("two arcs", two_arcs, &[(145.0, 55.0)], &[(110.0, 80.0)]);

            // Just below the arc start the ray meets only the arc's first
            // chord.
            check("first chord", pie, &[(190.0, 53.0)], &[]);
        }

        /// Standalone shapes add one crossing, or wind once, where they
        /// contain the point: three overlapping rects are odd, two even,
        /// and a rect or oval cancels against a square wound the other way.
        #[test]
        fn shapes_count_once_per_containment() {
            let mut three = Path::new();
            three.add_rect(rect(0.0, 0.0, 30.0, 30.0));
            three.add_rect(rect(10.0, 10.0, 40.0, 40.0));
            three.add_rect(rect(20.0, 20.0, 50.0, 50.0));
            for (fill, in_three, in_two) in [
                (PathFillType::NonZero, true, true),
                (PathFillType::EvenOdd, true, false),
            ] {
                let path = with(fill, three.clone());
                assert_eq!(path.contains(p(25.0, 25.0)), in_three, "{fill:?}");
                assert_eq!(path.contains(p(15.0, 15.0)), in_two, "{fill:?}");
            }

            // (0,0) -> (0,10) -> (10,10) -> (10,0) winds -1 about (5, 5);
            // the reverse winds +1.
            let square = [p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0)];
            let reversed = [square[3], square[2], square[1], square[0]];
            let shapes: [fn(&mut Path); 2] = [
                |path| path.add_rect(rect(0.0, 0.0, 10.0, 10.0)),
                |path| path.add_oval(rect(0.0, 0.0, 10.0, 10.0)),
            ];
            for shape in shapes {
                for (points, non_zero) in [(square, false), (reversed, true)] {
                    for fill in FILLS {
                        let mut path = Path::polygon(&points);
                        path.set_fill_type(fill);
                        shape(&mut path);
                        let expected = fill == PathFillType::NonZero && non_zero;
                        assert_eq!(path.contains(p(5.0, 5.0)), expected, "{fill:?} {points:?}");
                    }
                }
            }
        }

        /// Each corner with any radius gets one arc and each square corner
        /// none, whichever corner it is and whichever axis is zero.
        #[test]
        fn from_rrect_emits_an_arc_exactly_for_each_rounded_corner() {
            let arcs = |corners: [Radius<Pixels>; 4]| {
                let [tl, tr, br, bl] = corners;
                let rrect =
                    RRect::from_rect_and_corners(rect(0.0, 0.0, 100.0, 60.0), tl, tr, br, bl);
                Path::from_rrect(rrect)
                    .commands()
                    .iter()
                    .filter(|c| matches!(c, PathCommand::AddArc(..)))
                    .count()
            };
            let zero = Radius::circular(px(0.0));
            for k in 0..4 {
                let mut one_square = [Radius::circular(px(8.0)); 4];
                one_square[k] = zero;
                assert_eq!(arcs(one_square), 3, "corner {k} square");
                for radius in [Radius::new(px(6.0), px(0.0)), Radius::new(px(0.0), px(6.0))] {
                    let mut only = [zero; 4];
                    only[k] = radius;
                    assert_eq!(arcs(only), 1, "corner {k} alone with {radius:?}");
                }
            }
        }

        /// A quarter circle bulges past the chamfer between its ends:
        /// 0.85 r from the corner circle centre, towards the corner, is
        /// inside the arc but outside the chamfer; 1.15 r is outside both.
        #[test]
        fn from_rrect_corners_are_arcs_not_chamfers() {
            let path = Path::from_rrect(RRect::from_rect_and_corners(
                rect(0.0, 0.0, 100.0, 60.0),
                Radius::circular(px(10.0)),
                Radius::circular(px(12.0)),
                Radius::circular(px(14.0)),
                Radius::circular(px(16.0)),
            ));
            let s = std::f32::consts::FRAC_1_SQRT_2;
            // (centre, radius, direction towards the corner)
            for (c, r, d) in [
                ((10.0, 10.0), 10.0, (-s, -s)),
                ((88.0, 12.0), 12.0, (s, -s)),
                ((86.0, 46.0), 14.0, (s, s)),
                ((16.0, 44.0), 16.0, (-s, s)),
            ] {
                let at = |k: f32| p(c.0 + d.0 * k * r, c.1 + d.1 * k * r);
                assert!(path.contains(at(0.85)), "{c:?} inside the arc");
                assert!(!path.contains(at(1.15)), "{c:?} outside the arc");
            }
        }

        /// The fewest chords whose sagitta, `r (1 - cos(sweep / 2n))`, is
        /// within the flattening tolerance; the larger semi-axis of an
        /// ellipse; one chord for degenerate input; the ceiling for a
        /// radius too large to resolve.
        #[test]
        fn arc_chord_count_is_the_fewest_within_tolerance() {
            let tol = Path::ARC_FLATTENING_TOLERANCE;
            let sagitta =
                |r: f32, sweep: f32, n: usize| r * (1.0 - (sweep / (2.0 * n as f32)).cos());
            for r in [0.5_f32, 10.0, 100.0, 1000.0] {
                for sweep in [0.3_f32, std::f32::consts::FRAC_PI_2, std::f32::consts::TAU] {
                    let circle = rect(0.0, 0.0, 2.0 * r, 2.0 * r);
                    let n = Path::arc_chord_count(circle, sweep);
                    assert!(
                        sagitta(r, sweep, n) <= tol * 1.001,
                        "r {r} sweep {sweep}: {n} chords"
                    );
                    if n > 1 {
                        assert!(
                            sagitta(r, sweep, n - 1) > tol,
                            "r {r} sweep {sweep}: {n} is not the fewest"
                        );
                    }
                    assert_eq!(Path::arc_chord_count(circle, -sweep), n);
                }
            }
            let pi = std::f32::consts::PI;
            let wide = Path::arc_chord_count(rect(0.0, 0.0, 200.0, 2.0), pi);
            assert_eq!(
                wide,
                Path::arc_chord_count(rect(0.0, 0.0, 200.0, 200.0), pi)
            );
            assert_eq!(Path::arc_chord_count(rect(0.0, 0.0, 2.0, 200.0), pi), wide);

            let unit = rect(0.0, 0.0, 2.0, 2.0);
            assert_eq!(Path::arc_chord_count(rect(0.0, 0.0, 0.0, 0.0), 3.0 * pi), 1);
            assert_eq!(Path::arc_chord_count(unit, 0.0), 1);
            assert_eq!(Path::arc_chord_count(unit, f32::NAN), 1);
            assert_eq!(Path::arc_chord_count(unit, f32::INFINITY), 1);
            let huge = rect(0.0, 0.0, 2.0e9, 2.0e9);
            assert_eq!(Path::arc_chord_count(huge, 1.0), Path::MAX_ARC_CHORDS);
            // Resolvable, but wanting more chords than the ceiling.
            let wide_turn = rect(0.0, 0.0, 2.0e6, 2.0e6);
            assert_eq!(
                Path::arc_chord_count(wide_turn, std::f32::consts::TAU),
                Path::MAX_ARC_CHORDS
            );
            assert_eq!(Path::MAX_ARC_CHORDS, 2048);
        }

        /// Where the curve regions below sit: off the origin, so no control
        /// point has a zero coordinate that would hide a term of the
        /// Bézier polynomial.
        const OFFSET: (f32, f32) = (37.0, 53.0);

        /// Curves whose control points are evenly spaced in x, so x = w u
        /// and each curve is the graph of a function of x: the region
        /// between it and its baseline has an exact inside test. Returns
        /// the path and, for `u` in `0..1`, the curve's height and slope.
        ///
        /// - quadratic (0, 0), (w/2, h), (w, 0): y = 2 h u (1 - u);
        /// - cubic (0, 0), (w/3, a), (2w/3, b), (w, 0):
        ///   y = 3 a u (1 - u)^2 + 3 b u^2 (1 - u).
        fn under_curve(cubic: bool, w: f32) -> (Path, impl Fn(f32) -> (f32, f32)) {
            let (h, a, b) = (w, 1.2 * w, 0.6 * w);
            let at = |x: f32, y: f32| p(OFFSET.0 + x, OFFSET.1 + y);
            let mut path = Path::new();
            path.move_to(at(0.0, 0.0));
            if cubic {
                path.cubic_to(at(w / 3.0, a), at(2.0 * w / 3.0, b), at(w, 0.0));
            } else {
                path.quadratic_bezier_to(at(w / 2.0, h), at(w, 0.0));
            }
            path.close();
            let curve = move |u: f32| {
                let (height, per_u) = if cubic {
                    (
                        3.0 * a * u * (1.0 - u).powi(2) + 3.0 * b * u * u * (1.0 - u),
                        3.0 * a * ((1.0 - u).powi(2) - 2.0 * u * (1.0 - u))
                            + 3.0 * b * (2.0 * u * (1.0 - u) - u * u),
                    )
                } else {
                    (2.0 * h * u * (1.0 - u), 2.0 * h * (1.0 - 2.0 * u))
                };
                (height, per_u / w)
            };
            (path, curve)
        }

        proptest! {
            /// Containment matches the exact region at every scale, down
            /// to 0.15 px from the curve along its normal: the flattening
            /// stays within its 0.1 px tolerance however large the curve
            /// is (four fixed chords were three pixels off at w = 100).
            /// Half the points are drawn within 3 px of the curve, where
            /// under-flattening shows; the rest anywhere over the region.
            #[test]
            fn curve_containment_matches_the_exact_region(
                cubic in any::<bool>(),
                scale in prop::sample::select(vec![1.0_f32, 100.0, 1000.0]),
                // A third of the samples near each end, where the first and
                // last chords are.
                u in prop_oneof![0.001_f32..0.05, 0.01_f32..0.99, 0.95_f32..0.999],
                near in any::<bool>(),
                offset in -3.0_f32..3.0,
                v in -0.2_f32..1.2,
            ) {
                let w = 100.0 * scale;
                let (path, curve) = under_curve(cubic, w);
                let (top, slope) = curve(u);
                // Vertical distance per unit of distance along the normal.
                let stretch = slope.hypot(1.0);
                let y = if near { top + offset * stretch } else { v * top };
                prop_assume!((y - top).abs() > 0.15 * stretch && y.abs() > 0.15);
                let inside = y > 0.0 && y < top;
                let probe = p(OFFSET.0 + u * w, OFFSET.1 + y);
                for fill in FILLS {
                    prop_assert_eq!(
                        with(fill, path.clone()).contains(probe),
                        inside,
                        "{:?} {:?}",
                        fill,
                        probe
                    );
                }
            }
        }

        /// One chord for a straight or non-finite curve; the ceiling for a
        /// curve too large to flatten within the tolerance.
        #[test]
        fn bezier_chord_count_edges() {
            let line = [p(0.0, 0.0), p(5.0, 5.0), p(10.0, 10.0), p(15.0, 15.0)];
            assert_eq!(Path::bezier_chord_count(2.0, &line[..3]), 1);
            assert_eq!(Path::bezier_chord_count(3.0, &line), 1);
            let infinite = [p(0.0, 0.0), p(f32::INFINITY, 0.0), p(1.0, 0.0)];
            assert_eq!(Path::bezier_chord_count(2.0, &infinite), 1);
            let huge = [p(0.0, 0.0), p(1.0e12, 1.0e12), p(2.0e12, 0.0)];
            assert_eq!(Path::bezier_chord_count(2.0, &huge), Path::MAX_ARC_CHORDS);
        }
    }
}
