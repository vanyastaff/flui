//! Edge utilities for box model calculations.
//!
//! This module provides [`Edges`] for representing values associated with
//! the four edges of a rectangle (top, right, bottom, left). Common uses
//! include padding, margin, borders, and insets.

use std::fmt::{self, Debug};
use std::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Edges<T = f32> {
    /// The top edge value.
    pub top: T,
    /// The right edge value.
    pub right: T,
    /// The bottom edge value.
    pub bottom: T,
    /// The left edge value.
    pub left: T,
}

#[inline]
pub const fn edges<T>(top: T, right: T, bottom: T, left: T) -> Edges<T> {
    Edges {
        top,
        right,
        bottom,
        left,
    }
}

impl<T> Edges<T> {
    #[inline]
    pub const fn new(top: T, right: T, bottom: T, left: T) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    #[inline]
    pub fn all(value: T) -> Self
    where
        T: Clone,
    {
        Self {
            top: value.clone(),
            right: value.clone(),
            bottom: value.clone(),
            left: value,
        }
    }

    #[inline]
    pub fn symmetric(vertical: T, horizontal: T) -> Self
    where
        T: Clone,
    {
        Self {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical,
            left: horizontal,
        }
    }

    #[inline]
    pub fn horizontal(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top: T::default(),
            right: value.clone(),
            bottom: T::default(),
            left: value,
        }
    }

    #[inline]
    pub fn vertical(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top: value.clone(),
            right: T::default(),
            bottom: value,
            left: T::default(),
        }
    }

    #[inline]
    pub fn horizontal_total(&self) -> T
    where
        T: Add<Output = T> + Copy,
    {
        self.left + self.right
    }

    #[inline]
    pub fn vertical_total(&self) -> T
    where
        T: Add<Output = T> + Copy,
    {
        self.top + self.bottom
    }

    #[must_use]
    pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Edges<U> {
        Edges {
            top: f(&self.top),
            right: f(&self.right),
            bottom: f(&self.bottom),
            left: f(&self.left),
        }
    }

    #[must_use]
    pub fn any(&self, f: impl Fn(&T) -> bool) -> bool {
        f(&self.top) || f(&self.right) || f(&self.bottom) || f(&self.left)
    }

    #[must_use]
    pub fn all_satisfy(&self, f: impl Fn(&T) -> bool) -> bool {
        f(&self.top) && f(&self.right) && f(&self.bottom) && f(&self.left)
    }
}

