//! Bounds type - axis-aligned bounding rectangles.
//!
//! This module provides [`Bounds`] as a semantic alternative to [`Rect`].
//! Following GPUI's convention, bounds emphasize "origin + size" rather than
//! positional coordinates.
//!
//! # Type Safety
//!
//! `Bounds<T>` is generic over unit type `T`, preventing accidental mixing
//! of coordinate systems:
//!
//! ```ignore
//! use flui_types::geometry::{Bounds, Point, Size, Pixels, DevicePixels, px, device_px};
//!
//! let ui_bounds = Bounds::<Pixels>::new(
//!     Point::new(px(10.0), px(20.0)),
//!     Size::new(px(100.0), px(50.0))
//! );
//!
//! let device_bounds = Bounds::<DevicePixels>::new(
//!     Point::new(device_px(80), device_px(160)),
//!     Size::new(device_px(800), device_px(400))
//! );
//!
//! // These are different types - can't accidentally mix them!
//! ```
//!
//! # Spatial Operations
//!
//! Bounds supports spatial relationship queries:
//! - [`intersects()`](Bounds::intersects) - Check if bounds overlap
//! - [`intersect()`](Bounds::intersect) - Compute intersection
//! - [`union()`](Bounds::union) - Compute union (smallest containing bounds)
//! - [`contains()`](Bounds::contains) - Point-in-bounds test
//!
//! # Examples
//!
//! ```
//! use flui_types::geometry::{bounds, point, size};
//!
//! let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
//! let b = bounds(point(5.0, 5.0), size(10.0, 10.0));
//!
//! assert!(a.intersects(&b));
//! let intersection = a.intersect(&b);
//! assert_eq!(intersection.size.width, 5.0);
//! ```

use super::Pixels;
use super::traits::{NumericUnit, Unit};
use super::{Corner, Edges, Point, Rect, Size};
use std::fmt::{self, Display};
use std::ops::{Add, Div, Mul, Sub};

#[repr(C)]
pub struct Bounds<T: Unit> {
    /// The origin point (top-left corner).
    pub origin: Point<T>,
    /// The size.
    pub size: Size<T>,
}

impl<T: Unit> Default for Bounds<T> {
    fn default() -> Self {
        Self {
            origin: Point::new(T::zero(), T::zero()),
            size: Size::new(T::zero(), T::zero()),
        }
    }
}

#[must_use]

// ============================================================================
// Generic constructors
// ============================================================================

impl<T: Unit> Bounds<T> {
    #[must_use]
    pub const fn new(origin: Point<T>, size: Size<T>) -> Self {
        Self { origin, size }
    }

    #[must_use]
    pub fn map<U: Unit>(&self, f: impl Fn(T) -> U + Copy) -> Bounds<U>
    where
        T: Clone + fmt::Debug + Default + PartialEq,
    {
        Bounds {
            origin: self.origin.map(f),
            size: self.size.map(f),
        }
    }

    #[must_use]
    pub fn map_origin(self, f: impl Fn(T) -> T) -> Self
    where
        T: Clone + fmt::Debug + Default + PartialEq,
    {
        Self {
            origin: self.origin.map(f),
            size: self.size,
        }
    }

    #[must_use]
    pub fn map_size(self, f: impl Fn(T) -> T) -> Self
    where
        T: Clone,
    {
        Self {
            origin: self.origin,
            size: self.size.map(f),
        }
    }
}

// ============================================================================
// Alternative constructors (with Sub trait bound)
// ============================================================================

impl<T: Unit> Bounds<T>
where
    T: Sub<Output = T>,
{
    #[must_use]
    pub fn from_corners(top_left: Point<T>, bottom_right: Point<T>) -> Self {
        Self {
            origin: Point::new(top_left.x, top_left.y),
            size: Size::new(bottom_right.x - top_left.x, bottom_right.y - top_left.y),
        }
    }

    #[must_use]
    pub fn from_corner_and_size(corner: Corner, origin: Point<T>, size: Size<T>) -> Self {
        let origin = match corner {
            Corner::TopLeft => origin,
            Corner::TopRight => Point::new(origin.x - size.width, origin.y),
            Corner::BottomLeft => Point::new(origin.x, origin.y - size.height),
            Corner::BottomRight => Point::new(origin.x - size.width, origin.y - size.height),
        };
        Self { origin, size }
    }
}

impl<T: Unit> Bounds<T>
where
    T: Add<T, Output = T> + Sub<T, Output = T> + Div<f32, Output = T>,
{
    #[must_use]
    pub fn centered_at(center: Point<T>, size: Size<T>) -> Self {
        Self {
            origin: Point::new(center.x - size.width / 2.0, center.y - size.height / 2.0),
            size,
        }
    }
}

// ============================================================================
// Corner and edge accessors
// ============================================================================

