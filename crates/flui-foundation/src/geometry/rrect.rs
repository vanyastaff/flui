//! Rounded rectangle type.
//!
//! API design inspired by Flutter and kurbo.

use super::{
    Point, Rect, Size,
    traits::{NumericUnit, Unit},
};

/// A radius value with separate horizontal and vertical components.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Radius<T: Unit = f64> {
    /// Horizontal radius.
    pub x: T,
    /// Vertical radius.
    pub y: T,
}

// ============================================================================
// Constants (generic over Unit)
// ============================================================================

impl<T: Unit> Radius<T> {
    /// Creates a zero radius.
    #[inline]
    pub fn zero() -> Self {
        Self {
            x: T::zero(),
            y: T::zero(),
        }
    }
}

// ============================================================================
// Logical-pixel (`f64`) constants
// ============================================================================

impl Radius<f64> {
    /// A zero radius constant.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Radius<T> {
    /// Creates a radius with separate horizontal and vertical values.
    #[inline]
    #[must_use]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    /// Creates a circular radius (same horizontal and vertical).
    #[inline]
    #[must_use]
    pub const fn circular(r: T) -> Self {
        Self::new(r, r)
    }

    /// Creates an elliptical radius with separate horizontal and vertical
    /// values.
    #[inline]
    #[must_use]
    pub const fn elliptical(x: T, y: T) -> Self {
        Self::new(x, y)
    }

    /// Checks if this radius is zero.
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.x == T::zero() && self.y == T::zero()
    }

    /// Checks if this radius is circular (x equals y).
    #[inline]
    #[must_use]
    pub fn is_circular(&self) -> bool {
        self.x == self.y
    }
}

// ============================================================================
// Numeric Unit Operations
// ============================================================================

impl<T: NumericUnit> Radius<T>
where
    T: std::ops::Mul<f64, Output = T>,
{
    /// Scales this radius by a factor.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }

    /// Linearly interpolates between two radii.
    #[inline]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        Self::new(a.x * (1.0 - t) + b.x * t, a.y * (1.0 - t) + b.y * t)
    }
}

impl<T: NumericUnit + PartialOrd> Radius<T> {
    /// Clamps this radius to maximum values.
    #[inline]
    #[must_use]
    pub fn clamp(&self, max_x: T, max_y: T) -> Self {
        Self::new(
            if self.x > max_x { max_x } else { self.x },
            if self.y > max_y { max_y } else { self.y },
        )
    }
}

// ============================================================================
// Default Implementation
// ============================================================================

impl<T: Unit> Default for Radius<T> {
    fn default() -> Self {
        Self::new(T::zero(), T::zero())
    }
}

/// A rounded rectangle with independent corner radii.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RRect {
    /// The base rectangle.
    pub rect: Rect<f64>,
    /// Top-left corner radius.
    pub top_left: Radius<f64>,
    /// Top-right corner radius.
    pub top_right: Radius<f64>,
    /// Bottom-right corner radius.
    pub bottom_right: Radius<f64>,
    /// Bottom-left corner radius.
    pub bottom_left: Radius<f64>,
}

// ============================================================================
// Constructors
// ============================================================================

