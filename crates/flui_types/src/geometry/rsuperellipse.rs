//! Rounded superellipse (squircle) type.
//!
//! A superellipse with circular arc corners, matching iOS/SwiftUI's
//! `.continuous` corner style. This provides smoother corner transitions
//! than standard rounded rectangles.
//!
//! # Mathematical Background
//!
//! A standard superellipse follows the equation `|x|^n + |y|^n = 1`.
//! When n > 2, corners become rounded but can appear too pronounced.
//! RSuperellipse improves this by using circular arcs at corners,
//! creating softer transitions that match Apple's design language.

use super::{Offset, Point, Radius, Rect, Vec2};

/// A rounded superellipse shape (squircle).
///
/// Similar to [`RRect`](super::RRect) but with smoother corner transitions
/// that match iOS/SwiftUI's `.continuous` corner style.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{RSuperellipse, Rect, Radius};
///
/// // Uniform corners
/// let squircle = RSuperellipse::from_rect_and_radius(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Radius::circular(20.0),
/// );
///
/// // Different corners
/// let squircle = RSuperellipse::from_rect_and_corners(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     Radius::circular(10.0),  // top-left
///     Radius::circular(20.0),  // top-right
///     Radius::circular(15.0),  // bottom-right
///     Radius::circular(5.0),   // bottom-left
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct RSuperellipse {
    /// The bounding rectangle.
    rect: Rect,
    /// Top-left corner radius.
    tl_radius: Radius,
    /// Top-right corner radius.
    tr_radius: Radius,
    /// Bottom-right corner radius.
    br_radius: Radius,
    /// Bottom-left corner radius.
    bl_radius: Radius,
}

impl RSuperellipse {
    /// A zero-sized superellipse at the origin.
    pub const ZERO: Self = Self {
        rect: Rect::ZERO,
        tl_radius: Radius::ZERO,
        tr_radius: Radius::ZERO,
        br_radius: Radius::ZERO,
        bl_radius: Radius::ZERO,
    };

    // ========================================================================
    // Constructors
    // ========================================================================

    /// Creates a superellipse from left, top, right, bottom and uniform radius.
    #[inline]
    #[must_use]
    pub fn from_ltrb_r(left: f32, top: f32, right: f32, bottom: f32, radius: Radius) -> Self {
        Self {
            rect: Rect::from_ltrb(left, top, right, bottom),
            tl_radius: radius,
            tr_radius: radius,
            br_radius: radius,
            bl_radius: radius,
        }
    }

    /// Creates a superellipse from left, top, right, bottom and x/y radii.
    #[inline]
    #[must_use]
    pub fn from_ltrb_xy(left: f32, top: f32, right: f32, bottom: f32, rx: f32, ry: f32) -> Self {
        let radius = Radius::new(rx, ry);
        Self::from_ltrb_r(left, top, right, bottom, radius)
    }

    /// Creates a superellipse from left, top, right, bottom and individual corner radii.
    #[inline]
    #[must_use]
    pub fn from_ltrb_and_corners(
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        tl: Radius,
        tr: Radius,
        br: Radius,
        bl: Radius,
    ) -> Self {
        Self {
            rect: Rect::from_ltrb(left, top, right, bottom),
            tl_radius: tl,
            tr_radius: tr,
            br_radius: br,
            bl_radius: bl,
        }
    }

    /// Creates a superellipse from a rectangle and uniform radius.
    #[inline]
    #[must_use]
    pub fn from_rect_and_radius(rect: Rect, radius: Radius) -> Self {
        Self {
            rect,
            tl_radius: radius,
            tr_radius: radius,
            br_radius: radius,
            bl_radius: radius,
        }
    }

    /// Creates a superellipse from a rectangle and individual corner radii.
    #[inline]
    #[must_use]
    pub fn from_rect_and_corners(
        rect: Rect,
        tl: Radius,
        tr: Radius,
        br: Radius,
        bl: Radius,
    ) -> Self {
        Self {
            rect,
            tl_radius: tl,
            tr_radius: tr,
            br_radius: br,
            bl_radius: bl,
        }
    }

    /// Creates a superellipse from a rectangle with circular corners.
    #[inline]
    #[must_use]
    pub fn from_rect_circular(rect: Rect, radius: f32) -> Self {
        Self::from_rect_and_radius(rect, Radius::circular(radius))
    }

    // ========================================================================
    // Properties
    // ========================================================================

    /// Returns the bounding rectangle (without corner rounding).
    #[inline]
    #[must_use]
    pub fn outer_rect(&self) -> Rect {
        self.rect
    }

