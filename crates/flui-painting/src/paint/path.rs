//! Path types for vector drawing.
//!
//! A [`Path`] is a sequence of contours made of lines and quadratic and cubic Bézier curves,
//! plus a [`PathFillType`]. Its geometry is a `kurbo::BezPath` (ADR-0098 §7): containment is
//! kurbo's exact winding number, bounds are the curves' tight bounds, and rectangles, ovals
//! and arcs are built as curves at one fixed construction tolerance. No kurbo type appears in
//! the public API; [`PathCommand`] is FLUI's element vocabulary.

use std::sync::Arc;

use kurbo::{Affine, ParamCurve, ParamCurveArclen, PathEl, Shape};

use crate::paint::PathFillType;
use flui_foundation::geometry::{Offset, Point, RRect, Rect};

/// How close the curves built for an oval or an arc stay to the true ellipse, in logical
/// pixels. Tight enough that at an 8x device scale the error is still a hundredth of a
/// device pixel.
const CURVE_TOLERANCE: f64 = 1e-3;

/// A single element of a [`Path`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathCommand {
    /// Start a new contour at a point.
    MoveTo(Point<f64>),

    /// A straight line to a point.
    LineTo(Point<f64>),

    /// A quadratic Bézier curve: control point, end point.
    QuadraticTo(Point<f64>, Point<f64>),

    /// A cubic Bézier curve: first control point, second control point, end point.
    CubicTo(Point<f64>, Point<f64>, Point<f64>),

    /// Close the current contour with a line back to its start.
    Close,
}

/// The shape a path was constructed as, while nothing else has been added to it.
///
/// The analytic rounded-rectangle shadow reads it through [`Path::rrect_hint`]; any edit
/// after construction clears it.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum ShapeHint {
    None,
    Rect(Rect<f64>),
    Oval(Rect<f64>),
    RRect(RRect),
}

impl ShapeHint {
    fn translate(self, offset: Offset<f64>) -> Self {
        match self {
            Self::None => Self::None,
            Self::Rect(rect) => Self::Rect(rect.translate_offset(offset)),
            Self::Oval(rect) => Self::Oval(rect.translate_offset(offset)),
            Self::RRect(rrect) => Self::RRect(rrect.translate_offset(offset)),
        }
    }
}

/// A vector shape: contours of lines and Bézier curves, filled by a [`PathFillType`].
///
/// A contour left open is closed implicitly when the path is filled or hit-tested, and a
/// standalone shape ([`Self::add_rect`], [`Self::add_oval`]) always ends the open contour
/// before it. The geometry is copy-on-write (Skia's `SkPath` over `SkPathRef`): a clone
/// shares it, and the first mutation of a shared path copies it, so a path recorded into a
/// display list or a clip is a pointer-sized handle, not a copy.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "PathRepr", into = "PathRepr"))]
pub struct Path {
    /// The geometry, shared until mutated.
    geometry: Arc<kurbo::BezPath>,

    /// The shape a constructor built, until any further edit.
    hint: ShapeHint,

    /// The fill type for this path.
    fill_type: PathFillType,

    /// Cached bounding box (invalidated when the geometry changes).
    bounds: Option<Rect<f64>>,
}

fn to_kurbo(point: Point<f64>) -> kurbo::Point {
    kurbo::Point::new(point.x, point.y)
}

fn from_kurbo(point: kurbo::Point) -> Point<f64> {
    Point::new(point.x, point.y)
}

fn to_kurbo_rect(rect: Rect<f64>) -> kurbo::Rect {
    kurbo::Rect::new(rect.left(), rect.top(), rect.right(), rect.bottom())
}

/// One segment a stroke traces, with its place in its contour.
#[derive(Clone, Copy)]
struct Segment {
    curve: kurbo::PathSeg,
    /// The first segment after a `MoveTo`.
    starts_contour: bool,
    /// The edge a `Close` draws back to the contour's start.
    closing: bool,
    /// The last segment of a closed contour: its closing edge, or its last drawn segment
    /// when the contour already ended at its start.
    ends_closed_contour: bool,
}

/// Segments whose extent from their own start exceeds this are measured on a copy scaled to
/// unit size: kurbo squares coordinate deltas and forms multiples of control points, which
/// overflow long before a length or a sub-curve stops being representable, and an absolute
/// tolerance on such a segment would never converge.
const NORMALIZE_ABOVE: f64 = 1e100;

/// Segments whose extent from their own start is below this (and nonzero) are measured on a
/// copy scaled to unit size too: the same squared deltas underflow to zero long before a
/// length stops being representable.
const NORMALIZE_BELOW: f64 = 1e-100;

/// The largest coordinate magnitude among a segment's control points.
fn segment_extent(segment: kurbo::PathSeg) -> f64 {
    let points: &[kurbo::Point] = match &segment {
        kurbo::PathSeg::Line(line) => &[line.p0, line.p1],
        kurbo::PathSeg::Quad(quad) => &[quad.p0, quad.p1, quad.p2],
        kurbo::PathSeg::Cubic(cubic) => &[cubic.p0, cubic.p1, cubic.p2, cubic.p3],
    };
    points
        .iter()
        .fold(0.0, |extent: f64, p| extent.max(p.x.abs()).max(p.y.abs()))
}

