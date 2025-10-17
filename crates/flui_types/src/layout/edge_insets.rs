//! Edge insets for padding and margins
//!
//! This module contains types for representing padding and margins,
//! similar to Flutter's EdgeInsets system.

use std::ops::{Add, Div, Mul, Neg, Sub};

use crate::{Offset, Point, Rect, Size};

/// An immutable set of offsets in each of the four cardinal directions.
///
/// Similar to Flutter's `EdgeInsets`.
///
/// # Examples
///
/// ```
/// use flui_types::{EdgeInsets, Size};
///
/// let padding = EdgeInsets::all(10.0);
/// let size = Size::new(100.0, 100.0);
/// let padded = padding.expand_size(size);
/// assert_eq!(padded, Size::new(120.0, 120.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EdgeInsets {
    /// The offset from the left.
    pub left: f32,

    /// The offset from the top.
    pub top: f32,

    /// The offset from the right.
    pub right: f32,

    /// The offset from the bottom.
    pub bottom: f32,
}

impl EdgeInsets {
    /// Create edge insets with the given values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(insets.left, 1.0);
    /// assert_eq!(insets.top, 2.0);
    /// assert_eq!(insets.right, 3.0);
    /// assert_eq!(insets.bottom, 4.0);
    /// ```
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Create edge insets with all sides set to the same value.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::all(10.0);
    /// assert_eq!(insets.left, 10.0);
    /// assert_eq!(insets.top, 10.0);
    /// assert_eq!(insets.right, 10.0);
    /// assert_eq!(insets.bottom, 10.0);
    /// ```
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    /// Create edge insets with zero offsets.
    pub const ZERO: Self = Self::all(0.0);

    /// Create edge insets with only the left side set.
    pub const fn only_left(value: f32) -> Self {
        Self {
            left: value,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the top side set.
    pub const fn only_top(value: f32) -> Self {
        Self {
            left: 0.0,
            top: value,
            right: 0.0,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the right side set.
    pub const fn only_right(value: f32) -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: value,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the bottom side set.
    pub const fn only_bottom(value: f32) -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: value,
        }
    }

    /// Create edge insets with symmetric horizontal and vertical values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::symmetric(5.0, 10.0);
    /// assert_eq!(insets.left, 5.0);
    /// assert_eq!(insets.right, 5.0);
    /// assert_eq!(insets.top, 10.0);
    /// assert_eq!(insets.bottom, 10.0);
    /// ```
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    /// Create edge insets with only horizontal values.
    pub const fn horizontal(value: f32) -> Self {
        Self::symmetric(value, 0.0)
    }

    /// Create edge insets with only vertical values.
    pub const fn vertical(value: f32) -> Self {
        Self::symmetric(0.0, value)
    }

    /// Get the total horizontal insets (left + right).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::symmetric(5.0, 10.0);
    /// assert_eq!(insets.horizontal_total(), 10.0);
    /// ```
    pub fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }

    /// Get the total vertical insets (top + bottom).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::symmetric(5.0, 10.0);
    /// assert_eq!(insets.vertical_total(), 20.0);
    /// ```
    pub fn vertical_total(&self) -> f32 {
        self.top + self.bottom
    }

    /// Get the total insets as a size (width: left + right, height: top + bottom).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Size};
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(insets.total_size(), Size::new(4.0, 6.0));
    /// ```
    pub fn total_size(&self) -> Size {
        Size::new(self.horizontal_total(), self.vertical_total())
    }

    /// Get the top-left offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Offset};
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(insets.top_left(), Offset::new(1.0, 2.0));
    /// ```
    pub fn top_left(&self) -> Offset {
        Offset::new(self.left, self.top)
    }

    /// Get the bottom-right offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Offset};
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(insets.bottom_right(), Offset::new(3.0, 4.0));
    /// ```
    pub fn bottom_right(&self) -> Offset {
        Offset::new(self.right, self.bottom)
    }

    /// Check if all insets are zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// assert!(EdgeInsets::ZERO.is_zero());
    /// assert!(!EdgeInsets::all(1.0).is_zero());
    /// ```
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    /// Check if all insets are non-negative.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// assert!(EdgeInsets::all(5.0).is_non_negative());
    /// assert!(!EdgeInsets::new(-1.0, 0.0, 0.0, 0.0).is_non_negative());
    /// ```
    pub fn is_non_negative(&self) -> bool {
        self.left >= 0.0 && self.top >= 0.0 && self.right >= 0.0 && self.bottom >= 0.0
    }

    /// Clamp all insets to be non-negative.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::new(-1.0, 2.0, -3.0, 4.0);
    /// let clamped = insets.clamp_non_negative();
    /// assert_eq!(clamped, EdgeInsets::new(0.0, 2.0, 0.0, 4.0));
    /// ```
    pub fn clamp_non_negative(&self) -> Self {
        Self {
            left: self.left.max(0.0),
            top: self.top.max(0.0),
            right: self.right.max(0.0),
            bottom: self.bottom.max(0.0),
        }
    }

    /// Inflate a rectangle by these insets.
    ///
    /// Increases the rectangle's size by adding the insets to all sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Point, Rect, Size};
    ///
    /// let insets = EdgeInsets::all(10.0);
    /// let rect = Rect::from_min_size(Point::ZERO, Size::new(100.0, 100.0));
    /// let inflated = insets.inflate_rect(rect);
    /// assert_eq!(inflated.min, Point::new(-10.0, -10.0));
    /// assert_eq!(inflated.max, Point::new(110.0, 110.0));
    /// ```
    pub fn inflate_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            Point::new(rect.min.x - self.left, rect.min.y - self.top),
            Point::new(rect.max.x + self.right, rect.max.y + self.bottom),
        )
    }

    /// Deflate a rectangle by these insets.
    ///
    /// Decreases the rectangle's size by subtracting the insets from all sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Point, Rect, Size};
    ///
    /// let insets = EdgeInsets::all(10.0);
    /// let rect = Rect::from_min_size(Point::ZERO, Size::new(100.0, 100.0));
    /// let deflated = insets.deflate_rect(rect);
    /// assert_eq!(deflated.min, Point::new(10.0, 10.0));
    /// assert_eq!(deflated.max, Point::new(90.0, 90.0));
    /// ```
    pub fn deflate_rect(&self, rect: impl Into<Rect>) -> Rect {
        let rect = rect.into();
        Rect::from_min_max(
            Point::new(rect.min.x + self.left, rect.min.y + self.top),
            Point::new(rect.max.x - self.right, rect.max.y - self.bottom),
        )
    }

    /// Apply these insets to a size, shrinking it.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Size};
    ///
    /// let insets = EdgeInsets::symmetric(5.0, 10.0);
    /// let size = Size::new(100.0, 100.0);
    /// let shrunk = insets.shrink_size(size);
    /// assert_eq!(shrunk, Size::new(90.0, 80.0));
    /// ```
    pub fn shrink_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        Size::new(
            (size.width - self.horizontal_total()).max(0.0),
            (size.height - self.vertical_total()).max(0.0),
        )
    }

    /// Apply these insets to a size, expanding it.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Size};
    ///
    /// let insets = EdgeInsets::symmetric(5.0, 10.0);
    /// let size = Size::new(100.0, 100.0);
    /// let expanded = insets.expand_size(size);
    /// assert_eq!(expanded, Size::new(110.0, 120.0));
    /// ```
    pub fn expand_size(&self, size: impl Into<Size>) -> Size {
        let size = size.into();
        Size::new(
            size.width + self.horizontal_total(),
            size.height + self.vertical_total(),
        )
    }

    /// Flip the insets horizontally (swap left and right).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// let flipped = insets.flip_horizontal();
    /// assert_eq!(flipped, EdgeInsets::new(3.0, 2.0, 1.0, 4.0));
    /// ```
    pub fn flip_horizontal(&self) -> Self {
        Self {
            left: self.right,
            top: self.top,
            right: self.left,
            bottom: self.bottom,
        }
    }

    /// Flip the insets vertically (swap top and bottom).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// let flipped = insets.flip_vertical();
    /// assert_eq!(flipped, EdgeInsets::new(1.0, 4.0, 3.0, 2.0));
    /// ```
    pub fn flip_vertical(&self) -> Self {
        Self {
            left: self.left,
            top: self.bottom,
            right: self.right,
            bottom: self.top,
        }
    }
}

