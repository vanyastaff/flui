//! Path types for vector drawing.
//!
//! Provides Path structure for creating complex shapes with lines, curves, and arcs.

use crate::geometry::{px, NumericUnit, Offset, Pixels, Point, Rect, Vec2};
use crate::painting::PathFillType;
use smallvec::SmallVec;

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
    /// Arguments: bounding rectangle, start angle (radians), sweep angle (radians)
    AddArc(Rect<Pixels>, f32, f32),
}

#[derive(Clone, Debug)]
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: SmallVec::new(),
            fill_type: PathFillType::default(),
            bounds: None,
        }
    }

    #[must_use]
    pub fn with_fill_type(fill_type: PathFillType) -> Self {
        Self {
            commands: SmallVec::new(),
            fill_type,
            bounds: None,
        }
    }

    #[must_use]
    pub fn rectangle(rect: Rect<Pixels>) -> Self {
        let mut path = Self::new();
        path.add_rect(rect);
        path
    }

    #[must_use]
    pub fn oval(rect: Rect<Pixels>) -> Self {
        let mut path = Self::new();
        path.add_oval(rect);
        path
    }

    #[must_use]
    pub fn arc(rect: Rect<Pixels>, start_angle: f32, sweep_angle: f32) -> Self {
        let mut path = Self::new();
        path.add_arc(rect, start_angle, sweep_angle);
        path
    }

    #[must_use]
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
                rect.right() - tr_x * px(2.0),
                rect.top(),
                tr_x * px(2.0),
                tr_y * px(2.0),
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
                rect.right() - br_x * px(2.0),
                rect.bottom() - br_y * px(2.0),
                br_x * px(2.0),
                br_y * px(2.0),
            );
            path.add_arc(corner_rect, 0.0, std::f32::consts::FRAC_PI_2);
        }

        // Bottom edge
        path.line_to(Point::new(rect.left() + bl_x, rect.bottom()));

        // Bottom-left corner
        if bl_x > px(0.0) || bl_y > px(0.0) {
            let corner_rect = Rect::from_xywh(
                rect.left(),
                rect.bottom() - bl_y * px(2.0),
                bl_x * px(2.0),
                bl_y * px(2.0),
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
            let corner_rect =
                Rect::from_xywh(rect.left(), rect.top(), tl_x * px(2.0), tl_y * px(2.0));
            path.add_arc(
                corner_rect,
                std::f32::consts::PI,
                std::f32::consts::FRAC_PI_2,
            );
        }

        path.close();
        path
    }

    #[inline]
    pub fn set_fill_type(&mut self, fill_type: PathFillType) {
        self.fill_type = fill_type;
    }

    #[must_use]
    pub const fn fill_type(&self) -> PathFillType {
        self.fill_type
    }

    #[inline]
    pub fn move_to(&mut self, point: Point<Pixels>) {
        self.commands.push(PathCommand::MoveTo(point));
        self.bounds = None;
    }

    #[inline]
    pub fn line_to(&mut self, point: Point<Pixels>) {
        self.commands.push(PathCommand::LineTo(point));
        self.bounds = None;
    }

    #[inline]
    pub fn close(&mut self) {
        self.commands.push(PathCommand::Close);
    }

    #[inline]
    pub fn add_rect(&mut self, rect: Rect<Pixels>) {
        self.commands.push(PathCommand::AddRect(rect));
        self.bounds = None;
    }

    #[inline]
    pub fn add_oval(&mut self, rect: Rect<Pixels>) {
        self.commands.push(PathCommand::AddOval(rect));
        self.bounds = None;
    }

    #[inline]
    pub fn add_arc(&mut self, rect: Rect<Pixels>, start_angle: f32, sweep_angle: f32) {
        self.commands
            .push(PathCommand::AddArc(rect, start_angle, sweep_angle));
        self.bounds = None;
    }

    #[must_use]
    pub fn commands(&self) -> &[PathCommand] {
        &self.commands
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    #[inline]
    pub fn reset(&mut self) {
        self.commands.clear();
        self.bounds = None;
    }

    #[must_use]
    pub fn cached_bounds(&self) -> Option<Rect<Pixels>> {
        self.bounds
    }

    #[must_use]
    pub fn compute_bounds(&self) -> Rect<Pixels> {
        // Quick return if cached
        if let Some(bounds) = self.bounds {
            return bounds;
        }

        self.compute_bounds_internal()
    }

    /// Internal bounds computation (shared between bounds() and compute_bounds())
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
                PathCommand::AddRect(r)
                | PathCommand::AddOval(r)
                | PathCommand::AddArc(r, _, _) => {
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

    #[must_use]
    pub fn bounds(&mut self) -> Rect<Pixels> {
        if let Some(bounds) = self.bounds {
            return bounds;
        }

        let bounds = self.compute_bounds_internal();
        self.bounds = Some(bounds);
        bounds
    }

    #[must_use]
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
    fn contains_even_odd(&self, point: Point<Pixels>) -> bool {
        let mut crossings = 0;
        let mut current_pos = Point::new(px(0.0), px(0.0));
        let mut subpath_start = Point::new(px(0.0), px(0.0));

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
                PathCommand::AddArc(_, _, _) => {
                    // TODO: Implement arc containment
                    // For now, skip arcs (conservative - may miss some points)
                }
            }
        }

        crossings % 2 == 1
    }

    /// Winding number algorithm for non-zero fill rule.
    fn contains_non_zero(&self, point: Point<Pixels>) -> bool {
        let mut winding = 0;
        let mut current_pos = Point::new(px(0.0), px(0.0));
        let mut subpath_start = Point::new(px(0.0), px(0.0));

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
                PathCommand::AddArc(_, _, _) => {
                    // TODO: Implement arc winding
                }
            }
        }

        winding != 0
    }

    /// Tests if a horizontal ray from point intersects a line segment.
    fn ray_intersects_segment(
        &self,
        point: Point<Pixels>,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
    ) -> bool {
        // Ray extends to the right from point
        if (p1.y > point.y) == (p2.y > point.y) {
            return false; // Both endpoints on same side of ray
        }

        // Calculate x coordinate of intersection
        let x_intersect = p1.x + (point.y - p1.y) / (p2.y - p1.y) * (p2.x - p1.x);
        x_intersect > point.x
    }

    /// Compute winding contribution of a line segment.
    fn segment_winding(&self, point: Point<Pixels>, p1: Point<Pixels>, p2: Point<Pixels>) -> i32 {
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
    fn is_left(&self, p1: Point<Pixels>, p2: Point<Pixels>, point: Point<Pixels>) -> f32 {
        ((p2.x - p1.x) * (point.y - p1.y) - (point.x - p1.x) * (p2.y - p1.y)).0
    }

    /// Count crossings for quadratic bezier curve (approximated).
    fn count_curve_crossings_quad(
        &self,
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

            let start = self.eval_cubic(p0, p1, p2, p3, t1);
            let end = self.eval_cubic(p0, p1, p2, p3, t2);

            if self.ray_intersects_segment(point, start, end) {
                crossings += 1;
            }
        }

        crossings
    }

    /// Winding number for quadratic curve.
    fn curve_winding_quad(
        &self,
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

            let start = self.eval_quadratic(p0, p1, p2, t1);
            let end = self.eval_quadratic(p0, p1, p2, t2);

            winding += self.segment_winding(point, start, end);
        }

        winding
    }

    /// Winding number for cubic curve.
    fn curve_winding_cubic(
        &self,
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

            let start = self.eval_cubic(p0, p1, p2, p3, t1);
            let end = self.eval_cubic(p0, p1, p2, p3, t2);

            winding += self.segment_winding(point, start, end);
        }

        winding
    }

    /// Evaluate quadratic bezier at parameter t.
    fn eval_quadratic<T>(&self, p0: Point<T>, p1: Point<T>, p2: Point<T>, t: f32) -> Point<T>
    where
        T: NumericUnit + Into<f32> + From<f32>,
    {
        let t2 = t * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;

        Point::new(
            T::from(mt2 * p0.x.into() + 2.0 * mt * t * p1.x.into() + t2 * p2.x.into()),
            T::from(mt2 * p0.y.into() + 2.0 * mt * t * p1.y.into() + t2 * p2.y.into()),
        )
    }

    /// Evaluate cubic bezier at parameter t.
    fn eval_cubic<T>(
        &self,
        p0: Point<T>,
        p1: Point<T>,
        p2: Point<T>,
        p3: Point<T>,
        t: f32,
    ) -> Point<T>
    where
        T: NumericUnit + Into<f32> + From<f32>,
    {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        Point::new(
            T::from(
                mt3 * p0.x.into()
                    + 3.0 * mt2 * t * p1.x.into()
                    + 3.0 * mt * t2 * p2.x.into()
                    + t3 * p3.x.into(),
            ),
            T::from(
                mt3 * p0.y.into()
                    + 3.0 * mt2 * t * p1.y.into()
                    + 3.0 * mt * t2 * p2.y.into()
                    + t3 * p3.y.into(),
            ),
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
    use crate::geometry::px;

    // Path containment tests
}