// ============================================================================
// f32-specific methods
// ============================================================================

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Edges<super::units::Pixels> {
    /// Edge insets with all sides set to zero.
    pub const ZERO: Self = Self {
        top: super::units::Pixels(0.0),
        right: super::units::Pixels(0.0),
        bottom: super::units::Pixels(0.0),
        left: super::units::Pixels(0.0),
    };

    /// Create edge insets with only the left side set.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, px};
    ///
    /// let insets = Edges::only_left(px(10.0));
    /// assert_eq!(insets.left, px(10.0));
    /// assert_eq!(insets.top, px(0.0));
    /// ```
    #[inline]
    pub fn only_left(value: super::units::Pixels) -> Self {
        use super::units::px;
        Self {
            top: px(0.0),
            right: px(0.0),
            bottom: px(0.0),
            left: value,
        }
    }

    /// Create edge insets with only the top side set.
    #[inline]
    pub fn only_top(value: super::units::Pixels) -> Self {
        use super::units::px;
        Self {
            top: value,
            right: px(0.0),
            bottom: px(0.0),
            left: px(0.0),
        }
    }

    /// Create edge insets with only the right side set.
    #[inline]
    pub fn only_right(value: super::units::Pixels) -> Self {
        use super::units::px;
        Self {
            top: px(0.0),
            right: value,
            bottom: px(0.0),
            left: px(0.0),
        }
    }

    /// Create edge insets with only the bottom side set.
    #[inline]
    pub fn only_bottom(value: super::units::Pixels) -> Self {
        use super::units::px;
        Self {
            top: px(0.0),
            right: px(0.0),
            bottom: value,
            left: px(0.0),
        }
    }

    /// Get the total insets as a size (width: left + right, height: top + bottom).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, Size, px};
    ///
    /// let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));
    /// let size = insets.total_size();
    /// assert_eq!(size.width, px(60.0)); // left + right = 40 + 20
    /// assert_eq!(size.height, px(40.0)); // top + bottom = 10 + 30
    /// ```
    #[must_use]
    pub fn total_size(&self) -> super::Size<super::units::Pixels> {
        super::Size::new(self.horizontal_total(), self.vertical_total())
    }

    /// Get the top-left offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, Offset, px};
    ///
    /// let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));
    /// let offset = insets.top_left();
    /// assert_eq!(offset.dx, px(40.0)); // left
    /// assert_eq!(offset.dy, px(10.0)); // top
    /// ```
    #[must_use]
    pub fn top_left(&self) -> super::Offset<super::units::Pixels> {
        super::Offset::new(self.left, self.top)
    }

    /// Get the bottom-right offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, Offset, px};
    ///
    /// let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));
    /// let offset = insets.bottom_right();
    /// assert_eq!(offset.dx, px(20.0)); // right
    /// assert_eq!(offset.dy, px(30.0)); // bottom
    /// ```
    #[must_use]
    pub fn bottom_right(&self) -> super::Offset<super::units::Pixels> {
        super::Offset::new(self.right, self.bottom)
    }

    /// Check if all edge values are zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, px};
    ///
    /// let zero_insets = Edges::ZERO;
    /// assert!(zero_insets.is_zero());
    ///
    /// let non_zero = Edges::all(px(10.0));
    /// assert!(!non_zero.is_zero());
    /// ```
    #[must_use]
    pub fn is_zero(&self) -> bool {
        use super::units::px;
        self.left == px(0.0) && self.top == px(0.0) && self.right == px(0.0) && self.bottom == px(0.0)
    }

    /// Check if all edge values are non-negative.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, px};
    ///
    /// let positive = Edges::all(px(10.0));
    /// assert!(positive.is_non_negative());
    ///
    /// let negative = Edges::new(px(-5.0), px(10.0), px(10.0), px(10.0));
    /// assert!(!negative.is_non_negative());
    /// ```
    #[must_use]
    pub fn is_non_negative(&self) -> bool {
        use super::units::px;
        self.left >= px(0.0) && self.top >= px(0.0) && self.right >= px(0.0) && self.bottom >= px(0.0)
    }

    /// Clamp all edge values to be non-negative.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, px};
    ///
    /// let insets = Edges::new(px(-5.0), px(10.0), px(-3.0), px(20.0));
    /// let clamped = insets.clamp_non_negative();
    /// assert_eq!(clamped.top, px(0.0));
    /// assert_eq!(clamped.right, px(10.0));
    /// assert_eq!(clamped.bottom, px(0.0));
    /// assert_eq!(clamped.left, px(20.0));
    /// ```
    #[must_use]
    pub fn clamp_non_negative(&self) -> Self {
        use super::units::px;
        Self {
            top: if self.top.get() < 0.0 { px(0.0) } else { self.top },
            right: if self.right.get() < 0.0 { px(0.0) } else { self.right },
            bottom: if self.bottom.get() < 0.0 { px(0.0) } else { self.bottom },
            left: if self.left.get() < 0.0 { px(0.0) } else { self.left },
        }
    }

    #[must_use]
    pub fn scale(&self, factor: f32) -> Edges<super::units::ScaledPixels> {
        Edges {
            top: self.top.scale(factor),
            right: self.right.scale(factor),
            bottom: self.bottom.scale(factor),
            left: self.left.scale(factor),
        }
    }

    /// Inflates a rectangle by these edge insets.
    ///
    /// Increases the rectangle's size by adding the insets to all sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, Point, Rect, px};
    ///
    /// let insets = Edges::all(px(10.0));
    /// let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    /// let inflated = insets.inflate_rect(rect);
    /// assert_eq!(inflated.left(), px(-10.0));
    /// assert_eq!(inflated.top(), px(-10.0));
    /// assert_eq!(inflated.right(), px(110.0));
    /// assert_eq!(inflated.bottom(), px(110.0));
    /// ```
    #[must_use]
    pub fn inflate_rect(&self, rect: super::Rect<super::units::Pixels>) -> super::Rect<super::units::Pixels> {
        super::Rect::from_min_max(
            super::Point::new(rect.min.x - self.left, rect.min.y - self.top),
            super::Point::new(rect.max.x + self.right, rect.max.y + self.bottom),
        )
    }

    /// Deflates a rectangle by these edge insets.
    ///
    /// Decreases the rectangle's size by subtracting the insets from all sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, Point, Rect, px};
    ///
    /// let insets = Edges::all(px(10.0));
    /// let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    /// let deflated = insets.deflate_rect(rect);
    /// assert_eq!(deflated.left(), px(10.0));
    /// assert_eq!(deflated.top(), px(10.0));
    /// assert_eq!(deflated.right(), px(90.0));
    /// assert_eq!(deflated.bottom(), px(90.0));
    /// ```
    #[must_use]
    pub fn deflate_rect(&self, rect: super::Rect<super::units::Pixels>) -> super::Rect<super::units::Pixels> {
        super::Rect::from_min_max(
            super::Point::new(rect.min.x + self.left, rect.min.y + self.top),
            super::Point::new(rect.max.x - self.right, rect.max.y - self.bottom),
        )
    }

    /// Inflates a size by these edge insets.
    ///
    /// Increases the size by adding horizontal and vertical insets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, Size, px};
    ///
    /// let insets = Edges::all(px(10.0));
    /// let size = Size::new(px(100.0), px(100.0));
    /// let inflated = insets.inflate_size(size);
    /// assert_eq!(inflated.width, px(120.0)); // 100 + 10 + 10
    /// assert_eq!(inflated.height, px(120.0)); // 100 + 10 + 10
    /// ```
    #[must_use]
    pub fn inflate_size(&self, size: super::Size<super::units::Pixels>) -> super::Size<super::units::Pixels> {
        super::Size::new(
            size.width + self.left + self.right,
            size.height + self.top + self.bottom,
        )
    }

    /// Deflates a size by these edge insets.
    ///
    /// Decreases the size by subtracting horizontal and vertical insets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, Size, px};
    ///
    /// let insets = Edges::all(px(10.0));
    /// let size = Size::new(px(100.0), px(100.0));
    /// let deflated = insets.deflate_size(size);
    /// assert_eq!(deflated.width, px(80.0)); // 100 - 10 - 10
    /// assert_eq!(deflated.height, px(80.0)); // 100 - 10 - 10
    /// ```
    #[must_use]
    pub fn deflate_size(&self, size: super::Size<super::units::Pixels>) -> super::Size<super::units::Pixels> {
        super::Size::new(
            size.width - self.left - self.right,
            size.height - self.top - self.bottom,
        )
    }

    /// Flips the insets horizontally (swaps left and right).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, px};
    ///
    /// let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));
    /// let flipped = insets.flip_horizontal();
    /// assert_eq!(flipped.left, px(20.0));
    /// assert_eq!(flipped.right, px(40.0));
    /// ```
    #[must_use]
    pub fn flip_horizontal(&self) -> Self {
        Self {
            top: self.top,
            right: self.left,
            bottom: self.bottom,
            left: self.right,
        }
    }

    /// Flips the insets vertically (swaps top and bottom).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Edges, px};
    ///
    /// let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));
    /// let flipped = insets.flip_vertical();
    /// assert_eq!(flipped.top, px(30.0));
    /// assert_eq!(flipped.bottom, px(10.0));
    /// ```
    #[must_use]
    pub fn flip_vertical(&self) -> Self {
        Self {
            top: self.bottom,
            right: self.right,
            bottom: self.top,
            left: self.left,
        }
    }

}

