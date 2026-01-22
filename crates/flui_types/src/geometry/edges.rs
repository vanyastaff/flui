//! Edge utilities for box model calculations.
//!
//! This module provides [`Edges`] for representing values associated with
//! the four edges of a rectangle (top, right, bottom, left). Common uses
//! include padding, margin, borders, and insets.

use std::fmt::{self, Debug};
use std::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};

/// Values associated with the four edges of a rectangle.
///
/// `Edges` is used throughout FLUI for box model properties like padding,
/// margin, borders, and insets. It provides convenient constructors and
/// operations for working with edge-based values.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Edges, edges};
///
/// // Uniform edges
/// let padding = Edges::all(10.0);
/// assert_eq!(padding.top, 10.0);
/// assert_eq!(padding.left, 10.0);
///
/// // Asymmetric edges
/// let margin = edges(5.0, 10.0, 5.0, 10.0);
/// assert_eq!(margin.top, 5.0);
/// assert_eq!(margin.right, 10.0);
///
/// // Vertical and horizontal
/// let border = Edges::symmetric(2.0, 4.0);
/// assert_eq!(border.top, 2.0);    // vertical
/// assert_eq!(border.left, 4.0);   // horizontal
/// ```
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

/// Constructs `Edges` with the specified values for each side.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::edges;
///
/// let e = edges(10.0, 20.0, 10.0, 20.0);
/// assert_eq!(e.top, 10.0);
/// assert_eq!(e.right, 20.0);
/// ```
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
    /// Creates new `Edges` with the specified values for each side.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let e = Edges::new(5.0, 10.0, 5.0, 10.0);
    /// ```
    #[inline]
    pub const fn new(top: T, right: T, bottom: T, left: T) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Creates `Edges` where all sides have the same value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let padding = Edges::all(10.0);
    /// assert_eq!(padding.top, 10.0);
    /// assert_eq!(padding.right, 10.0);
    /// assert_eq!(padding.bottom, 10.0);
    /// assert_eq!(padding.left, 10.0);
    /// ```
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

    /// Creates `Edges` with symmetric vertical and horizontal values.
    ///
    /// # Arguments
    ///
    /// * `vertical` - Value for top and bottom edges
    /// * `horizontal` - Value for left and right edges
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let padding = Edges::symmetric(10.0, 20.0);
    /// assert_eq!(padding.top, 10.0);
    /// assert_eq!(padding.bottom, 10.0);
    /// assert_eq!(padding.left, 20.0);
    /// assert_eq!(padding.right, 20.0);
    /// ```
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

    /// Creates `Edges` with only horizontal values (left and right).
    ///
    /// Top and bottom are set to the default value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let padding = Edges::horizontal(20.0);
    /// assert_eq!(padding.left, 20.0);
    /// assert_eq!(padding.right, 20.0);
    /// assert_eq!(padding.top, 0.0);
    /// ```
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

    /// Creates `Edges` with only vertical values (top and bottom).
    ///
    /// Left and right are set to the default value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let padding = Edges::vertical(10.0);
    /// assert_eq!(padding.top, 10.0);
    /// assert_eq!(padding.bottom, 10.0);
    /// assert_eq!(padding.left, 0.0);
    /// ```
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

    /// Returns the total horizontal extent (left + right).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let padding = Edges::new(10.0, 20.0, 10.0, 30.0);
    /// assert_eq!(padding.horizontal_total(), 50.0);
    /// ```
    #[inline]
    pub fn horizontal_total(&self) -> T
    where
        T: Add<Output = T> + Copy,
    {
        self.left + self.right
    }

    /// Returns the total vertical extent (top + bottom).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let padding = Edges::new(10.0, 20.0, 15.0, 20.0);
    /// assert_eq!(padding.vertical_total(), 25.0);
    /// ```
    #[inline]
    pub fn vertical_total(&self) -> T
    where
        T: Add<Output = T> + Copy,
    {
        self.top + self.bottom
    }

    /// Applies a function to each edge value, producing new `Edges`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let e = Edges::all(10);
    /// let doubled = e.map(|&x| x * 2);
    /// assert_eq!(doubled.top, 20);
    /// ```
    #[inline]
    #[must_use]
    pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Edges<U> {
        Edges {
            top: f(&self.top),
            right: f(&self.right),
            bottom: f(&self.bottom),
            left: f(&self.left),
        }
    }

    /// Returns `true` if any edge satisfies the predicate.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let e = Edges::new(0.0, 10.0, 0.0, 0.0);
    /// assert!(e.any(|&x| x > 0.0));
    ///
    /// let zeros = Edges::all(0.0);
    /// assert!(!zeros.any(|&x| x > 0.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn any(&self, f: impl Fn(&T) -> bool) -> bool {
        f(&self.top) || f(&self.right) || f(&self.bottom) || f(&self.left)
    }

    /// Returns `true` if all edges satisfy the predicate.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let e = Edges::all(10.0);
    /// assert!(e.all_satisfy(|&x| x > 0.0));
    ///
    /// let mixed = Edges::new(10.0, 0.0, 10.0, 10.0);
    /// assert!(!mixed.all_satisfy(|&x| x > 0.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn all_satisfy(&self, f: impl Fn(&T) -> bool) -> bool {
        f(&self.top) && f(&self.right) && f(&self.bottom) && f(&self.left)
    }
}