/// A segment moved to its own start and, when large, scaled to unit size, so measuring and
/// subdividing it neither loses adjacent large coordinates to rounding nor overflows.
struct LocalSegment {
    /// The segment as the path holds it.
    original: kurbo::PathSeg,
    curve: kurbo::PathSeg,
    /// Where the local origin sits in path space.
    origin: kurbo::Vec2,
    /// The local copy's unit, in path units.
    unit: f64,
}

impl LocalSegment {
    fn new(segment: kurbo::PathSeg) -> Self {
        let origin = segment.start().to_vec2();
        let moved = Affine::translate(-origin) * segment;
        // Moving to the start can itself overflow for an edge spanning most of f64; such a
        // segment is scaled where it stands.
        let (curve, origin) = if segment_extent(moved).is_finite() {
            (moved, origin)
        } else {
            (segment, kurbo::Vec2::ZERO)
        };
        let extent = segment_extent(curve);
        let normalize = extent.is_finite()
            && (extent > NORMALIZE_ABOVE || (extent > 0.0 && extent < NORMALIZE_BELOW));
        if normalize {
            Self {
                original: segment,
                curve: divided(curve, extent),
                origin,
                unit: extent,
            }
        } else {
            Self {
                original: segment,
                curve,
                origin,
                unit: 1.0,
            }
        }
    }

    fn length(&self) -> f64 {
        self.curve.arclen(CURVE_TOLERANCE) * self.unit
    }

    /// The curve parameter at arclength `length` from the start.
    fn param_at(&self, length: f64) -> f64 {
        self.curve.inv_arclen(length / self.unit, CURVE_TOLERANCE)
    }

    /// The part between parameters `t0` and `t1`, back in path space. An unscaled segment is
    /// subdivided as the path holds it, so its points stay exact.
    fn subsegment(&self, t0: f64, t1: f64) -> kurbo::PathSeg {
        if self.unit == 1.0 {
            return self.original.subsegment(t0..t1);
        }
        let mut piece = Affine::translate(self.origin)
            * Affine::scale(self.unit)
            * self.curve.subsegment(t0..t1);
        // Exact ends come from the segment itself, not from the round trip through the local
        // copy, which can round a far endpoint away.
        if t0 == 0.0 {
            piece = with_start(piece, self.original.start());
        }
        if t1 == 1.0 {
            piece = with_end(piece, self.original.end());
        }
        piece
    }
}

/// `segment` with every point divided by `extent`. Division, not multiplication by the
/// reciprocal: a subnormal extent has an infinite reciprocal but divides finitely.
fn divided(segment: kurbo::PathSeg, extent: f64) -> kurbo::PathSeg {
    let scale = |p: kurbo::Point| kurbo::Point::new(p.x / extent, p.y / extent);
    match segment {
        kurbo::PathSeg::Line(line) => {
            kurbo::PathSeg::Line(kurbo::Line::new(scale(line.p0), scale(line.p1)))
        }
        kurbo::PathSeg::Quad(quad) => kurbo::PathSeg::Quad(kurbo::QuadBez::new(
            scale(quad.p0),
            scale(quad.p1),
            scale(quad.p2),
        )),
        kurbo::PathSeg::Cubic(cubic) => kurbo::PathSeg::Cubic(kurbo::CubicBez::new(
            scale(cubic.p0),
            scale(cubic.p1),
            scale(cubic.p2),
            scale(cubic.p3),
        )),
    }
}

/// `segment` with its first point replaced by `start`.
fn with_start(segment: kurbo::PathSeg, start: kurbo::Point) -> kurbo::PathSeg {
    match segment {
        kurbo::PathSeg::Line(line) => kurbo::PathSeg::Line(kurbo::Line::new(start, line.p1)),
        kurbo::PathSeg::Quad(quad) => {
            kurbo::PathSeg::Quad(kurbo::QuadBez::new(start, quad.p1, quad.p2))
        }
        kurbo::PathSeg::Cubic(cubic) => {
            kurbo::PathSeg::Cubic(kurbo::CubicBez::new(start, cubic.p1, cubic.p2, cubic.p3))
        }
    }
}

/// `segment` with its last point replaced by `end`.
fn with_end(segment: kurbo::PathSeg, end: kurbo::Point) -> kurbo::PathSeg {
    match segment {
        kurbo::PathSeg::Line(line) => kurbo::PathSeg::Line(kurbo::Line::new(line.p0, end)),
        kurbo::PathSeg::Quad(quad) => {
            kurbo::PathSeg::Quad(kurbo::QuadBez::new(quad.p0, quad.p1, end))
        }
        kurbo::PathSeg::Cubic(cubic) => {
            kurbo::PathSeg::Cubic(kurbo::CubicBez::new(cubic.p0, cubic.p1, cubic.p2, end))
        }
    }
}

/// A segment's arclength, measured on its [`LocalSegment`].
fn segment_length(segment: kurbo::PathSeg) -> f64 {
    LocalSegment::new(segment).length()
}