impl RRect {
    /// Creates a rounded rectangle with independent corner radii.
    #[inline]
    #[must_use]
    pub const fn new(
        rect: Rect<f64>,
        top_left: Radius<f64>,
        top_right: Radius<f64>,
        bottom_right: Radius<f64>,
        bottom_left: Radius<f64>,
    ) -> Self {
        Self {
            rect,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    /// Creates a rounded rectangle with the same radius for all corners.
    #[inline]
    #[must_use]
    pub const fn from_rect_and_radius(rect: Rect<f64>, radius: Radius<f64>) -> Self {
        Self::new(rect, radius, radius, radius, radius)
    }

    /// Creates a rounded rectangle with a circular radius for all corners.
    #[inline]
    #[must_use]
    pub const fn from_rect_circular(rect: Rect<f64>, radius: f64) -> Self {
        Self::from_rect_and_radius(rect, Radius::circular(radius))
    }

    /// Creates a rounded rectangle with an elliptical radius for all corners.
    #[inline]
    #[must_use]
    pub const fn from_rect_elliptical(rect: Rect<f64>, radius_x: f64, radius_y: f64) -> Self {
        Self::from_rect_and_radius(rect, Radius::elliptical(radius_x, radius_y))
    }

    /// Creates a rounded rectangle with separate x and y radii for all corners.
    #[inline]
    #[must_use]
    pub const fn from_rect_xy(rect: Rect<f64>, radius_x: f64, radius_y: f64) -> Self {
        Self::from_rect_elliptical(rect, radius_x, radius_y)
    }

    /// Creates a rounded rectangle with independent corner radii.
    #[inline]
    #[must_use]
    pub const fn from_rect_and_corners(
        rect: Rect<f64>,
        top_left: Radius<f64>,
        top_right: Radius<f64>,
        bottom_right: Radius<f64>,
        bottom_left: Radius<f64>,
    ) -> Self {
        Self::new(rect, top_left, top_right, bottom_right, bottom_left)
    }

    /// Creates a rounded rectangle from position, size, and circular radius.
    #[inline]
    #[must_use]
    pub fn from_xywh_circular(x: f64, y: f64, width: f64, height: f64, radius: f64) -> Self {
        Self::from_rect_circular(Rect::from_xywh(x, y, width, height), radius)
    }

    /// Creates a rounded rectangle from a plain rectangle (no rounding).
    #[inline]
    #[must_use]
    pub const fn from_rect(rect: Rect<f64>) -> Self {
        Self::from_rect_and_radius(rect, Radius::ZERO)
    }
}

// ============================================================================
// Accessors
// ============================================================================

impl RRect {
    /// Returns the left edge x-coordinate.
    #[inline]
    #[must_use]
    pub fn left(&self) -> f64 {
        self.rect.left()
    }

    /// Returns the top edge y-coordinate.
    #[inline]
    #[must_use]
    pub fn top(&self) -> f64 {
        self.rect.top()
    }

    /// Returns the right edge x-coordinate.
    #[inline]
    #[must_use]
    pub fn right(&self) -> f64 {
        self.rect.right()
    }

    /// Returns the bottom edge y-coordinate.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> f64 {
        self.rect.bottom()
    }

    /// Returns the width of the rectangle.
    #[inline]
    #[must_use]
    pub fn width(&self) -> f64 {
        self.rect.width()
    }

    /// Returns the height of the rectangle.
    #[inline]
    #[must_use]
    pub fn height(&self) -> f64 {
        self.rect.height()
    }

    /// Returns the size of the rectangle.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size<f64> {
        self.rect.size()
    }

    /// Returns the center point of the rectangle.
    #[inline]
    #[must_use]
    pub fn center(&self) -> Point<f64> {
        self.rect.center()
    }

    /// Returns the bounding rectangle (without rounded corners).
    #[inline]
    #[must_use]
    pub const fn bounding_rect(&self) -> Rect<f64> {
        self.rect
    }
}

// ============================================================================
// Queries
// ============================================================================

impl RRect {
    /// Checks if this is a plain rectangle (all radii are zero).
    #[inline]
    #[must_use]
    pub fn is_rect(&self) -> bool {
        self.top_left.is_zero()
            && self.top_right.is_zero()
            && self.bottom_right.is_zero()
            && self.bottom_left.is_zero()
    }

    /// Checks if all corners have circular radii.
    #[inline]
    #[must_use]
    pub fn is_circular(&self) -> bool {
        self.top_left.is_circular()
            && self.top_right.is_circular()
            && self.bottom_right.is_circular()
            && self.bottom_left.is_circular()
    }

