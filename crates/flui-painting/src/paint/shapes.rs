//! Rounded polygons and the morph between two of them.
//!
//! A [`RoundedPolygon`] is a closed outline of cubic Béziers built from a polygon whose
//! corners are rounded by a circular arc and, optionally, smoothed by a flanking curve on each
//! side ([`CornerRounding`]). A [`Morph`] pairs two such outlines corner to corner so that
//! [`Morph::path`] at `t = 0` traces the first, at `t = 1` the second, and in between a shape
//! that keeps matching corners together. [`material_shapes`] holds the fixed Material 3
//! Expressive shapes the loading indicators morph between.
//!
//! Every outline runs in the positive angular direction (from +x toward +y), whatever the order
//! the vertices were given in, so two polygons always morph without turning inside out, and a
//! corner is convex or concave by the outline, not by the input order.

use std::f64::consts::{PI, TAU};

use flui_foundation::geometry::{Point, Rect};
use kurbo::{Affine, CubicBez, ParamCurve, ParamCurveArclen, Vec2};

use crate::paint::Path;

/// A corner whose `sin` of the edge angle is at most this is treated as straight: a rounding
/// circle tangent to both edges would be infinitely far away.
const STRAIGHT_SIN: f64 = 1e-3;

/// Below this relative size a cubic of the outline is a point, and an edge a corner consumed
/// whole is dropped. Relative to the polygon's extent, so the outline of a tiny polygon keeps
/// its curves.
const RELATIVE_DISTANCE: f64 = 1e-9;

/// Two corner matches closer than this share an outline position, measured as a fraction of
/// the perimeter.
const PROGRESS_EPSILON: f64 = 1e-4;

/// A cubic ending this far (in progress) past a split point is split there; nearer, it is
/// taken whole.
const SPLIT_EPSILON: f64 = 1e-6;

/// Relative accuracy of the arc-length measure the morph splits cubics by.
const ARCLEN_ACCURACY: f64 = 1e-9;

/// Why a [`RoundedPolygon`] or a [`CornerRounding`] cannot be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShapeError {
    /// A polygon needs at least three vertices.
    #[error("a polygon needs at least three vertices, got {count}")]
    TooFewVertices {
        /// The number of vertices given.
        count: usize,
    },
    /// A vertex coordinate is NaN or infinite.
    #[error("vertex {index} is not finite")]
    NonFiniteVertex {
        /// The vertex's index in the input.
        index: usize,
    },
    /// A vertex equals the next one, so the edge between them has no direction.
    #[error("vertex {index} coincides with the next vertex")]
    DuplicateVertex {
        /// The index of the first of the two vertices in the input.
        index: usize,
    },
    /// All vertices lie on one line, or a rectangle is empty or inverted.
    #[error("the polygon has zero area")]
    ZeroArea,
    /// A per-vertex rounding list does not have one entry per vertex.
    #[error("{roundings} corner roundings for {vertices} vertices")]
    RoundingCountMismatch {
        /// The number of vertices.
        vertices: usize,
        /// The number of roundings.
        roundings: usize,
    },
    /// A radius (of a rounding, a circle or a star) is negative, NaN, infinite or, where a
    /// shape needs one, zero; or a star's inner radius is not below its outer radius.
    #[error("a radius is out of range")]
    InvalidRadius,
    /// A rounding's smoothing is outside `[0, 1]`.
    #[error("a corner smoothing is outside [0, 1]")]
    InvalidSmoothing,
    /// A finite input made an intermediate or final value of the outline overflow.
    #[error("the outline overflows the finite range")]
    Overflow,
}

/// How one corner of a [`RoundedPolygon`] is rounded.
///
/// The corner is cut by a circular arc of `radius`, tangent to both edges. `smoothing` in
/// `[0, 1]` replaces part of that arc with a flanking curve on each side that eases from the
/// edge into the arc: at `0` the corner is a plain arc, at `1` no arc is left and the two
/// flanking curves meet. Smoothing lengthens the cut along each edge up to twice the plain
/// arc's. When two corners need more of an edge than it has, both shrink in proportion: the
/// arcs first keep their radius and give up smoothing, then the radii shrink too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerRounding {
    radius: f64,
    smoothing: f64,
}

impl CornerRounding {
    /// A sharp corner.
    pub const UNROUNDED: Self = Self {
        radius: 0.0,
        smoothing: 0.0,
    };

    /// A corner rounded by an arc of `radius` with `smoothing` in `[0, 1]`.
    ///
    /// # Errors
    ///
    /// [`ShapeError::InvalidRadius`] for a negative or non-finite radius and
    /// [`ShapeError::InvalidSmoothing`] for a smoothing outside `[0, 1]`.
    pub fn try_new(radius: f64, smoothing: f64) -> Result<Self, ShapeError> {
        if !(radius.is_finite() && radius >= 0.0) {
            return Err(ShapeError::InvalidRadius);
        }
        if !(0.0..=1.0).contains(&smoothing) {
            return Err(ShapeError::InvalidSmoothing);
        }
        Ok(Self { radius, smoothing })
    }

    /// A corner rounded by a plain circular arc of `radius`.
    ///
    /// # Errors
    ///
    /// [`ShapeError::InvalidRadius`] for a negative or non-finite radius.
    pub fn circular(radius: f64) -> Result<Self, ShapeError> {
        Self::try_new(radius, 0.0)
    }

    /// The radius of the corner's arc before any shrinking to fit.
    #[must_use]
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// The share of the corner given to the flanking curves, in `[0, 1]`.
    #[must_use]
    pub fn smoothing(&self) -> f64 {
        self.smoothing
    }
}

impl Default for CornerRounding {
    fn default() -> Self {
        Self::UNROUNDED
    }
}

