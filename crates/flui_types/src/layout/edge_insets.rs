//! Edge insets for padding and margins
//!
//! **DEPRECATED:** This module is deprecated in favor of `geometry::Edges<Pixels>`.
//!
//! The `EdgeInsets` type has been replaced by `Edges<Pixels>` to maintain consistency
//! with the generic Unit system. All functionality has been migrated to `Edges<Pixels>`.
//!
//! Migration guide:
//! - Replace `EdgeInsets::new(...)` with `Edges::<Pixels>::new(...)`
//! - Replace `EdgeInsets::all(10.0)` with `Edges::all(px(10.0))`
//! - Replace `EdgeInsets::symmetric(h, v)` with `Edges::symmetric(px(v), px(h))`
//! - All methods are available on `Edges<Pixels>` with the same names

use crate::geometry::{Edges, Pixels};

/// **DEPRECATED:** Use `Edges<Pixels>` instead.
///
/// This type alias is provided for backward compatibility only.
/// New code should use `Edges<Pixels>` directly.
#[deprecated(
    since = "0.1.0",
    note = "Use `Edges<Pixels>` from `geometry` module instead"
)]
pub type EdgeInsets = Edges<Pixels>;

/// **DEPRECATED:** Use `Edges<Pixels>` with directionality support instead.
///
/// Directional edge insets that support both LTR and RTL text directions.
/// This type will be migrated to a generic solution in a future update.
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

    /// Resolve to absolute Edges based on text direction.
    ///
    /// # Arguments
    ///
    /// * `is_ltr` - true for left-to-right, false for right-to-left
    #[allow(deprecated)]
    pub fn resolve(&self, is_ltr: bool) -> EdgeInsets {
        use crate::geometry::px;
        if is_ltr {
            Edges::new(px(self.top), px(self.end), px(self.bottom), px(self.start))
        } else {
            Edges::new(px(self.top), px(self.start), px(self.bottom), px(self.end))
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

impl std::ops::Add for EdgeInsetsDirectional {
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

impl std::ops::Sub for EdgeInsetsDirectional {
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

impl std::ops::Mul<f32> for EdgeInsetsDirectional {
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

impl std::ops::Div<f32> for EdgeInsetsDirectional {
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

/// Edge insets geometry that can be either absolute or directional.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EdgeInsetsGeometry {
    /// Absolute edge insets (left, top, right, bottom).
    #[allow(deprecated)]
    Absolute(EdgeInsets),
    /// Directional edge insets (start, top, end, bottom).
    Directional(EdgeInsetsDirectional),
}

impl EdgeInsetsGeometry {
    /// Resolve to absolute Edges based on text direction.
    ///
    /// # Arguments
    ///
    /// * `is_ltr` - true for left-to-right, false for right-to-left
    #[allow(deprecated)]
    pub fn resolve(&self, is_ltr: bool) -> EdgeInsets {
        match self {
            EdgeInsetsGeometry::Absolute(insets) => *insets,
            EdgeInsetsGeometry::Directional(insets) => insets.resolve(is_ltr),
        }
    }

    /// Get the total horizontal insets.
    pub fn horizontal_total(&self) -> f32 {
        match self {
            EdgeInsetsGeometry::Absolute(insets) => insets.horizontal_total().get(),
            EdgeInsetsGeometry::Directional(insets) => insets.horizontal_total(),
        }
    }

    /// Get the total vertical insets.
    pub fn vertical_total(&self) -> f32 {
        match self {
            EdgeInsetsGeometry::Absolute(insets) => insets.vertical_total().get(),
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

#[allow(deprecated)]
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
        use crate::geometry::px;
        EdgeInsetsGeometry::Absolute(Edges::all(px(0.0)))
    }
}