// Arithmetic operators
impl<T> Add for Edges<T>
where
    T: Add<Output = T>,
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            top: self.top + rhs.top,
            right: self.right + rhs.right,
            bottom: self.bottom + rhs.bottom,
            left: self.left + rhs.left,
        }
    }
}

impl<T> AddAssign for Edges<T>
where
    T: AddAssign + Copy,
{
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.top += rhs.top;
        self.right += rhs.right;
        self.bottom += rhs.bottom;
        self.left += rhs.left;
    }
}

impl<T> Sub for Edges<T>
where
    T: Sub<Output = T>,
{
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            top: self.top - rhs.top,
            right: self.right - rhs.right,
            bottom: self.bottom - rhs.bottom,
            left: self.left - rhs.left,
        }
    }
}

impl<T> SubAssign for Edges<T>
where
    T: SubAssign + Copy,
{
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.top -= rhs.top;
        self.right -= rhs.right;
        self.bottom -= rhs.bottom;
        self.left -= rhs.left;
    }
}

impl<T> Mul for Edges<T>
where
    T: Mul<Output = T> + Clone,
{
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            top: self.top.clone() * rhs.top,
            right: self.right.clone() * rhs.right,
            bottom: self.bottom.clone() * rhs.bottom,
            left: self.left * rhs.left,
        }
    }
}

