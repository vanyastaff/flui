//! Corner utilities for rounded rectangles and border radii.
//!
//! This module provides [`Corners`] for representing values associated with
//! the four corners of a rectangle. Common uses include border radius,
//! corner rounding, and corner-specific styling.

use std::fmt::{self, Debug};

/// Values associated with the four corners of a rectangle.
///
/// `Corners` is used throughout FLUI for corner-related properties like
/// border radius. It provides convenient constructors for common patterns
/// like uniform corners or specific corner rounding.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::{Corners, corners};
///
/// // Uniform corners (all same radius)
/// let border_radius = Corners::all(8.0);
/// assert_eq!(border_radius.top_left, 8.0);
///
/// // Specific corners
/// let c = corners(4.0, 8.0, 8.0, 4.0);
/// assert_eq!(c.top_left, 4.0);
/// assert_eq!(c.top_right, 8.0);
/// ```
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Corners<T = f32> {
    /// The top-left corner value.
    pub top_left: T,
    /// The top-right corner value.
    pub top_right: T,
    /// The bottom-right corner value.
    pub bottom_right: T,
    /// The bottom-left corner value.
    pub bottom_left: T,
}

/// Constructs `Corners` with the specified value for each corner.
///
/// # Examples
///
/// ```rust
/// use flui_types::geometry::corners;
///
/// let c = corners(4.0, 8.0, 8.0, 4.0);
/// assert_eq!(c.top_left, 4.0);
/// assert_eq!(c.bottom_right, 8.0);
/// ```
#[inline]
pub const fn corners<T>(top_left: T, top_right: T, bottom_right: T, bottom_left: T) -> Corners<T> {
    Corners {
        top_left,
        top_right,
        bottom_right,
        bottom_left,
    }
}

impl<T> Corners<T> {
    /// Creates new `Corners` with the specified values for each corner.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::new(4.0, 8.0, 8.0, 4.0);
    /// ```
    #[inline]
    pub const fn new(top_left: T, top_right: T, bottom_right: T, bottom_left: T) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    /// Creates `Corners` where all corners have the same value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let border_radius = Corners::all(8.0);
    /// assert_eq!(border_radius.top_left, 8.0);
    /// assert_eq!(border_radius.top_right, 8.0);
    /// assert_eq!(border_radius.bottom_right, 8.0);
    /// assert_eq!(border_radius.bottom_left, 8.0);
    /// ```
    #[inline]
    pub fn all(value: T) -> Self
    where
        T: Clone,
    {
        Self {
            top_left: value.clone(),
            top_right: value.clone(),
            bottom_right: value.clone(),
            bottom_left: value,
        }
    }

