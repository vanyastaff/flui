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

use super::traits::Unit;
use super::{Corner, Edges, Point, Rect, Size};
use std::fmt::{self, Display};
use std::ops::{Add, Div, Mul, Sub};

/// An axis-aligned bounding rectangle defined by an origin point and size.
///
/// Generic over unit type `T` (defaults to `f32` for backwards compatibility).
/// Use `Bounds<Pixels>` for logical pixels, `Bounds<DevicePixels>` for device pixels, etc.
///
/// `Bounds` is semantically equivalent to [`Rect`] but emphasizes
/// the "origin + size" mental model, matching GPUI's naming convention.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{bounds, point, size};
///
/// let b = bounds(point(10.0, 20.0), size(100.0, 50.0));
/// assert_eq!(b.origin.x, 10.0);
/// assert_eq!(b.size.width, 100.0);
/// assert_eq!(b.center(), point(60.0, 45.0));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(C)]
pub struct Bounds<T: Unit = f32> {
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

/// Constructs a `Bounds` with the given origin and size (f32 only).
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{bounds, point, size};
///
/// let b = bounds(point(0.0, 0.0), size(100.0, 50.0));
/// ```
#[inline]
#[must_use]
pub const fn bounds(origin: Point<f32>, size: Size<f32>) -> Bounds<f32> {
    Bounds { origin, size }
}

// ============================================================================
// Generic constructors
// ============================================================================

impl<T: Unit> Bounds<T> {
    /// Creates a new `Bounds`.
    #[inline]
    #[must_use]
    pub const fn new(origin: Point<T>, size: Size<T>) -> Self {
        Self { origin, size }
    }

    /// Maps the bounds through a function.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(10.0, 20.0), size(100.0, 50.0));
    /// let doubled = b.map(|x| x * 2.0);
    /// assert_eq!(doubled.origin.x, 20.0);
    /// assert_eq!(doubled.size.width, 200.0);
    /// ```
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

    /// Maps only the origin, keeping size unchanged.
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

    /// Maps only the size, keeping origin unchanged.
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
    /// Constructs bounds from two opposite corner points.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Bounds, point};
    ///
    /// let b = Bounds::from_corners(point(10.0, 20.0), point(110.0, 70.0));
    /// assert_eq!(b.origin, point(10.0, 20.0));
    /// assert_eq!(b.size.width, 100.0);
    /// assert_eq!(b.size.height, 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_corners(top_left: Point<T>, bottom_right: Point<T>) -> Self {
        Self {
            origin: Point::new(top_left.x, top_left.y),
            size: Size::new(bottom_right.x - top_left.x, bottom_right.y - top_left.y),
        }
    }

    /// Constructs bounds from a specific corner and size.
    ///
    /// The `corner` parameter specifies which corner the `origin` represents.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Bounds, Corner, point, size};
    ///
    /// // Bottom-right corner at (100, 100), size 50x50
    /// let b = Bounds::from_corner_and_size(
    ///     Corner::BottomRight,
    ///     point(100.0, 100.0),
    ///     size(50.0, 50.0)
    /// );
    /// assert_eq!(b.origin, point(50.0, 50.0));
    /// ```
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
    /// Creates bounds centered at the given point with the given size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Bounds, point, size};
    ///
    /// let b = Bounds::centered_at(point(50.0, 50.0), size(20.0, 10.0));
    /// assert_eq!(b.origin, point(40.0, 45.0));
    /// assert_eq!(b.center(), point(50.0, 50.0));
    /// ```
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
    /// Returns the top edge y-coordinate.
    #[inline]
    #[must_use]
    pub fn top(&self) -> T {
        self.origin.y
    }

    /// Returns the bottom edge y-coordinate.
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> T {
        self.origin.y + self.size.height
    }

    /// Returns the left edge x-coordinate.
    #[inline]
    #[must_use]
    pub fn left(&self) -> T {
        self.origin.x
    }

    /// Returns the right edge x-coordinate.
    #[inline]
    #[must_use]
    pub fn right(&self) -> T {
        self.origin.x + self.size.width
    }

    /// Returns the top-left corner (same as origin).
    #[inline]
    #[must_use]
    pub fn top_left(&self) -> Point<T> {
        self.origin
    }

    /// Returns the top-right corner.
    #[inline]
    #[must_use]
    pub fn top_right(&self) -> Point<T> {
        Point::new(self.origin.x + self.size.width, self.origin.y)
    }