impl<T, S> MulAssign<S> for Edges<T>
where
    T: Mul<S, Output = T> + Clone,
    S: Clone,
{
    #[inline]
    fn mul_assign(&mut self, rhs: S) {
        self.top = self.top.clone() * rhs.clone();
        self.right = self.right.clone() * rhs.clone();
        self.bottom = self.bottom.clone() * rhs.clone();
        self.left = self.left.clone() * rhs;
    }
}

// ============================================================================
// From implementations for Edges<Pixels>
// ============================================================================

impl From<super::units::Pixels> for Edges<super::units::Pixels> {
    fn from(value: super::units::Pixels) -> Self {
        Self::all(value)
    }
}

impl From<(super::units::Pixels, super::units::Pixels)> for Edges<super::units::Pixels> {
    fn from((vertical, horizontal): (super::units::Pixels, super::units::Pixels)) -> Self {
        Self::symmetric(vertical, horizontal)
    }
}

impl From<(super::units::Pixels, super::units::Pixels, super::units::Pixels, super::units::Pixels)> for Edges<super::units::Pixels> {
    fn from((top, right, bottom, left): (super::units::Pixels, super::units::Pixels, super::units::Pixels, super::units::Pixels)) -> Self {
        Self::new(top, right, bottom, left)
    }
}

// ============================================================================
// Along trait - Axis-based access
// ============================================================================

impl<T: Clone> super::traits::Along for Edges<T> {
    type Unit = (T, T);

    #[inline]
    fn along(&self, axis: super::traits::Axis) -> Self::Unit {
        match axis {
            super::traits::Axis::Horizontal => (self.left.clone(), self.right.clone()),
            super::traits::Axis::Vertical => (self.top.clone(), self.bottom.clone()),
        }
    }

    #[inline]
    fn apply_along(
        &self,
        axis: super::traits::Axis,
        f: impl FnOnce(Self::Unit) -> Self::Unit,
    ) -> Self {
        match axis {
            super::traits::Axis::Horizontal => {
                let (left, right) = f((self.left.clone(), self.right.clone()));
                Self {
                    top: self.top.clone(),
                    right,
                    bottom: self.bottom.clone(),
                    left,
                }
            }
            super::traits::Axis::Vertical => {
                let (top, bottom) = f((self.top.clone(), self.bottom.clone()));
                Self {
                    top,
                    right: self.right.clone(),
                    bottom,
                    left: self.left.clone(),
                }
            }
        }
    }
}

// ============================================================================
// Debug formatting
// ============================================================================

impl<T: Debug> Debug for Edges<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Edges")
            .field("top", &self.top)
            .field("right", &self.right)
            .field("bottom", &self.bottom)
            .field("left", &self.left)
            .finish()
    }
}