impl Path {
    /// Creates an empty path with the default fill type (non-zero).
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::with_fill_type(PathFillType::default())
    }

    /// Creates an empty path with the given fill type.
    #[must_use]
    #[inline]
    pub fn with_fill_type(fill_type: PathFillType) -> Self {
        Self {
            geometry: Arc::default(),
            hint: ShapeHint::None,
            fill_type,
            bounds: None,
        }
    }

    /// Creates a path consisting of a single rectangle.
    #[must_use]
    pub fn rectangle(rect: Rect<f64>) -> Self {
        let mut path = Self::new();
        path.add_rect(rect);
        path.hint = ShapeHint::Rect(rect);
        path
    }

    /// Creates a path consisting of a single oval inscribed in `rect`.
    #[must_use]
    pub fn oval(rect: Rect<f64>) -> Self {
        let mut path = Self::new();
        path.add_oval(rect);
        path.hint = ShapeHint::Oval(rect);
        path
    }

    /// Creates a circular path around `center` with the given `radius`.
    #[must_use]
    pub fn circle(center: Point<f64>, radius: f64) -> Self {
        Self::oval(Rect::from_ltrb(
            center.x - radius,
            center.y - radius,
            center.x + radius,
            center.y + radius,
        ))
    }

    /// Creates a closed polygon through `points`.
    #[must_use]
    pub fn polygon(points: &[Point<f64>]) -> Self {
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

    /// Creates a path consisting of a single arc on the oval inscribed in `rect`, starting at
    /// `start_angle` and sweeping by `sweep_angle` (both in radians, clockwise in y-down
    /// coordinates).
    #[must_use]
    pub fn arc(rect: Rect<f64>, start_angle: f64, sweep_angle: f64) -> Self {
        let mut path = Self::new();
        path.add_arc(rect, start_angle, sweep_angle);
        path
    }

    /// Creates a path outlining a rounded rectangle, each corner an elliptical quarter arc.
    ///
    /// A rounded rectangle with no rounding is a plain rectangle.
    #[must_use]
    pub fn from_rrect(rrect: RRect) -> Self {
        if rrect.is_rect() {
            return Self::rectangle(rrect.bounding_rect());
        }
        use std::f64::consts::{FRAC_PI_2, PI};
        let rect = rrect.bounding_rect();
        let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        let (tl, tr, br, bl) = (
            rrect.top_left,
            rrect.top_right,
            rrect.bottom_right,
            rrect.bottom_left,
        );
        let corner = |path: &mut Self, x: f64, y: f64, rx: f64, ry: f64, start: f64| {
            if rx > 0.0 && ry > 0.0 {
                path.add_arc(
                    Rect::from_ltrb(x - rx, y - ry, x + rx, y + ry),
                    start,
                    FRAC_PI_2,
                );
            }
        };

        let mut path = Self::new();
        path.move_to(Point::new(l + tl.x, t));
        path.line_to(Point::new(r - tr.x, t));
        corner(&mut path, r - tr.x, t + tr.y, tr.x, tr.y, -FRAC_PI_2);
        path.line_to(Point::new(r, b - br.y));
        corner(&mut path, r - br.x, b - br.y, br.x, br.y, 0.0);
        path.line_to(Point::new(l + bl.x, b));
        corner(&mut path, l + bl.x, b - bl.y, bl.x, bl.y, FRAC_PI_2);
        path.line_to(Point::new(l, t + tl.y));
        corner(&mut path, l + tl.x, t + tl.y, tl.x, tl.y, PI);
        path.close();
        path.hint = ShapeHint::RRect(rrect);
        path
    }

    /// The rounded rectangle this path was built from by [`Self::from_rrect`], while nothing
    /// else has been added to it.
    ///
    /// The GPU shadow uses it to route rounded-rectangle shadows (Material `Card`,
    /// `FloatingActionButton`, `Dialog`, `Chip`, …) through the analytical single-pass
    /// Gaussian instead of the path-fill approximation. A plain rectangle, an oval or any
    /// hand-built path returns `None`.
    #[must_use]
    pub fn rrect_hint(&self) -> Option<RRect> {
        match self.hint {
            ShapeHint::RRect(rrect) => Some(rrect),
            _ => None,
        }
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

    /// Returns `true` if `point` lies inside this path under its [`PathFillType`]: a non-zero
    /// winding number, or an odd one for even-odd.
    ///
    /// Every open contour counts as closed, as it is when filled. The winding number is exact
    /// for curves (kurbo solves each curve for the ray's crossing), so a shape built from arcs
    /// is contained as the curve it draws.
    #[must_use]
    pub fn contains(&self, point: Point<f64>) -> bool {
        let winding = self.winding(to_kurbo(point));
        match self.fill_type {
            PathFillType::NonZero => winding != 0,
            PathFillType::EvenOdd => winding % 2 != 0,
        }
    }

    /// The winding number of `point` with every open contour closed.
    fn winding(&self, point: kurbo::Point) -> i32 {
        let elements = self.geometry.elements();
        let mut closed = Vec::with_capacity(elements.len() + 1);
        let mut open = false;
        for &element in elements {
            match element {
                PathEl::MoveTo(_) => {
                    if open {
                        closed.push(PathEl::ClosePath);
                    }
                    open = true;
                }
                PathEl::ClosePath => open = false,
                _ => {}
            }
            closed.push(element);
        }
        if open {
            closed.push(PathEl::ClosePath);
        }
        closed.as_slice().winding(point)
    }

    /// The geometry, copied if it is shared, with the cached bounds and the shape hint
    /// dropped: every edit goes through here.
    fn edit(&mut self) -> &mut kurbo::BezPath {
        self.bounds = None;
        self.hint = ShapeHint::None;
        Arc::make_mut(&mut self.geometry)
    }

    /// Where the pen is: the end of the last element, the start of a contour just closed, or
    /// the origin on an empty path.
    fn pen(&self) -> kurbo::Point {
        let mut contour_start = kurbo::Point::ZERO;
        let mut pen = kurbo::Point::ZERO;
        for element in self.geometry.elements() {
            match *element {
                PathEl::MoveTo(p) => {
                    contour_start = p;
                    pen = p;
                }
                PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => pen = p,
                PathEl::ClosePath => pen = contour_start,
            }
        }
        pen
    }

    /// Whether the last contour is still open (begun and not closed).
    fn contour_open(&self) -> bool {
        !matches!(
            self.geometry.elements().last(),
            None | Some(PathEl::ClosePath)
        )
    }

    /// Starts a contour at the pen if none is open, so a segment always has a start.
    fn begin_at_pen(&mut self) {
        if !self.contour_open() {
            let pen = self.pen();
            self.edit().move_to(pen);
        }
    }

    /// Starts a new contour at `point` without drawing.
    pub fn move_to(&mut self, point: Point<f64>) {
        self.edit().move_to(to_kurbo(point));
    }

    /// Adds a straight line from the pen to `point`.
    ///
    /// With no contour open it starts from the pen: the origin on an empty path, or the start
    /// of the contour just closed.
    pub fn line_to(&mut self, point: Point<f64>) {
        self.begin_at_pen();
        self.edit().line_to(to_kurbo(point));
    }

    /// Adds a quadratic Bézier curve from the pen to `end`, pulled toward `control`.
    /// Starts where [`Self::line_to`] does.
    pub fn quadratic_bezier_to(&mut self, control: Point<f64>, end: Point<f64>) {
        self.begin_at_pen();
        self.edit().quad_to(to_kurbo(control), to_kurbo(end));
    }

    /// Adds a cubic Bézier curve from the pen to `end`, with `control1` shaping its start and
    /// `control2` its end. Starts where [`Self::line_to`] does.
    pub fn cubic_to(&mut self, control1: Point<f64>, control2: Point<f64>, end: Point<f64>) {
        self.begin_at_pen();
        self.edit()
            .curve_to(to_kurbo(control1), to_kurbo(control2), to_kurbo(end));
    }

    /// Closes the current contour with a line back to its start. Does nothing when no contour
    /// is open.
    pub fn close(&mut self) {
        if self.contour_open() {
            self.edit().close_path();
        }
    }

    /// Adds `rect` as a closed contour of its own, clockwise from its top-left corner (y-down).
    /// An open contour before it stays open: a stroke does not gain a closing edge.
    pub fn add_rect(&mut self, rect: Rect<f64>) {
        let path = self.edit();
        path.move_to((rect.left(), rect.top()));
        path.line_to((rect.right(), rect.top()));
        path.line_to((rect.right(), rect.bottom()));
        path.line_to((rect.left(), rect.bottom()));
        path.close_path();
    }

    /// Adds the oval inscribed in `rect` as a closed contour of its own, clockwise from its
    /// rightmost point (y-down), like [`Self::add_rect`].
    pub fn add_oval(&mut self, rect: Rect<f64>) {
        let ellipse = kurbo::Ellipse::from_rect(to_kurbo_rect(rect));
        self.edit().extend(ellipse.path_elements(CURVE_TOLERANCE));
        // kurbo leaves the ellipse's contour open; a standalone shape is closed.
        self.close();
    }

    /// Adds an arc on the oval inscribed in `rect`, starting at `start_angle` and sweeping by
    /// `sweep_angle` (both in radians).
    ///
    /// # Contour joining
    ///
    /// Unlike an arc primitive that always starts a **new** contour (as Skia's `addArc` does),
    /// this joins an open contour with a line from the pen to the arc's start, so a shape built
    /// from edges and corner arcs ([`Self::from_rrect`]) is one continuous contour instead of
    /// four open pieces, each of which would be filled with a chord across its corner. With no
    /// contour open the arc starts its own. A caller wanting a fresh contour calls
    /// [`Self::move_to`] to the arc's start first.
    pub fn add_arc(&mut self, rect: Rect<f64>, start_angle: f64, sweep_angle: f64) {
        let arc = kurbo::Arc::new(
            to_kurbo_rect(rect).center(),
            kurbo::Vec2::new(rect.width() / 2.0, rect.height() / 2.0),
            start_angle,
            sweep_angle,
            0.0,
        );
        let start = arc.center
            + kurbo::Vec2::new(
                arc.radii.x * start_angle.cos(),
                arc.radii.y * start_angle.sin(),
            );
        if self.contour_open() {
            if self.pen() != start {
                self.edit().line_to(start);
            }
        } else {
            self.edit().move_to(start);
        }
        self.edit().extend(arc.append_iter(CURVE_TOLERANCE));
    }

    /// The path's elements in order.
    pub fn commands(&self) -> impl ExactSizeIterator<Item = PathCommand> + '_ {
        self.geometry
            .elements()
            .iter()
            .map(|element| match *element {
                PathEl::MoveTo(p) => PathCommand::MoveTo(from_kurbo(p)),
                PathEl::LineTo(p) => PathCommand::LineTo(from_kurbo(p)),
                PathEl::QuadTo(c, p) => PathCommand::QuadraticTo(from_kurbo(c), from_kurbo(p)),
                PathEl::CurveTo(c1, c2, p) => {
                    PathCommand::CubicTo(from_kurbo(c1), from_kurbo(c2), from_kurbo(p))
                }
                PathEl::ClosePath => PathCommand::Close,
            })
    }

    /// Whether `self` and `other` share one geometry buffer — true for a clone until either
    /// side is mutated.
    #[must_use]
    pub fn shares_commands_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.geometry, &other.geometry)
    }

    /// Returns `true` if the path has no elements.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.geometry.elements().is_empty()
    }

    /// Removes every element. The fill type is kept.
    pub fn reset(&mut self) {
        self.edit().truncate(0);
    }

    /// The cached bounding box, if [`Self::bounds`] computed one and no edit has invalidated
    /// it since.
    #[must_use]
    #[inline]
    pub fn cached_bounds(&self) -> Option<Rect<f64>> {
        self.bounds
    }

    /// The path's tight bounding box, without caching it. `Rect::ZERO` for an empty path.
    #[must_use]
    pub fn compute_bounds(&self) -> Rect<f64> {
        if let Some(bounds) = self.bounds {
            return bounds;
        }
        if self.is_empty() {
            return Rect::ZERO;
        }
        let bounds = self.geometry.bounding_box();
        Rect::from_ltrb(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
    }

    /// The path's tight bounding box, cached until the next edit.
    pub fn bounds(&mut self) -> Rect<f64> {
        let bounds = self.compute_bounds();
        self.bounds = Some(bounds);
        bounds
    }

    /// A copy of this path moved by `offset`.
    #[must_use]
    pub fn translate(&self, offset: Offset<f64>) -> Self {
        let mut path = (*self.geometry).clone();
        path.apply_affine(Affine::translate((offset.dx, offset.dy)));
        Self {
            geometry: Arc::new(path),
            hint: self.hint.translate(offset),
            fill_type: self.fill_type,
            bounds: None,
        }
    }

    /// A path with this geometry and fill type and no shape hint.
    fn derived(&self, geometry: kurbo::BezPath) -> Self {
        Self {
            geometry: Arc::new(geometry),
            hint: ShapeHint::None,
            fill_type: self.fill_type,
            bounds: None,
        }
    }

    /// The path's segments in order.
    ///
    /// A closed contour contributes its closing line, so the segments trace exactly what a
    /// stroke draws.
    fn segments(&self) -> Vec<Segment> {
        let mut segments: Vec<Segment> = Vec::new();
        let mut contour_start = kurbo::Point::ZERO;
        let mut pen = kurbo::Point::ZERO;
        let mut new_contour = true;
        for element in self.geometry.elements() {
            let (curve, closing) = match *element {
                PathEl::MoveTo(p) => {
                    contour_start = p;
                    pen = p;
                    new_contour = true;
                    continue;
                }
                PathEl::LineTo(p) => (kurbo::PathSeg::Line(kurbo::Line::new(pen, p)), false),
                PathEl::QuadTo(c, p) => {
                    (kurbo::PathSeg::Quad(kurbo::QuadBez::new(pen, c, p)), false)
                }
                PathEl::CurveTo(c1, c2, p) => (
                    kurbo::PathSeg::Cubic(kurbo::CubicBez::new(pen, c1, c2, p)),
                    false,
                ),
                PathEl::ClosePath => {
                    let line = kurbo::Line::new(pen, contour_start);
                    pen = contour_start;
                    if line.p0 == line.p1 {
                        // No closing edge to trace: the contour's last drawn segment ends it.
                        if !new_contour && let Some(last) = segments.last_mut() {
                            last.ends_closed_contour = true;
                        }
                        continue;
                    }
                    (kurbo::PathSeg::Line(line), true)
                }
            };
            pen = curve.end();
            segments.push(Segment {
                curve,
                starts_contour: new_contour,
                closing,
                ends_closed_contour: closing,
            });
            new_contour = false;
        }
        segments
    }

    /// The length of every contour together, in logical pixels: what a stroke traces,
    /// closing lines included. Curves are measured to within a thousandth of a logical pixel; a
    /// segment with coordinates beyond `1e100` is measured to a thousandth of its own size.
    ///
    /// `0.0` for an empty path, and for one whose geometry is not finite.
    #[must_use]
    pub fn length(&self) -> f64 {
        // A non-finite point that traces no segment (a bare `MoveTo`) still poisons the path.
        if !self.is_finite() {
            return 0.0;
        }
        let length: f64 = self
            .segments()
            .iter()
            .map(|segment| segment_length(segment.curve))
            .sum();
        if length.is_finite() { length } else { 0.0 }
    }

    /// The part of this path between the fractions `start` and `end` of its
    /// [`length`](Self::length), as a stroke would trace it.
    ///
    /// Both fractions are clamped to `[0, 1]`. With `start > end` the trim wraps through the
    /// path's start: it keeps `[start, 1]` and then `[0, end]`, the way an animated offset
    /// moves a trimmed window past the end of a closed shape. Every contour piece the window
    /// covers becomes a contour of its own. A closed contour the window covers whole, in one
    /// piece, keeps its `Close`, so a stroke joins its seam instead of capping two ends; a
    /// partial piece stays open.
    ///
    /// Equal fractions, a non-finite fraction, and a path of no (or non-finite) length give
    /// an empty path. `trim(0.0, 1.0)` is the path itself. The fill type is kept.
    #[must_use]
    pub fn trim(&self, start: f64, end: f64) -> Self {
        if !start.is_finite() || !end.is_finite() {
            return Self::with_fill_type(self.fill_type);
        }
        let (start, end) = (start.clamp(0.0, 1.0), end.clamp(0.0, 1.0));
        let total = self.length();
        if start <= 0.0 && end >= 1.0 && total > 0.0 {
            return self.clone();
        }
        if total <= 0.0 || start == end {
            return Self::with_fill_type(self.fill_type);
        }
        let windows = if start < end {
            vec![(start * total, end * total)]
        } else {
            vec![(start * total, total), (0.0, end * total)]
        };

        let segments: Vec<_> = self
            .segments()
            .into_iter()
            .map(|segment| {
                let local = LocalSegment::new(segment.curve);
                let length = local.length();
                (segment, local, length)
            })
            .collect();
        let mut trimmed = kurbo::BezPath::new();
        for (from, to) in windows {
            // The end of the piece most recently appended in this window, while the next
            // segment would continue it without a gap.
            let mut joined: Option<kurbo::Point> = None;
            // Whether that piece began at its contour's start, so reaching the end of a
            // closed contour means it covered the contour whole.
            let mut from_contour_start = false;
            // A contour whose first edges have no length starts at its first edge that does.
            let mut pending_start = false;
            let mut cursor = 0.0;
            for (segment, local, length) in &segments {
                let starts_contour = segment.starts_contour || pending_start;
                let (seg_from, seg_to) = (cursor, cursor + length);
                cursor = seg_to;
                if *length <= 0.0 {
                    // A zero-length edge that ends a closed contour still closes a piece that
                    // ran unbroken from the contour's start.
                    // A zero-length contour of its own closes nothing that came before it.
                    if !segment.starts_contour
                        && segment.ends_closed_contour
                        && from_contour_start
                        && joined.is_some()
                    {
                        trimmed.close_path();
                        joined = None;
                    }
                    pending_start = starts_contour;
                    if segment.starts_contour {
                        joined = None;
                    }
                    continue;
                }
                pending_start = false;
                let (lo, hi) = (from.max(seg_from), to.min(seg_to));
                if hi <= lo {
                    if starts_contour || hi < lo {
                        joined = None;
                    }
                    continue;
                }
                let t0 = if lo <= seg_from {
                    0.0
                } else {
                    local.param_at(lo - seg_from)
                };
                let t1 = if hi >= seg_to {
                    1.0
                } else {
                    local.param_at(hi - seg_from)
                };
                let piece = local.subsegment(t0, t1);
                let continues = !starts_contour && t0 == 0.0 && joined == Some(piece.start());
                if !continues {
                    trimmed.move_to(piece.start());
                    from_contour_start = starts_contour && t0 == 0.0;
                }
                let whole = from_contour_start && segment.ends_closed_contour && t1 == 1.0;
                if !(whole && segment.closing) {
                    trimmed.push(piece.as_path_el());
                }
                if whole {
                    trimmed.close_path();
                    joined = None;
                } else {
                    joined = (t1 == 1.0).then(|| piece.end());
                }
            }
        }
        // Subdivision can still lose a representable sub-curve to rounding; never publish it.
        let path = self.derived(trimmed);
        if path.is_finite() {
            path
        } else {
            Self::with_fill_type(self.fill_type)
        }
    }

    /// This path mapped through the 2D affine part of `matrix`, or `None` when the matrix
    /// is not finite or projects with perspective.
    ///
    /// Curves stay curves: an affine map of a Bézier's control points is exactly the map of
    /// the curve. Perspective is refused because projecting the control points of a curve
    /// does not project the curve. The fill type is kept; the shape hint is not.
    #[must_use]
    pub fn transform(&self, matrix: &flui_foundation::geometry::Matrix4) -> Option<Self> {
        let m = &matrix.m;
        if m.iter().any(|value| !value.is_finite()) || m[3] != 0.0 || m[7] != 0.0 || m[15] == 0.0 {
            return None;
        }
        let w = m[15];
        // Divide each mapped coordinate once, after its homogeneous numerator, so a tiny `w`
        // does not overflow the coefficients when the image is representable. A numerator
        // that overflows on its own falls back to the pre-divided coefficients.
        // Terms that overflow on their own but cancel (`1e308·2 − 1e308·2`) are retried with
        // every term scaled to unit size, so a representable image is never lost to an
        // intermediate infinity.
        // A nonzero product that underflows below the normal range loses precision even though
        // the quotient may be representable (`MIN_POSITIVE · MIN_POSITIVE / MIN_POSITIVE`).
        let underflows = |k: f64, p: f64| k != 0.0 && p != 0.0 && (k * p).abs() < f64::MIN_POSITIVE;
        // Prefer a nonzero finite result: an evaluation that underflowed to zero must not hide
        // a representable one.
        let nonzero = |value: f64| value.is_finite() && value != 0.0;
        // `(kx·px + ky·py) / w`, the linear part alone, through the corrective forms.
        let linear = |kx: f64, px: f64, ky: f64, py: f64| {
            let exact = !underflows(kx, px) && !underflows(ky, py);
            let direct = (kx * px + ky * py) / w;
            if direct.is_finite() && exact {
                return direct;
            }
            let pre_divided = (kx / w) * px + (ky / w) * py;
            if pre_divided.is_finite() && exact {
                return pre_divided;
            }
            let coefficients = kx.abs().max(ky.abs());
            if coefficients == 0.0 {
                return 0.0;
            }
            let point = px.abs().max(py.abs()).max(1.0);
            let unit = (kx / coefficients) * (px / point) + (ky / coefficients) * (py / point);
            let scaled = unit * (coefficients / w) * point;
            let reordered = unit * point / w * coefficients;
            if nonzero(scaled) {
                scaled
            } else if nonzero(reordered) {
                reordered
            } else if nonzero(direct) {
                direct
            } else if scaled.is_finite() {
                scaled
            } else if reordered.is_finite() {
                reordered
            } else {
                direct
            }
        };
        let coordinate = |kx: f64, ky: f64, k0: f64, px: f64, py: f64| {
            // A term whose point component is zero contributes exactly zero: drop its
            // coefficient so it neither forms `∞ · 0` nor sets the normalization scale.
            let kx = if px == 0.0 { 0.0 } else { kx };
            let ky = if py == 0.0 { 0.0 } else { ky };
            let mapped = (kx * px + ky * py + k0) / w;
            if mapped.is_finite() && !underflows(kx, px) && !underflows(ky, py) {
                return mapped;
            }
            // The offset is kept apart from the linear part, so products that cancel (`MAX·2 −
            // MAX·2`) do not drag a small offset into the same normalization and lose it.
            let separated = linear(kx, px, ky, py) + k0 / w;
            if nonzero(separated) {
                separated
            } else if nonzero(mapped) {
                // When the separated parts overflow against each other, a finite direct
                // quotient is still the best representable answer.
                mapped
            } else if separated.is_finite() {
                separated
            } else {
                mapped
            }
        };
        let map = |p: kurbo::Point| {
            kurbo::Point::new(
                coordinate(m[0], m[4], m[12], p.x, p.y),
                coordinate(m[1], m[5], m[13], p.x, p.y),
            )
        };
        let mut geometry = kurbo::BezPath::new();
        for &element in self.geometry.elements() {
            geometry.push(match element {
                PathEl::MoveTo(p) => PathEl::MoveTo(map(p)),
                PathEl::LineTo(p) => PathEl::LineTo(map(p)),
                PathEl::QuadTo(c, p) => PathEl::QuadTo(map(c), map(p)),
                PathEl::CurveTo(c1, c2, p) => PathEl::CurveTo(map(c1), map(c2), map(p)),
                PathEl::ClosePath => PathEl::ClosePath,
            });
        }
        let path = self.derived(geometry);
        path.is_finite().then_some(path)
    }

    /// The path between `a` (`t = 0`) and `b` (`t = 1`), point by point, or `None` when the
    /// two are not compatible.
    ///
    /// Compatible paths have the same fill type and the same sequence of element kinds, so
    /// each point of one has a partner in the other: a morph between two hand-matched
    /// shapes. `t` may overshoot `[0, 1]` (a springy curve does); a coordinate both paths
    /// share stays put however far `t` overshoots, and a result that is still not finite, or
    /// a non-finite `t`, gives `None`. The ends are exact: `t = 0` is `a` itself and `t = 1`
    /// is `b` itself, shape hint included, so a rounded rectangle keeps its analytic route.
    #[must_use]
    pub fn lerp(a: &Self, b: &Self, t: f64) -> Option<Self> {
        if !t.is_finite() || a.fill_type != b.fill_type {
            return None;
        }
        let (from, to) = (a.geometry.elements(), b.geometry.elements());
        if from.len() != to.len() {
            return None;
        }
        // Equal coordinates need no arithmetic, so overshoot cannot overflow them. Otherwise
        // the weighted sum keeps both ends exact; when its products overflow although the
        // result is representable, the difference form and then the weighted sum on operands
        // scaled to unit size are tried before giving up.
        let mix1 = |x0: f64, x1: f64| {
            if x0 == x1 {
                return x0;
            }
            // Inside [0, 1] the weighted sum is the accurate form; for an overshoot the
            // difference form is, because `1 − t` rounds once t is large.
            let weighted = x0 * (1.0 - t) + x1 * t;
            let difference = x0 + (x1 - x0) * t;
            let (first, second) = if (0.0..=1.0).contains(&t) {
                (weighted, difference)
            } else {
                (difference, weighted)
            };
            if first.is_finite() {
                return first;
            }
            if second.is_finite() {
                return second;
            }
            let scale = x0.abs().max(x1.abs());
            ((x0 / scale) * (1.0 - t) + (x1 / scale) * t) * scale
        };
        let mix = |p0: kurbo::Point, p1: kurbo::Point| {
            kurbo::Point::new(mix1(p0.x, p1.x), mix1(p0.y, p1.y))
        };
        let mut geometry = kurbo::BezPath::new();
        for (&e0, &e1) in from.iter().zip(to) {
            geometry.push(match (e0, e1) {
                (PathEl::MoveTo(p0), PathEl::MoveTo(p1)) => PathEl::MoveTo(mix(p0, p1)),
                (PathEl::LineTo(p0), PathEl::LineTo(p1)) => PathEl::LineTo(mix(p0, p1)),
                (PathEl::QuadTo(c0, p0), PathEl::QuadTo(c1, p1)) => {
                    PathEl::QuadTo(mix(c0, c1), mix(p0, p1))
                }
                (PathEl::CurveTo(a0, b0, p0), PathEl::CurveTo(a1, b1, p1)) => {
                    PathEl::CurveTo(mix(a0, a1), mix(b0, b1), mix(p0, p1))
                }
                (PathEl::ClosePath, PathEl::ClosePath) => PathEl::ClosePath,
                _ => return None,
            });
        }
        // Compatibility is established; the ends are the inputs themselves.
        // A non-finite input is refused at its own end too.
        if t == 0.0 {
            return a.is_finite().then(|| a.clone());
        }
        if t == 1.0 {
            return b.is_finite().then(|| b.clone());
        }
        let path = a.derived(geometry);
        path.is_finite().then_some(path)
    }

    /// Whether every point of the geometry is finite.
    fn is_finite(&self) -> bool {
        self.geometry.elements().iter().all(|element| {
            let points: &[kurbo::Point] = match element {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => std::slice::from_ref(p),
                PathEl::QuadTo(c, p) => &[*c, *p],
                PathEl::CurveTo(c1, c2, p) => &[*c1, *c2, *p],
                PathEl::ClosePath => &[],
            };
            points.iter().all(|p| p.x.is_finite() && p.y.is_finite())
        })
    }
}

