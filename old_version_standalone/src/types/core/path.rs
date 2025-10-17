//! Path types for vector graphics and curves.
//!
//! This module provides types for creating complex vector paths, including
//! lines, curves, and Bezier paths.

use super::point::Point;
use super::rect::Rect;

/// A segment in a vector path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSegment {
    /// Move to a point without drawing.
    MoveTo(Point),
    /// Draw a straight line to a point.
    LineTo(Point),
    /// Draw a quadratic Bezier curve (control point, end point).
    QuadraticTo(Point, Point),
    /// Draw a cubic Bezier curve (control1, control2, end point).
    CubicTo(Point, Point, Point),
    /// Close the current path (return to start).
    Close,
}

impl PathSegment {
    /// Get the end point of this segment (if applicable).
    pub fn end_point(&self) -> Option<Point> {
        match self {
            PathSegment::MoveTo(p) => Some(*p),
            PathSegment::LineTo(p) => Some(*p),
            PathSegment::QuadraticTo(_, end) => Some(*end),
            PathSegment::CubicTo(_, _, end) => Some(*end),
            PathSegment::Close => None,
        }
    }

    /// Check if this is a curve segment.
    pub fn is_curve(&self) -> bool {
        matches!(
            self,
            PathSegment::QuadraticTo(_, _) | PathSegment::CubicTo(_, _, _)
        )
    }
}

/// A vector path composed of multiple segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// The segments that make up this path.
    pub segments: Vec<PathSegment>,
}

impl Path {
    /// Create an empty path.
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Create a path with initial capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            segments: Vec::with_capacity(capacity),
        }
    }

    /// Move to a point without drawing.
    pub fn move_to(mut self, point: impl Into<Point>) -> Self {
        self.segments.push(PathSegment::MoveTo(point.into()));
        self
    }

    /// Draw a line to a point.
    pub fn line_to(mut self, point: impl Into<Point>) -> Self {
        self.segments.push(PathSegment::LineTo(point.into()));
        self
    }

    /// Draw a quadratic Bezier curve.
    pub fn quadratic_to(
        mut self,
        control: impl Into<Point>,
        end: impl Into<Point>,
    ) -> Self {
        self.segments.push(PathSegment::QuadraticTo(
            control.into(),
            end.into(),
        ));
        self
    }

    /// Draw a cubic Bezier curve.
    pub fn cubic_to(
        mut self,
        control1: impl Into<Point>,
        control2: impl Into<Point>,
        end: impl Into<Point>,
    ) -> Self {
        self.segments.push(PathSegment::CubicTo(
            control1.into(),
            control2.into(),
            end.into(),
        ));
        self
    }

    /// Close the path (return to start).
    pub fn close(mut self) -> Self {
        self.segments.push(PathSegment::Close);
        self
    }

    /// Get the number of segments in the path.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Check if the path is empty.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Check if the path is closed.
    pub fn is_closed(&self) -> bool {
        self.segments
            .last()
            .map_or(false, |s| matches!(s, PathSegment::Close))
    }

    /// Get an approximate bounding box of the path.
    pub fn bounding_box(&self) -> Option<Rect> {
        if self.segments.is_empty() {
            return None;
        }

        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for segment in &self.segments {
            if let Some(point) = segment.end_point() {
                min_x = min_x.min(point.x);
                min_y = min_y.min(point.y);
                max_x = max_x.max(point.x);
                max_y = max_y.max(point.y);
            }

            // For curves, also check control points
            match segment {
                PathSegment::QuadraticTo(ctrl, _) => {
                    min_x = min_x.min(ctrl.x);
                    min_y = min_y.min(ctrl.y);
                    max_x = max_x.max(ctrl.x);
                    max_y = max_y.max(ctrl.y);
                }
                PathSegment::CubicTo(ctrl1, ctrl2, _) => {
                    min_x = min_x.min(ctrl1.x).min(ctrl2.x);
                    min_y = min_y.min(ctrl1.y).min(ctrl2.y);
                    max_x = max_x.max(ctrl1.x).max(ctrl2.x);
                    max_y = max_y.max(ctrl1.y).max(ctrl2.y);
                }
                _ => {}
            }
        }

        if min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite() {
            Some(Rect::from_min_max(
                Point::new(min_x, min_y),
                Point::new(max_x, max_y),
            ))
        } else {
            None
        }
    }

    /// Create a rectangle path.
    pub fn rect(rect: impl Into<Rect>) -> Self {
        let rect = rect.into();
        Self::new()
            .move_to(rect.min)
            .line_to(Point::new(rect.max.x, rect.min.y))
            .line_to(rect.max)
            .line_to(Point::new(rect.min.x, rect.max.y))
            .close()
    }

    /// Create a circle path (approximated with cubic Bezier curves).
    pub fn circle(center: impl Into<Point>, radius: f32) -> Self {
        let center = center.into();
        // Magic constant for circle approximation with Bezier curves
        const KAPPA: f32 = 0.5522847498;
        let k = radius * KAPPA;

        let top = Point::new(center.x, center.y - radius);
        let right = Point::new(center.x + radius, center.y);
        let bottom = Point::new(center.x, center.y + radius);
        let left = Point::new(center.x - radius, center.y);

        Self::new()
            .move_to(top)
            .cubic_to(
                Point::new(center.x + k, center.y - radius),
                Point::new(center.x + radius, center.y - k),
                right,
            )
            .cubic_to(
                Point::new(center.x + radius, center.y + k),
                Point::new(center.x + k, center.y + radius),
                bottom,
            )
            .cubic_to(
                Point::new(center.x - k, center.y + radius),
                Point::new(center.x - radius, center.y + k),
                left,
            )
            .cubic_to(
                Point::new(center.x - radius, center.y - k),
                Point::new(center.x - k, center.y - radius),
                top,
            )
            .close()
    }
}