    /// Returns the left edge.
    #[inline]
    #[must_use]
    pub fn left(&self) -> f32 {
        self.rect.left()
    }

    /// Returns the top edge.
    #[inline]
    #[must_use]
    pub fn top(&self) -> f32 {
        self.rect.top()
    }

    /// Returns the right edge.
    #[inline]
    #[must_use]
    pub fn right(&self) -> f32 {
        self.rect.right()
    }

    /// Returns the bottom edge.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.rect.bottom()
    }

    /// Returns the width.
    #[inline]
    #[must_use]
    pub fn width(&self) -> f32 {
        self.rect.width()
    }

    /// Returns the height.
    #[inline]
    #[must_use]
    pub fn height(&self) -> f32 {
        self.rect.height()
    }

    /// Returns the top-left corner radius.
    #[inline]
    #[must_use]
    pub fn tl_radius(&self) -> Radius {
        self.tl_radius
    }

    /// Returns the top-right corner radius.
    #[inline]
    #[must_use]
    pub fn tr_radius(&self) -> Radius {
        self.tr_radius
    }

    /// Returns the bottom-right corner radius.
    #[inline]
    #[must_use]
    pub fn br_radius(&self) -> Radius {
        self.br_radius
    }

    /// Returns the bottom-left corner radius.
    #[inline]
    #[must_use]
    pub fn bl_radius(&self) -> Radius {
        self.bl_radius
    }

    /// Returns `true` if all corners have the same radius.
    #[inline]
    #[must_use]
    pub fn has_uniform_corners(&self) -> bool {
        self.tl_radius == self.tr_radius
            && self.tr_radius == self.br_radius
            && self.br_radius == self.bl_radius
    }

    /// Returns `true` if all corners are circular (x == y for all radii).
    #[inline]
    #[must_use]
    pub fn has_circular_corners(&self) -> bool {
        self.tl_radius.is_circular()
            && self.tr_radius.is_circular()
            && self.br_radius.is_circular()
            && self.bl_radius.is_circular()
    }

    /// Returns `true` if this is a plain rectangle (all radii are zero).
    #[inline]
    #[must_use]
    pub fn is_rect(&self) -> bool {
        self.tl_radius.is_zero()
            && self.tr_radius.is_zero()
            && self.br_radius.is_zero()
            && self.bl_radius.is_zero()
    }

    /// Returns `true` if this superellipse is empty (zero or negative size).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rect.is_empty()
    }

    // ========================================================================
    // Safe inner rectangles
    // ========================================================================

    /// Returns the largest axis-aligned rectangle fully inside the superellipse.
    ///
    /// This is guaranteed to be entirely within the curved corners.
    #[inline]
    #[must_use]
    pub fn safe_inner_rect(&self) -> Rect {
        // Use the maximum corner radii to determine safe insets
        let inset_left = self.tl_radius.x.max(self.bl_radius.x);
        let inset_right = self.tr_radius.x.max(self.br_radius.x);
        let inset_top = self.tl_radius.y.max(self.tr_radius.y);
        let inset_bottom = self.bl_radius.y.max(self.br_radius.y);

        Rect::from_ltrb(
            self.left() + inset_left,
            self.top() + inset_top,
            self.right() - inset_right,
            self.bottom() - inset_bottom,
        )
    }

    /// Returns a wide middle rectangle (full width, constrained height).
    #[inline]
    #[must_use]
    pub fn wide_middle_rect(&self) -> Rect {
        let inset_top = self.tl_radius.y.max(self.tr_radius.y);
        let inset_bottom = self.bl_radius.y.max(self.br_radius.y);

        Rect::from_ltrb(
            self.left(),
            self.top() + inset_top,
            self.right(),
            self.bottom() - inset_bottom,
        )
    }

    /// Returns a tall middle rectangle (full height, constrained width).
    #[inline]
    #[must_use]
    pub fn tall_middle_rect(&self) -> Rect {
        let inset_left = self.tl_radius.x.max(self.bl_radius.x);
        let inset_right = self.tr_radius.x.max(self.br_radius.x);

        Rect::from_ltrb(
            self.left() + inset_left,
            self.top(),
            self.right() - inset_right,
            self.bottom(),
        )
    }

    // ========================================================================
    // Transformations
    // ========================================================================

    /// Shifts the superellipse by the given offset.
    #[inline]
    #[must_use]
    pub fn shift(&self, offset: Offset) -> Self {
        Self {
            rect: self.rect.translate(Vec2::new(offset.dx, offset.dy)),
            ..*self
        }
    }

    /// Expands all edges outward by the given delta.
    #[inline]
    #[must_use]
    pub fn inflate(&self, delta: f32) -> Self {
        Self {
            rect: self.rect.inflate(delta, delta),
            tl_radius: Radius::new(self.tl_radius.x + delta, self.tl_radius.y + delta),
            tr_radius: Radius::new(self.tr_radius.x + delta, self.tr_radius.y + delta),
            br_radius: Radius::new(self.br_radius.x + delta, self.br_radius.y + delta),
            bl_radius: Radius::new(self.bl_radius.x + delta, self.bl_radius.y + delta),
        }
    }

    /// Contracts all edges inward by the given delta.
    #[inline]
    #[must_use]
    pub fn deflate(&self, delta: f32) -> Self {
        self.inflate(-delta)
    }

    /// Scales the superellipse by a factor around the origin.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            rect: Rect::from_ltrb(
                self.rect.left() * factor,
                self.rect.top() * factor,
                self.rect.right() * factor,
                self.rect.bottom() * factor,
            ),
            tl_radius: self.tl_radius.scale(factor),
            tr_radius: self.tr_radius.scale(factor),
            br_radius: self.br_radius.scale(factor),
            bl_radius: self.bl_radius.scale(factor),
        }
    }

    // ========================================================================
    // Hit testing
    // ========================================================================

    /// Returns `true` if the point is inside the superellipse.
    ///
    /// This uses an approximation based on the corner radii and superellipse
    /// formula for accurate hit testing.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        // Quick bounding box check
        if !self.rect.contains(point) {
            return false;
        }

        let x = point.x;
        let y = point.y;

        // Check if point is in one of the corner regions
        // Top-left corner
        if x < self.left() + self.tl_radius.x && y < self.top() + self.tl_radius.y {
            return self.point_in_corner(
                x - self.left() - self.tl_radius.x,
                y - self.top() - self.tl_radius.y,
                self.tl_radius,
            );
        }

        // Top-right corner
        if x > self.right() - self.tr_radius.x && y < self.top() + self.tr_radius.y {
            return self.point_in_corner(
                x - self.right() + self.tr_radius.x,
                y - self.top() - self.tr_radius.y,
                self.tr_radius,
            );
        }

        // Bottom-right corner
        if x > self.right() - self.br_radius.x && y > self.bottom() - self.br_radius.y {
            return self.point_in_corner(
                x - self.right() + self.br_radius.x,
                y - self.bottom() + self.br_radius.y,
                self.br_radius,
            );
        }

        // Bottom-left corner
        if x < self.left() + self.bl_radius.x && y > self.bottom() - self.bl_radius.y {
            return self.point_in_corner(
                x - self.left() - self.bl_radius.x,
                y - self.bottom() + self.bl_radius.y,
                self.bl_radius,
            );
        }

        // Point is in the non-corner region
        true
    }

    /// Checks if a point is inside a corner using superellipse formula.
    ///
    /// For a superellipse with n ≈ 2.5 (iOS-style squircle), we use:
    /// `|x/rx|^n + |y/ry|^n <= 1`
    #[inline]
    fn point_in_corner(&self, dx: f32, dy: f32, radius: Radius) -> bool {
        if radius.x <= 0.0 || radius.y <= 0.0 {
            return true; // Sharp corner, already passed bbox check
        }

        // Superellipse exponent (2.5 approximates iOS squircle)
        const N: f32 = 2.5;

        let nx = (dx.abs() / radius.x).powf(N);
        let ny = (dy.abs() / radius.y).powf(N);

        nx + ny <= 1.0
    }

    // ========================================================================
    // Interpolation
    // ========================================================================

    /// Linear interpolation between two superellipses.
    #[inline]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            rect: Rect::lerp(a.rect, b.rect, t),
            tl_radius: Radius::lerp(a.tl_radius, b.tl_radius, t),
            tr_radius: Radius::lerp(a.tr_radius, b.tr_radius, t),
            br_radius: Radius::lerp(a.br_radius, b.br_radius, t),
            bl_radius: Radius::lerp(a.bl_radius, b.bl_radius, t),
        }
    }
}

