//! Rounded rectangle type

use crate::geometry::{Point, Rect, Size};
use crate::styling::Radius;

/// A rectangle with rounded corners
///
/// RRect represents a rectangle with independently configurable corner radii.
/// Each corner can have different x and y radii, allowing for elliptical corners.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{RRect, Rect};
/// use flui_types::styling::Radius;
///
/// // Create a rounded rectangle with uniform corner radius
/// let rrect = RRect::from_rect_and_radius(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Radius::circular(10.0)
/// );
///
/// // Create with different radii for each corner
/// let rrect = RRect::from_rect_and_corners(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Radius::circular(10.0),    // top-left
///     Radius::circular(20.0),    // top-right
///     Radius::elliptical(10.0, 5.0), // bottom-right
///     Radius::circular(5.0),     // bottom-left
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RRect {
    /// The base rectangle
    pub rect: Rect,
    /// Top-left corner radius
    pub top_left: Radius,
    /// Top-right corner radius
    pub top_right: Radius,
    /// Bottom-right corner radius
    pub bottom_right: Radius,
    /// Bottom-left corner radius
    pub bottom_left: Radius,
}

impl RRect {
    /// Creates a new RRect with explicit corner radii
    ///
    /// # Arguments
    ///
    /// * `rect` - The base rectangle
    /// * `top_left` - Top-left corner radius
    /// * `top_right` - Top-right corner radius
    /// * `bottom_right` - Bottom-right corner radius
    /// * `bottom_left` - Bottom-left corner radius
    #[inline]
    #[must_use]
    pub const fn new(
        rect: Rect,
        top_left: Radius,
        top_right: Radius,
        bottom_right: Radius,
        bottom_left: Radius,
    ) -> Self {
        Self {
            rect,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    /// Creates a rounded rectangle from a rect and uniform corner radius
    ///
    /// All corners will have the same radius.
    ///
    /// # Arguments
    ///
    /// * `rect` - The base rectangle
    /// * `radius` - The corner radius
    #[inline]
    #[must_use]
    pub const fn from_rect_and_radius(rect: Rect, radius: Radius) -> Self {
        Self::new(rect, radius, radius, radius, radius)
    }

    /// Creates a rounded rectangle with circular corners
    ///
    /// # Arguments
    ///
    /// * `rect` - The base rectangle
    /// * `radius` - The circular corner radius
    #[inline]
    #[must_use]
    pub const fn from_rect_circular(rect: Rect, radius: f32) -> Self {
        Self::from_rect_and_radius(rect, Radius::circular(radius))
    }

    /// Creates a rounded rectangle with elliptical corners
    ///
    /// # Arguments
    ///
    /// * `rect` - The base rectangle
    /// * `radius_x` - The x-axis radius for all corners
    /// * `radius_y` - The y-axis radius for all corners
    #[inline]
    #[must_use]
    pub const fn from_rect_elliptical(rect: Rect, radius_x: f32, radius_y: f32) -> Self {
        Self::from_rect_and_radius(rect, Radius::elliptical(radius_x, radius_y))
    }

    /// Creates a rounded rectangle from a rect with individual corner radii
    ///
    /// # Arguments
    ///
    /// * `rect` - The base rectangle
    /// * `top_left` - Top-left corner radius
    /// * `top_right` - Top-right corner radius
    /// * `bottom_right` - Bottom-right corner radius
    /// * `bottom_left` - Bottom-left corner radius
    #[inline]
    #[must_use]
    pub const fn from_rect_and_corners(
        rect: Rect,
        top_left: Radius,
        top_right: Radius,
        bottom_right: Radius,
        bottom_left: Radius,
    ) -> Self {
        Self::new(rect, top_left, top_right, bottom_right, bottom_left)
    }

    /// Creates a rounded rectangle from x, y, width, height, and uniform circular radius
    ///
    /// # Arguments
    ///
    /// * `x` - X coordinate
    /// * `y` - Y coordinate
    /// * `width` - Width
    /// * `height` - Height
    /// * `radius` - Circular corner radius
    #[inline]
    #[must_use]
    pub fn from_xywh_circular(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Self {
        Self::from_rect_circular(Rect::from_xywh(x, y, width, height), radius)
    }

    /// Creates a rectangle with no rounding (all radii are zero)
    #[inline]
    #[must_use]
    pub const fn from_rect(rect: Rect) -> Self {
        Self::from_rect_and_radius(rect, Radius::ZERO)
    }

    /// Returns true if all corner radii are zero (no rounding)
    #[inline]
    #[must_use]
    pub fn is_rect(&self) -> bool {
        self.top_left == Radius::ZERO
            && self.top_right == Radius::ZERO
            && self.bottom_right == Radius::ZERO
            && self.bottom_left == Radius::ZERO
    }

    /// Returns true if all corners have circular radii (x == y for each corner)
    #[inline]
    #[must_use]
    pub fn is_circular(&self) -> bool {
        self.top_left.is_circular()
            && self.top_right.is_circular()
            && self.bottom_right.is_circular()
            && self.bottom_left.is_circular()
    }

    /// Returns true if all corners have the same radius
    #[inline]
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.top_left == self.top_right
            && self.top_right == self.bottom_right
            && self.bottom_right == self.bottom_left
    }

    /// Returns the left edge
    #[inline]
    #[must_use]
    pub fn left(&self) -> f32 {
        self.rect.left()
    }

    /// Returns the top edge
    #[inline]
    #[must_use]
    pub fn top(&self) -> f32 {
        self.rect.top()
    }

    /// Returns the right edge
    #[inline]
    #[must_use]
    pub fn right(&self) -> f32 {
        self.rect.right()
    }

    /// Returns the bottom edge
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.rect.bottom()
    }

    /// Returns the width of the rounded rectangle
    #[inline]
    #[must_use]
    pub fn width(&self) -> f32 {
        self.rect.width()
    }

    /// Returns the height of the rounded rectangle
    #[inline]
    #[must_use]
    pub fn height(&self) -> f32 {
        self.rect.height()
    }

    /// Returns the size of the rounded rectangle
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size {
        self.rect.size()
    }

    /// Returns the center point of the rounded rectangle
    #[inline]
    #[must_use]
    pub fn center(&self) -> Point {
        self.rect.center()
    }

    /// Checks if a point is inside the rounded rectangle
    ///
    /// This performs proper geometric testing including the rounded corners.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        // First check if point is in the base rectangle
        if !self.rect.contains(point) {
            return false;
        }

        // If no rounding, we're done
        if self.is_rect() {
            return true;
        }

        let x = point.x;
        let y = point.y;

        // Check top-left corner
        if x < self.left() + self.top_left.x && y < self.top() + self.top_left.y {
            let dx = x - (self.left() + self.top_left.x);
            let dy = y - (self.top() + self.top_left.y);
            if dx * dx / (self.top_left.x * self.top_left.x)
                + dy * dy / (self.top_left.y * self.top_left.y)
                > 1.0
            {
                return false;
            }
        }

        // Check top-right corner
        if x > self.right() - self.top_right.x && y < self.top() + self.top_right.y {
            let dx = x - (self.right() - self.top_right.x);
            let dy = y - (self.top() + self.top_right.y);
            if dx * dx / (self.top_right.x * self.top_right.x)
                + dy * dy / (self.top_right.y * self.top_right.y)
                > 1.0
            {
                return false;
            }
        }

        // Check bottom-right corner
        if x > self.right() - self.bottom_right.x && y > self.bottom() - self.bottom_right.y {
            let dx = x - (self.right() - self.bottom_right.x);
            let dy = y - (self.bottom() - self.bottom_right.y);
            if dx * dx / (self.bottom_right.x * self.bottom_right.x)
                + dy * dy / (self.bottom_right.y * self.bottom_right.y)
                > 1.0
            {
                return false;
            }
        }

        // Check bottom-left corner
        if x < self.left() + self.bottom_left.x && y > self.bottom() - self.bottom_left.y {
            let dx = x - (self.left() + self.bottom_left.x);
            let dy = y - (self.bottom() - self.bottom_left.y);
            if dx * dx / (self.bottom_left.x * self.bottom_left.x)
                + dy * dy / (self.bottom_left.y * self.bottom_left.y)
                > 1.0
            {
                return false;
            }
        }

        true
    }