impl Default for Path {
    fn default() -> Self {
        Self::new()
    }
}

/// Cubic Bezier curve with two control points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    pub start: Point,
    pub control1: Point,
    pub control2: Point,
    pub end: Point,
}

impl CubicBezier {
    /// Create a new cubic Bezier curve.
    pub const fn new(start: Point, control1: Point, control2: Point, end: Point) -> Self {
        Self {
            start,
            control1,
            control2,
            end,
        }
    }

    /// Evaluate the curve at parameter t (0.0 to 1.0).
    pub fn at(&self, t: f32) -> Point {
        let t = t.clamp(0.0, 1.0);
        let one_minus_t = 1.0 - t;
        let one_minus_t_sq = one_minus_t * one_minus_t;
        let one_minus_t_cube = one_minus_t_sq * one_minus_t;
        let t_sq = t * t;
        let t_cube = t_sq * t;

        let x = one_minus_t_cube * self.start.x
            + 3.0 * one_minus_t_sq * t * self.control1.x
            + 3.0 * one_minus_t * t_sq * self.control2.x
            + t_cube * self.end.x;

        let y = one_minus_t_cube * self.start.y
            + 3.0 * one_minus_t_sq * t * self.control1.y
            + 3.0 * one_minus_t * t_sq * self.control2.y
            + t_cube * self.end.y;

        Point::new(x, y)
    }

    /// Get the tangent (velocity) at parameter t.
    pub fn tangent_at(&self, t: f32) -> Point {
        let t = t.clamp(0.0, 1.0);
        let one_minus_t = 1.0 - t;
        let one_minus_t_sq = one_minus_t * one_minus_t;
        let t_sq = t * t;

        let x = 3.0 * one_minus_t_sq * (self.control1.x - self.start.x)
            + 6.0 * one_minus_t * t * (self.control2.x - self.control1.x)
            + 3.0 * t_sq * (self.end.x - self.control2.x);

        let y = 3.0 * one_minus_t_sq * (self.control1.y - self.start.y)
            + 6.0 * one_minus_t * t * (self.control2.y - self.control1.y)
            + 3.0 * t_sq * (self.end.y - self.control2.y);

        Point::new(x, y)
    }

    /// Split the curve at parameter t into two curves.
    pub fn split_at(&self, t: f32) -> (CubicBezier, CubicBezier) {
        let t = t.clamp(0.0, 1.0);

        // De Casteljau's algorithm
        let p01 = Point::lerp(self.start, self.control1, t);
        let p12 = Point::lerp(self.control1, self.control2, t);
        let p23 = Point::lerp(self.control2, self.end, t);

        let p012 = Point::lerp(p01, p12, t);
        let p123 = Point::lerp(p12, p23, t);

        let p0123 = Point::lerp(p012, p123, t);

        let left = CubicBezier::new(self.start, p01, p012, p0123);
        let right = CubicBezier::new(p0123, p123, p23, self.end);

        (left, right)
    }

    /// Get approximate bounding box.
    pub fn bounding_box(&self) -> Rect {
        let mut min_x = self.start.x.min(self.end.x);
        let mut max_x = self.start.x.max(self.end.x);
        let mut min_y = self.start.y.min(self.end.y);
        let mut max_y = self.start.y.max(self.end.y);

        min_x = min_x.min(self.control1.x).min(self.control2.x);
        max_x = max_x.max(self.control1.x).max(self.control2.x);
        min_y = min_y.min(self.control1.y).min(self.control2.y);
        max_y = max_y.max(self.control1.y).max(self.control2.y);

        Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
    }
}

/// Quadratic Bezier curve with one control point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadraticBezier {
    pub start: Point,
    pub control: Point,
    pub end: Point,
}

impl QuadraticBezier {
    /// Create a new quadratic Bezier curve.
    pub const fn new(start: Point, control: Point, end: Point) -> Self {
        Self {
            start,
            control,
            end,
        }
    }