/// A part of the outline: the curves rounding one corner, or the straight edge to the next.
#[derive(Clone, Debug, PartialEq)]
enum Feature {
    Corner { cubics: Vec<CubicBez>, convex: bool },
    Edge(CubicBez),
}

impl Feature {
    fn cubics(&self) -> &[CubicBez] {
        match self {
            Self::Corner { cubics, .. } => cubics,
            Self::Edge(cubic) => std::slice::from_ref(cubic),
        }
    }

    fn mapped(&self, map: &impl Fn(kurbo::Point) -> kurbo::Point) -> Self {
        match self {
            Self::Corner { cubics, convex } => Self::Corner {
                cubics: cubics.iter().map(|&c| map_cubic(c, map)).collect(),
                convex: *convex,
            },
            Self::Edge(cubic) => Self::Edge(map_cubic(*cubic, map)),
        }
    }
}

/// A closed outline of cubic Béziers: a polygon with rounded corners.
///
/// Built by [`try_new`](Self::try_new) from vertices and one rounding, by
/// [`with_roundings`](Self::with_roundings) with a rounding per vertex, or by the
/// [`star`](Self::star), [`circle`](Self::circle) and [`rectangle`](Self::rectangle) factories.
/// Every coordinate of the outline, and its extent, is finite. Draw it with
/// [`path`](Self::path); morph it into another with [`Morph`].
#[derive(Clone, Debug, PartialEq)]
pub struct RoundedPolygon {
    /// Corner, edge, corner, edge, … starting with the corner at the first vertex.
    features: Vec<Feature>,
    /// The drawn outline: the features' cubics with point-sized ones dropped, starting in the
    /// middle of the first corner's arc when it is rounded.
    outline: Vec<CubicBez>,
}

impl RoundedPolygon {
    /// The polygon through `vertices`, every corner rounded by `rounding`.
    ///
    /// # Errors
    ///
    /// [`ShapeError`] for fewer than three vertices, a non-finite vertex, a vertex equal to the
    /// next one, collinear vertices, or an outline that overflows.
    pub fn try_new(vertices: &[Point<f64>], rounding: CornerRounding) -> Result<Self, ShapeError> {
        Self::build(vertices, |_| rounding)
    }

    /// The polygon through `vertices`, corner `i` rounded by `roundings[i]`.
    ///
    /// # Errors
    ///
    /// [`ShapeError::RoundingCountMismatch`] unless there is one rounding per vertex, and the
    /// errors of [`try_new`](Self::try_new).
    pub fn with_roundings(
        vertices: &[Point<f64>],
        roundings: &[CornerRounding],
    ) -> Result<Self, ShapeError> {
        if roundings.len() != vertices.len() {
            return Err(ShapeError::RoundingCountMismatch {
                vertices: vertices.len(),
                roundings: roundings.len(),
            });
        }
        Self::build(vertices, |i| roundings[i])
    }

    /// A star around the origin with `points` outer vertices at `radius`, the first on +x, and
    /// as many inner vertices at `inner_radius` halfway between them.
    ///
    /// # Errors
    ///
    /// [`ShapeError::TooFewVertices`] for fewer than three points;
    /// [`ShapeError::InvalidRadius`] unless `0 < inner_radius < radius`, both finite; and the
    /// errors of [`try_new`](Self::try_new).
    pub fn star(
        points: usize,
        radius: f64,
        inner_radius: f64,
        rounding: CornerRounding,
        inner_rounding: CornerRounding,
    ) -> Result<Self, ShapeError> {
        if points < 3 {
            return Err(ShapeError::TooFewVertices { count: points });
        }
        if !(radius.is_finite() && inner_radius > 0.0 && inner_radius < radius) {
            return Err(ShapeError::InvalidRadius);
        }
        let count = points.checked_mul(2).ok_or(ShapeError::Overflow)?;
        let step = PI / points as f64;
        let vertices: Vec<_> = (0..count)
            .map(|i| {
                let r = if i % 2 == 0 { radius } else { inner_radius };
                polar(r, step * i as f64)
            })
            .collect();
        Self::build(&vertices, |i| {
            if i % 2 == 0 { rounding } else { inner_rounding }
        })
    }

    /// A circle of `radius` around the origin, drawn as `sides` arcs.
    ///
    /// It is a regular polygon whose corners are rounded until the arcs meet at the middle of
    /// each side, so a morph matches its `sides` corners with another shape's.
    ///
    /// # Errors
    ///
    /// [`ShapeError::TooFewVertices`] for fewer than three sides, [`ShapeError::InvalidRadius`]
    /// for a radius that is not positive and finite, and [`ShapeError::Overflow`].
    pub fn circle(radius: f64, sides: usize) -> Result<Self, ShapeError> {
        if sides < 3 {
            return Err(ShapeError::TooFewVertices { count: sides });
        }
        if !(radius.is_finite() && radius > 0.0) {
            return Err(ShapeError::InvalidRadius);
        }
        let step = TAU / sides as f64;
        let vertex_radius = radius / (PI / sides as f64).cos();
        if !vertex_radius.is_finite() {
            return Err(ShapeError::Overflow);
        }
        let vertices: Vec<_> = (0..sides)
            .map(|i| polar(vertex_radius, step * i as f64))
            .collect();
        Self::try_new(&vertices, CornerRounding::circular(radius)?)
    }

    /// The rectangle `rect` with every corner rounded by `rounding`; its first vertex is the
    /// corner at the largest x and y.
    ///
    /// # Errors
    ///
    /// [`ShapeError::ZeroArea`] for an empty rectangle, inverted ones included, and the errors
    /// of [`try_new`](Self::try_new), such as [`ShapeError::NonFiniteVertex`] for a non-finite one.
    pub fn rectangle(rect: Rect<f64>, rounding: CornerRounding) -> Result<Self, ShapeError> {
        let (left, top, right, bottom) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        let finite = [left, top, right, bottom].iter().all(|v| v.is_finite());
        if finite && !(right > left && bottom > top) {
            return Err(ShapeError::ZeroArea);
        }
        let vertices = [
            Point::new(right, bottom),
            Point::new(left, bottom),
            Point::new(left, top),
            Point::new(right, top),
        ];
        Self::try_new(&vertices, rounding)
    }

