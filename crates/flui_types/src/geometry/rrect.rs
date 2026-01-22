//! Rounded rectangle type.
//!
//! API design inspired by Flutter and kurbo.

use super::{Offset, Point, Rect, Size, Vec2};
use super::traits::{NumericUnit, Unit};

/// Corner radius for rounded rectangles.
///
/// Supports both circular (same x and y) and elliptical radii.
///
/// Generic over unit type `T`. Common usage:
/// - `Radius<Pixels>` - UI corner radius
/// - `Radius<f32>` - Normalized/dimensionless radius
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Radius<T: Unit = f32> {
    /// Horizontal radius.
    pub x: T,
    /// Vertical radius.
    pub y: T,
}

// ============================================================================
// Constants (f32 only for backwards compatibility)
// ============================================================================

impl Radius<f32> {
    /// Zero radius (no rounding).
    pub const ZERO: Self = Self::new(0.0, 0.0);
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Radius<T> {
    /// Creates a new radius.
    #[inline]
    #[must_use]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    /// Creates a circular radius (x == y).
    #[inline]
    #[must_use]
    pub const fn circular(r: T) -> Self {
        Self::new(r, r)
    }

    /// Creates an elliptical radius.
    #[inline]
    #[must_use]
    pub const fn elliptical(x: T, y: T) -> Self {
        Self::new(x, y)
    }

}

// ============================================================================
// Numeric Unit Operations
// ============================================================================

impl<T: NumericUnit> Radius<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    /// Scales the radius by a factor.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
}

impl<T: NumericUnit + PartialOrd> Radius<T> {
    /// Clamps the radius to maximum values.
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
// f32 Float Operations
// ============================================================================

impl Radius<f32> {
    /// Returns `true` if this is a circular radius.
    #[inline]
    #[must_use]
    pub fn is_circular(&self) -> bool {
        (self.x - self.y).abs() < f32::EPSILON
    }

    /// Returns `true` if this radius is zero.
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.x.abs() < f32::EPSILON && self.y.abs() < f32::EPSILON
    }

    /// Linear interpolation between two radii.
    #[inline]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
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

/// A rectangle with rounded corners.
///
/// Each corner can have a different radius, and radii can be elliptical.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{RRect, Rect, Radius};
///
/// // Uniform circular corners
/// let rrect = RRect::from_rect_and_radius(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Radius::circular(10.0),
/// );
///
/// // Different corners
/// let rrect = RRect::from_rect_and_corners(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Radius::circular(10.0),  // top-left
///     Radius::circular(20.0),  // top-right
///     Radius::circular(15.0),  // bottom-right
///     Radius::circular(5.0),   // bottom-left
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RRect {
    /// The base rectangle.
    pub rect: Rect,
    /// Top-left corner radius.
    pub top_left: Radius,
    /// Top-right corner radius.
    pub top_right: Radius,
    /// Bottom-right corner radius.
    pub bottom_right: Radius,
    /// Bottom-left corner radius.
    pub bottom_left: Radius,
}

// ============================================================================
// Constructors
// ============================================================================

impl RRect {
    /// Creates a new rounded rectangle with explicit corner radii.
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

    /// Creates a rounded rectangle with uniform corner radius.
    #[inline]
    #[must_use]
    pub const fn from_rect_and_radius(rect: Rect, radius: Radius) -> Self {
        Self::new(rect, radius, radius, radius, radius)
    }

    /// Creates a rounded rectangle with circular corners.
    #[inline]
    #[must_use]
    pub const fn from_rect_circular(rect: Rect, radius: f32) -> Self {
        Self::from_rect_and_radius(rect, Radius::circular(radius))
    }

    /// Creates a rounded rectangle with elliptical corners.
    #[inline]
    #[must_use]
    pub const fn from_rect_elliptical(rect: Rect, radius_x: f32, radius_y: f32) -> Self {
        Self::from_rect_and_radius(rect, Radius::elliptical(radius_x, radius_y))
    }

    /// Creates a rounded rectangle from rect and x/y radii (Flutter-style alias).
    #[inline]
    #[must_use]
    pub const fn from_rect_xy(rect: Rect, radius_x: f32, radius_y: f32) -> Self {
        Self::from_rect_elliptical(rect, radius_x, radius_y)
    }

    /// Creates a rounded rectangle with individual corner radii.
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

    /// Creates a rounded rectangle from coordinates and radius.
    #[inline]
    #[must_use]
    pub fn from_xywh_circular(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Self {
        Self::from_rect_circular(Rect::from_xywh(x, y, width, height), radius)
    }

    /// Creates a rectangle with no rounding.
    #[inline]
    #[must_use]
    pub const fn from_rect(rect: Rect) -> Self {
        Self::from_rect_and_radius(rect, Radius::ZERO)
    }
}

// ============================================================================
// Accessors
// ============================================================================

impl RRect {
    /// Left edge.
    #[inline]
    #[must_use]
    pub fn left(&self) -> f32 {
        self.rect.left()
    }