    /// Evaluate the curve at parameter t (0.0 to 1.0).
    pub fn at(&self, t: f32) -> Point {
        let t = t.clamp(0.0, 1.0);
        let one_minus_t = 1.0 - t;
        let one_minus_t_sq = one_minus_t * one_minus_t;
        let t_sq = t * t;

        let x = one_minus_t_sq * self.start.x
            + 2.0 * one_minus_t * t * self.control.x
            + t_sq * self.end.x;

        let y = one_minus_t_sq * self.start.y
            + 2.0 * one_minus_t * t * self.control.y
            + t_sq * self.end.y;

        Point::new(x, y)
    }

    /// Convert to cubic Bezier.
    pub fn to_cubic(&self) -> CubicBezier {
        // Control points for cubic: CP0 = P0 + 2/3 * (P1 - P0)
        //                           CP1 = P2 + 2/3 * (P1 - P2)
        let control1 = Point::new(
            self.start.x + 2.0 / 3.0 * (self.control.x - self.start.x),
            self.start.y + 2.0 / 3.0 * (self.control.y - self.start.y),
        );

        let control2 = Point::new(
            self.end.x + 2.0 / 3.0 * (self.control.x - self.end.x),
            self.end.y + 2.0 / 3.0 * (self.control.y - self.end.y),
        );

        CubicBezier::new(self.start, control1, control2, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_segment_end_point() {
        let p = Point::new(10.0, 20.0);
        assert_eq!(PathSegment::MoveTo(p).end_point(), Some(p));
        assert_eq!(PathSegment::LineTo(p).end_point(), Some(p));
        assert_eq!(PathSegment::Close.end_point(), None);
    }

    #[test]
    fn test_path_segment_is_curve() {
        assert!(!PathSegment::MoveTo(Point::ZERO).is_curve());
        assert!(!PathSegment::LineTo(Point::ZERO).is_curve());
        assert!(PathSegment::QuadraticTo(Point::ZERO, Point::ZERO).is_curve());
        assert!(PathSegment::CubicTo(Point::ZERO, Point::ZERO, Point::ZERO).is_curve());
    }

    #[test]
    fn test_path_builder() {
        let path = Path::new()
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .line_to(Point::new(10.0, 10.0))
            .close();

        assert_eq!(path.len(), 4);
        assert!(path.is_closed());
    }

    #[test]
    fn test_path_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let path = Path::rect(rect);

        assert_eq!(path.len(), 5); // 4 lines + close
        assert!(path.is_closed());
    }

    #[test]
    fn test_path_bounding_box() {
        let path = Path::new()
            .move_to(Point::new(10.0, 20.0))
            .line_to(Point::new(50.0, 30.0))
            .line_to(Point::new(30.0, 60.0));

        let bbox = path.bounding_box().unwrap();
        assert_eq!(bbox.min, Point::new(10.0, 20.0));
        assert_eq!(bbox.max, Point::new(50.0, 60.0));
    }

    #[test]
    fn test_cubic_bezier_at() {
        let curve = CubicBezier::new(
            Point::new(0.0, 0.0),
            Point::new(10.0, 20.0),
            Point::new(20.0, 20.0),
            Point::new(30.0, 0.0),
        );

        let start = curve.at(0.0);
        assert_eq!(start, Point::new(0.0, 0.0));

        let end = curve.at(1.0);
        assert_eq!(end, Point::new(30.0, 0.0));

        let mid = curve.at(0.5);
        assert!(mid.x > 10.0 && mid.x < 20.0);
        assert!(mid.y > 0.0);
    }

    #[test]
    fn test_cubic_bezier_split() {
        let curve = CubicBezier::new(
            Point::new(0.0, 0.0),
            Point::new(10.0, 20.0),
            Point::new(20.0, 20.0),
            Point::new(30.0, 0.0),
        );

        let (left, right) = curve.split_at(0.5);

        assert_eq!(left.start, curve.start);
        assert_eq!(right.end, curve.end);
        assert_eq!(left.end, right.start);
    }

    #[test]
    fn test_quadratic_bezier_at() {
        let curve = QuadraticBezier::new(
            Point::new(0.0, 0.0),
            Point::new(15.0, 20.0),
            Point::new(30.0, 0.0),
        );

        assert_eq!(curve.at(0.0), Point::new(0.0, 0.0));
        assert_eq!(curve.at(1.0), Point::new(30.0, 0.0));

        let mid = curve.at(0.5);
        assert_eq!(mid, Point::new(15.0, 10.0));
    }

    #[test]
    fn test_quadratic_to_cubic() {
        let quad = QuadraticBezier::new(
            Point::new(0.0, 0.0),
            Point::new(15.0, 20.0),
            Point::new(30.0, 0.0),
        );

        let cubic = quad.to_cubic();

        // Start and end should match
        assert_eq!(cubic.start, quad.start);
        assert_eq!(cubic.end, quad.end);

        // Curves should evaluate to similar points
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let quad_point = quad.at(t);
            let cubic_point = cubic.at(t);
            assert!((quad_point.x - cubic_point.x).abs() < 0.01);
            assert!((quad_point.y - cubic_point.y).abs() < 0.01);
        }
    }
}