    /// The outline as a closed path.
    #[must_use]
    pub fn path(&self) -> Path {
        cubics_path(&self.outline)
    }

    /// The box around every anchor and control point of the corner and edge curves: never
    /// smaller than the outline, and what [`normalized`](Self::normalized) fits to the unit
    /// square.
    ///
    /// The curves are taken whole, before the drawn outline splits the first corner's arc to
    /// start in its middle, so the box does not depend on which vertex comes first: a regular
    /// shape gets a box as symmetric as the shape.
    #[must_use]
    pub fn bounds(&self) -> Rect<f64> {
        let mut points = self
            .features
            .iter()
            .flat_map(Feature::cubics)
            .flat_map(|c| [c.p0, c.p1, c.p2, c.p3]);
        let first = points.next().unwrap_or_default();
        let bounds = points.fold(kurbo::Rect::from_points(first, first), |b, p| b.union_pt(p));
        Rect::from_ltrb(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
    }

    /// This polygon scaled uniformly and moved so that [`bounds`](Self::bounds) fits the unit
    /// square: its longer side spans `[0, 1]` and the shorter one is centred.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let bounds = self.bounds();
        let (width, height) = (bounds.width(), bounds.height());
        let side = width.max(height);
        // Construction keeps the extent finite and positive, so this map is too. It divides
        // by the side rather than multiplying by its reciprocal, which a subnormal side would
        // make infinite.
        let (dx, dy) = ((side - width) / 2.0, (side - height) / 2.0);
        let (left, top) = (bounds.left(), bounds.top());
        self.mapped(|p| kurbo::Point::new((p.x - left + dx) / side, (p.y - top + dy) / side))
    }

    /// This polygon mapped through an orientation-preserving affine map.
    fn transformed(&self, affine: Affine) -> Self {
        debug_assert!(
            affine.determinant() > 0.0,
            "BUG: the map must keep orientation"
        );
        self.mapped(|p| affine * p)
    }

    /// This polygon with every point mapped through `map`, which must keep orientation.
    fn mapped(&self, map: impl Fn(kurbo::Point) -> kurbo::Point) -> Self {
        Self {
            features: self.features.iter().map(|f| f.mapped(&map)).collect(),
            outline: self.outline.iter().map(|&c| map_cubic(c, &map)).collect(),
        }
    }

    fn build(
        vertices: &[Point<f64>],
        rounding: impl Fn(usize) -> CornerRounding,
    ) -> Result<Self, ShapeError> {
        let n = vertices.len();
        if n < 3 {
            return Err(ShapeError::TooFewVertices { count: n });
        }
        if let Some(index) = vertices.iter().position(|v| !v.is_finite()) {
            return Err(ShapeError::NonFiniteVertex { index });
        }
        if let Some(index) = (0..n).position(|i| vertices[i] == vertices[(i + 1) % n]) {
            return Err(ShapeError::DuplicateVertex { index });
        }
        let points: Vec<kurbo::Point> = vertices
            .iter()
            .map(|v| kurbo::Point::new(v.x, v.y))
            .collect();
        let area = signed_area(&points)?;
        // The outline runs in the positive angular direction: a clockwise input is walked
        // backwards from its first vertex, carrying each vertex's rounding along.
        let order: Vec<usize> = if area > 0.0 {
            (0..n).collect()
        } else {
            std::iter::once(0).chain((1..n).rev()).collect()
        };
        let points: Vec<_> = order.iter().map(|&i| points[i]).collect();
        let corners = order
            .iter()
            .enumerate()
            .map(|(i, &source)| {
                Corner::try_new(
                    points[(i + n - 1) % n],
                    points[i],
                    points[(i + 1) % n],
                    rounding(source),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let extent = points
            .iter()
            .map(|&p| chebyshev(p - points[0]))
            .fold(0.0, f64::max);
        let tolerance = extent * RELATIVE_DISTANCE;

        // Each edge's budget: the two corners' arcs first, then their smoothing, both shrunk
        // in proportion when the edge is too short. Compared in halves, so two cuts that are
        // each finite cannot overflow their sum.
        let lengths = (0..n)
            .map(|i| length(points[(i + 1) % n] - points[i]))
            .collect::<Result<Vec<_>, _>>()?;
        let allowed: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let (a, b) = (&corners[i], &corners[(i + 1) % n]);
                let length = lengths[i] / 2.0;
                let round = a.round_cut / 2.0 + b.round_cut / 2.0;
                let full = a.cut / 2.0 + b.cut / 2.0;
                let (round_ratio, smooth_ratio) = if round > length {
                    (length / round, 0.0)
                } else if full > length {
                    (1.0, (length - round) / (full - round))
                } else {
                    (1.0, 1.0)
                };
                let share =
                    |c: &Corner| c.round_cut * round_ratio + (c.cut - c.round_cut) * smooth_ratio;
                (share(a), share(b))
            })
            .collect();

        let corner_cubics: Vec<Vec<CubicBez>> = (0..n)
            .map(|i| {
                let before = allowed[(i + n - 1) % n].1;
                let after = allowed[i].0;
                corners[i].cubics(before, after, tolerance)
            })
            .collect::<Result<_, _>>()?;

        let edges: Vec<CubicBez> = (0..n)
            .map(|i| {
                let (this, next) = (&corner_cubics[i], &corner_cubics[(i + 1) % n]);
                line(this[this.len() - 1].p3, next[0].p0)
            })
            .collect();
        let features: Vec<Feature> = corner_cubics
            .into_iter()
            .zip(&corners)
            .zip(edges)
            .flat_map(|((cubics, corner), edge)| {
                [
                    Feature::Corner {
                        cubics,
                        convex: corner.convex,
                    },
                    Feature::Edge(edge),
                ]
            })
            .collect();

        let outline = flatten(&features, tolerance);
        let finite = outline
            .iter()
            .all(|c| [c.p0, c.p1, c.p2, c.p3].iter().all(|p| p.is_finite()));
        if !finite {
            return Err(ShapeError::Overflow);
        }
        let polygon = Self { features, outline };
        let bounds = polygon.bounds();
        if !(bounds.width().is_finite() && bounds.height().is_finite()) {
            return Err(ShapeError::Overflow);
        }
        Ok(polygon)
    }
}

/// One vertex of the polygon with the cuts its rounding wants along each edge.
struct Corner {
    point: kurbo::Point,
    /// Unit direction toward the previous vertex.
    to_previous: Vec2,
    /// Unit direction toward the next vertex.
    to_next: Vec2,
    radius: f64,
    smoothing: f64,
    /// How far along each edge the plain arc reaches.
    round_cut: f64,
    /// How far along each edge the arc with its smoothing reaches.
    cut: f64,
    convex: bool,
}

impl Corner {
    fn try_new(
        previous: kurbo::Point,
        point: kurbo::Point,
        next: kurbo::Point,
        rounding: CornerRounding,
    ) -> Result<Self, ShapeError> {
        let to_previous = unit(previous - point)?;
        let to_next = unit(next - point)?;
        let cos = to_previous.dot(to_next).clamp(-1.0, 1.0);
        let sin = (1.0 - cos * cos).sqrt();
        // The rounding circle touches each edge `radius / tan(θ / 2)` from the vertex.
        let round_cut = if sin > STRAIGHT_SIN {
            rounding.radius * ((1.0 + cos) / sin)
        } else {
            0.0
        };
        let cut = (1.0 + rounding.smoothing) * round_cut;
        if !cut.is_finite() {
            return Err(ShapeError::Overflow);
        }
        Ok(Self {
            point,
            to_previous,
            to_next,
            radius: rounding.radius,
            smoothing: rounding.smoothing,
            round_cut,
            cut,
            // Turning left from the incoming edge to the outgoing one.
            convex: (-to_previous).cross(to_next) > 0.0,
        })
    }

