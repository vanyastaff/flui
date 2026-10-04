//! Path types for vector drawing.
//!
//! A [`Path`] is a sequence of contours made of lines and quadratic and cubic Bézier curves,
//! plus a [`PathFillType`]. Its geometry is a `kurbo::BezPath` (ADR-0098 §7): containment is
//! kurbo's exact winding number, bounds are the curves' tight bounds, and rectangles, ovals
//! and arcs are built as curves at one fixed construction tolerance. No kurbo type appears in
//! the public API; [`PathCommand`] is FLUI's element vocabulary.

use std::sync::Arc;

use kurbo::{Affine, PathEl, Shape};

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
