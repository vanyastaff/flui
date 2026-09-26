//! Corner utilities for rounded rectangles and border radii.
//!
//! This module provides [`Corners`] for representing values associated with
//! the four corners of a rectangle. Common uses include border radius,
//! corner rounding, and corner-specific styling.

/// Corner-specific values for rectangles (e.g., border radii).
///
/// Generic over type `T` to support various value types.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    /// Returns the value for the specified corner.
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

    /// Returns the maximum value among all corners.
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
        if h_max > v_max { h_max } else { v_max }
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
        if h_min < v_min { h_min } else { v_min }
    }
}

// ============================================================================
// Specialized implementations for Pixels
// ============================================================================

impl Corners<super::units::Pixels> {
    /// Scales these corner values by the given factor.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Corners<super::units::Pixels> {
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
                // Top and bottom horizontal pairs (top-left/top-right,
                // bottom-left/bottom-right) Return average or first pair - here
                // we return top corners
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Along, Axis};
    use crate::{Corner, Radius, px};

    const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomRight,
        Corner::BottomLeft,
    ];

    /// Distinct values, so each field is told apart from the others.
    fn distinct() -> Corners<i32> {
        corners(1, 2, 3, 4)
    }

    #[test]
    fn construction_is_clockwise_from_top_left() {
        let c = Corners::new(1, 2, 3, 4);
        assert_eq!(
            (c.top_left, c.top_right, c.bottom_right, c.bottom_left),
            (1, 2, 3, 4)
        );
        assert_eq!(c, distinct());
        assert_eq!(ALL.map(|k| c.corner(k)), [1, 2, 3, 4]);
        assert_eq!(c.map(|v| v * 10), corners(10, 20, 30, 40));
    }

    #[test]
    fn one_sided_constructors_default_the_rest() {
        assert_eq!(Corners::all(7), corners(7, 7, 7, 7));
        assert_eq!(Corners::top(7), corners(7, 7, 0, 0));
        assert_eq!(Corners::bottom(7), corners(0, 0, 7, 7));
        assert_eq!(Corners::left(7), corners(7, 0, 0, 7));
        assert_eq!(Corners::right(7), corners(0, 7, 7, 0));
    }

    /// Every placement of four distinct values, so each comparison in
    /// `max`/`min` decides the result somewhere.
    #[test]
    fn max_and_min_find_the_extremes_in_any_corner() {
        let values = [3, 1, 4, 2];
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let idx = [a, b, c, d];
                        if (1..4).any(|i| idx[..i].contains(&idx[i])) {
                            continue;
                        }
                        let k = corners(values[a], values[b], values[c], values[d]);
                        assert_eq!((k.max(), k.min()), (4, 1), "{k:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn scale_multiplies_each_corner() {
        let k = corners(px(1.0), px(2.0), px(3.0), px(4.0));
        assert_eq!(k.scale(1.5), corners(px(1.5), px(3.0), px(4.5), px(6.0)));
    }

    /// Horizontal is the top pair, vertical the left pair.
    #[test]
    fn along_reads_and_replaces_one_pair() {
        let k = distinct();
        assert_eq!(k.along(Axis::Horizontal), (1, 2));
        assert_eq!(k.along(Axis::Vertical), (1, 4));
        assert_eq!(
            k.apply_along(Axis::Horizontal, |(a, b)| (a + 10, b + 20)),
            corners(11, 22, 3, 4)
        );
        assert_eq!(
            k.apply_along(Axis::Vertical, |(a, b)| (a + 10, b + 20)),
            corners(11, 2, 3, 24)
        );
    }

    #[test]
    fn corner_sides_and_reflections() {
        for k in ALL {
            let top = matches!(k, Corner::TopLeft | Corner::TopRight);
            let left = matches!(k, Corner::TopLeft | Corner::BottomLeft);
            assert_eq!(
                (k.is_top(), k.is_bottom(), k.is_left(), k.is_right()),
                (top, !top, left, !left),
                "{k:?}"
            );

            let across = k.other_side_along(Axis::Horizontal);
            assert_eq!((across.is_top(), across.is_left()), (top, !left), "{k:?}");
            let down = k.other_side_along(Axis::Vertical);
            assert_eq!((down.is_top(), down.is_left()), (!top, left), "{k:?}");
            assert_eq!(
                k.opposite(),
                across.other_side_along(Axis::Vertical),
                "{k:?}"
            );
        }
    }

    #[test]
    fn radius_constructors() {
        assert_eq!(Radius::circular(px(3.0)), Radius::new(px(3.0), px(3.0)));
        assert_eq!(
            Radius::elliptical(px(3.0), px(5.0)),
            Radius {
                x: px(3.0),
                y: px(5.0)
            }
        );
        assert_eq!(Radius::<crate::Pixels>::zero(), Radius::ZERO);
        assert!(Radius::ZERO.is_zero());
        assert!(!Radius::new(px(0.0), px(1.0)).is_zero());
        assert!(!Radius::new(px(1.0), px(0.0)).is_zero());
    }
}