impl<T: Unit> Bounds<T>
where
    T: Add<T, Output = T>,
{
    #[must_use]
    pub fn top(&self) -> T {
        self.origin.y
    }

    #[must_use]
    pub fn bottom(&self) -> T {
        self.origin.y + self.size.height
    }

    #[must_use]
    pub fn left(&self) -> T {
        self.origin.x
    }

    #[must_use]
    pub fn right(&self) -> T {
        self.origin.x + self.size.width
    }

    #[must_use]
    pub fn top_left(&self) -> Point<T> {
        self.origin
    }

    #[must_use]
    pub fn top_right(&self) -> Point<T> {
        Point::new(self.origin.x + self.size.width, self.origin.y)
    }

    #[must_use]
    pub fn bottom_left(&self) -> Point<T> {
        Point::new(self.origin.x, self.origin.y + self.size.height)
    }

    #[must_use]
    pub fn bottom_right(&self) -> Point<T> {
        Point::new(self.origin.x + self.size.width, self.origin.y + self.size.height)
    }

    #[must_use]
    pub fn corner(&self, corner: Corner) -> Point<T> {
        match corner {
            Corner::TopLeft => self.top_left(),
            Corner::TopRight => self.top_right(),
            Corner::BottomLeft => self.bottom_left(),
            Corner::BottomRight => self.bottom_right(),
        }
    }

    #[must_use]
    pub fn center(&self) -> Point<T>
    where
        T: Div<f32, Output = T>,
    {
        Point::new(
            self.origin.x + self.size.width / 2.0,
            self.origin.y + self.size.height / 2.0,
        )
    }
}

// ============================================================================
// Spatial relationship queries
// ============================================================================

impl<T: Unit> Bounds<T>
where
    T: PartialOrd + Add<T, Output = T>,
{
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        let my_br = self.bottom_right();
        let their_br = other.bottom_right();

        self.origin.x < their_br.x
            && my_br.x > other.origin.x
            && self.origin.y < their_br.y
            && my_br.y > other.origin.y
    }

    #[must_use]
    pub fn contains(&self, point: &Point<T>) -> bool {
        point.x >= self.origin.x
            && point.x <= self.origin.x + self.size.width
            && point.y >= self.origin.y
            && point.y <= self.origin.y + self.size.height
    }

    #[must_use]
    pub fn is_contained_within(&self, other: &Self) -> bool {
        other.contains(&self.origin) && other.contains(&self.bottom_right())
    }
}

impl<T: Unit> Bounds<T>
where
    T: PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[must_use]
    pub fn intersect(&self, other: &Self) -> Self {
        let top_left = Point::new(
            if self.origin.x >= other.origin.x {
                self.origin.x
            } else {
                other.origin.x
            },
            if self.origin.y >= other.origin.y {
                self.origin.y
            } else {
                other.origin.y
            },
        );

        let my_br = self.bottom_right();
        let their_br = other.bottom_right();

        let bottom_right = Point::new(
            if my_br.x <= their_br.x {
                my_br.x
            } else {
                their_br.x
            },
            if my_br.y <= their_br.y {
                my_br.y
            } else {
                their_br.y
            },
        );

        Self::from_corners(top_left, bottom_right)
    }

    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        let top_left = Point::new(
            if self.origin.x <= other.origin.x {
                self.origin.x
            } else {
                other.origin.x
            },
            if self.origin.y <= other.origin.y {
                self.origin.y
            } else {
                other.origin.y
            },
        );

        let my_br = self.bottom_right();
        let their_br = other.bottom_right();

        let bottom_right = Point::new(
            if my_br.x >= their_br.x {
                my_br.x
            } else {
                their_br.x
            },
            if my_br.y >= their_br.y {
                my_br.y
            } else {
                their_br.y
            },
        );

        Self::from_corners(top_left, bottom_right)
    }

    #[must_use]
    pub fn localize(&self, point: &Point<T>) -> Option<Point<T>> {
        if self.contains(point) {
            Some(Point::new(point.x - self.origin.x, point.y - self.origin.y))
        } else {
            None
        }
    }
}

// ============================================================================
// Expansion and contraction
// ============================================================================

impl<T: Unit> Bounds<T>
where
    T: Add<T, Output = T> + Sub<T, Output = T>,
{
    #[must_use]
    pub fn dilate(&self, amount: T) -> Self {
        let double_amount = amount + amount;
        Self {
            origin: Point::new(self.origin.x - amount, self.origin.y - amount),
            size: Size::new(
                self.size.width + double_amount,
                self.size.height + double_amount,
            ),
        }
    }

    #[must_use]
    pub fn extend(&self, amount: Edges<T>) -> Self {
        Self {
            origin: Point::new(self.origin.x - amount.left, self.origin.y - amount.top),
            size: Size::new(
                self.size.width + amount.left + amount.right,
                self.size.height + amount.top + amount.bottom,
            ),
        }
    }

    #[must_use]
    pub fn inset(&self, amount: T) -> Self
    where
        T: std::ops::Neg<Output = T>,
    {
        self.dilate(-amount)
    }

    #[must_use]
    pub fn space_within(&self, outer: &Self) -> Edges<T> {
        Edges {
            top: self.top() - outer.top(),
            right: outer.right() - self.right(),
            bottom: outer.bottom() - self.bottom(),
            left: self.left() - outer.left(),
        }
    }
}