    /// The corner's curves given how much of each edge it may use: one point-sized cubic for a
    /// sharp corner, otherwise the incoming flank, the arc and the outgoing flank.
    fn cubics(&self, before: f64, after: f64, tolerance: f64) -> Result<Vec<CubicBez>, ShapeError> {
        let allowed = before.min(after);
        if self.round_cut <= tolerance || allowed <= tolerance || self.radius <= tolerance {
            return Ok(vec![line(self.point, self.point)]);
        }
        // The tighter edge sets the arc; each edge keeps its own smoothing.
        let round_cut = allowed.min(self.round_cut);
        let radius = self.radius * (round_cut / self.round_cut);
        let bisector = unit((self.to_previous + self.to_next) / 2.0)?;
        let center = self.point + bisector * radius.hypot(round_cut);
        let smoothing = |allowed: f64| {
            if allowed > self.cut {
                self.smoothing
            } else if allowed > self.round_cut {
                self.smoothing * (allowed - self.round_cut) / (self.cut - self.round_cut)
            } else {
                0.0
            }
        };
        let flank = |along: Vec2, across: Vec2, smoothing: f64| {
            let tangent = self.point + along * round_cut;
            let other = self.point + across * round_cut;
            let start = self.point + along * (round_cut * (1.0 + smoothing));
            let toward = mix(tangent, mix(tangent, other, 0.5), smoothing);
            let end = center + unit(toward - center).unwrap_or(along) * radius;
            // The flank leaves the edge along it and meets the circle along its tangent.
            let control = intersect(start, along, end, (end - center).turn_90()).unwrap_or(tangent);
            CubicBez::new(start, mix(start, control, 2.0 / 3.0), control, end)
        };
        let incoming = flank(self.to_previous, self.to_next, smoothing(before));
        let outgoing = flank(self.to_next, self.to_previous, smoothing(after));
        let outgoing = CubicBez::new(outgoing.p3, outgoing.p2, outgoing.p1, outgoing.p0);
        let arc = circular_arc(center, incoming.p3, outgoing.p0);
        let cubics = vec![incoming, arc, outgoing];
        let finite = cubics
            .iter()
            .all(|c| [c.p0, c.p1, c.p2, c.p3].iter().all(|p| p.is_finite()));
        if finite {
            Ok(cubics)
        } else {
            Err(ShapeError::Overflow)
        }
    }
}

/// The drawn outline: rotated to start in the middle of the first corner's arc when it has
/// one, with point-sized cubics folded into their predecessor and the end closed exactly.
fn flatten(features: &[Feature], tolerance: f64) -> Vec<CubicBez> {
    let mut cubics: Vec<CubicBez> = features
        .iter()
        .flat_map(|f| f.cubics().iter().copied())
        .collect();
    if let [incoming, arc, _] = features[0].cubics() {
        let (first, second) = split(*arc, 0.5);
        cubics.drain(..2);
        cubics.insert(0, second);
        cubics.push(*incoming);
        cubics.push(first);
    }
    let mut outline: Vec<CubicBez> = Vec::with_capacity(cubics.len());
    for cubic in cubics {
        let point_sized = chebyshev(cubic.p3 - cubic.p0) <= tolerance;
        match outline.last_mut() {
            Some(last) if point_sized => last.p3 = cubic.p3,
            _ if point_sized => {}
            _ => outline.push(cubic),
        }
    }
    let start = outline[0].p0;
    let last = outline.len() - 1;
    outline[last].p3 = start;
    outline
}

/// A morph between two [`RoundedPolygon`]s.
///
/// Corners of the same convexity are matched nearest first, without crossings, and the two
/// outlines are cut into the same number of cubics around the matches, so a corner travels to
/// its partner rather than sliding along the outline. The shapes are compared where they
/// stand: give them a common frame (for instance both [`normalized`](RoundedPolygon::normalized))
/// for the matching to pair what looks alike.
#[derive(Clone, Debug, PartialEq)]
pub struct Morph {
    start: Path,
    end: Path,
}

impl Morph {
    /// The morph from `start` (`t = 0`) to `end` (`t = 1`).
    #[must_use]
    pub fn new(start: &RoundedPolygon, end: &RoundedPolygon) -> Self {
        let first = Measured::new(start);
        let second = Measured::new(end);
        let mapper = ProgressMap::new(&first, &second);
        let cut = mapper.map(0.0);
        let first_cubics = first.cubics;
        let second_cubics = second.cut_and_shift(cut);

        let mut pairs = Vec::with_capacity(first_cubics.len() + second_cubics.len());
        let (mut a_iter, mut b_iter) =
            (first_cubics.iter().copied(), second_cubics.iter().copied());
        let (mut a, mut b) = (a_iter.next(), b_iter.next());
        while let (Some(ca), Some(cb)) = (a, b) {
            let a_end = if a_iter.len() == 0 { 1.0 } else { ca.end };
            let b_end = if b_iter.len() == 0 {
                1.0
            } else {
                mapper.map_back((cb.end + cut).rem_euclid(1.0))
            };
            let at = a_end.min(b_end);
            let (piece_a, rest_a) = if a_end > at + SPLIT_EPSILON {
                let (head, tail) = ca.cut_at(at);
                (head, Some(tail))
            } else {
                (ca, a_iter.next())
            };
            let (piece_b, rest_b) = if b_end > at + SPLIT_EPSILON {
                let (head, tail) = cb.cut_at((mapper.map(at) - cut).rem_euclid(1.0));
                (head, Some(tail))
            } else {
                (cb, b_iter.next())
            };
            pairs.push((piece_a.cubic, piece_b.cubic));
            a = rest_a;
            b = rest_b;
        }
        let (starts, ends): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        Self {
            start: cubics_path(&closed(starts)),
            end: cubics_path(&closed(ends)),
        }
    }