    /// Creates `Corners` with only the top corners set.
    ///
    /// Bottom corners are set to the default value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::top(8.0);
    /// assert_eq!(c.top_left, 8.0);
    /// assert_eq!(c.top_right, 8.0);
    /// assert_eq!(c.bottom_left, 0.0);
    /// assert_eq!(c.bottom_right, 0.0);
    /// ```
    #[inline]
    pub fn top(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top_left: value.clone(),
            top_right: value,
            bottom_right: T::default(),
            bottom_left: T::default(),
        }
    }

    /// Creates `Corners` with only the bottom corners set.
    ///
    /// Top corners are set to the default value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::bottom(8.0);
    /// assert_eq!(c.bottom_left, 8.0);
    /// assert_eq!(c.bottom_right, 8.0);
    /// assert_eq!(c.top_left, 0.0);
    /// assert_eq!(c.top_right, 0.0);
    /// ```
    #[inline]
    pub fn bottom(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top_left: T::default(),
            top_right: T::default(),
            bottom_right: value.clone(),
            bottom_left: value,
        }
    }

    /// Creates `Corners` with only the left corners set.
    ///
    /// Right corners are set to the default value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::left(8.0);
    /// assert_eq!(c.top_left, 8.0);
    /// assert_eq!(c.bottom_left, 8.0);
    /// assert_eq!(c.top_right, 0.0);
    /// assert_eq!(c.bottom_right, 0.0);
    /// ```
    #[inline]
    pub fn left(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top_left: value.clone(),
            top_right: T::default(),
            bottom_right: T::default(),
            bottom_left: value,
        }
    }

    /// Creates `Corners` with only the right corners set.
    ///
    /// Left corners are set to the default value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::right(8.0);
    /// assert_eq!(c.top_right, 8.0);
    /// assert_eq!(c.bottom_right, 8.0);
    /// assert_eq!(c.top_left, 0.0);
    /// assert_eq!(c.bottom_left, 0.0);
    /// ```
    #[inline]
    pub fn right(value: T) -> Self
    where
        T: Clone + Default,
    {
        Self {
            top_left: T::default(),
            top_right: value.clone(),
            bottom_right: value,
            bottom_left: T::default(),
        }
    }

    /// Applies a function to each corner value, producing new `Corners`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::all(4);
    /// let doubled = c.map(|&x| x * 2);
    /// assert_eq!(doubled.top_left, 8);
    /// assert_eq!(doubled.bottom_right, 8);
    /// ```
    #[inline]
    #[must_use]
    pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Corners<U> {
        Corners {
            top_left: f(&self.top_left),
            top_right: f(&self.top_right),
            bottom_right: f(&self.bottom_right),
            bottom_left: f(&self.bottom_left),
        }
    }

    /// Returns the value of a specific corner by enum.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Corners, Corner};
    ///
    /// let c = Corners::new(4.0, 8.0, 12.0, 6.0);
    /// assert_eq!(c.corner(Corner::TopLeft), 4.0);
    /// assert_eq!(c.corner(Corner::BottomRight), 12.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn corner(&self, corner: super::Corner) -> T
    where
        T: Clone,
    {
        match corner {
            super::Corner::TopLeft => self.top_left.clone(),
            super::Corner::TopRight => self.top_right.clone(),
            super::Corner::BottomLeft => self.bottom_left.clone(),
            super::Corner::BottomRight => self.bottom_right.clone(),
        }
    }

    /// Returns the maximum corner value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::new(4.0, 8.0, 12.0, 6.0);
    /// assert_eq!(c.max(), 12.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn max(&self) -> T
    where
        T: Copy + PartialOrd,
    {
        let h_max = if self.top_left > self.top_right {
            self.top_left
        } else {
            self.top_right
        };
        let v_max = if self.bottom_left > self.bottom_right {
            self.bottom_left
        } else {
            self.bottom_right
        };
        if h_max > v_max {
            h_max
        } else {
            v_max
        }
    }

    /// Returns the minimum corner value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::Corners;
    ///
    /// let c = Corners::new(4.0, 8.0, 12.0, 6.0);
    /// assert_eq!(c.min(), 4.0);
    /// ```
    #[inline]
    pub fn min(&self) -> T
    where
        T: Copy + PartialOrd,
    {
        let h_min = if self.top_left < self.top_right {
            self.top_left
        } else {
            self.top_right
        };
        let v_min = if self.bottom_left < self.bottom_right {
            self.bottom_left
        } else {
            self.bottom_right
        };
        if h_min < v_min {
            h_min
        } else {
            v_min
        }
    }
}

// ============================================================================
// f32-specific methods
// ============================================================================

impl Corners<f32> {
    /// Clamps corner radii to fit within the given rectangle size.
    ///
    /// Ensures that corner radii don't exceed half the width/height,
    /// preventing overlapping corners in rounded rectangles.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Corners, Size, size};
    ///
    /// let corners = Corners::all(50.0);
    /// let small_size = size(80.0, 60.0);
    /// let clamped = corners.clamp_for_size(small_size);
    ///
    /// // Radii are clamped to fit the smaller dimension
    /// assert!(clamped.top_left <= 30.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn clamp_for_size(&self, size: super::Size<f32>) -> Self {
        // Maximum radius is half the minimum dimension
        let max_radius = (size.width.min(size.height) / 2.0).max(0.0);

        Self {
            top_left: self.top_left.min(max_radius),
            top_right: self.top_right.min(max_radius),
            bottom_right: self.bottom_right.min(max_radius),
            bottom_left: self.bottom_left.min(max_radius),
        }
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Corners<super::units::Pixels> {
    /// Scales all corners by the given factor, producing `Corners<ScaledPixels>`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_types::geometry::{Corners, px};
    ///
    /// let corners = Corners::all(px(8.0));
    /// let scaled = corners.scale(2.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Corners<super::units::ScaledPixels> {
        Corners {
            top_left: self.top_left.scale(factor),
            top_right: self.top_right.scale(factor),
            bottom_right: self.bottom_right.scale(factor),
            bottom_left: self.bottom_left.scale(factor),
        }
    }
}

// ============================================================================
// Along trait - Axis-based access
// ============================================================================

impl<T: Clone> super::traits::Along for Corners<T> {
    type Unit = (T, T);