    /// Top edge.
    #[inline]
    #[must_use]
    pub fn top(&self) -> f32 {
        self.rect.top()
    }

    /// Right edge.
    #[inline]
    #[must_use]
    pub fn right(&self) -> f32 {
        self.rect.right()
    }

    /// Bottom edge.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.rect.bottom()
    }

    /// Width.
    #[inline]
    #[must_use]
    pub fn width(&self) -> f32 {
        self.rect.width()
    }

    /// Height.
    #[inline]
    #[must_use]
    pub fn height(&self) -> f32 {
        self.rect.height()
    }

    /// Size.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size<f32> {
        self.rect.size()
    }

    /// Center point.
    #[inline]
    #[must_use]
    pub fn center(&self) -> Point<f32> {
        self.rect.center()
    }

    /// Bounding rectangle.
    #[inline]
    #[must_use]
    pub const fn bounding_rect(&self) -> Rect {
        self.rect
    }
}

// ============================================================================
// Queries
// ============================================================================

impl RRect {
    /// Returns `true` if all radii are zero.
    #[inline]
    #[must_use]
    pub fn is_rect(&self) -> bool {
        self.top_left.is_zero()
            && self.top_right.is_zero()
            && self.bottom_right.is_zero()
            && self.bottom_left.is_zero()
    }

    /// Returns `true` if all corners are circular.
    #[inline]
    #[must_use]
    pub fn is_circular(&self) -> bool {
        self.top_left.is_circular()
            && self.top_right.is_circular()
            && self.bottom_right.is_circular()
            && self.bottom_left.is_circular()
    }

    /// Returns `true` if all corners have the same radius.
    #[inline]
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.top_left == self.top_right
            && self.top_right == self.bottom_right
            && self.bottom_right == self.bottom_left
    }

    /// Returns `true` if any corner has a radius.
    #[inline]
    #[must_use]
    pub fn has_rounding(&self) -> bool {
        !self.is_rect()
    }

    /// Returns `true` if the rectangle is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rect.is_empty()
    }

    /// Maximum corner radius.
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

    /// Area (approximate, accounts for rounded corners).
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        if self.is_rect() {
            return self.rect.area();
        }

        let rect_area = self.rect.area();
        let corner_cutout = |r: Radius| -> f32 { r.x * r.y * (1.0 - std::f32::consts::FRAC_PI_4) };

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

impl RRect {
    /// Returns `true` if the point is inside the rounded rectangle.
    #[must_use]
    pub fn contains(&self, point: Point<f32>) -> bool {
        if !self.rect.contains(point) {
            return false;
        }

        if self.is_rect() {
            return true;
        }

        let x = point.x;
        let y = point.y;

        // Check each corner
        // Top-left
        if x < self.left() + self.top_left.x && y < self.top() + self.top_left.y {
            let dx = x - (self.left() + self.top_left.x);
            let dy = y - (self.top() + self.top_left.y);
            if self.top_left.x > 0.0
                && self.top_left.y > 0.0
                && dx * dx / (self.top_left.x * self.top_left.x)
                    + dy * dy / (self.top_left.y * self.top_left.y)
                    > 1.0
            {
                return false;
            }
        }

        // Top-right
        if x > self.right() - self.top_right.x && y < self.top() + self.top_right.y {
            let dx = x - (self.right() - self.top_right.x);
            let dy = y - (self.top() + self.top_right.y);
            if self.top_right.x > 0.0
                && self.top_right.y > 0.0
                && dx * dx / (self.top_right.x * self.top_right.x)
                    + dy * dy / (self.top_right.y * self.top_right.y)
                    > 1.0
            {
                return false;
            }
        }

        // Bottom-right
        if x > self.right() - self.bottom_right.x && y > self.bottom() - self.bottom_right.y {
            let dx = x - (self.right() - self.bottom_right.x);
            let dy = y - (self.bottom() - self.bottom_right.y);
            if self.bottom_right.x > 0.0
                && self.bottom_right.y > 0.0
                && dx * dx / (self.bottom_right.x * self.bottom_right.x)
                    + dy * dy / (self.bottom_right.y * self.bottom_right.y)
                    > 1.0
            {
                return false;
            }
        }

        // Bottom-left
        if x < self.left() + self.bottom_left.x && y > self.bottom() - self.bottom_left.y {
            let dx = x - (self.left() + self.bottom_left.x);
            let dy = y - (self.bottom() - self.bottom_left.y);
            if self.bottom_left.x > 0.0
                && self.bottom_left.y > 0.0
                && dx * dx / (self.bottom_left.x * self.bottom_left.x)
                    + dy * dy / (self.bottom_left.y * self.bottom_left.y)
                    > 1.0
            {
                return false;
            }
        }

        true
    }
}