    /// Returns a new RRect with all corner radii scaled by the given factor
    #[inline]
    #[must_use]
    pub fn scale_radii(&self, factor: f32) -> Self {
        Self::new(
            self.rect,
            self.top_left.scale(factor),
            self.top_right.scale(factor),
            self.bottom_right.scale(factor),
            self.bottom_left.scale(factor),
        )
    }

    /// Returns a new RRect expanded by the given delta
    ///
    /// The rectangle is expanded and the corner radii remain the same.
    #[inline]
    #[must_use]
    pub fn expand(&self, delta: f32) -> Self {
        Self::new(
            self.rect.expand(delta),
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        )
    }

    /// Returns a new RRect shrunk by the given delta
    ///
    /// The rectangle is shrunk and the corner radii remain the same.
    #[inline]
    #[must_use]
    pub fn shrink(&self, delta: f32) -> Self {
        self.expand(-delta)
    }

    /// Linearly interpolate between two RRects
    ///
    /// # Arguments
    ///
    /// * `a` - Start RRect
    /// * `b` - End RRect
    /// * `t` - Interpolation factor (0.0 = a, 1.0 = b)
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        // Manually lerp the rect since Rect doesn't have lerp method
        let min = Point::lerp(a.rect.min, b.rect.min, t);
        let max = Point::lerp(a.rect.max, b.rect.max, t);
        let rect = Rect::from_min_max(min, max);