// ============================================================================
// Arithmetic operators
// ============================================================================

impl<T, Rhs> Mul<Rhs> for Bounds<T>
where
    T: Unit + Mul<Rhs, Output = T> + Clone + fmt::Debug + Default + PartialEq,
    Rhs: Copy,
{
    type Output = Bounds<T>;

    #[inline]
    fn mul(self, rhs: Rhs) -> Self::Output {
        Bounds {
            origin: self.origin * rhs,
            size: self.size * rhs,
        }
    }
}

impl<T, Rhs> Div<Rhs> for Bounds<T>
where
    T: Unit + Div<Rhs, Output = T> + Clone + fmt::Debug + Default + PartialEq,
    Rhs: Copy,
{
    type Output = Bounds<T>;

    #[inline]
    fn div(self, rhs: Rhs) -> Self::Output {
        Bounds {
            origin: self.origin / rhs,
            size: self.size / rhs,
        }
    }
}

impl<T: Unit> Add<Point<T>> for Bounds<T>
where
    T: Add<T, Output = T>,
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: Point<T>) -> Self {
        Self {
            origin: Point::new(self.origin.x + rhs.x, self.origin.y + rhs.y),
            size: self.size,
        }
    }
}

impl<T: Unit> Sub<Point<T>> for Bounds<T>
where
    T: Sub<T, Output = T>,
{
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Point<T>) -> Self {
        Self {
            origin: Point::new(self.origin.x - rhs.x, self.origin.y - rhs.y),
            size: self.size,
        }
    }
}

// ============================================================================
// f32-specific Display
// ============================================================================

impl Display for Bounds<Pixels> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} - {} (size {})",
            self.origin,
            self.bottom_right(),
            self.size
        )
    }
}

// ============================================================================
// Conversions (f32 only)
// ============================================================================

impl<T: NumericUnit> From<Rect<T>> for Bounds<T>
where
    T: std::ops::Sub<Output = T>,
{
    #[inline]
    fn from(rect: Rect<T>) -> Self {
        Self {
            origin: rect.origin(),
            size: rect.size(),
        }
    }
}

impl<T: Unit + NumericUnit> From<Bounds<T>> for Rect<T> {
    #[inline]
    fn from(bounds: Bounds<T>) -> Self {
        Rect::from_origin_size(bounds.origin, bounds.size)
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Bounds<super::units::Pixels> {
    #[must_use]
    pub fn scale(&self, factor: f32) -> Bounds<super::units::ScaledPixels> {
        Bounds {
            origin: self.origin.scale(factor),
            size: self.size.scale(factor),
        }
    }
}

// ============================================================================
// Specialized implementations for ScaledPixels
// ============================================================================

impl Bounds<super::units::ScaledPixels> {
    #[must_use]
    pub fn to_device_pixels(&self) -> Bounds<super::units::DevicePixels> {
        Bounds {
            origin: self.origin.to_device_pixels(),
            size: self.size.to_device_pixels(),
        }
    }
}

// ============================================================================
// Trait Implementations
// ============================================================================

impl<T: Unit> super::traits::IsZero for Bounds<T>
where
    T: super::traits::IsZero
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.origin.is_zero() && self.size.is_zero()
    }
}

impl<T: Unit> super::traits::ApproxEq for Bounds<T>
where
    T: super::traits::ApproxEq
{
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        self.origin.approx_eq_eps(&other.origin, epsilon)
            && self.size.width.approx_eq_eps(&other.size.width, epsilon)
            && self.size.height.approx_eq_eps(&other.size.height, epsilon)
    }
}

// ============================================================================
// Additional utility methods (generic)
// ============================================================================

impl<T: Unit> Bounds<T> {
    #[must_use]
    pub fn is_empty(&self) -> bool
    where
        T: super::traits::IsZero + PartialOrd,
    {
        self.size.width.is_zero() || self.size.height.is_zero()
            || self.size.width < T::zero() || self.size.height < T::zero()
    }

    #[must_use]
    pub fn cast<U: Unit>(self) -> Bounds<U>
    where
        T: Into<U>,
    {
        Bounds {
            origin: self.origin.cast(),
            size: self.size.cast(),
        }
    }

    #[must_use]
    pub fn to_f32(self) -> Bounds<Pixels>
    where
        T: Into<f32>,
    {
        Bounds {
            origin: Point::new(Pixels(self.origin.x.into()), Pixels(self.origin.y.into())),
            size: Size::new(Pixels(self.size.width.into()), Pixels(self.size.height.into())),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================