impl From<Rect> for RSuperellipse {
    /// Creates a superellipse with zero radii (plain rectangle).
    fn from(rect: Rect) -> Self {
        Self::from_rect_and_radius(rect, Radius::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_rect_and_radius() {
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let radius = Radius::circular(15.0);
        let se = RSuperellipse::from_rect_and_radius(rect, radius);

        assert_eq!(se.outer_rect(), rect);
        assert_eq!(se.tl_radius(), radius);
        assert_eq!(se.tr_radius(), radius);
        assert_eq!(se.br_radius(), radius);
        assert_eq!(se.bl_radius(), radius);
        assert!(se.has_uniform_corners());
        assert!(se.has_circular_corners());
    }

    #[test]
    fn test_from_rect_and_corners() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let tl = Radius::circular(10.0);
        let tr = Radius::circular(20.0);
        let br = Radius::circular(15.0);
        let bl = Radius::circular(5.0);

        let se = RSuperellipse::from_rect_and_corners(rect, tl, tr, br, bl);

        assert_eq!(se.tl_radius(), tl);
        assert_eq!(se.tr_radius(), tr);
        assert_eq!(se.br_radius(), br);
        assert_eq!(se.bl_radius(), bl);
        assert!(!se.has_uniform_corners());
    }

    #[test]
    fn test_is_rect() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let se_rect = RSuperellipse::from_rect_and_radius(rect, Radius::ZERO);
        assert!(se_rect.is_rect());

        let se_rounded = RSuperellipse::from_rect_circular(rect, 10.0);
        assert!(!se_rounded.is_rect());
    }