    #[inline]
    fn along(&self, axis: super::traits::Axis) -> Self::Unit {
        match axis {
            super::traits::Axis::Horizontal => {
                // Top and bottom horizontal pairs (top-left/top-right, bottom-left/bottom-right)
                // Return average or first pair - here we return top corners
                (self.top_left.clone(), self.top_right.clone())
            }
            super::traits::Axis::Vertical => {
                // Left and right vertical pairs (top-left/bottom-left, top-right/bottom-right)
                // Return left corners
                (self.top_left.clone(), self.bottom_left.clone())
            }
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
                let (top_left, top_right) = f((self.top_left.clone(), self.top_right.clone()));
                Self {
                    top_left,
                    top_right,
                    bottom_right: self.bottom_right.clone(),
                    bottom_left: self.bottom_left.clone(),
                }
            }
            super::traits::Axis::Vertical => {
                let (top_left, bottom_left) = f((self.top_left.clone(), self.bottom_left.clone()));
                Self {
                    top_left,
                    top_right: self.top_right.clone(),
                    bottom_right: self.bottom_right.clone(),
                    bottom_left,
                }
            }
        }
    }
}

// ============================================================================
// Debug formatting
// ============================================================================

impl<T: Debug> Debug for Corners<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Corners")
            .field("top_left", &self.top_left)
            .field("top_right", &self.top_right)
            .field("bottom_right", &self.bottom_right)
            .field("bottom_left", &self.bottom_left)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_corners_all() {
        let c = Corners::all(8.0);
        assert_eq!(c.top_left, 8.0);
        assert_eq!(c.top_right, 8.0);
        assert_eq!(c.bottom_right, 8.0);
        assert_eq!(c.bottom_left, 8.0);
    }

    #[test]
    fn test_corners_sides() {
        let top = Corners::top(8.0);
        assert_eq!(top.top_left, 8.0);
        assert_eq!(top.top_right, 8.0);
        assert_eq!(top.bottom_left, 0.0);
        assert_eq!(top.bottom_right, 0.0);

        let bottom = Corners::bottom(8.0);
        assert_eq!(bottom.bottom_left, 8.0);
        assert_eq!(bottom.bottom_right, 8.0);
        assert_eq!(bottom.top_left, 0.0);
        assert_eq!(bottom.top_right, 0.0);

        let left = Corners::left(8.0);
        assert_eq!(left.top_left, 8.0);
        assert_eq!(left.bottom_left, 8.0);
        assert_eq!(left.top_right, 0.0);
        assert_eq!(left.bottom_right, 0.0);

        let right = Corners::right(8.0);
        assert_eq!(right.top_right, 8.0);
        assert_eq!(right.bottom_right, 8.0);
        assert_eq!(right.top_left, 0.0);
        assert_eq!(right.bottom_left, 0.0);
    }

    #[test]
    fn test_corners_min_max() {
        let c = Corners::new(4.0, 8.0, 12.0, 6.0);
        assert_eq!(c.max(), 12.0);
        assert_eq!(c.min(), 4.0);
    }

    #[test]
    fn test_corners_map() {
        let c = Corners::all(4);
        let doubled = c.map(|&x| x * 2);
        assert_eq!(doubled.top_left, 8);
        assert_eq!(doubled.top_right, 8);
        assert_eq!(doubled.bottom_left, 8);
        assert_eq!(doubled.bottom_right, 8);
    }

    #[test]
    fn test_corners_corner_accessor() {
        use super::super::Corner;

        let c = Corners::new(4.0, 8.0, 12.0, 6.0);
        assert_eq!(c.corner(Corner::TopLeft), 4.0);
        assert_eq!(c.corner(Corner::TopRight), 8.0);
        assert_eq!(c.corner(Corner::BottomRight), 12.0);
        assert_eq!(c.corner(Corner::BottomLeft), 6.0);
    }

    #[test]
    fn test_corners_clamp_for_size() {
        use super::super::size;

        let corners = Corners::all(50.0);
        let small_size = size(80.0, 60.0);
        let clamped = corners.clamp_for_size(small_size);

        // All corners clamped to 30.0 (half of 60.0, the smaller dimension)
        assert_eq!(clamped.top_left, 30.0);
        assert_eq!(clamped.top_right, 30.0);
        assert_eq!(clamped.bottom_right, 30.0);
        assert_eq!(clamped.bottom_left, 30.0);

        // Already small corners remain unchanged
        let small_corners = Corners::all(10.0);
        let clamped_small = small_corners.clamp_for_size(small_size);
        assert_eq!(clamped_small.top_left, 10.0);
    }
}