    /// Returns the bottom-left corner.
    #[inline]
    #[must_use]
    pub fn bottom_left(&self) -> Point<T> {
        Point::new(self.origin.x, self.origin.y + self.size.height)
    }

    /// Returns the bottom-right corner.
    #[inline]
    #[must_use]
    pub fn bottom_right(&self) -> Point<T> {
        Point::new(self.origin.x + self.size.width, self.origin.y + self.size.height)
    }

    /// Returns the coordinates of the specified corner.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size, Corner};
    ///
    /// let b = bounds(point(10.0, 20.0), size(100.0, 50.0));
    /// assert_eq!(b.corner(Corner::TopLeft), point(10.0, 20.0));
    /// assert_eq!(b.corner(Corner::BottomRight), point(110.0, 70.0));
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(0.0, 0.0), size(100.0, 50.0));
    /// assert_eq!(b.center(), point(50.0, 25.0));
    /// ```
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
    /// Checks if this bounds intersects with another bounds.
    ///
    /// Two bounds intersect if they overlap in 2D space.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
    /// let b = bounds(point(5.0, 5.0), size(10.0, 10.0));
    /// let c = bounds(point(20.0, 20.0), size(10.0, 10.0));
    ///
    /// assert!(a.intersects(&b));  // Overlapping
    /// assert!(!a.intersects(&c)); // Separate
    /// ```
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

    /// Checks if a point is inside this bounds.
    ///
    /// The bounds includes its edges (closed interval).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(0.0, 0.0), size(10.0, 10.0));
    ///
    /// assert!(b.contains(&point(5.0, 5.0)));
    /// assert!(b.contains(&point(0.0, 0.0))); // Edge included
    /// assert!(!b.contains(&point(15.0, 5.0)));
    /// ```
    #[inline]
    #[must_use]
    pub fn contains(&self, point: &Point<T>) -> bool {
        point.x >= self.origin.x
            && point.x <= self.origin.x + self.size.width
            && point.y >= self.origin.y
            && point.y <= self.origin.y + self.size.height
    }

    /// Checks if this bounds is completely contained within another.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let outer = bounds(point(0.0, 0.0), size(100.0, 100.0));
    /// let inner = bounds(point(10.0, 10.0), size(50.0, 50.0));
    ///
    /// assert!(inner.is_contained_within(&outer));
    /// assert!(!outer.is_contained_within(&inner));
    /// ```
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
    /// Calculates the intersection of two bounds.
    ///
    /// If the bounds don't intersect, returns a bounds with zero or negative size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
    /// let b = bounds(point(5.0, 5.0), size(10.0, 10.0));
    ///
    /// let intersection = a.intersect(&b);
    /// assert_eq!(intersection, bounds(point(5.0, 5.0), size(5.0, 5.0)));
    /// ```
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

    /// Computes the union (smallest bounds containing both).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
    /// let b = bounds(point(5.0, 5.0), size(15.0, 15.0));
    ///
    /// let union = a.union(&b);
    /// assert_eq!(union, bounds(point(0.0, 0.0), size(20.0, 20.0)));
    /// ```
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

    /// Converts a point to the coordinate space defined by this bounds.
    ///
    /// Returns `None` if the point is outside the bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(10.0, 20.0), size(100.0, 50.0));
    ///
    /// assert_eq!(b.localize(&point(15.0, 25.0)), Some(point(5.0, 5.0)));
    /// assert_eq!(b.localize(&point(200.0, 200.0)), None);
    /// ```
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
    /// Expands the bounds by the given amount in all directions.
    ///
    /// Also called "outset" - adds padding/margin around the bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(10.0, 10.0), size(10.0, 10.0));
    /// let expanded = b.dilate(5.0);
    ///
    /// assert_eq!(expanded, bounds(point(5.0, 5.0), size(20.0, 20.0)));
    /// ```
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

    /// Extends the bounds by different amounts in each direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, edges, point, size};
    ///
    /// let b = bounds(point(10.0, 10.0), size(10.0, 10.0));
    /// let extended = b.extend(edges(5.0, 3.0, 5.0, 3.0));
    ///
    /// assert_eq!(extended, bounds(point(7.0, 5.0), size(16.0, 20.0)));
    /// ```
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

    /// Shrinks the bounds by the given amount in all directions.
    ///
    /// Equivalent to `dilate` with negated amount. Opposite of [`dilate()`](Self::dilate).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(0.0, 0.0), size(20.0, 20.0));
    /// let inset = b.inset(5.0);
    ///
    /// assert_eq!(inset, bounds(point(5.0, 5.0), size(10.0, 10.0)));
    /// ```
    #[inline]
    #[must_use]
    pub fn inset(&self, amount: T) -> Self
    where
        T: std::ops::Neg<Output = T>,
    {
        self.dilate(-amount)
    }

    /// Calculates the space between this bounds and an outer bounds.
    ///
    /// Returns edges showing how much space is available in each direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, edges, point, size};
    ///
    /// let inner = bounds(point(10.0, 10.0), size(20.0, 20.0));
    /// let outer = bounds(point(0.0, 0.0), size(50.0, 50.0));
    ///
    /// let space = inner.space_within(&outer);
    /// assert_eq!(space.top, 10.0);    // 10 - 0
    /// assert_eq!(space.left, 10.0);   // 10 - 0
    /// assert_eq!(space.right, 20.0);  // 50 - 30
    /// assert_eq!(space.bottom, 20.0); // 50 - 30
    /// ```
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