        Self::new(
            rect,
            Radius::lerp(a.top_left, b.top_left, t),
            Radius::lerp(a.top_right, b.top_right, t),
            Radius::lerp(a.bottom_right, b.bottom_right, t),
            Radius::lerp(a.bottom_left, b.bottom_left, t),
        )
    }

    // ===== Helper methods for rendering =====

    /// Returns the maximum corner radius in either dimension.
    ///
    /// Useful for determining buffer sizes for rendering.
    #[inline]
    #[must_use]
    pub fn max_radius(&self) -> f32 {
        let max_x = self
            .top_left
            .x
            .max(self.top_right.x)
            .max(self.bottom_right.x)
            .max(self.bottom_left.x);
        let max_y = self
            .top_left
            .y
            .max(self.top_right.y)
            .max(self.bottom_right.y)
            .max(self.bottom_left.y);
        max_x.max(max_y)
    }

    /// Returns true if any corner has a radius.
    #[inline]
    #[must_use]
    pub fn has_rounding(&self) -> bool {
        !self.is_rect()
    }

    /// Returns the four corner centers for rendering elliptical arcs.
    ///
    /// Returns (top-left, top-right, bottom-right, bottom-left) centers.
    #[inline]
    #[must_use]
    pub fn corner_centers(&self) -> [Point; 4] {
        [
            Point::new(self.left() + self.top_left.x, self.top() + self.top_left.y),
            Point::new(
                self.right() - self.top_right.x,
                self.top() + self.top_right.y,
            ),
            Point::new(
                self.right() - self.bottom_right.x,
                self.bottom() - self.bottom_right.y,
            ),
            Point::new(
                self.left() + self.bottom_left.x,
                self.bottom() - self.bottom_left.y,
            ),
        ]
    }

    /// Returns the bounding rectangle that encompasses all corners.
    #[inline]
    #[must_use]
    pub const fn bounding_rect(&self) -> Rect {
        self.rect
    }

    /// Returns a new RRect with radii clamped to the rectangle size.
    ///
    /// Ensures corner radii don't exceed half the width or height.
    #[must_use]
    pub fn clamp_radii(&self) -> Self {
        let max_x = self.width() / 2.0;
        let max_y = self.height() / 2.0;

        Self::new(
            self.rect,
            Radius::elliptical(self.top_left.x.min(max_x), self.top_left.y.min(max_y)),
            Radius::elliptical(self.top_right.x.min(max_x), self.top_right.y.min(max_y)),
            Radius::elliptical(
                self.bottom_right.x.min(max_x),
                self.bottom_right.y.min(max_y),
            ),
            Radius::elliptical(self.bottom_left.x.min(max_x), self.bottom_left.y.min(max_y)),
        )
    }

    /// Returns the area of the rounded rectangle (approximate).
    ///
    /// This is the rectangle area minus the corner cutouts.
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        if self.is_rect() {
            return self.width() * self.height();
        }

        // Rectangle area minus the corner cutouts
        let rect_area = self.width() * self.height();
        let corner_area = |r: Radius| {
            // Area of square minus quarter circle
            r.x * r.y * (1.0 - std::f32::consts::FRAC_PI_4)
        };

        rect_area
            - corner_area(self.top_left)
            - corner_area(self.top_right)
            - corner_area(self.bottom_right)
            - corner_area(self.bottom_left)
    }

    /// Returns true if this RRect is empty (zero or negative size).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rect.is_empty()
    }

    /// Translate (move) the rounded rectangle by an offset.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: crate::geometry::Offset) -> Self {
        Self::new(
            self.rect.translate(offset.dx, offset.dy),
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        )
    }
}

