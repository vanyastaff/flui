//! Edge insets for padding and margins
//!
//! This module contains types for representing padding and margins,
//! similar to Flutter's EdgeInsets system.

use crate::geometry::{px, Pixels};
use std::ops::{Add, Div, Mul, Neg, Sub};

use crate::{Offset, Point, Rect, Size};

#[derive(Copy, Clone, Debug, PartialEq)]
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

    #[must_use]
    pub fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }

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
    /// assert_eq!(insets.total_size(), Size::new(px(4.0), px(6.0)));
    /// ```
    pub fn total_size(&self) -> Size<Pixels> {
        Size::new(px(self.horizontal_total()), px(self.vertical_total()))
    }

    /// Get the top-left offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Offset};
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(insets.top_left(), Offset::new(px(1.0), px(2.0)));
    /// ```
    pub fn top_left(&self) -> Offset<Pixels> {
        Offset::new(px(self.left), px(self.top))
    }

    /// Get the bottom-right offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Offset};
    ///
    /// let insets = EdgeInsets::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(insets.bottom_right(), Offset::new(px(3.0), px(4.0)));
    /// ```
    pub fn bottom_right(&self) -> Offset<Pixels> {
        Offset::new(px(self.right), px(self.bottom))
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    #[must_use]
    pub fn is_non_negative(&self) -> bool {
        self.left >= 0.0 && self.top >= 0.0 && self.right >= 0.0 && self.bottom >= 0.0
    }

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
    /// let rect = Rect::from_origin_size(Point::ZERO, Size::new(px(100.0), px(100.0)));
    /// let inflated = insets.inflate_rect(rect);
    /// assert_eq!(inflated.min, Point::new(px(-10.0), px(-10.0)));
    /// assert_eq!(inflated.max, Point::new(px(110.0), px(110.0)));
    /// ```
    pub fn inflate_rect(&self, rect: impl Into<Rect<Pixels>>) -> Rect<Pixels> {
        let rect = rect.into();
        Rect::from_min_max(
            Point::new(rect.min.x - px(self.left), rect.min.y - px(self.top)),
            Point::new(rect.max.x + px(self.right), rect.max.y + px(self.bottom)),
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
    /// let rect = Rect::from_origin_size(Point::ZERO, Size::new(px(100.0), px(100.0)));
    /// let deflated = insets.deflate_rect(rect);
    /// assert_eq!(deflated.min, Point::new(px(10.0), px(10.0)));
    /// assert_eq!(deflated.max, Point::new(px(90.0), px(90.0)));
    /// ```
    pub fn deflate_rect(&self, rect: impl Into<Rect<Pixels>>) -> Rect<Pixels> {
        let rect = rect.into();
        Rect::from_min_max(
            Point::new(rect.min.x + px(self.left), rect.min.y + px(self.top)),
            Point::new(rect.max.x - px(self.right), rect.max.y - px(self.bottom)),
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
    /// let size = Size::new(px(100.0), px(100.0));
    /// let shrunk = insets.shrink_size(size);
    /// assert_eq!(shrunk, Size::new(px(90.0), px(80.0)));
    /// ```

    /// Apply these insets to a size, expanding it.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{EdgeInsets, Size};
    ///
    /// let insets = EdgeInsets::symmetric(5.0, 10.0);
    /// let size = Size::new(px(100.0), px(100.0));
    /// let expanded = insets.expand_size(size);
    /// assert_eq!(expanded, Size::new(px(110.0), px(120.0)));
    /// ```

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

#[derive(Copy, Clone, Debug, PartialEq)]
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

#[derive(Copy, Clone, Debug, PartialEq)]
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
