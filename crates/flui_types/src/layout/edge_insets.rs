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

    /// Create uniform edge insets (same value on all sides).
    ///
    /// This is an alias for `all()` that may be more intuitive in some contexts.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::EdgeInsets;
    ///
    /// let padding = EdgeInsets::uniform(16.0);
    /// assert_eq!(padding, EdgeInsets::all(16.0));
    /// ```
    #[inline]
    pub const fn uniform(value: f32) -> Self {
        Self::all(value)
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
    #[inline]
    #[must_use]
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
    #[inline]
    #[must_use]
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
    #[inline]
    #[must_use]
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
    #[inline]
    #[must_use]
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
    #[inline]
    #[must_use]
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

/// An immutable set of offsets in each of the four cardinal directions using start/end instead of left/right.
///
/// This is useful for internationalization where the direction of text may vary (LTR vs RTL).
/// Start corresponds to the beginning of text direction (left in LTR, right in RTL).
/// End corresponds to the end of text direction (right in LTR, left in RTL).
///
/// Similar to Flutter's `EdgeInsetsDirectional`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EdgeInsetsDirectional {
    /// The offset from the start (left in LTR, right in RTL).
    pub start: f32,
    /// The offset from the top.
    pub top: f32,
    /// The offset from the end (right in LTR, left in RTL).
    pub end: f32,
    /// The offset from the bottom.
    pub bottom: f32,
}

impl EdgeInsetsDirectional {
    /// Create directional edge insets with the given values.
    pub const fn new(start: f32, top: f32, end: f32, bottom: f32) -> Self {
        Self {
            start,
            top,
            end,
            bottom,
        }
    }

    /// Create directional edge insets with all sides set to the same value.
    pub const fn all(value: f32) -> Self {
        Self {
            start: value,
            top: value,
            end: value,
            bottom: value,
        }
    }

    /// Create edge insets with zero offsets.
    pub const ZERO: Self = Self::all(0.0);

    /// Create edge insets with only the start side set.
    pub const fn only_start(value: f32) -> Self {
        Self {
            start: value,
            top: 0.0,
            end: 0.0,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the top side set.
    pub const fn only_top(value: f32) -> Self {
        Self {
            start: 0.0,
            top: value,
            end: 0.0,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the end side set.
    pub const fn only_end(value: f32) -> Self {
        Self {
            start: 0.0,
            top: 0.0,
            end: value,
            bottom: 0.0,
        }
    }

    /// Create edge insets with only the bottom side set.
    pub const fn only_bottom(value: f32) -> Self {
        Self {
            start: 0.0,
            top: 0.0,
            end: 0.0,
            bottom: value,
        }
    }

    /// Create edge insets with symmetric horizontal and vertical values.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            start: horizontal,
            top: vertical,
            end: horizontal,
            bottom: vertical,
        }
    }

    /// Resolve to absolute EdgeInsets based on text direction.
    ///
    /// # Arguments
    ///
    /// * `is_ltr` - true for left-to-right, false for right-to-left
    pub fn resolve(&self, is_ltr: bool) -> EdgeInsets {
        if is_ltr {
            EdgeInsets::new(self.start, self.top, self.end, self.bottom)
        } else {
            EdgeInsets::new(self.end, self.top, self.start, self.bottom)
        }
    }

    /// Get the total horizontal insets (start + end).
    pub fn horizontal_total(&self) -> f32 {
        self.start + self.end
    }

    /// Get the total vertical insets (top + bottom).
    pub fn vertical_total(&self) -> f32 {
        self.top + self.bottom
    }

    /// Check if all insets are zero.
    pub fn is_zero(&self) -> bool {
        self.start == 0.0 && self.top == 0.0 && self.end == 0.0 && self.bottom == 0.0
    }

    /// Check if all insets are non-negative.
    pub fn is_non_negative(&self) -> bool {
        self.start >= 0.0 && self.top >= 0.0 && self.end >= 0.0 && self.bottom >= 0.0
    }
}

impl Add for EdgeInsetsDirectional {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            start: self.start + rhs.start,
            top: self.top + rhs.top,
            end: self.end + rhs.end,
            bottom: self.bottom + rhs.bottom,
        }
    }
}

impl Sub for EdgeInsetsDirectional {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            start: self.start - rhs.start,
            top: self.top - rhs.top,
            end: self.end - rhs.end,
            bottom: self.bottom - rhs.bottom,
        }
    }
}

impl Mul<f32> for EdgeInsetsDirectional {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            start: self.start * rhs,
            top: self.top * rhs,
            end: self.end * rhs,
            bottom: self.bottom * rhs,
        }
    }
}

impl Div<f32> for EdgeInsetsDirectional {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            start: self.start / rhs,
            top: self.top / rhs,
            end: self.end / rhs,
            bottom: self.bottom / rhs,
        }
    }
}

/// Base class for EdgeInsets and EdgeInsetsDirectional.
///
/// This enum allows working with both absolute and directional insets uniformly.
/// Similar to Flutter's `EdgeInsetsGeometry`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EdgeInsetsGeometry {
    /// Absolute edge insets (left, top, right, bottom).
    Absolute(EdgeInsets),
    /// Directional edge insets (start, top, end, bottom).
    Directional(EdgeInsetsDirectional),
}

impl EdgeInsetsGeometry {
    /// Resolve to absolute EdgeInsets based on text direction.
    ///
    /// # Arguments
    ///
    /// * `is_ltr` - true for left-to-right, false for right-to-left
    pub fn resolve(&self, is_ltr: bool) -> EdgeInsets {
        match self {
            EdgeInsetsGeometry::Absolute(insets) => *insets,
            EdgeInsetsGeometry::Directional(insets) => insets.resolve(is_ltr),
        }
    }