impl Default for RRect {
    fn default() -> Self {
        Self::from_rect(Rect::default())
    }
}

impl From<Rect> for RRect {
    fn from(rect: Rect) -> Self {
        Self::from_rect(rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrect_creation() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_circular(rect, 10.0);

        assert_eq!(rrect.rect, rect);
        assert_eq!(rrect.top_left, Radius::circular(10.0));
        assert_eq!(rrect.top_right, Radius::circular(10.0));
        assert_eq!(rrect.bottom_right, Radius::circular(10.0));
        assert_eq!(rrect.bottom_left, Radius::circular(10.0));
    }

    #[test]
    fn test_rrect_from_xywh() {
        let rrect = RRect::from_xywh_circular(10.0, 20.0, 100.0, 100.0, 15.0);

        assert_eq!(rrect.left(), 10.0);
        assert_eq!(rrect.top(), 20.0);
        assert_eq!(rrect.right(), 110.0);
        assert_eq!(rrect.bottom(), 120.0);
        assert_eq!(rrect.top_left.x, 15.0);
    }

    #[test]
    fn test_rrect_is_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect1 = RRect::from_rect(rect);
        let rrect2 = RRect::from_rect_circular(rect, 10.0);

        assert!(rrect1.is_rect());
        assert!(!rrect2.is_rect());
    }

    #[test]
    fn test_rrect_is_circular() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect1 = RRect::from_rect_circular(rect, 10.0);
        let rrect2 = RRect::from_rect_elliptical(rect, 10.0, 20.0);