    /// The shape at `t`: the start shape at `0`, the end shape at `1`, exactly.
    ///
    /// `t` may overshoot `[0, 1]`, as a springy curve does; a non-finite `t`, or an overshoot
    /// that would leave the finite range, gives `None`.
    #[must_use]
    pub fn path(&self, t: f64) -> Option<Path> {
        Path::lerp(&self.start, &self.end, t)
    }
}

/// A cubic with the share of its outline's perimeter it covers.
#[derive(Clone, Copy, Debug)]
struct MeasuredCubic {
    cubic: CubicBez,
    /// The same cubic in its outline's unit frame, where arc lengths are measured: the
    /// outline's perimeter can overflow although its coordinates and extent are finite.
    unit: CubicBez,
    start: f64,
    end: f64,
}

impl MeasuredCubic {
    /// Splits the cubic where the outline reaches `progress`, by arc length.
    fn cut_at(&self, progress: f64) -> (Self, Self) {
        let span = self.end - self.start;
        let share = if span > 0.0 {
            ((progress - self.start) / span).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let length = self.unit.arclen(ARCLEN_ACCURACY);
        let t = self.unit.inv_arclen(share * length, ARCLEN_ACCURACY);
        let progress = self.start + share * span;
        let (cubic_head, cubic_tail) = split(self.cubic, t);
        let (unit_head, unit_tail) = split(self.unit, t);
        (
            Self {
                cubic: cubic_head,
                unit: unit_head,
                start: self.start,
                end: progress,
            },
            Self {
                cubic: cubic_tail,
                unit: unit_tail,
                start: progress,
                end: self.end,
            },
        )
    }
}

/// A corner's place on the outline, for matching.
#[derive(Clone, Copy, Debug)]
struct CornerMark {
    progress: f64,
    convex: bool,
    point: kurbo::Point,
}

/// An outline measured by arc length, with its corners placed on it.
struct Measured {
    cubics: Vec<MeasuredCubic>,
    corners: Vec<CornerMark>,
}

impl Measured {
    fn new(polygon: &RoundedPolygon) -> Self {
        let raw: Vec<CubicBez> = polygon
            .features
            .iter()
            .flat_map(|f| f.cubics().iter().copied())
            .collect();
        // The unit frame: the bounds' corner at the origin, the longer side of length one.
        let bounds = polygon.bounds();
        let side = bounds.width().max(bounds.height());
        let (left, top) = (bounds.left(), bounds.top());
        let frame = |p: kurbo::Point| kurbo::Point::new((p.x - left) / side, (p.y - top) / side);
        let units: Vec<CubicBez> = raw.iter().map(|&c| map_cubic(c, &frame)).collect();
        let lengths: Vec<f64> = units.iter().map(|c| c.arclen(ARCLEN_ACCURACY)).collect();
        let total: f64 = lengths.iter().sum();
        let mut ends = Vec::with_capacity(raw.len());
        let mut sum = 0.0;
        for length in &lengths {
            sum += length;
            ends.push(sum / total);
        }
        let start_of = |i: usize| if i == 0 { 0.0 } else { ends[i - 1] };

        let mut corners = Vec::new();
        let mut index = 0;
        for feature in &polygon.features {
            let count = feature.cubics().len();
            if let Feature::Corner { cubics, convex } = feature {
                let middle = index + count / 2;
                let first = cubics[0].p0;
                let last = cubics[cubics.len() - 1].p3;
                corners.push(CornerMark {
                    progress: f64::midpoint(start_of(middle), ends[middle]),
                    convex: *convex,
                    point: mix(first, last, 0.5),
                });
            }
            index += count;
        }

        // Point-sized cubics are folded into a neighbour so the outline stays contiguous.
        let mut cubics: Vec<MeasuredCubic> = Vec::with_capacity(raw.len());
        let mut pending_start = None;
        for (i, (&cubic, &unit)) in raw.iter().zip(&units).enumerate() {
            let (start, end) = (start_of(i), ends[i]);
            if end - start <= RELATIVE_DISTANCE {
                match cubics.last_mut() {
                    Some(last) => {
                        last.cubic.p3 = cubic.p3;
                        last.unit.p3 = unit.p3;
                        last.end = end;
                    }
                    None => pending_start = pending_start.or(Some((cubic.p0, unit.p0))),
                }
                continue;
            }
            let (mut cubic, mut unit) = (cubic, unit);
            if let Some((point, unit_point)) = pending_start.take() {
                cubic.p0 = point;
                unit.p0 = unit_point;
            }
            cubics.push(MeasuredCubic {
                cubic,
                unit,
                start: cubics.last().map_or(0.0, |c| c.end),
                end,
            });
        }
        if let Some(last) = cubics.last_mut() {
            last.end = 1.0;
        }
        Self { cubics, corners }
    }

