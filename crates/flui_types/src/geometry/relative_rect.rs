//! RelativeRect - positioning relative to parent bounds
//!
//! Similar to Flutter's `RelativeRect`. Used for `Positioned` widget
//! and animations like `RelativeRectTween`.

use crate::{Offset, Rect, Size};

use super::traits::{NumericUnit, Unit};
use std::ops::{Add, Sub, Mul, Neg};

/// A set of offsets from each edge of a rectangle.
///
/// Used to describe the position of a rectangle relative to another rectangle.
/// This is useful for `Positioned` widgets within a `Stack`.
///
/// Unlike `EdgeInsets`, the values represent offsets from the corresponding
/// edges of the parent, not padding inwards.
///
/// Generic over unit type `T`. Common usage:
/// - `RelativeRect<Pixels>` - UI positioning
/// - `RelativeRect<f32>` - Normalized/dimensionless positioning
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{RelativeRect, px, Pixels};
/// use flui_types::Size;
///
/// // Position 10 pixels from left, 20 from top
/// let rect = RelativeRect::<Pixels>::from_ltrb(px(10.0), px(20.0), px(30.0), px(40.0));
///
/// // Convert to Rect given parent size
/// let parent = Size::<Pixels>::new(px(200.0), px(300.0));
/// let positioned = rect.to_rect(parent);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RelativeRect<T: Unit> {
    /// Offset from the left edge of the parent.
    pub left: T,
    /// Offset from the top edge of the parent.
    pub top: T,
    /// Offset from the right edge of the parent (parent.width - child.right).
    pub right: T,
    /// Offset from the bottom edge of the parent (parent.height - child.bottom).
    pub bottom: T,
}

// ============================================================================
// Constants (f32 only for backwards compatibility)
// ============================================================================

impl RelativeRect<f32> {
    /// A rect that covers the entire parent.
    pub const FILL: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> RelativeRect<T> {
    /// Creates a RelativeRect from left, top, right, bottom values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{RelativeRect, px};
    ///
    /// let rect = RelativeRect::from_ltrb(px(10.0), px(20.0), px(30.0), px(40.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn from_ltrb(left: T, top: T, right: T, bottom: T) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

// ============================================================================
// Numeric Unit Operations
// ============================================================================

impl<T: NumericUnit> RelativeRect<T>
where
    T: Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{

    /// Creates a RelativeRect positioned at the given offset with the given size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{RelativeRect, px};
    /// use flui_types::{Offset, Size};
    ///
    /// let relative = RelativeRect::from_size(
    ///     Offset::new(px(10.0), px(20.0)),
    ///     Size::new(px(50.0), px(60.0)),
    ///     Size::new(px(200.0), px(300.0)),
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn from_size(offset: Offset<T>, size: Size<T>, parent: Size<T>) -> Self {
        Self {
            left: offset.dx,
            top: offset.dy,
            right: parent.width - offset.dx - size.width,
            bottom: parent.height - offset.dy - size.height,
        }
    }

    /// Creates a RelativeRect with only left and top specified.
    ///
    /// Right and bottom will be set such that the child has the given size.
    #[inline]
    #[must_use]
    pub fn from_left_top_width_height(
        left: T,
        top: T,
        width: T,
        height: T,
        parent: Size<T>,
    ) -> Self {
        Self {
            left,
            top,
            right: parent.width - left - width,
            bottom: parent.height - top - height,
        }
    }



    /// Returns the size of this rect given the parent size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{RelativeRect, px};
    /// use flui_types::Size;
    ///
    /// let relative = RelativeRect::from_ltrb(px(10.0), px(20.0), px(30.0), px(40.0));
    /// let parent = Size::new(px(200.0), px(300.0));
    /// let size = relative.to_size(parent);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_size(&self, parent: Size<T>) -> Size<T> {
        Size::new(
            parent.width - self.left - self.right,
            parent.height - self.top - self.bottom,
        )
    }

    /// Returns a new RelativeRect shifted by the given offset.
    #[inline]
    #[must_use]
    pub fn shift(&self, offset: Offset<T>) -> Self {
        Self {
            left: self.left + offset.dx,
            top: self.top + offset.dy,
            right: self.right - offset.dx,
            bottom: self.bottom - offset.dy,
        }
    }

    /// Returns a new RelativeRect inflated by the given delta.
    ///
    /// Positive delta makes the rect larger, negative makes it smaller.
    #[inline]
    #[must_use]
    pub fn inflate(&self, delta: T) -> Self {
        Self {
            left: self.left - delta,
            top: self.top - delta,
            right: self.right - delta,
            bottom: self.bottom - delta,
        }
    }

    /// Returns a new RelativeRect deflated by the given delta.
    ///
    /// This is the same as `inflate(-delta)`.
    #[inline]
    #[must_use]
    pub fn deflate(&self, delta: T) -> Self
    where
        T: Neg<Output = T>,
    {
        self.inflate(-delta)
    }
}

// ============================================================================
// f32 Float Operations
// ============================================================================

impl RelativeRect<f32> {
    /// Creates a RelativeRect from a Rect and a parent Size.
    ///
    /// The child rect's position is converted to offsets from the parent edges.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::RelativeRect;
    /// use flui_types::{Rect, Size};
    ///
    /// let child = Rect::from_xywh(10.0, 20.0, 50.0, 60.0);
    /// let parent = Size::new(200.0, 300.0);
    /// let relative = RelativeRect::from_rect(child, parent);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_rect(rect: Rect, parent: Size<f32>) -> Self {
        Self {
            left: rect.left(),
            top: rect.top(),
            right: parent.width - rect.right(),
            bottom: parent.height - rect.bottom(),
        }
    }