// ============================================================================
// Transformations
// ============================================================================

impl RRect {
    /// Scales corner radii by a factor.
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

    /// Expands the rectangle by a margin (radii unchanged).
    #[inline]
    #[must_use]
    pub fn inflate(&self, delta: f32) -> Self {
        Self::new(
            self.rect.inflate(delta, delta),
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        )
    }

    /// Contracts the rectangle by a margin (radii unchanged).
    #[inline]
    #[must_use]
    pub fn inset(&self, delta: f32) -> Self {
        self.inflate(-delta)
    }

    /// Translates the rounded rectangle.
    #[inline]
    #[must_use]
    pub fn translate(&self, offset: Vec2<f32>) -> Self {
        Self::new(
            self.rect.translate(offset),
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        )
    }

    /// Translates using an Offset.
    #[inline]
    #[must_use]
    pub fn translate_offset(&self, offset: Offset<f32>) -> Self {
        self.translate(Vec2::new(offset.dx, offset.dy))
    }

    /// Clamps radii to fit within the rectangle.
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

    /// Corner centers for rendering elliptical arcs.
    #[inline]
    #[must_use]
    pub fn corner_centers(&self) -> [Point<f32>; 4] {
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

    /// Linear interpolation between two rounded rectangles.
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
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

impl From<Rect> for RRect {
    fn from(rect: Rect) -> Self {
        Self::from_rect(rect)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radius() {
        let r = Radius::circular(10.0);
        assert!(r.is_circular());
        assert!(!r.is_zero());

        let r2 = Radius::elliptical(10.0, 20.0);
        assert!(!r2.is_circular());

        assert!(Radius::ZERO.is_zero());
    }

    #[test]
    fn test_rrect_creation() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_circular(rect, 10.0);

        assert_eq!(rrect.rect, rect);
        assert_eq!(rrect.top_left, Radius::circular(10.0));
        assert!(rrect.is_uniform());
        assert!(rrect.is_circular());
    }

    #[test]
    fn test_rrect_queries() {
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let rrect = RRect::from_rect_circular(rect, 5.0);

        assert_eq!(rrect.left(), 10.0);
        assert_eq!(rrect.top(), 20.0);
        assert_eq!(rrect.right(), 110.0);
        assert_eq!(rrect.bottom(), 70.0);
        assert_eq!(rrect.width(), 100.0);
        assert_eq!(rrect.height(), 50.0);
    }

    #[test]
    fn test_is_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let rrect1 = RRect::from_rect(rect);
        assert!(rrect1.is_rect());

        let rrect2 = RRect::from_rect_circular(rect, 10.0);
        assert!(!rrect2.is_rect());
        assert!(rrect2.has_rounding());
    }

    #[test]
    fn test_contains() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_circular(rect, 10.0);

        assert!(rrect.contains(Point::new(50.0, 50.0)));
        assert!(rrect.contains(Point::new(10.0, 10.0)));
        assert!(!rrect.contains(Point::new(0.0, 0.0))); // corner is rounded
        assert!(!rrect.contains(Point::new(-1.0, 50.0)));
    }

    #[test]
    fn test_transformations() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = RRect::from_rect_circular(rect, 10.0);

        let scaled = rrect.scale_radii(2.0);
        assert_eq!(scaled.top_left.x, 20.0);

        let inflated = rrect.inflate(5.0);
        assert_eq!(inflated.width(), 110.0);

        let translated = rrect.translate(Vec2::new(10.0, 20.0));
        assert_eq!(translated.left(), 10.0);
        assert_eq!(translated.top(), 20.0);
    }

    #[test]
    fn test_lerp() {
        let r1 = RRect::from_xywh_circular(0.0, 0.0, 100.0, 100.0, 10.0);
        let r2 = RRect::from_xywh_circular(0.0, 0.0, 200.0, 200.0, 20.0);

        let mid = RRect::lerp(r1, r2, 0.5);
        assert_eq!(mid.width(), 150.0);
        assert_eq!(mid.top_left.x, 15.0);
    }

    #[test]
    fn test_clamp_radii() {
        let rect = Rect::from_xywh(0.0, 0.0, 20.0, 20.0);
        let rrect = RRect::from_rect_circular(rect, 50.0); // too big
        let clamped = rrect.clamp_radii();

        assert_eq!(clamped.top_left.x, 10.0); // max is half width
    }
}