    /// The outline restarted at `progress`, its progress shifted to start from `0` there.
    fn cut_and_shift(&self, progress: f64) -> Vec<MeasuredCubic> {
        let shift = |c: MeasuredCubic| MeasuredCubic {
            start: (c.start - progress).rem_euclid(1.0),
            end: (c.end - progress).rem_euclid(1.0),
            ..c
        };
        if progress <= SPLIT_EPSILON || progress >= 1.0 - SPLIT_EPSILON {
            return self.cubics.clone();
        }
        let index = self
            .cubics
            .iter()
            .position(|c| c.end >= progress)
            .unwrap_or(self.cubics.len() - 1);
        let target = self.cubics[index];
        let (head, tail) = target.cut_at(progress);
        let mut shifted = Vec::with_capacity(self.cubics.len() + 1);
        let mut first = shift(tail);
        first.start = 0.0;
        shifted.push(first);
        shifted.extend(self.cubics[index + 1..].iter().copied().map(shift));
        shifted.extend(self.cubics[..index].iter().copied().map(shift));
        let mut last = shift(head);
        last.end = 1.0;
        shifted.push(last);
        shifted
    }
}

/// A piecewise-linear, cyclic map from one outline's progress to the other's through the
/// matched corners, and back.
struct ProgressMap {
    pairs: Vec<(f64, f64)>,
}

impl ProgressMap {
    fn new(first: &Measured, second: &Measured) -> Self {
        let mut candidates = Vec::new();
        for (i, a) in first.corners.iter().enumerate() {
            for (j, b) in second.corners.iter().enumerate() {
                if a.convex == b.convex {
                    candidates.push((length(a.point - b.point).unwrap_or(f64::INFINITY), i, j));
                }
            }
        }
        candidates.sort_by(|x, y| x.0.total_cmp(&y.0));

        let mut pairs: Vec<(f64, f64)> = Vec::new();
        let (mut used_first, mut used_second) = (
            vec![false; first.corners.len()],
            vec![false; second.corners.len()],
        );
        for (_, i, j) in candidates {
            if used_first[i] || used_second[j] {
                continue;
            }
            let (p, q) = (first.corners[i].progress, second.corners[j].progress);
            let at = pairs.partition_point(|&(x, _)| x < p);
            if let Some(&(x, _)) = pairs.get(at)
                && x == p
            {
                continue;
            }
            let n = pairs.len();
            if n >= 1 {
                let before = pairs[(at + n - 1) % n];
                let after = pairs[at % n];
                let crowded = progress_distance(p, before.0) < PROGRESS_EPSILON
                    || progress_distance(p, after.0) < PROGRESS_EPSILON
                    || progress_distance(q, before.1) < PROGRESS_EPSILON
                    || progress_distance(q, after.1) < PROGRESS_EPSILON;
                if crowded || (n > 1 && !progress_in_range(q, before.1, after.1)) {
                    continue;
                }
            }
            pairs.insert(at, (p, q));
            used_first[i] = true;
            used_second[j] = true;
        }
        match pairs.as_slice() {
            [] => pairs = vec![(0.0, 0.0), (0.5, 0.5)],
            &[(p, q)] => {
                let opposite = ((p + 0.5).rem_euclid(1.0), (q + 0.5).rem_euclid(1.0));
                pairs.push(opposite);
                pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
            }
            _ => {}
        }
        Self { pairs }
    }

    fn map(&self, x: f64) -> f64 {
        linear_map(self.pairs.iter().map(|&(a, b)| (a, b)), x)
    }

