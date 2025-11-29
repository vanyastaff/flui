//! RelativeRect - positioning relative to parent bounds
//!
//! Similar to Flutter's `RelativeRect`. Used for `Positioned` widget
//! and animations like `RelativeRectTween`.

use crate::{Offset, Rect, Size};

/// A set of offsets from each edge of a rectangle.
///
/// Used to describe the position of a rectangle relative to another rectangle.
/// This is useful for `Positioned` widgets within a `Stack`.
///
/// Unlike `EdgeInsets`, the values represent offsets from the corresponding
/// edges of the parent, not padding inwards.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::RelativeRect;
/// use flui_types::{Rect, Size};
///
/// // Position 10 pixels from left, 20 from top
/// let rect = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
///
/// // Convert to Rect given parent size
/// let parent = Size::new(200.0, 300.0);
/// let positioned = rect.to_rect(parent);
/// assert_eq!(positioned.left, 10.0);
/// assert_eq!(positioned.top, 20.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RelativeRect {
    /// Offset from the left edge of the parent.
    pub left: f32,
    /// Offset from the top edge of the parent.
    pub top: f32,
    /// Offset from the right edge of the parent (parent.width - child.right).
    pub right: f32,
    /// Offset from the bottom edge of the parent (parent.height - child.bottom).
    pub bottom: f32,
}

impl RelativeRect {
    /// A rect that covers the entire parent.
    pub const FILL: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    /// Creates a RelativeRect from left, top, right, bottom values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::RelativeRect;
    ///
    /// let rect = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
    /// assert_eq!(rect.left, 10.0);
    /// assert_eq!(rect.top, 20.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn from_ltrb(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

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
    ///
    /// assert_eq!(relative.left, 10.0);
    /// assert_eq!(relative.top, 20.0);
    /// assert_eq!(relative.right, 140.0); // 200 - 10 - 50
    /// assert_eq!(relative.bottom, 220.0); // 300 - 20 - 60
    /// ```
    #[inline]
    #[must_use]
    pub fn from_rect(rect: Rect, parent: Size) -> Self {
        Self {
            left: rect.left(),
            top: rect.top(),
            right: parent.width - rect.right(),
            bottom: parent.height - rect.bottom(),
        }
    }

    /// Creates a RelativeRect positioned at the given offset with the given size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::RelativeRect;
    /// use flui_types::{Offset, Size};
    ///
    /// let relative = RelativeRect::from_size(
    ///     Offset::new(10.0, 20.0),
    ///     Size::new(50.0, 60.0),
    ///     Size::new(200.0, 300.0),
    /// );
    ///
    /// assert_eq!(relative.left, 10.0);
    /// assert_eq!(relative.top, 20.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_size(offset: Offset, size: Size, parent: Size) -> Self {
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
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        parent: Size,
    ) -> Self {
        Self {
            left,
            top,
            right: parent.width - left - width,
            bottom: parent.height - top - height,
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
    ///
    /// assert_eq!(rect.left, 10.0);
    /// assert_eq!(rect.top, 20.0);
    /// assert_eq!(rect.right, 170.0); // 200 - 30
    /// assert_eq!(rect.bottom, 260.0); // 300 - 40
    /// ```
    #[inline]
    #[must_use]
    pub fn to_rect(&self, parent: Size) -> Rect {
        Rect::from_ltrb(
            self.left,
            self.top,
            parent.width - self.right,
            parent.height - self.bottom,
        )
    }

    /// Returns the size of this rect given the parent size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::RelativeRect;
    /// use flui_types::Size;
    ///
    /// let relative = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
    /// let parent = Size::new(200.0, 300.0);
    /// let size = relative.to_size(parent);
    ///
    /// assert_eq!(size.width, 160.0); // 200 - 10 - 30
    /// assert_eq!(size.height, 240.0); // 300 - 20 - 40
    /// ```
    #[inline]
    #[must_use]
    pub fn to_size(&self, parent: Size) -> Size {
        Size::new(
            parent.width - self.left - self.right,
            parent.height - self.top - self.bottom,
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
    ///
    /// assert_eq!(mid.left, 5.0);
    /// assert_eq!(mid.top, 10.0);
    /// assert_eq!(mid.right, 90.0);
    /// assert_eq!(mid.bottom, 80.0);
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

    /// Returns a new RelativeRect shifted by the given offset.
    #[inline]
    #[must_use]
    pub fn shift(&self, offset: Offset) -> Self {
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
    pub fn inflate(&self, delta: f32) -> Self {
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
    pub fn deflate(&self, delta: f32) -> Self {
        self.inflate(-delta)
    }
}

impl Default for RelativeRect {
    fn default() -> Self {
        Self::FILL
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