impl Default for Path {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// The serialized form of a [`Path`]: its elements, fill type and shape hint.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
struct PathRepr {
    commands: Vec<PathCommand>,
    fill_type: PathFillType,
    hint: ShapeHint,
}

#[cfg(feature = "serde")]
impl From<Path> for PathRepr {
    fn from(path: Path) -> Self {
        Self {
            commands: path.commands().collect(),
            fill_type: path.fill_type,
            hint: path.hint,
        }
    }
}

/// Deserialization replays the elements through the public builders, so input no builder
/// could have produced — a contour that starts without a `MoveTo`, a `Close` with no open
/// contour — is normalized the way the builders normalize it instead of reaching kurbo's
/// `BezPath` invariants. A path the builders made replays to the same elements.
#[cfg(feature = "serde")]
impl From<PathRepr> for Path {
    fn from(repr: PathRepr) -> Self {
        let mut path = Self::with_fill_type(repr.fill_type);
        for command in repr.commands {
            match command {
                PathCommand::MoveTo(p) => path.move_to(p),
                PathCommand::LineTo(p) => path.line_to(p),
                PathCommand::QuadraticTo(c, p) => path.quadratic_bezier_to(c, p),
                PathCommand::CubicTo(c1, c2, p) => path.cubic_to(c1, c2, p),
                PathCommand::Close => path.close(),
            }
        }
        // A hint may select a different renderer, so only retain it when its
        // constructor reproduces the submitted geometry exactly.
        let hinted = match repr.hint {
            ShapeHint::None => None,
            ShapeHint::Rect(rect) => Some(Self::rectangle(rect)),
            ShapeHint::Oval(rect) => Some(Self::oval(rect)),
            ShapeHint::RRect(rrect) => Some(Self::from_rrect(rrect)),
        };
        if let Some(hinted) = hinted
            && path.geometry == hinted.geometry
        {
            path.hint = hinted.hint;
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::geometry::Point;

    /// The command shape a circular Material surface (`CircleBorder`, and
    /// `from_rrect` for a fully-rounded box) actually produces: four
    /// quadrant arcs over the same bounding rect, each preceded by a
    /// degenerate `LineTo` to the arc's start.
    fn quadrant_arc_circle(diameter: f64) -> Path {
        use core::f64::consts::FRAC_PI_2;
        let rect = Rect::from_xywh(0.0, 0.0, diameter, diameter);
        let mid = (diameter / 2.0);
        let end = diameter;

        let mut path = Path::new();
        path.move_to(Point::new(mid, 0.0));
        path.line_to(Point::new(mid, 0.0));
        path.add_arc(rect, -FRAC_PI_2, FRAC_PI_2);
        path.line_to(Point::new(end, mid));
        path.add_arc(rect, 0.0, FRAC_PI_2);
        path.line_to(Point::new(mid, end));
        path.add_arc(rect, FRAC_PI_2, FRAC_PI_2);
        path.line_to(Point::new(0.0, mid));
        path.add_arc(rect, core::f64::consts::PI, FRAC_PI_2);
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
                path.contains(Point::new(9.0, 9.0)),
                "{fill_type:?}: (9,9) is 15.6px from the centre of a 20px \
                 radius, inside the disc but outside the endpoint diamond"
            );
            assert!(
                path.contains(Point::new(20.0, 20.0)),
                "{fill_type:?}: the centre is inside"
            );
            assert!(
                path.contains(Point::new(38.0, 20.0)),
                "{fill_type:?}: a point just inside the right extreme"
            );
        }
    }
}
