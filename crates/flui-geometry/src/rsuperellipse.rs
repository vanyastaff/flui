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

use super::{Pixels, Radius, Rect, px};

/// A rounded superellipse (squircle) with independent corner radii.
///
/// Like a rounded rectangle, but corners blend smoothly into the edges,
/// matching iOS/SwiftUI's `.continuous` corner style. Corresponds to
/// Flutter's `RSuperellipse`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RSuperellipse {
    /// The bounding rectangle.
    rect: Rect<Pixels>,
    /// Top-left corner radius.
    tl_radius: Radius<Pixels>,
    /// Top-right corner radius.
    tr_radius: Radius<Pixels>,
    /// Bottom-right corner radius.
    br_radius: Radius<Pixels>,
    /// Bottom-left corner radius.
    bl_radius: Radius<Pixels>,
}

impl RSuperellipse {
    /// A zero-sized superellipse at the origin.
    pub const ZERO: Self = Self {
        rect: Rect::from_ltrb(px(0.0), px(0.0), px(0.0), px(0.0)),
        tl_radius: Radius::ZERO,
        tr_radius: Radius::ZERO,
        br_radius: Radius::ZERO,
        bl_radius: Radius::ZERO,
    };

    // ========================================================================
    // Constructors
    // ========================================================================

    /// Creates a rounded superellipse from edge coordinates with the same
    /// radius for all corners.
    #[inline]
    #[must_use]
    pub fn from_ltrb_r(
        left: Pixels,
        top: Pixels,
        right: Pixels,
        bottom: Pixels,
        radius: Radius<Pixels>,
    ) -> Self {
        Self {
            rect: Rect::from_ltrb(left, top, right, bottom),
            tl_radius: radius,
            tr_radius: radius,
            br_radius: radius,
            bl_radius: radius,
        }
    }

    /// Creates a rounded superellipse from edge coordinates with separate
    /// x and y radii for all corners.
    #[inline]
    #[must_use]
    pub fn from_ltrb_xy(
        left: Pixels,
        top: Pixels,
        right: Pixels,
        bottom: Pixels,
        rx: Pixels,
        ry: Pixels,
    ) -> Self {
        let radius = Radius::new(rx, ry);
        Self::from_ltrb_r(left, top, right, bottom, radius)
    }