impl Display for Bounds<f32> {
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

impl From<Rect> for Bounds<f32> {
    #[inline]
    fn from(rect: Rect) -> Self {
        Self {
            origin: rect.origin(),
            size: rect.size(),
        }
    }
}

impl From<Bounds<f32>> for Rect {
    #[inline]
    fn from(bounds: Bounds<f32>) -> Self {
        Rect::from_origin_size(bounds.origin, bounds.size)
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Bounds<super::units::Pixels> {
    /// Scales the bounds by a given factor, producing `Bounds<ScaledPixels>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Bounds, Point, Size, px};
    ///
    /// let bounds = Bounds::new(
    ///     Point::new(px(10.0), px(20.0)),
    ///     Size::new(px(100.0), px(200.0))
    /// );
    /// let scaled = bounds.scale(2.0);
    /// ```
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
    /// Converts to device pixels by rounding both origin and size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Bounds, Point, Size, scaled_px};
    ///
    /// let bounds = Bounds::new(
    ///     Point::new(scaled_px(10.5), scaled_px(20.3)),
    ///     Size::new(scaled_px(100.7), scaled_px(200.2))
    /// );
    /// let device = bounds.to_device_pixels();
    /// ```
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
    /// Returns true if the bounds has zero or negative area.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let empty = bounds(point(0.0, 0.0), size(0.0, 10.0));
    /// assert!(empty.is_empty());
    ///
    /// let valid = bounds(point(0.0, 0.0), size(10.0, 10.0));
    /// assert!(!valid.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool
    where
        T: super::traits::IsZero + PartialOrd,
    {
        self.size.width.is_zero() || self.size.height.is_zero()
            || self.size.width < T::zero() || self.size.height < T::zero()
    }

    /// Casts the bounds to a different unit type.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Bounds, Point, Size, Pixels, px};
    ///
    /// let px_bounds = Bounds::<Pixels>::new(
    ///     Point::new(px(10.0), px(20.0)),
    ///     Size::new(px(100.0), px(50.0))
    /// );
    /// let f32_bounds: Bounds<f32> = px_bounds.cast();
    /// assert_eq!(f32_bounds.origin.x, 10.0);
    /// ```
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

    /// Converts to f32 bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Bounds, Point, Size, Pixels, px};
    ///
    /// let px_bounds = Bounds::<Pixels>::new(
    ///     Point::new(px(10.0), px(20.0)),
    ///     Size::new(px(100.0), px(50.0))
    /// );
    /// let f32_bounds = px_bounds.to_f32();
    /// assert_eq!(f32_bounds.origin.x, 10.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_f32(self) -> Bounds<f32>
    where
        T: Into<f32>,
    {
        Bounds {
            origin: Point::new(self.origin.x.into(), self.origin.y.into()),
            size: Size::new(self.size.width.into(), self.size.height.into()),
        }
    }
}

impl Bounds<f32> {
    /// A bounds with zero origin and zero size.
    pub const ZERO: Self = Self {
        origin: Point::ORIGIN,
        size: Size::ZERO,
    };

    /// Returns the area of the bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(0.0, 0.0), size(10.0, 5.0));
    /// assert_eq!(b.area(), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        self.size.width * self.size.height
    }

    /// Returns the perimeter of the bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let b = bounds(point(0.0, 0.0), size(10.0, 5.0));
    /// assert_eq!(b.perimeter(), 30.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn perimeter(&self) -> f32 {
        2.0 * (self.size.width + self.size.height)
    }

