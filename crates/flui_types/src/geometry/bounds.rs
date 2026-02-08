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

use super::traits::{NumericUnit, Unit};
use super::Pixels;
use super::{Corner, Edges, Point, Rect, Size};
use std::fmt::{self, Display};
use std::ops::{Add, Div, Mul, Sub};

/// Axis-aligned bounding rectangle defined by origin and size.
///
/// Generic over unit type `T` for type-safe coordinate system handling.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
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

// ============================================================================
// Generic constructors
// ============================================================================

impl<T: Unit> Bounds<T> {
    /// Creates new bounds from an origin point and size.
    #[inline]
    #[must_use]
    pub const fn new(origin: Point<T>, size: Size<T>) -> Self {
        Self { origin, size }
    }

    /// Maps the bounds to a different unit type by applying a function to each component.
    #[inline]
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

    /// Transforms the origin by applying a function while keeping the size unchanged.
    #[inline]
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

    /// Transforms the size by applying a function while keeping the origin unchanged.
    #[inline]
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
    /// Creates bounds from two corner points.
    ///
    /// Computes the size from the difference between bottom-right and top-left corners.
    #[inline]
    #[must_use]
    pub fn from_corners(top_left: Point<T>, bottom_right: Point<T>) -> Self {
        Self {
            origin: Point::new(top_left.x, top_left.y),
            size: Size::new(bottom_right.x - top_left.x, bottom_right.y - top_left.y),
        }
    }

    /// Creates bounds from a specific corner point and size.
    ///
    /// The corner parameter determines which corner the origin point represents.
    #[inline]
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
    /// Creates bounds centered at the given point with the specified size.
    #[inline]
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
    /// Returns the Y coordinate of the top edge.
    #[inline]
    #[must_use]
    pub fn top(&self) -> T {
        self.origin.y
    }

    /// Returns the Y coordinate of the bottom edge.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> T {
        self.origin.y + self.size.height
    }

    /// Returns the X coordinate of the left edge.
    #[inline]
    #[must_use]
    pub fn left(&self) -> T {
        self.origin.x
    }

    /// Returns the X coordinate of the right edge.
    #[inline]
    #[must_use]
    pub fn right(&self) -> T {
        self.origin.x + self.size.width
    }

    /// Returns the top-left corner point.
    #[inline]
    #[must_use]
    pub fn top_left(&self) -> Point<T> {
        self.origin
    }

    /// Returns the top-right corner point.
    #[inline]
    #[must_use]
    pub fn top_right(&self) -> Point<T> {
        Point::new(self.origin.x + self.size.width, self.origin.y)
    }

    /// Returns the bottom-left corner point.
    #[inline]
    #[must_use]
    pub fn bottom_left(&self) -> Point<T> {
        Point::new(self.origin.x, self.origin.y + self.size.height)
    }

    /// Returns the bottom-right corner point.
    #[inline]
    #[must_use]
    pub fn bottom_right(&self) -> Point<T> {
        Point::new(
            self.origin.x + self.size.width,
            self.origin.y + self.size.height,
        )
    }

    /// Returns the point at the specified corner.
    #[inline]
    #[must_use]
    pub fn corner(&self, corner: Corner) -> Point<T> {
        match corner {
            Corner::TopLeft => self.top_left(),
            Corner::TopRight => self.top_right(),
            Corner::BottomLeft => self.bottom_left(),
            Corner::BottomRight => self.bottom_right(),
        }
    }

    /// Returns the center point of the bounds.
    #[inline]
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
    /// Checks if these bounds overlap with another bounds.
    ///
    /// Returns `true` if the two bounds share any area.
    #[inline]
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        let my_br = self.bottom_right();
        let their_br = other.bottom_right();

        self.origin.x < their_br.x
            && my_br.x > other.origin.x
            && self.origin.y < their_br.y
            && my_br.y > other.origin.y
    }

    /// Checks if the given point is inside these bounds.
    ///
    /// Returns `true` if the point is within or on the edges of the bounds.
    #[inline]
    #[must_use]
    pub fn contains(&self, point: &Point<T>) -> bool {
        point.x >= self.origin.x
            && point.x <= self.origin.x + self.size.width
            && point.y >= self.origin.y
            && point.y <= self.origin.y + self.size.height
    }

    /// Checks if these bounds are completely contained within another bounds.
    #[inline]
    #[must_use]
    pub fn is_contained_within(&self, other: &Self) -> bool {
        other.contains(&self.origin) && other.contains(&self.bottom_right())
    }
}

impl<T: Unit> Bounds<T>
where
    T: PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    /// Computes the intersection of these bounds with another bounds.
    ///
    /// Returns a bounds representing the overlapping area. If there is no overlap,
    /// the returned bounds may have negative or zero size.
    #[inline]
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

    /// Computes the union of these bounds with another bounds.
    ///
    /// Returns the smallest bounds that completely contains both input bounds.
    #[inline]
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

    /// Converts a global point to local coordinates relative to these bounds.
    ///
    /// Returns `Some(point)` with coordinates relative to the origin if the point is within
    /// the bounds, or `None` if the point is outside.
    #[inline]
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
    /// Expands the bounds uniformly in all directions by the specified amount.
    ///
    /// The origin moves outward by `amount`, and the size increases by `2 * amount`.
    #[inline]
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

    /// Extends the bounds by different amounts on each edge.
    ///
    /// The edges parameter specifies how much to expand each side.
    #[inline]
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

    /// Shrinks the bounds uniformly in all directions by the specified amount.
    ///
    /// This is the opposite of `dilate` - equivalent to `dilate(-amount)`.
    #[inline]
    #[must_use]
    pub fn inset(&self, amount: T) -> Self
    where
        T: std::ops::Neg<Output = T>,
    {
        self.dilate(-amount)
    }

    /// Computes the spacing between these bounds and an outer bounds.
    ///
    /// Returns edges representing the distance from each side of these bounds
    /// to the corresponding side of the outer bounds.
    #[inline]
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
    /// Scales the bounds by a factor, converting to scaled pixels.
    #[inline]
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
    /// Converts scaled pixel bounds to device pixels by rounding.
    #[inline]
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
    T: super::traits::IsZero,
{
    #[inline]
    fn is_zero(&self) -> bool {
        self.origin.is_zero() && self.size.is_zero()
    }
}

impl<T: Unit> super::traits::ApproxEq for Bounds<T>
where
    T: super::traits::ApproxEq,
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
    /// Returns `true` if the bounds have zero or negative area.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool
    where
        T: super::traits::IsZero + PartialOrd,
    {
        self.size.width.is_zero()
            || self.size.height.is_zero()
            || self.size.width < T::zero()
            || self.size.height < T::zero()
    }

    /// Casts the bounds to a different unit type.
    #[inline]
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

    /// Converts the bounds to f32-based Pixels.
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Bounds<Pixels>
    where
        T: Into<f32>,
    {
        Bounds {
            origin: Point::new(Pixels(self.origin.x.into()), Pixels(self.origin.y.into())),
            size: Size::new(
                Pixels(self.size.width.into()),
                Pixels(self.size.height.into()),
            ),
        }
    }
}