    fn map_back(&self, y: f64) -> f64 {
        let mut inverse: Vec<(f64, f64)> = self.pairs.iter().map(|&(a, b)| (b, a)).collect();
        // The targets are cyclically ordered; start the list at the smallest.
        let lowest = inverse
            .iter()
            .enumerate()
            .min_by(|x, y| x.1.0.total_cmp(&y.1.0))
            .map_or(0, |(i, _)| i);
        inverse.rotate_left(lowest);
        linear_map(inverse.into_iter(), y)
    }
}

/// Maps `x` through the cyclic piecewise-linear function given by `(from, to)` knots in
/// increasing cyclic order of `from`.
fn linear_map(knots: impl Iterator<Item = (f64, f64)>, x: f64) -> f64 {
    let knots: Vec<(f64, f64)> = knots.collect();
    let n = knots.len();
    let segment = (0..n)
        .find(|&i| progress_in_range(x, knots[i].0, knots[(i + 1) % n].0))
        .unwrap_or(0);
    let (from, to) = (knots[segment], knots[(segment + 1) % n]);
    let width = (to.0 - from.0).rem_euclid(1.0);
    let height = (to.1 - from.1).rem_euclid(1.0);
    let share = if width < 1e-3 {
        0.5
    } else {
        (x - from.0).rem_euclid(1.0) / width
    };
    (from.1 + height * share).rem_euclid(1.0)
}

fn progress_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs();
    d.min(1.0 - d)
}

/// Whether `p` lies on the cyclic interval from `from` to `to`.
fn progress_in_range(p: f64, from: f64, to: f64) -> bool {
    if to >= from {
        (from..=to).contains(&p)
    } else {
        p >= from || p <= to
    }
}

/// The cubics with the last one ending exactly where the first starts.
fn closed(mut cubics: Vec<CubicBez>) -> Vec<CubicBez> {
    let start = cubics[0].p0;
    let last = cubics.len() - 1;
    cubics[last].p3 = start;
    cubics
}

fn cubics_path(cubics: &[CubicBez]) -> Path {
    let point = |p: kurbo::Point| Point::new(p.x, p.y);
    let mut path = Path::new();
    path.move_to(point(cubics[0].p0));
    for c in cubics {
        path.cubic_to(point(c.p1), point(c.p2), point(c.p3));
    }
    path.close();
    path
}

fn polar(radius: f64, angle: f64) -> Point<f64> {
    let (sin, cos) = angle.sin_cos();
    Point::new(radius * cos, radius * sin)
}

fn line(from: kurbo::Point, to: kurbo::Point) -> CubicBez {
    CubicBez::new(from, mix(from, to, 1.0 / 3.0), mix(from, to, 2.0 / 3.0), to)
}

/// The point a share `t` in `[0, 1]` of the way from `a` to `b`, as a weighted sum: neither
/// term exceeds its endpoint, so points near the edges of the finite range cannot overflow
/// the way `a + (b − a) · t` can.
fn mix(a: kurbo::Point, b: kurbo::Point, t: f64) -> kurbo::Point {
    let s = 1.0 - t;
    kurbo::Point::new(a.x * s + b.x * t, a.y * s + b.y * t)
}

/// The two halves of `cubic` either side of `t`, by de Casteljau's construction from
/// [`mix`]es, so a cubic with finite points splits into cubics with finite points.
fn split(cubic: CubicBez, t: f64) -> (CubicBez, CubicBez) {
    let CubicBez { p0, p1, p2, p3 } = cubic;
    let (first, second, third) = (mix(p0, p1, t), mix(p1, p2, t), mix(p2, p3, t));
    let (head_control, tail_control) = (mix(first, second, t), mix(second, third, t));
    let middle = mix(head_control, tail_control, t);
    (
        CubicBez::new(p0, first, head_control, middle),
        CubicBez::new(middle, tail_control, third, p3),
    )
}

/// The unit vector along `v`, refusing a vector whose length overflows or vanishes.
fn unit(v: Vec2) -> Result<Vec2, ShapeError> {
    let scale = chebyshev(v);
    if !(scale.is_finite() && scale > 0.0) {
        return Err(ShapeError::Overflow);
    }
    let v = divided(v, scale);
    Ok(divided(v, v.hypot()))
}

/// The length of `v`, computed without overflowing on the way to a representable result.
fn length(v: Vec2) -> Result<f64, ShapeError> {
    let scale = chebyshev(v);
    let length = if scale > 0.0 {
        scale * divided(v, scale).hypot()
    } else {
        0.0
    };
    if length.is_finite() {
        Ok(length)
    } else {
        Err(ShapeError::Overflow)
    }
}

/// The larger of `v`'s two components in magnitude.
fn chebyshev(v: Vec2) -> f64 {
    v.x.abs().max(v.y.abs())
}

/// Twice the polygon's signed area, or that area relative to its extent squared, positive
/// when it turns from +x toward +y, or [`ShapeError::ZeroArea`] when its vertices are
/// collinear.
///
/// Measured as given while that sum is finite and nonzero: scaling first would underflow
/// a thin side of a huge polygon to zero. A sum that overflows, or underflows to zero for a
/// tiny polygon, is measured again on the polygon scaled to unit size, so every polygon with
/// finite coordinates and extent has an orientation.
fn signed_area(points: &[kurbo::Point]) -> Result<f64, ShapeError> {
    let origin = points[0];
    let offsets: Vec<Vec2> = points.iter().map(|&p| p - origin).collect();
    let extent = offsets
        .iter()
        .map(|d| d.x.abs().max(d.y.abs()))
        .fold(0.0, f64::max);
    if !extent.is_finite() {
        return Err(ShapeError::Overflow);
    }
    let area: f64 = offsets.windows(2).map(|pair| pair[0].cross(pair[1])).sum();
    if area.is_finite() && area != 0.0 {
        return Ok(area);
    }
    let area: f64 = offsets
        .windows(2)
        .map(|pair| divided(pair[0], extent).cross(divided(pair[1], extent)))
        .sum();
    // Only an exact zero: a thin polygon is still a polygon.
    if area == 0.0 {
        return Err(ShapeError::ZeroArea);
    }
    Ok(area)
}

/// `v / d` component by component: kurbo's division multiplies by the reciprocal, which a
/// subnormal divisor makes infinite.
fn divided(v: Vec2, d: f64) -> Vec2 {
    Vec2::new(v.x / d, v.y / d)
}

fn map_cubic(cubic: CubicBez, map: &impl Fn(kurbo::Point) -> kurbo::Point) -> CubicBez {
    CubicBez::new(map(cubic.p0), map(cubic.p1), map(cubic.p2), map(cubic.p3))
}

/// Where the line through `a` along `da` meets the line through `b` along `db`, unless they
/// are nearly parallel.
fn intersect(a: kurbo::Point, da: Vec2, b: kurbo::Point, db: Vec2) -> Option<kurbo::Point> {
    let db = unit(db).ok()?;
    let normal = db.turn_90();
    let den = da.dot(normal);
    if den.abs() < 1e-4 {
        return None;
    }
    let k = (b - a).dot(normal) / den;
    k.is_finite().then(|| a + da * k)
}

/// One cubic approximating the arc of the circle around `center` from `a` to `b`, the short
/// way round.
fn circular_arc(center: kurbo::Point, a: kurbo::Point, b: kurbo::Point) -> CubicBez {
    let (ra, rb) = (a - center, b - center);
    let (ua, ub) = (
        unit(ra).unwrap_or(Vec2::ZERO),
        unit(rb).unwrap_or(Vec2::ZERO),
    );
    let sweep = ua.cross(ub).atan2(ua.dot(ub));
    let k = length(ra).unwrap_or(f64::INFINITY) * (4.0 / 3.0) * (sweep / 4.0).tan();
    CubicBez::new(a, a + ua.turn_90() * k, b - ub.turn_90() * k, b)
}

/// The fixed Material 3 Expressive shapes, each fitted to the unit square.
///
/// Their parameters are Material's; the loading indicators morph between them.
#[expect(
    clippy::expect_used,
    reason = "fixed parameters, each shape built by the shapes contract table"
)]
pub mod material_shapes {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