    /// Checks if all corners have the same radius.
    #[inline]
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.top_left == self.top_right
            && self.top_right == self.bottom_right
            && self.bottom_right == self.bottom_left
    }

    /// Checks if this has any rounding (opposite of is_rect).
    #[inline]
    #[must_use]
    pub fn has_rounding(&self) -> bool {
        !self.is_rect()
    }

    /// Checks if the rectangle is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rect.is_empty()
    }

    /// Returns the maximum radius value across all corners.
    #[inline]
    #[must_use]
    pub fn max_radius(&self) -> f64 {
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

    /// Computes the area of the rounded rectangle.
    #[inline]
    #[must_use]
    pub fn area(&self) -> f64 {
        if self.is_rect() {
            return self.rect.area();
        }

        let rect_area = self.rect.area();
        let corner_cutout =
            |r: Radius<f64>| -> f64 { r.x * r.y * (1.0 - std::f64::consts::FRAC_PI_4) };

        rect_area
            - corner_cutout(self.top_left)
            - corner_cutout(self.top_right)
            - corner_cutout(self.bottom_right)
            - corner_cutout(self.bottom_left)
    }
}

// ============================================================================
// Hit Testing
// ============================================================================

impl RRect {}

// ============================================================================
// Transformations
// ============================================================================

impl RRect {
    /// Scales all corner radii by a factor.
    #[inline]
    #[must_use]
    pub fn scale_radii(&self, factor: f64) -> Self {
        Self::new(
            self.rect,
            self.top_left.scale(factor),
            self.top_right.scale(factor),
            self.bottom_right.scale(factor),
            self.bottom_left.scale(factor),
        )
    }

    /// Translates the rounded rect by an offset; corner radii are
    /// translation-invariant and pass through unchanged.
    ///
    /// Mirrors [`Rect::translate_offset`] (Flutter `RRect.shift`).
    #[inline]
    #[must_use]
    pub fn translate_offset(&self, offset: crate::geometry::Offset<f64>) -> Self {
        Self {
            rect: self.rect.translate_offset(offset),
            ..*self
        }
    }

    /// Moves the edges *and the corner radii* outward by `delta`.
    ///
    /// The radii have to travel with the edges: a corner keeps its shape only
    /// if its radius grows by the same amount the box did. Holding them fixed
    /// produces a rounded rect whose corners are too tight for the box they
    /// bound, which shows up as a border of uneven thickness wherever an
    /// inflated and a deflated copy are drawn as a ring.
    ///
    /// Each radius axis is clamped at zero independently, so a `delta` past one
    /// axis's radius squares the corner off on that axis alone.
    #[inline]
    #[must_use]
    pub fn inflate(&self, delta: f64) -> Self {
        let grow = |radius: Radius<f64>| {
            Radius::new((radius.x + delta).max(0.0), (radius.y + delta).max(0.0))
        };
        Self::new(
            self.rect.inflate(delta, delta),
            grow(self.top_left),
            grow(self.top_right),
            grow(self.bottom_right),
            grow(self.bottom_left),
        )
    }

    /// Moves the edges and corner radii inward by `delta` — see
    /// [`Self::inflate`], of which this is the negation.
    #[inline]
    #[must_use]
    pub fn inset(&self, delta: f64) -> Self {
        self.inflate(-delta)
    }

    /// Clamps all corner radii to fit within the rectangle dimensions.
    #[inline]
    #[must_use]
    pub fn clamp_radii(&self) -> Self {
        let max_x = self.width() * 0.5;
        let max_y = self.height() * 0.5;

        Self::new(
            self.rect,
            self.top_left.clamp(max_x, max_y),
            self.top_right.clamp(max_x, max_y),
            self.bottom_right.clamp(max_x, max_y),
            self.bottom_left.clamp(max_x, max_y),
        )
    }