    #[test]
    fn test_shift() {
        let se = RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 10.0);
        let shifted = se.shift(Offset::new(50.0, 25.0));

        assert_eq!(shifted.left(), 50.0);
        assert_eq!(shifted.top(), 25.0);
        assert_eq!(shifted.width(), 100.0);
        assert_eq!(shifted.height(), 100.0);
        assert_eq!(shifted.tl_radius(), se.tl_radius());
    }

    #[test]
    fn test_inflate_deflate() {
        let se = RSuperellipse::from_rect_circular(Rect::from_xywh(10.0, 10.0, 80.0, 80.0), 10.0);

        let inflated = se.inflate(5.0);
        assert_eq!(inflated.left(), 5.0);
        assert_eq!(inflated.top(), 5.0);
        assert_eq!(inflated.right(), 95.0);
        assert_eq!(inflated.bottom(), 95.0);
        assert_eq!(inflated.tl_radius().x, 15.0);

        let deflated = se.deflate(5.0);
        assert_eq!(deflated.left(), 15.0);
        assert_eq!(deflated.top(), 15.0);
        assert_eq!(deflated.right(), 85.0);
        assert_eq!(deflated.bottom(), 85.0);
        assert_eq!(deflated.tl_radius().x, 5.0);
    }

    #[test]
    fn test_contains_center() {
        let se = RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);

        // Center should be inside
        assert!(se.contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn test_contains_outside() {
        let se = RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);

        // Outside bounding box
        assert!(!se.contains(Point::new(-10.0, 50.0)));
        assert!(!se.contains(Point::new(110.0, 50.0)));
        assert!(!se.contains(Point::new(50.0, -10.0)));
        assert!(!se.contains(Point::new(50.0, 110.0)));
    }

    #[test]
    fn test_contains_corner() {
        let se = RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);

        // Corner point (0, 0) should be outside due to rounding
        assert!(!se.contains(Point::new(0.0, 0.0)));

        // Point just inside corner curve
        assert!(se.contains(Point::new(10.0, 10.0)));
    }

    #[test]
    fn test_safe_inner_rect() {
        let se = RSuperellipse::from_rect_and_corners(
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            Radius::circular(10.0),
            Radius::circular(20.0),
            Radius::circular(15.0),
            Radius::circular(5.0),
        );

        let inner = se.safe_inner_rect();

        // Left inset: max(tl.x=10, bl.x=5) = 10
        assert_eq!(inner.left(), 10.0);
        // Right inset: max(tr.x=20, br.x=15) = 20
        assert_eq!(inner.right(), 80.0);
        // Top inset: max(tl.y=10, tr.y=20) = 20
        assert_eq!(inner.top(), 20.0);
        // Bottom inset: max(bl.y=5, br.y=15) = 15
        assert_eq!(inner.bottom(), 85.0);
    }

    #[test]
    fn test_lerp() {
        let a = RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 10.0);
        let b = RSuperellipse::from_rect_circular(Rect::from_xywh(100.0, 0.0, 100.0, 100.0), 30.0);

        let mid = RSuperellipse::lerp(a, b, 0.5);

        assert_eq!(mid.left(), 50.0);
        assert_eq!(mid.tl_radius().x, 20.0);
    }

    #[test]
    fn test_from_rect() {
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let se: RSuperellipse = rect.into();

        assert_eq!(se.outer_rect(), rect);
        assert!(se.is_rect());
    }
}