        assert!(rrect1.is_circular());
        assert!(!rrect2.is_circular());
    }

    #[test]
    fn test_rrect_is_uniform() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect1 = RRect::from_rect_circular(rect, 10.0);
        let rrect2 = RRect::from_rect_and_corners(
            rect,
            Radius::circular(10.0),
            Radius::circular(20.0),
            Radius::circular(10.0),
            Radius::circular(10.0),
        );

        assert!(rrect1.is_uniform());
        assert!(!rrect2.is_uniform());
    }

    #[test]
    fn test_rrect_dimensions() {
        let rrect = RRect::from_xywh_circular(10.0, 20.0, 100.0, 100.0, 15.0);

        assert_eq!(rrect.width(), 100.0);
        assert_eq!(rrect.height(), 100.0);
        assert_eq!(rrect.size(), Size::new(100.0, 100.0));
        assert_eq!(rrect.center(), Point::new(60.0, 70.0));
    }

    #[test]
    fn test_rrect_contains_simple() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect(rect); // No rounding

        assert!(rrect.contains(Point::new(50.0, 50.0)));
        assert!(rrect.contains(Point::new(0.0, 0.0)));
        assert!(rrect.contains(Point::new(100.0, 100.0)));
        assert!(!rrect.contains(Point::new(-1.0, 50.0)));
        assert!(!rrect.contains(Point::new(101.0, 50.0)));
    }

    #[test]
    fn test_rrect_contains_rounded() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_circular(rect, 10.0);

        // Center should always be inside
        assert!(rrect.contains(Point::new(50.0, 50.0)));

        // Points on straight edges should be inside
        assert!(rrect.contains(Point::new(50.0, 0.0)));
        assert!(rrect.contains(Point::new(50.0, 100.0)));

        // Corner at (0,0) has radius 10, so (0,0) should be outside
        assert!(!rrect.contains(Point::new(0.0, 0.0)));

        // But (10, 10) should be inside (center of corner arc)
        assert!(rrect.contains(Point::new(10.0, 10.0)));
    }

    #[test]
    fn test_rrect_scale_radii() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_circular(rect, 10.0);
        let scaled = rrect.scale_radii(2.0);

        assert_eq!(scaled.top_left, Radius::circular(20.0));
        assert_eq!(scaled.top_right, Radius::circular(20.0));
        assert_eq!(scaled.rect, rect); // Rect unchanged
    }

    #[test]
    fn test_rrect_expand_shrink() {
        let rect = Rect::from_xywh(10.0, 10.0, 80.0, 80.0);
        let rrect = RRect::from_rect_circular(rect, 5.0);

        let expanded = rrect.expand(10.0);
        assert_eq!(expanded.left(), 0.0);
        assert_eq!(expanded.top(), 0.0);
        assert_eq!(expanded.right(), 100.0);
        assert_eq!(expanded.bottom(), 100.0);

        let shrunk = rrect.shrink(5.0);
        assert_eq!(shrunk.left(), 15.0);
        assert_eq!(shrunk.top(), 15.0);
        assert_eq!(shrunk.right(), 85.0);
        assert_eq!(shrunk.bottom(), 85.0);
    }

    #[test]
    fn test_rrect_lerp() {
        let rect1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rect2 = Rect::from_xywh(0.0, 0.0, 200.0, 200.0);
        let rrect1 = RRect::from_rect_circular(rect1, 10.0);
        let rrect2 = RRect::from_rect_circular(rect2, 20.0);

        let mid = RRect::lerp(rrect1, rrect2, 0.5);

        assert_eq!(mid.width(), 150.0);
        assert_eq!(mid.height(), 150.0);
        assert_eq!(mid.top_left.x, 15.0);
    }

    #[test]
    fn test_rrect_default() {
        let rrect = RRect::default();
        assert_eq!(rrect.rect, Rect::default());
        assert!(rrect.is_rect());
    }

    #[test]
    fn test_rrect_from_rect_conversion() {
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 100.0);
        let rrect: RRect = rect.into();

        assert_eq!(rrect.rect, rect);
        assert!(rrect.is_rect());
    }

    #[test]
    fn test_rrect_individual_corners() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_and_corners(
            rect,
            Radius::circular(5.0),  // top-left
            Radius::circular(10.0), // top-right
            Radius::circular(15.0), // bottom-right
            Radius::circular(20.0), // bottom-left
        );

        assert_eq!(rrect.top_left, Radius::circular(5.0));
        assert_eq!(rrect.top_right, Radius::circular(10.0));
        assert_eq!(rrect.bottom_right, Radius::circular(15.0));
        assert_eq!(rrect.bottom_left, Radius::circular(20.0));
        assert!(!rrect.is_uniform());
    }

    #[test]
    fn test_rrect_elliptical_corners() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_elliptical(rect, 20.0, 10.0);

        assert_eq!(rrect.top_left, Radius::elliptical(20.0, 10.0));
        assert!(!rrect.is_circular());
        assert!(rrect.is_uniform());
    }
}