    /// Converts this RelativeRect to a Rect given the parent size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::RelativeRect;
    /// use flui_types::Size;
    ///
    /// let relative = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
    /// let parent = Size::new(200.0, 300.0);
    /// let rect = relative.to_rect(parent);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_rect(&self, parent: Size<f32>) -> Rect {
        Rect::from_ltrb(
            self.left,
            self.top,
            parent.width - self.right,
            parent.height - self.bottom,
        )
    }

    /// Returns whether this rect has non-negative dimensions.
    ///
    /// A rect with negative dimensions means left + right > parent.width
    /// or top + bottom > parent.height.
    #[inline]
    #[must_use]
    pub fn has_infinite_dimensions(&self) -> bool {
        self.left.is_infinite()
            || self.top.is_infinite()
            || self.right.is_infinite()
            || self.bottom.is_infinite()
    }

    /// Returns whether all values are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.left.is_finite()
            && self.top.is_finite()
            && self.right.is_finite()
            && self.bottom.is_finite()
    }
}

// ============================================================================
// Lerp Support (f32 only)
// ============================================================================

impl RelativeRect<f32> {
    /// Linearly interpolates between two RelativeRects.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::RelativeRect;
    ///
    /// let a = RelativeRect::from_ltrb(0.0, 0.0, 100.0, 100.0);
    /// let b = RelativeRect::from_ltrb(10.0, 20.0, 80.0, 60.0);
    /// let mid = RelativeRect::lerp(a, b, 0.5);
    /// ```
    #[inline]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            left: a.left + (b.left - a.left) * t,
            top: a.top + (b.top - a.top) * t,
            right: a.right + (b.right - a.right) * t,
            bottom: a.bottom + (b.bottom - a.bottom) * t,
        }
    }
}

// ============================================================================
// Default Implementation
// ============================================================================

impl<T: Unit> Default for RelativeRect<T> {
    fn default() -> Self {
        Self {
            left: T::zero(),
            top: T::zero(),
            right: T::zero(),
            bottom: T::zero(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_ltrb() {
        let rect = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
        assert_eq!(rect.left, 10.0);
        assert_eq!(rect.top, 20.0);
        assert_eq!(rect.right, 30.0);
        assert_eq!(rect.bottom, 40.0);
    }

    #[test]
    fn test_from_rect() {
        let child = Rect::from_xywh(10.0, 20.0, 50.0, 60.0);
        let parent = Size::new(200.0, 300.0);
        let relative = RelativeRect::from_rect(child, parent);

        assert_eq!(relative.left, 10.0);
        assert_eq!(relative.top, 20.0);
        assert_eq!(relative.right, 140.0);
        assert_eq!(relative.bottom, 220.0);
    }

    #[test]
    fn test_to_rect() {
        let relative = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
        let parent = Size::new(200.0, 300.0);
        let rect = relative.to_rect(parent);

        assert_eq!(rect.left(), 10.0);
        assert_eq!(rect.top(), 20.0);
        assert_eq!(rect.right(), 170.0);
        assert_eq!(rect.bottom(), 260.0);
    }

    #[test]
    fn test_to_size() {
        let relative = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
        let parent = Size::new(200.0, 300.0);
        let size = relative.to_size(parent);

        assert_eq!(size.width, 160.0);
        assert_eq!(size.height, 240.0);
    }

    #[test]
    fn test_fill() {
        let fill = RelativeRect::FILL;
        let parent = Size::new(100.0, 200.0);
        let rect = fill.to_rect(parent);

        assert_eq!(rect.left(), 0.0);
        assert_eq!(rect.top(), 0.0);
        assert_eq!(rect.right(), 100.0);
        assert_eq!(rect.bottom(), 200.0);
    }

    #[test]
    fn test_lerp() {
        let a = RelativeRect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let b = RelativeRect::from_ltrb(10.0, 20.0, 80.0, 60.0);
        let mid = RelativeRect::lerp(a, b, 0.5);

        assert_eq!(mid.left, 5.0);
        assert_eq!(mid.top, 10.0);
        assert_eq!(mid.right, 90.0);
        assert_eq!(mid.bottom, 80.0);
    }

    #[test]
    fn test_shift() {
        let rect = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
        let shifted = rect.shift(Offset::new(5.0, 10.0));

        assert_eq!(shifted.left, 15.0);
        assert_eq!(shifted.top, 30.0);
        assert_eq!(shifted.right, 25.0);
        assert_eq!(shifted.bottom, 30.0);
    }

    #[test]
    fn test_inflate() {
        let rect = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
        let inflated = rect.inflate(5.0);

        assert_eq!(inflated.left, 5.0);
        assert_eq!(inflated.top, 15.0);
        assert_eq!(inflated.right, 25.0);
        assert_eq!(inflated.bottom, 35.0);
    }
}