// ============================================================================
// f32-specific methods
// ============================================================================

impl Edges<f32> {
    /// Returns the maximum value among all edges.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let e = Edges::new(5.0, 10.0, 3.0, 7.0);
    /// assert_eq!(e.max(), 10.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn max(&self) -> f32 {
        self.top.max(self.right).max(self.bottom).max(self.left)
    }

    /// Returns the minimum value among all edges.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Edges;
    ///
    /// let e = Edges::new(5.0, 10.0, 3.0, 7.0);
    /// assert_eq!(e.min(), 3.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn min(&self) -> f32 {
        self.top.min(self.right).min(self.bottom).min(self.left)
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Edges<super::units::Pixels> {
    /// Scales all edges by the given factor, producing `Edges<ScaledPixels>`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Edges, px};
    ///
    /// let padding = Edges::all(px(10.0));
    /// let scaled = padding.scale(2.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Edges<super::units::ScaledPixels> {
        Edges {
            top: self.top.scale(factor),
            right: self.right.scale(factor),
            bottom: self.bottom.scale(factor),
            left: self.left.scale(factor),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edges_all() {
        let e = Edges::all(10.0);
        assert_eq!(e.top, 10.0);
        assert_eq!(e.right, 10.0);
        assert_eq!(e.bottom, 10.0);
        assert_eq!(e.left, 10.0);
    }

    #[test]
    fn test_edges_symmetric() {
        let e = Edges::symmetric(10.0, 20.0);
        assert_eq!(e.top, 10.0);
        assert_eq!(e.bottom, 10.0);
        assert_eq!(e.left, 20.0);
        assert_eq!(e.right, 20.0);
    }

    #[test]
    fn test_edges_horizontal_vertical() {
        let h = Edges::horizontal(20.0);
        assert_eq!(h.left, 20.0);
        assert_eq!(h.right, 20.0);
        assert_eq!(h.top, 0.0);
        assert_eq!(h.bottom, 0.0);

        let v = Edges::vertical(10.0);
        assert_eq!(v.top, 10.0);
        assert_eq!(v.bottom, 10.0);
        assert_eq!(v.left, 0.0);
        assert_eq!(v.right, 0.0);
    }

    #[test]
    fn test_edges_totals() {
        let e = Edges::new(10.0, 20.0, 15.0, 30.0);
        assert_eq!(e.horizontal_total(), 50.0);
        assert_eq!(e.vertical_total(), 25.0);
    }

    #[test]
    fn test_edges_arithmetic() {
        let e1 = Edges::all(10.0);
        let e2 = Edges::all(5.0);

        let sum = e1 + e2;
        assert_eq!(sum.top, 15.0);

        let diff = e1 - e2;
        assert_eq!(diff.top, 5.0);
    }

    #[test]
    fn test_edges_map() {
        let e = Edges::all(10);
        let doubled = e.map(|&x| x * 2);
        assert_eq!(doubled.top, 20);
        assert_eq!(doubled.left, 20);
    }

    #[test]
    fn test_edges_any() {
        let e = Edges::new(0.0, 10.0, 0.0, 0.0);
        assert!(e.any(|&x| x > 0.0));

        let zeros = Edges::all(0.0);
        assert!(!zeros.any(|&x| x > 0.0));
    }

    #[test]
    fn test_edges_all_satisfy() {
        let e = Edges::all(10.0);
        assert!(e.all_satisfy(|&x| x > 0.0));

        let mixed = Edges::new(10.0, 0.0, 10.0, 10.0);
        assert!(!mixed.all_satisfy(|&x| x > 0.0));
    }

    #[test]
    fn test_edges_max_min() {
        let e = Edges::new(5.0, 10.0, 3.0, 7.0);
        assert_eq!(e.max(), 10.0);
        assert_eq!(e.min(), 3.0);
    }
}