    /// Get the total horizontal insets.
    pub fn horizontal_total(&self) -> f32 {
        match self {
            EdgeInsetsGeometry::Absolute(insets) => insets.horizontal_total(),
            EdgeInsetsGeometry::Directional(insets) => insets.horizontal_total(),
        }
    }

    /// Get the total vertical insets.
    pub fn vertical_total(&self) -> f32 {
        match self {
            EdgeInsetsGeometry::Absolute(insets) => insets.vertical_total(),
            EdgeInsetsGeometry::Directional(insets) => insets.vertical_total(),
        }
    }

    /// Check if all insets are zero.
    pub fn is_zero(&self) -> bool {
        match self {
            EdgeInsetsGeometry::Absolute(insets) => insets.is_zero(),
            EdgeInsetsGeometry::Directional(insets) => insets.is_zero(),
        }
    }
}

impl From<EdgeInsets> for EdgeInsetsGeometry {
    fn from(insets: EdgeInsets) -> Self {
        EdgeInsetsGeometry::Absolute(insets)
    }
}

impl From<EdgeInsetsDirectional> for EdgeInsetsGeometry {
    fn from(insets: EdgeInsetsDirectional) -> Self {
        EdgeInsetsGeometry::Directional(insets)
    }
}

impl Default for EdgeInsetsGeometry {
    fn default() -> Self {
        EdgeInsetsGeometry::Absolute(EdgeInsets::ZERO)
    }
}

#[cfg(test)]
mod directional_tests {
    use super::*;

    #[test]
    fn test_edge_insets_directional_creation() {
        let all = EdgeInsetsDirectional::all(10.0);
        assert_eq!(all.start, 10.0);
        assert_eq!(all.end, 10.0);

        let custom = EdgeInsetsDirectional::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(custom.start, 1.0);
        assert_eq!(custom.top, 2.0);
        assert_eq!(custom.end, 3.0);
        assert_eq!(custom.bottom, 4.0);
    }

    #[test]
    fn test_edge_insets_directional_resolve_ltr() {
        let directional = EdgeInsetsDirectional::new(10.0, 20.0, 30.0, 40.0);
        let resolved = directional.resolve(true); // LTR

        assert_eq!(resolved.left, 10.0); // start -> left
        assert_eq!(resolved.top, 20.0);
        assert_eq!(resolved.right, 30.0); // end -> right
        assert_eq!(resolved.bottom, 40.0);
    }

    #[test]
    fn test_edge_insets_directional_resolve_rtl() {
        let directional = EdgeInsetsDirectional::new(10.0, 20.0, 30.0, 40.0);
        let resolved = directional.resolve(false); // RTL

        assert_eq!(resolved.left, 30.0); // end -> left
        assert_eq!(resolved.top, 20.0);
        assert_eq!(resolved.right, 10.0); // start -> right
        assert_eq!(resolved.bottom, 40.0);
    }

    #[test]
    fn test_edge_insets_directional_arithmetic() {
        let a = EdgeInsetsDirectional::all(10.0);
        let b = EdgeInsetsDirectional::all(5.0);

        let sum = a + b;
        assert_eq!(sum.start, 15.0);

        let diff = a - b;
        assert_eq!(diff.start, 5.0);

        let product = a * 2.0;
        assert_eq!(product.start, 20.0);

        let quotient = a / 2.0;
        assert_eq!(quotient.start, 5.0);
    }

    #[test]
    fn test_edge_insets_geometry_from_absolute() {
        let insets = EdgeInsets::all(10.0);
        let geometry: EdgeInsetsGeometry = insets.into();

        let resolved = geometry.resolve(true);
        assert_eq!(resolved, insets);

        let resolved_rtl = geometry.resolve(false);
        assert_eq!(resolved_rtl, insets); // Absolute insets don't change with direction
    }

    #[test]
    fn test_edge_insets_geometry_from_directional() {
        let directional = EdgeInsetsDirectional::new(10.0, 20.0, 30.0, 40.0);
        let geometry: EdgeInsetsGeometry = directional.into();

        let resolved_ltr = geometry.resolve(true);
        assert_eq!(resolved_ltr.left, 10.0);
        assert_eq!(resolved_ltr.right, 30.0);

        let resolved_rtl = geometry.resolve(false);
        assert_eq!(resolved_rtl.left, 30.0);
        assert_eq!(resolved_rtl.right, 10.0);
    }

    #[test]
    fn test_edge_insets_geometry_totals() {
        let abs_geometry: EdgeInsetsGeometry = EdgeInsets::symmetric(5.0, 10.0).into();
        assert_eq!(abs_geometry.horizontal_total(), 10.0);
        assert_eq!(abs_geometry.vertical_total(), 20.0);

        let dir_geometry: EdgeInsetsGeometry = EdgeInsetsDirectional::symmetric(5.0, 10.0).into();
        assert_eq!(dir_geometry.horizontal_total(), 10.0);
        assert_eq!(dir_geometry.vertical_total(), 20.0);
    }

    #[test]
    fn test_edge_insets_geometry_is_zero() {
        let zero_abs: EdgeInsetsGeometry = EdgeInsets::ZERO.into();
        assert!(zero_abs.is_zero());

        let zero_dir: EdgeInsetsGeometry = EdgeInsetsDirectional::ZERO.into();
        assert!(zero_dir.is_zero());

        let non_zero: EdgeInsetsGeometry = EdgeInsets::all(1.0).into();
        assert!(!non_zero.is_zero());
    }
}
