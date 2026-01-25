//! Corner utilities for rounded rectangles and border radii.
//!
//! This module provides [`Corners`] for representing values associated with
//! the four corners of a rectangle. Common uses include border radius,
//! corner rounding, and corner-specific styling.

/// Corner-specific values for rectangles (e.g., border radii).
///
/// Generic over type `T` to support various value types.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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

/// Convenience function to create [`Corners`] with explicit values.
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
    /// Creates new corner values with explicit values for each corner.
    #[inline]
    pub const fn new(top_left: T, top_right: T, bottom_right: T, bottom_left: T) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    /// Creates corner values with the same value for all corners.
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

    /// Creates corner values with the given value for top corners only.
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

    /// Creates corner values with the given value for bottom corners only.
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

    /// Creates corner values with the given value for left corners only.
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

    /// Creates corner values with the given value for right corners only.
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

    /// Maps each corner value using the provided function.
    #[must_use]
    pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Corners<U> {
        Corners {
            top_left: f(&self.top_left),
            top_right: f(&self.top_right),
            bottom_right: f(&self.bottom_right),
            bottom_left: f(&self.bottom_left),
        }
    }

    /// Returns the value for the specified corner.
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

    /// Returns the maximum value among all corners.
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

    /// Returns the minimum value among all corners.
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
// Specialized implementations for Pixels
// ============================================================================

impl Corners<super::units::Pixels> {
    /// Scales these corner values to scaled pixels.
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
