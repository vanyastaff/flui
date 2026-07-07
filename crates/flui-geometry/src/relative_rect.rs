//! RelativeRect - positioning relative to parent bounds
//!
//! Similar to Flutter's `RelativeRect`. Used for `Positioned` widget
//! and animations like `RelativeRectTween`.

use std::ops::{Add, Mul, Neg, Sub};

use super::traits::{NumericUnit, Unit};
use crate::{Offset, Size};

/// A rectangle expressed as distances from the edges of a parent rectangle.
///
/// Unlike a plain rect, each field is an inset from the corresponding parent
/// edge, so the described rectangle depends on the parent's size. Equivalent
/// to Flutter's `RelativeRect`, used by `Positioned` and `RelativeRectTween`.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RelativeRect<T: Unit> {
    /// Offset from the left edge of the parent.
    pub left: T,
    /// Offset from the top edge of the parent.
    pub top: T,
    /// Offset from the right edge of the parent (parent.width - child.right).
    pub right: T,
    /// Offset from the bottom edge of the parent (parent.height -
    /// child.bottom).
    pub bottom: T,
}

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> RelativeRect<T> {
    /// Creates a relative rect from offsets to the left, top, right, and
    /// bottom parent edges.
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
    /// Creates a relative rect for a child at `offset` with the given `size`
    /// inside a parent of size `parent`.
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

    /// Creates a relative rect from a left/top position and a width/height
    /// inside a parent of size `parent`.
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

    /// Returns the size of the described rectangle within a parent of size
    /// `parent`.
    #[inline]
    #[must_use]
    pub fn to_size(&self, parent: Size<T>) -> Size<T> {
        Size::new(
            parent.width - self.left - self.right,
            parent.height - self.top - self.bottom,
        )
    }

    /// Returns a new relative rect translated by the given offset, keeping
    /// the same size.
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

    /// Returns a new relative rect grown by `delta` on each side (each edge
    /// inset is reduced by `delta`).
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

    /// Returns a new relative rect shrunk by `delta` on each side.
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