impl From<f32> for EdgeInsets {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl From<(f32, f32)> for EdgeInsets {
    fn from((horizontal, vertical): (f32, f32)) -> Self {
        Self::symmetric(horizontal, vertical)
    }
}

impl From<(f32, f32, f32, f32)> for EdgeInsets {
    fn from((left, top, right, bottom): (f32, f32, f32, f32)) -> Self {
        Self::new(left, top, right, bottom)
    }
}

impl Add for EdgeInsets {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            left: self.left + rhs.left,
            top: self.top + rhs.top,
            right: self.right + rhs.right,
            bottom: self.bottom + rhs.bottom,
        }
    }
}

impl Sub for EdgeInsets {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            left: self.left - rhs.left,
            top: self.top - rhs.top,
            right: self.right - rhs.right,
            bottom: self.bottom - rhs.bottom,
        }
    }
}

impl Mul<f32> for EdgeInsets {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            left: self.left * rhs,
            top: self.top * rhs,
            right: self.right * rhs,
            bottom: self.bottom * rhs,
        }
    }
}

impl Div<f32> for EdgeInsets {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            left: self.left / rhs,
            top: self.top / rhs,
            right: self.right / rhs,
            bottom: self.bottom / rhs,
        }
    }
}

impl Neg for EdgeInsets {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            left: -self.left,
            top: -self.top,
            right: -self.right,
            bottom: -self.bottom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_insets_creation() {
        let all = EdgeInsets::all(10.0);
        assert_eq!(all.left, 10.0);
        assert_eq!(all.top, 10.0);
        assert_eq!(all.right, 10.0);
        assert_eq!(all.bottom, 10.0);

        let symmetric = EdgeInsets::symmetric(5.0, 10.0);
        assert_eq!(symmetric.left, 5.0);
        assert_eq!(symmetric.right, 5.0);
        assert_eq!(symmetric.top, 10.0);
        assert_eq!(symmetric.bottom, 10.0);

        let custom = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(custom.left, 1.0);
        assert_eq!(custom.top, 2.0);
        assert_eq!(custom.right, 3.0);
        assert_eq!(custom.bottom, 4.0);

        let zero = EdgeInsets::ZERO;
        assert!(zero.is_zero());
    }