    /// Creates a rounded superellipse from edge coordinates with independent
    /// corner radii.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn from_ltrb_and_corners(
        left: Pixels,
        top: Pixels,
        right: Pixels,
        bottom: Pixels,
        tl: Radius<Pixels>,
        tr: Radius<Pixels>,
        br: Radius<Pixels>,
        bl: Radius<Pixels>,
    ) -> Self {
        Self {
            rect: Rect::from_ltrb(left, top, right, bottom),
            tl_radius: tl,
            tr_radius: tr,
            br_radius: br,
            bl_radius: bl,
        }
    }

    /// Creates a rounded superellipse from a rectangle with the same radius
    /// for all corners.
    #[inline]
    #[must_use]
    pub fn from_rect_and_radius(rect: Rect<Pixels>, radius: Radius<Pixels>) -> Self {
        Self {
            rect,
            tl_radius: radius,
            tr_radius: radius,
            br_radius: radius,
            bl_radius: radius,
        }
    }

    /// Creates a rounded superellipse from a rectangle with independent
    /// corner radii.
    #[inline]
    #[must_use]
    pub fn from_rect_and_corners(
        rect: Rect<Pixels>,
        tl: Radius<Pixels>,
        tr: Radius<Pixels>,
        br: Radius<Pixels>,
        bl: Radius<Pixels>,
    ) -> Self {
        Self {
            rect,
            tl_radius: tl,
            tr_radius: tr,
            br_radius: br,
            bl_radius: bl,
        }
    }

    /// Creates a rounded superellipse from a rectangle with a circular radius
    /// for all corners.
    #[inline]
    #[must_use]
    pub fn from_rect_circular(rect: Rect<Pixels>, radius: Pixels) -> Self {
        Self::from_rect_and_radius(rect, Radius::circular(radius))
    }

    // ========================================================================
    // Properties
    // ========================================================================

    /// Returns the bounding rectangle.
    #[inline]
    #[must_use]
    pub fn outer_rect(&self) -> Rect<Pixels> {
        self.rect
    }

    /// Returns the left edge x-coordinate.
    #[inline]
    #[must_use]
    pub fn left(&self) -> Pixels {
        self.rect.left()
    }

    /// Returns the top edge y-coordinate.
    #[inline]
    #[must_use]
    pub fn top(&self) -> Pixels {
        self.rect.top()
    }

    /// Returns the right edge x-coordinate.
    #[inline]
    #[must_use]
    pub fn right(&self) -> Pixels {
        self.rect.right()
    }

    /// Returns the bottom edge y-coordinate.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> Pixels {
        self.rect.bottom()
    }

    /// Returns the width of the bounding rectangle.
    #[inline]
    #[must_use]
    pub fn width(&self) -> Pixels {
        self.rect.width()
    }

    /// Returns the height of the bounding rectangle.
    #[inline]
    #[must_use]
    pub fn height(&self) -> Pixels {
        self.rect.height()
    }

    /// Returns the top-left corner radius.
    #[inline]
    #[must_use]
    pub fn tl_radius(&self) -> Radius<Pixels> {
        self.tl_radius
    }

    /// Returns the top-right corner radius.
    #[inline]
    #[must_use]
    pub fn tr_radius(&self) -> Radius<Pixels> {
        self.tr_radius
    }

    /// Returns the bottom-right corner radius.
    #[inline]
    #[must_use]
    pub fn br_radius(&self) -> Radius<Pixels> {
        self.br_radius
    }

    /// Returns the bottom-left corner radius.
    #[inline]
    #[must_use]
    pub fn bl_radius(&self) -> Radius<Pixels> {
        self.bl_radius
    }

    /// Checks if all four corner radii are equal.
    #[inline]
    #[must_use]
    pub fn has_uniform_corners(&self) -> bool {
        self.tl_radius == self.tr_radius
            && self.tr_radius == self.br_radius
            && self.br_radius == self.bl_radius
    }

    /// Checks if every corner radius is circular (x equals y).
    #[inline]
    #[must_use]
    pub fn has_circular_corners(&self) -> bool {
        self.tl_radius.is_circular()
            && self.tr_radius.is_circular()
            && self.br_radius.is_circular()
            && self.bl_radius.is_circular()
    }

    /// Checks if all corner radii are zero (a plain rectangle).
    #[inline]
    #[must_use]
    pub fn is_rect(&self) -> bool {
        self.tl_radius.is_zero()
            && self.tr_radius.is_zero()
            && self.br_radius.is_zero()
            && self.bl_radius.is_zero()
    }

    /// Checks if the bounding rectangle is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rect.is_empty()
    }

    // ========================================================================
    // Safe inner rectangles
    // ========================================================================

    /// Returns the largest axis-aligned rectangle guaranteed to lie inside
    /// the shape.
    ///
    /// Each side is inset by the maximum corner radius component along that
    /// side, so the result avoids all four corner regions.
    #[inline]
    #[must_use]
    pub fn safe_inner_rect(&self) -> Rect<Pixels> {
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

    /// Returns the widest inner rectangle spanning the full width.
    ///
    /// Only the top and bottom edges are inset (by the maximum vertical
    /// corner radii); the left and right edges match the bounding rectangle.
    #[inline]
    #[must_use]
    pub fn wide_middle_rect(&self) -> Rect<Pixels> {
        let inset_top = self.tl_radius.y.max(self.tr_radius.y);
        let inset_bottom = self.bl_radius.y.max(self.br_radius.y);

        Rect::from_ltrb(
            self.left(),
            self.top() + inset_top,
            self.right(),
            self.bottom() - inset_bottom,
        )
    }

    /// Returns the tallest inner rectangle spanning the full height.
    ///
    /// Only the left and right edges are inset (by the maximum horizontal
    /// corner radii); the top and bottom edges match the bounding rectangle.
    #[inline]
    #[must_use]
    pub fn tall_middle_rect(&self) -> Rect<Pixels> {
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

    /// Returns a copy inflated by `delta` on all sides, with corner radii
    /// grown by the same amount.
    #[inline]
    #[must_use]
    pub fn inflate(&self, delta: Pixels) -> Self {
        Self {
            rect: self.rect.inflate(delta, delta),
            tl_radius: Radius::new(self.tl_radius.x + delta, self.tl_radius.y + delta),
            tr_radius: Radius::new(self.tr_radius.x + delta, self.tr_radius.y + delta),
            br_radius: Radius::new(self.br_radius.x + delta, self.br_radius.y + delta),
            bl_radius: Radius::new(self.bl_radius.x + delta, self.bl_radius.y + delta),
        }
    }

    /// Returns a copy deflated by `delta` on all sides, with corner radii
    /// shrunk by the same amount.
    #[inline]
    #[must_use]
    pub fn deflate(&self, delta: Pixels) -> Self {
        self.inflate(-delta)
    }

    /// Returns a copy with the bounding rectangle and all corner radii scaled
    /// by `factor`.
    ///
    /// Edge coordinates are scaled about the origin, not the shape's center.
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

    #[inline]
    #[allow(dead_code, clippy::unused_self)] // Helper for future contains() implementation
    fn point_in_corner(&self, dx: f32, dy: f32, radius: Radius<Pixels>) -> bool {
        // Superellipse exponent (2.5 approximates iOS squircle)
        const N: f32 = 2.5;

        if radius.x <= px(0.0) || radius.y <= px(0.0) {
            return true; // Sharp corner, already passed bbox check
        }

        let nx = (dx.abs() / radius.x.0).powf(N);
        let ny = (dy.abs() / radius.y.0).powf(N);

        nx + ny <= 1.0
    }

    // ========================================================================
    // Interpolation
    // ========================================================================

    /// Linearly interpolates between two rounded superellipses.
    ///
    /// The bounding rectangle and each corner radius are interpolated
    /// component-wise.
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

impl From<Rect<Pixels>> for RSuperellipse {
    /// Creates a superellipse with zero radii (plain rectangle).
    fn from(rect: Rect<Pixels>) -> Self {
        Self::from_rect_and_radius(rect, Radius::ZERO)
    }
}