    use flui_foundation::geometry::Point;
    use kurbo::{Affine, Vec2};

    use super::{CornerRounding, RoundedPolygon};

    fn rounding(radius: f64) -> CornerRounding {
        CornerRounding::circular(radius).expect("BUG: Material radii are valid")
    }

    /// A circle drawn as ten arcs.
    #[must_use]
    pub fn circle() -> RoundedPolygon {
        RoundedPolygon::circle(1.0, 10)
            .expect("BUG: a unit circle is valid")
            .normalized()
    }

    /// An ellipse at 64% aspect, tilted 45°.
    #[must_use]
    pub fn oval() -> RoundedPolygon {
        RoundedPolygon::circle(1.0, 8)
            .expect("BUG: a unit circle is valid")
            .transformed(Affine::rotate(-FRAC_PI_4) * Affine::scale_non_uniform(1.0, 0.64))
            .normalized()
    }

    /// An eight-pointed star with shallow, rounded points.
    #[must_use]
    pub fn sunny() -> RoundedPolygon {
        RoundedPolygon::star(8, 1.0, 0.8, rounding(0.15), rounding(0.15))
            .expect("BUG: Material's sunny star is valid")
            .normalized()
    }

    /// A nine-lobed cookie, a lobe pointing up.
    #[must_use]
    pub fn cookie_9() -> RoundedPolygon {
        RoundedPolygon::star(9, 1.0, 0.8, rounding(0.5), rounding(0.5))
            .expect("BUG: Material's nine-sided cookie is valid")
            .transformed(Affine::rotate(-FRAC_PI_2))
            .normalized()
    }

    /// A four-lobed cookie.
    #[must_use]
    pub fn cookie_4() -> RoundedPolygon {
        custom(&[(1.237, 1.236, 0.258), (0.5, 0.918, 0.233)], 4, false)
    }

    /// A rounded pentagon, a point up.
    #[must_use]
    pub fn pentagon() -> RoundedPolygon {
        custom(
            &[
                (0.5, -0.009, 0.172),
                (1.03, 0.365, 0.164),
                (0.828, 0.97, 0.169),
            ],
            1,
            true,
        )
    }

    /// A pill lying along a diagonal.
    #[must_use]
    pub fn pill() -> RoundedPolygon {
        custom(
            &[
                (0.961, 0.039, 0.426),
                (1.001, 0.428, 0.0),
                (1.0, 0.609, 1.0),
            ],
            2,
            true,
        )
    }

    /// A ten-pointed burst with softened points.
    #[must_use]
    pub fn soft_burst() -> RoundedPolygon {
        custom(&[(0.193, 0.277, 0.053), (0.176, 0.055, 0.053)], 10, false)
    }

    /// A polygon from `(x, y, radius)` design points around `(0.5, 0.5)`, repeated `reps` times
    /// around the centre and, when `mirrored`, reflected within each repetition.
    fn custom(points: &[(f64, f64, f64)], reps: usize, mirrored: bool) -> RoundedPolygon {
        let center = kurbo::Point::new(0.5, 0.5);
        let polar: Vec<(f64, f64, f64)> = points
            .iter()
            .map(|&(x, y, r)| {
                let v = Vec2::new(x, y) - center.to_vec2();
                (v.atan2(), v.hypot(), r)
            })
            .collect();
        let at = |angle: f64, distance: f64| {
            let (sin, cos) = angle.sin_cos();
            Point::new(center.x + distance * cos, center.y + distance * sin)
        };
        let mut vertices = Vec::new();
        let mut roundings = Vec::new();
        if mirrored {
            let sectors = 2 * reps;
            let sector = TAU / sectors as f64;
            let first_angle = polar[0].0;
            for k in 0..sectors {
                let base = sector * k as f64;
                if k % 2 == 0 {
                    for &(angle, distance, r) in &polar {
                        vertices.push(at(base + angle, distance));
                        roundings.push(rounding(r));
                    }
                } else {
                    for &(angle, distance, r) in polar[1..].iter().rev() {
                        vertices.push(at(base + sector - angle + 2.0 * first_angle, distance));
                        roundings.push(rounding(r));
                    }
                }
            }
        } else {
            for k in 0..reps {
                let turn = TAU * k as f64 / reps as f64;
                for &(angle, distance, r) in &polar {
                    vertices.push(at(angle + turn, distance));
                    roundings.push(rounding(r));
                }
            }
        }
        RoundedPolygon::with_roundings(&vertices, &roundings)
            .expect("BUG: Material's design points form a valid polygon")
            .normalized()
    }
}
