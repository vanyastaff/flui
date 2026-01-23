//! Rounded rectangle type.
//!
//! API design inspired by Flutter and kurbo.

use super::{px, Pixels};
use super::{Offset, Point, Rect, Size, Vec2};
use super::traits::{NumericUnit, Unit};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Radius<T: Unit> {
    /// Horizontal radius.
    pub x: T,
    /// Vertical radius.
    pub y: T,
}

// ============================================================================
// Constants (generic over Unit)
// ============================================================================

impl<T: Unit> Radius<T> {
    #[inline]
    pub fn zero() -> Self {
        Self {
            x: T::zero(),
            y: T::zero(),
        }
    }
}

// ============================================================================
// Pixels-specific Constants
// ============================================================================

impl Radius<Pixels> {
    pub const ZERO: Self = Self {
        x: Pixels::ZERO,
        y: Pixels::ZERO,
    };
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> Radius<T> {
    #[must_use]
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub const fn circular(r: T) -> Self {
        Self::new(r, r)
    }

    #[must_use]
    pub const fn elliptical(x: T, y: T) -> Self {
        Self::new(x, y)
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.x == T::zero() && self.y == T::zero()
    }

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
    T: std::ops::Mul<f32, Output = T>,
{
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }

    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self::new(
            a.x * (1.0 - t) + b.x * t,
            a.y * (1.0 - t) + b.y * t,
        )
    }
}

impl<T: NumericUnit + PartialOrd> Radius<T> {
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

// ============================================================================
// Default Implementation
// ============================================================================

impl<T: Unit> Default for Radius<T> {
    fn default() -> Self {
        Self::new(T::zero(), T::zero())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RRect {
    /// The base rectangle.
    pub rect: Rect<Pixels>,
    /// Top-left corner radius.
    pub top_left: Radius<Pixels>,
    /// Top-right corner radius.
    pub top_right: Radius<Pixels>,
    /// Bottom-right corner radius.
    pub bottom_right: Radius<Pixels>,
    /// Bottom-left corner radius.
    pub bottom_left: Radius<Pixels>,
}

// ============================================================================
// Constructors
// ============================================================================

impl RRect {
    #[must_use]
    pub const fn new(
        rect: Rect<Pixels>,
        top_left: Radius<Pixels>,
        top_right: Radius<Pixels>,
        bottom_right: Radius<Pixels>,
        bottom_left: Radius<Pixels>,
    ) -> Self {
        Self {
            rect,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    #[must_use]
    pub const fn from_rect_and_radius(rect: Rect<Pixels>, radius: Radius<Pixels>) -> Self {
        Self::new(rect, radius, radius, radius, radius)
    }

    #[must_use]
    pub const fn from_rect_circular(rect: Rect<Pixels>, radius: Pixels) -> Self {
        Self::from_rect_and_radius(rect, Radius::circular(radius))
    }

    #[must_use]
    pub const fn from_rect_elliptical(rect: Rect<Pixels>, radius_x: Pixels, radius_y: Pixels) -> Self {
        Self::from_rect_and_radius(rect, Radius::elliptical(radius_x, radius_y))
    }

    #[must_use]
    pub const fn from_rect_xy(rect: Rect<Pixels>, radius_x: Pixels, radius_y: Pixels) -> Self {
        Self::from_rect_elliptical(rect, radius_x, radius_y)
    }

    #[must_use]
    pub const fn from_rect_and_corners(
        rect: Rect<Pixels>,
        top_left: Radius<Pixels>,
        top_right: Radius<Pixels>,
        bottom_right: Radius<Pixels>,
        bottom_left: Radius<Pixels>,
    ) -> Self {
        Self::new(rect, top_left, top_right, bottom_right, bottom_left)
    }

    #[must_use]
    pub fn from_xywh_circular(x: Pixels, y: Pixels, width: Pixels, height: Pixels, radius: Pixels) -> Self {
        Self::from_rect_circular(Rect::from_xywh(x, y, width, height), radius)
    }

    #[must_use]
    pub const fn from_rect(rect: Rect<Pixels>) -> Self {
        Self::from_rect_and_radius(rect, Radius::ZERO)
    }
}

// ============================================================================
// Accessors
// ============================================================================

impl RRect {
    #[must_use]
    pub fn left(&self) -> Pixels {
        self.rect.left()
    }

    #[must_use]
    pub fn top(&self) -> Pixels {
        self.rect.top()
    }

    #[must_use]
    pub fn right(&self) -> Pixels {
        self.rect.right()
    }

    #[must_use]
    pub fn bottom(&self) -> Pixels {
        self.rect.bottom()
    }

    #[must_use]
    pub fn width(&self) -> Pixels {
        self.rect.width()
    }

    #[must_use]
    pub fn height(&self) -> Pixels {
        self.rect.height()
    }

    #[must_use]
    pub fn size(&self) -> Size<Pixels> {
        self.rect.size()
    }

    #[must_use]
    pub fn center(&self) -> Point<Pixels> {
        self.rect.center()
    }

    #[must_use]
    pub const fn bounding_rect(&self) -> Rect<Pixels> {
        self.rect
    }
}

// ============================================================================
// Queries
// ============================================================================

impl RRect {
    #[must_use]
    pub fn is_rect(&self) -> bool {
        self.top_left.is_zero()
            && self.top_right.is_zero()
            && self.bottom_right.is_zero()
            && self.bottom_left.is_zero()
    }

    #[must_use]
    pub fn is_circular(&self) -> bool {
        self.top_left.is_circular()
            && self.top_right.is_circular()
            && self.bottom_right.is_circular()
            && self.bottom_left.is_circular()
    }

    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.top_left == self.top_right
            && self.top_right == self.bottom_right
            && self.bottom_right == self.bottom_left
    }

    #[must_use]
    pub fn has_rounding(&self) -> bool {
        !self.is_rect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rect.is_empty()
    }

    #[must_use]
    pub fn max_radius(&self) -> Pixels {
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

    #[must_use]
    pub fn area(&self) -> Pixels {
        if self.is_rect() {
            return px(self.rect.area());
        }

        let rect_area = self.rect.area();
        let corner_cutout = |r: Radius<Pixels>| -> Pixels { r.x * r.y * px(1.0 - std::f32::consts::FRAC_PI_4) };

        px(rect_area
            - corner_cutout(self.top_left).0
            - corner_cutout(self.top_right).0
            - corner_cutout(self.bottom_right).0
            - corner_cutout(self.bottom_left).0)
    }
}

// ============================================================================
// Hit Testing
// ============================================================================

impl RRect {

}

// ============================================================================
// Transformations
// ============================================================================

impl RRect {
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

    #[must_use]
    pub fn inflate(&self, delta: Pixels) -> Self {
        Self::new(
            self.rect.inflate(delta, delta),
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        )
    }

    #[must_use]
    pub fn inset(&self, delta: Pixels) -> Self {
        self.inflate(-delta)
    }



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

    #[must_use]
    pub fn corner_centers(&self) -> [Point<Pixels>; 4] {
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

impl From<Rect<Pixels>> for RRect {
    fn from(rect: Rect<Pixels>) -> Self {
        Self::from_rect(rect)
    }
}

// ============================================================================
// Tests
// ============================================================================