    /// Returns true if the bounds is valid (positive size and finite values).
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.origin.is_valid()
            && self.size.width.is_finite()
            && self.size.height.is_finite()
            && self.size.width >= 0.0
            && self.size.height >= 0.0
    }

    /// Linear interpolation between two bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{bounds, point, size};
    ///
    /// let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
    /// let b = bounds(point(10.0, 10.0), size(20.0, 20.0));
    /// let mid = a.lerp(&b, 0.5);
    /// assert_eq!(mid.origin.x, 5.0);
    /// assert_eq!(mid.size.width, 15.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            origin: self.origin.lerp(other.origin, t),
            size: self.size.lerp(other.size, t),
        }
    }

    /// Rounds the origin and size to integer values.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Self {
        Self {
            origin: self.origin.round(),
            size: self.size.round(),
        }
    }

    /// Floors the origin and ceilings the size (expands to contain original).
    #[inline]
    #[must_use]
    pub fn expand_to_integers(&self) -> Self {
        let min = Point::new(self.origin.x.floor(), self.origin.y.floor());
        let max = Point::new(
            (self.origin.x + self.size.width).ceil(),
            (self.origin.y + self.size.height).ceil(),
        );
        Self::from_corners(min, max)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{point, size};

    #[test]
    fn test_bounds_creation() {
        let b = bounds(point(10.0, 20.0), size(100.0, 50.0));
        assert_eq!(b.origin.x, 10.0);
        assert_eq!(b.origin.y, 20.0);
        assert_eq!(b.size.width, 100.0);
        assert_eq!(b.size.height, 50.0);
    }

    #[test]
    fn test_bounds_from_corners() {
        let b = Bounds::from_corners(point(10.0, 20.0), point(110.0, 70.0));
        assert_eq!(b.origin, point(10.0, 20.0));
        assert_eq!(b.size, size(100.0, 50.0));
    }

    #[test]
    fn test_bounds_from_corner_and_size() {
        let b = Bounds::from_corner_and_size(
            Corner::BottomRight,
            point(100.0, 100.0),
            size(50.0, 50.0),
        );
        assert_eq!(b.origin, point(50.0, 50.0));
        assert_eq!(b.bottom_right(), point(100.0, 100.0));
    }

    #[test]
    fn test_bounds_centered_at() {
        let b = Bounds::centered_at(point(50.0, 50.0), size(20.0, 10.0));
        assert_eq!(b.origin, point(40.0, 45.0));
        assert_eq!(b.center(), point(50.0, 50.0));
    }

    #[test]
    fn test_corner_accessors() {
        let b = bounds(point(10.0, 20.0), size(100.0, 50.0));

        assert_eq!(b.top_left(), point(10.0, 20.0));
        assert_eq!(b.top_right(), point(110.0, 20.0));
        assert_eq!(b.bottom_left(), point(10.0, 70.0));
        assert_eq!(b.bottom_right(), point(110.0, 70.0));
        assert_eq!(b.corner(Corner::TopLeft), point(10.0, 20.0));
        assert_eq!(b.corner(Corner::BottomRight), point(110.0, 70.0));
    }

    #[test]
    fn test_edge_accessors() {
        let b = bounds(point(10.0, 20.0), size(100.0, 50.0));

        assert_eq!(b.top(), 20.0);
        assert_eq!(b.bottom(), 70.0);
        assert_eq!(b.left(), 10.0);
        assert_eq!(b.right(), 110.0);
    }

    #[test]
    fn test_center() {
        let b = bounds(point(0.0, 0.0), size(100.0, 50.0));
        assert_eq!(b.center(), point(50.0, 25.0));
    }

    #[test]
    fn test_intersects() {
        let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
        let b = bounds(point(5.0, 5.0), size(10.0, 10.0));
        let c = bounds(point(20.0, 20.0), size(10.0, 10.0));

        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn test_intersect() {
        let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
        let b = bounds(point(5.0, 5.0), size(10.0, 10.0));

        let intersection = a.intersect(&b);
        assert_eq!(intersection, bounds(point(5.0, 5.0), size(5.0, 5.0)));

        // Non-intersecting bounds
        let c = bounds(point(20.0, 20.0), size(10.0, 10.0));
        let no_intersection = a.intersect(&c);
        // Size will be negative
        assert!(no_intersection.size.width < 0.0);
    }

    #[test]
    fn test_union() {
        let a = bounds(point(0.0, 0.0), size(10.0, 10.0));
        let b = bounds(point(5.0, 5.0), size(15.0, 15.0));

        let union = a.union(&b);
        assert_eq!(union, bounds(point(0.0, 0.0), size(20.0, 20.0)));
    }

    #[test]
    fn test_contains() {
        let b = bounds(point(0.0, 0.0), size(10.0, 10.0));

        assert!(b.contains(&point(5.0, 5.0)));
        assert!(b.contains(&point(0.0, 0.0))); // Edge
        assert!(b.contains(&point(10.0, 10.0))); // Edge
        assert!(!b.contains(&point(15.0, 5.0)));
    }

    #[test]
    fn test_is_contained_within() {
        let outer = bounds(point(0.0, 0.0), size(100.0, 100.0));
        let inner = bounds(point(10.0, 10.0), size(50.0, 50.0));

        assert!(inner.is_contained_within(&outer));
        assert!(!outer.is_contained_within(&inner));
    }

    #[test]
    fn test_localize() {
        let b = bounds(point(10.0, 20.0), size(100.0, 50.0));

        assert_eq!(b.localize(&point(15.0, 25.0)), Some(point(5.0, 5.0)));
        assert_eq!(b.localize(&point(200.0, 200.0)), None);
    }

    #[test]
    fn test_dilate() {
        let b = bounds(point(10.0, 10.0), size(10.0, 10.0));
        let expanded = b.dilate(5.0);

        assert_eq!(expanded, bounds(point(5.0, 5.0), size(20.0, 20.0)));
    }

    #[test]
    fn test_inset() {
        let b = bounds(point(0.0, 0.0), size(20.0, 20.0));
        let inset = b.inset(5.0);

        assert_eq!(inset, bounds(point(5.0, 5.0), size(10.0, 10.0)));
    }

    #[test]
    fn test_extend() {
        use super::super::edges;

        let b = bounds(point(10.0, 10.0), size(10.0, 10.0));
        let extended = b.extend(edges(5.0, 3.0, 5.0, 3.0));

        assert_eq!(extended, bounds(point(7.0, 5.0), size(16.0, 20.0)));
    }

    #[test]
    fn test_space_within() {
        let inner = bounds(point(10.0, 10.0), size(20.0, 20.0));
        let outer = bounds(point(0.0, 0.0), size(50.0, 50.0));

        let space = inner.space_within(&outer);
        assert_eq!(space.top, 10.0);
        assert_eq!(space.left, 10.0);
        assert_eq!(space.right, 20.0);
        assert_eq!(space.bottom, 20.0);
    }

    #[test]
    fn test_operators() {
        let b = bounds(point(10.0, 20.0), size(100.0, 50.0));

        let scaled = b * 2.0;
        assert_eq!(scaled, bounds(point(20.0, 40.0), size(200.0, 100.0)));

        let divided = b / 2.0;
        assert_eq!(divided, bounds(point(5.0, 10.0), size(50.0, 25.0)));

        let translated = b + point(5.0, 5.0);
        assert_eq!(translated, bounds(point(15.0, 25.0), size(100.0, 50.0)));

        let back = translated - point(5.0, 5.0);
        assert_eq!(back, b);
    }

    #[test]
    fn test_display() {
        let b = bounds(point(0.0, 0.0), size(10.0, 10.0));
        let display = format!("{}", b);
        assert!(display.contains("(0, 0)"));
        assert!(display.contains("(10, 10)"));
    }

    #[test]
    fn test_rect_conversion() {
        let b = bounds(point(10.0, 20.0), size(100.0, 50.0));
        let r: Rect = b.into();

        assert_eq!(r.left(), 10.0);
        assert_eq!(r.top(), 20.0);
        assert_eq!(r.width(), 100.0);
        assert_eq!(r.height(), 50.0);

        let b2: Bounds<f32> = r.into();
        assert_eq!(b, b2);
    }

    #[test]
    fn test_default() {
        let b: Bounds<f32> = Bounds::default();
        assert_eq!(b.origin, point(0.0, 0.0));
        assert_eq!(b.size, size(0.0, 0.0));
    }
}
