//! RelativeRect - positioning relative to parent bounds
//!
//! Similar to Flutter's `RelativeRect`. Used for `Positioned` widget
//! and animations like `RelativeRectTween`.

use super::Pixels;
use crate::{Offset, Rect, Size};

use super::traits::{NumericUnit, Unit};
use std::ops::{Add, Sub, Mul, Neg};

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

// ============================================================================
// Basic Constructors (generic over Unit)
// ============================================================================

impl<T: Unit> RelativeRect<T> {
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

    #[must_use]
    pub fn from_size(offset: Offset<T>, size: Size<T>, parent: Size<T>) -> Self {
        Self {
            left: offset.dx,
            top: offset.dy,
            right: parent.width - offset.dx - size.width,
            bottom: parent.height - offset.dy - size.height,
        }
    }

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

    #[must_use]
    pub fn to_size(&self, parent: Size<T>) -> Size<T> {
        Size::new(
            parent.width - self.left - self.right,
            parent.height - self.top - self.bottom,
        )
    }

    #[must_use]
    pub fn shift(&self, offset: Offset<T>) -> Self {
        Self {
            left: self.left + offset.dx,
            top: self.top + offset.dy,
            right: self.right - offset.dx,
            bottom: self.bottom - offset.dy,
        }
    }

    #[must_use]
    pub fn inflate(&self, delta: T) -> Self {
        Self {
            left: self.left - delta,
            top: self.top - delta,
            right: self.right - delta,
            bottom: self.bottom - delta,
        }
    }

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

// ============================================================================
// Lerp Support (f32 only)
// ============================================================================

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