    /// Whether `point` lies inside the rounded rectangle: inside the base
    /// rect, and — where it falls in a corner's radius box — inside that
    /// corner's ellipse. A corner with a zero radius on either axis is a
    /// square corner.
    #[must_use]
    pub fn contains(&self, point: Point<f64>) -> bool {
        if !self.rect.contains(point) {
            return false;
        }
        let (x, y) = (point.x, point.y);
        let (left, top, right, bottom) = (self.left(), self.top(), self.right(), self.bottom());
        // Each corner: (its ellipse centre, its radii, whether `point` is in
        // its radius box). Only the box `point` falls in can exclude it.
        let corners = [
            (
                self.top_left,
                left,
                top,
                x < left + self.top_left.x,
                y < top + self.top_left.y,
            ),
            (
                self.top_right,
                right,
                top,
                x > right - self.top_right.x,
                y < top + self.top_right.y,
            ),
            (
                self.bottom_right,
                right,
                bottom,
                x > right - self.bottom_right.x,
                y > bottom - self.bottom_right.y,
            ),
            (
                self.bottom_left,
                left,
                bottom,
                x < left + self.bottom_left.x,
                y > bottom - self.bottom_left.y,
            ),
        ];
        for (radius, edge_x, edge_y, in_x, in_y) in corners {
            let (rx, ry) = (radius.x, radius.y);
            if !(in_x && in_y) || rx <= 0.0 || ry <= 0.0 {
                continue;
            }
            let cx = if edge_x == left {
                left + rx
            } else {
                right - rx
            };
            let cy = if edge_y == top { top + ry } else { bottom - ry };
            let (nx, ny) = ((x - cx) / rx, (y - cy) / ry);
            return nx * nx + ny * ny <= 1.0;
        }
        true
    }

    /// Returns the center points of each corner's radius.
    #[inline]
    #[must_use]
    pub fn corner_centers(&self) -> [Point<f64>; 4] {
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

    /// Linearly interpolates between two rounded rectangles.
    #[inline]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        Self::new(
            a.rect.lerp(b.rect, t),
            Radius::lerp(a.top_left, b.top_left, t),
            Radius::lerp(a.top_right, b.top_right, t),
            Radius::lerp(a.bottom_right, b.bottom_right, t),
            Radius::lerp(a.bottom_left, b.bottom_left, t),
        )
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl From<Rect<f64>> for RRect {
    fn from(rect: Rect<f64>) -> Self {
        Self::from_rect(rect)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Offset;

    #[test]
    fn translate_offset_moves_rect_and_keeps_radii() {
        let rrect = RRect::from_rect_and_radius(
            Rect::from_origin_size(Point::ZERO, Size::new(40.0, 40.0)),
            Radius::circular(8.0),
        );
        let moved = rrect.translate_offset(Offset::new(70.0, 10.0));
        assert_eq!(
            moved.rect,
            Rect::from_origin_size(Point::new(70.0, 10.0), Size::new(40.0, 40.0),),
        );
        assert_eq!(moved.top_left, rrect.top_left);
        assert_eq!(moved.bottom_right, rrect.bottom_right);
    }

    #[test]
    fn inflate_moves_the_radii_with_the_edges() {
        // Growing the box by `delta` without growing the radii leaves corners
        // too tight for the box they now bound. Concretely: this rrect's
        // corners are already the full half-side, so it is a circle -- and it
        // has to stay one after inflating.
        let rrect = RRect::from_rect_and_radius(
            Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
            Radius::circular(20.0),
        );
        let bigger = rrect.inflate(5.0);

        assert_eq!(bigger.rect, Rect::from_ltrb(-5.0, -5.0, 45.0, 45.0));
        assert_eq!(bigger.top_left, Radius::circular(25.0));
        assert_eq!(bigger.bottom_right, Radius::circular(25.0));
    }

    #[test]
    fn inset_shrinks_the_radii_and_clamps_them_at_zero() {
        let rrect = RRect::from_rect_and_radius(
            Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
            Radius::circular(6.0),
        );

        // The common case: an inner ring for a 2 px border.
        let inner = rrect.inset(2.0);
        assert_eq!(inner.rect, Rect::from_ltrb(2.0, 2.0, 38.0, 38.0));
        assert_eq!(inner.top_left, Radius::circular(4.0));

        // Past the radius the corner squares off rather than inverting.
        let squared = rrect.inset(10.0);
        assert_eq!(squared.top_left, Radius::circular(0.0));
        assert_eq!(squared.bottom_left, Radius::circular(0.0));
    }

    #[test]
    fn inset_clamps_each_radius_axis_independently() {
        let rrect = RRect::from_rect_and_radius(
            Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
            Radius::elliptical(9.0, 3.0),
        );

        // Only the y axis reaches the clamp.
        let squared = rrect.inset(5.0);
        assert_eq!(squared.top_left, Radius::elliptical(4.0, 0.0));
    }
}