    #[test]
    fn test_edge_insets_only() {
        let left = EdgeInsets::only_left(5.0);
        assert_eq!(left.left, 5.0);
        assert_eq!(left.top, 0.0);
        assert_eq!(left.right, 0.0);
        assert_eq!(left.bottom, 0.0);

        let top = EdgeInsets::only_top(5.0);
        assert_eq!(top.top, 5.0);
        assert_eq!(top.left, 0.0);

        let right = EdgeInsets::only_right(5.0);
        assert_eq!(right.right, 5.0);

        let bottom = EdgeInsets::only_bottom(5.0);
        assert_eq!(bottom.bottom, 5.0);
    }

    #[test]
    fn test_edge_insets_totals() {
        let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(insets.horizontal_total(), 4.0);
        assert_eq!(insets.vertical_total(), 6.0);
        assert_eq!(insets.total_size(), Size::new(4.0, 6.0));
    }

    #[test]
    fn test_edge_insets_rect_operations() {
        let insets = EdgeInsets::all(10.0);
        let rect = Rect::from_min_size(Point::new(0.0, 0.0), Size::new(100.0, 100.0));

        // Deflate (shrink)
        let deflated = insets.deflate_rect(rect);
        assert_eq!(deflated.min, Point::new(10.0, 10.0));
        assert_eq!(deflated.max, Point::new(90.0, 90.0));
        assert_eq!(deflated.size(), Size::new(80.0, 80.0));

        // Inflate (expand)
        let inflated = insets.inflate_rect(rect);
        assert_eq!(inflated.min, Point::new(-10.0, -10.0));
        assert_eq!(inflated.max, Point::new(110.0, 110.0));
        assert_eq!(inflated.size(), Size::new(120.0, 120.0));
    }

    #[test]
    fn test_edge_insets_size_operations() {
        let insets = EdgeInsets::symmetric(5.0, 10.0);
        let size = Size::new(100.0, 100.0);

        let shrunk = insets.shrink_size(size);
        assert_eq!(shrunk, Size::new(90.0, 80.0));

        let expanded = insets.expand_size(size);
        assert_eq!(expanded, Size::new(110.0, 120.0));
    }

    #[test]
    fn test_edge_insets_arithmetic() {
        let a = EdgeInsets::all(10.0);
        let b = EdgeInsets::all(5.0);

        let sum = a + b;
        assert_eq!(sum, EdgeInsets::all(15.0));

        let diff = a - b;
        assert_eq!(diff, EdgeInsets::all(5.0));

        let product = a * 2.0;
        assert_eq!(product, EdgeInsets::all(20.0));

        let quotient = a / 2.0;
        assert_eq!(quotient, EdgeInsets::all(5.0));

        let negated = -a;
        assert_eq!(negated, EdgeInsets::all(-10.0));
    }

    #[test]
    fn test_edge_insets_flip() {
        let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);

        let h_flipped = insets.flip_horizontal();
        assert_eq!(h_flipped.left, 3.0);
        assert_eq!(h_flipped.right, 1.0);
        assert_eq!(h_flipped.top, 2.0);
        assert_eq!(h_flipped.bottom, 4.0);

        let v_flipped = insets.flip_vertical();
        assert_eq!(v_flipped.left, 1.0);
        assert_eq!(v_flipped.right, 3.0);
        assert_eq!(v_flipped.top, 4.0);
        assert_eq!(v_flipped.bottom, 2.0);
    }

    #[test]
    fn test_edge_insets_validation() {
        let positive = EdgeInsets::all(5.0);
        assert!(positive.is_non_negative());

        let negative = EdgeInsets::new(-5.0, 10.0, -3.0, 2.0);
        assert!(!negative.is_non_negative());

        let clamped = negative.clamp_non_negative();
        assert!(clamped.is_non_negative());
        assert_eq!(clamped.left, 0.0);
        assert_eq!(clamped.top, 10.0);
        assert_eq!(clamped.right, 0.0);
        assert_eq!(clamped.bottom, 2.0);
    }

    #[test]
    fn test_edge_insets_conversions() {
        let from_f32: EdgeInsets = 10.0.into();
        assert_eq!(from_f32, EdgeInsets::all(10.0));

        let from_tuple: EdgeInsets = (5.0, 10.0).into();
        assert_eq!(from_tuple, EdgeInsets::symmetric(5.0, 10.0));

        let from_tuple4: EdgeInsets = (1.0, 2.0, 3.0, 4.0).into();
        assert_eq!(from_tuple4, EdgeInsets::new(1.0, 2.0, 3.0, 4.0));
    }
}
