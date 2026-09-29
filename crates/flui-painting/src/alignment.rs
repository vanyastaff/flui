//! Alignment types for layout widgets
//!
//! This module contains enums and utilities for aligning children
//! within parent containers, similar to Flutter's alignment system.

use std::ops::{Add, Neg};

use flui_foundation::geometry::{Offset, Rect, Size};

/// A point within a rectangle, in normalized coordinates.
///
/// Mirrors Flutter's `Alignment`: both axes run from -1.0 to 1.0,
/// where (-1, -1) is the top-left corner, (0, 0) the center, and
/// (1, 1) the bottom-right corner. Values outside that range place
/// the point outside the rectangle. For text-direction-aware
/// alignment, use `AlignmentDirectional`.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Alignment {
    /// Horizontal alignment: -1.0 = left, 0.0 = center, 1.0 = right
    pub x: f64,

    /// Vertical alignment: -1.0 = top, 0.0 = center, 1.0 = bottom
    pub y: f64,
}

impl Alignment {
    /// Create a new alignment with the given x and y values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::Alignment;
    ///
    /// let alignment = Alignment::new(0.5, -0.5);
    /// assert_eq!(alignment.x, 0.5);
    /// assert_eq!(alignment.y, -0.5);
    /// ```
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Top left alignment (-1, -1).
    pub const TOP_LEFT: Self = Self::new(-1.0, -1.0);

    /// Top center alignment (0, -1).
    pub const TOP_CENTER: Self = Self::new(0.0, -1.0);

    /// Top right alignment (1, -1).
    pub const TOP_RIGHT: Self = Self::new(1.0, -1.0);

    /// Center left alignment (-1, 0).
    pub const CENTER_LEFT: Self = Self::new(-1.0, 0.0);

    /// Center alignment (0, 0).
    pub const CENTER: Self = Self::new(0.0, 0.0);

    /// Center right alignment (1, 0).
    pub const CENTER_RIGHT: Self = Self::new(1.0, 0.0);

    /// Bottom left alignment (-1, 1).
    pub const BOTTOM_LEFT: Self = Self::new(-1.0, 1.0);

    /// Bottom center alignment (0, 1).
    pub const BOTTOM_CENTER: Self = Self::new(0.0, 1.0);

    /// Bottom right alignment (1, 1).
    pub const BOTTOM_RIGHT: Self = Self::new(1.0, 1.0);

    /// Linearly interpolates between two alignments.
    ///
    /// `t == 0.0` returns `a`; `t == 1.0` returns `b`. Values of `t` outside
    /// `[0, 1]` extrapolate — they are **not** clamped. This matches Flutter's
    /// `Alignment.lerp` contract and lets overshoot animation curves
    /// (elastic, back) propagate through `Tween<Alignment>` without flattening.
    #[must_use]
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        Self::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
    }

    /// Returns the offset into `free_space` where this alignment places its origin.
    ///
    /// `free_space` is the gap between the parent and child (`parent_size − child_size`).
    /// The result is the child's top-left offset within the parent, in logical pixels.
    ///
    /// Mirrors Flutter `Alignment.alongSize`: `Offset(w/2 + x*w/2, h/2 + y*h/2)`.
    ///
    /// The companion methods `inscribe(Size, Rect)` (Flutter `Alignment.inscribe`) and
    /// `along_offset(Offset)` (Flutter `Alignment.alongOffset`) are intentionally deferred
    /// to a later phase — they serve `FittedBox` and direct `Offset`-input consumers
    /// respectively.  Their absence here is deliberate, not an oversight.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::Alignment;
    /// use flui_foundation::geometry::{Offset, Size};
    ///
    /// // Center: free space 100×50 → offset (50, 25)
    /// let offset = Alignment::CENTER.along_size(Size::new(100.0, 50.0));
    /// assert_eq!(offset, Offset::new(50.0, 25.0));
    /// ```
    #[must_use]
    #[inline]
    pub fn along_size(self, free_space: Size<f64>) -> Offset<f64> {
        Offset::new(
            free_space.width * f64::midpoint(1.0, self.x),
            free_space.height * f64::midpoint(1.0, self.y),
        )
    }

    /// Maps this normalized alignment to a pixel-coordinate `Offset` inside
    /// `rect`.
    ///
    /// The result is an absolute position within `rect`:
    /// - `Alignment::TOP_LEFT.align_within(rect)` returns the top-left corner.
    /// - `Alignment::CENTER.align_within(rect)` returns the center point.
    /// - `Alignment::BOTTOM_RIGHT.align_within(rect)` returns the bottom-right corner.
    ///
    /// Values outside `[-1, 1]` place the point outside `rect`, which is legal
    /// and useful for follower-layer off-rectangle anchors.
    ///
    /// Unlike [`along_size`](Self::along_size), which requires the caller to
    /// pre-compute `parent_size − child_size`, this method works directly on a
    /// positioned `Rect` and accounts for its origin offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::Alignment;
    /// use flui_foundation::geometry::Rect;
    ///
    /// let rect = flui_foundation::geometry::Rect::from_ltwh(10.0, 20.0, 100.0, 200.0);
    /// // Center of a 100×200 rect anchored at (10, 20) is (60, 120).
    /// let center = Alignment::CENTER.align_within(rect);
    /// assert_eq!(center.dx, 60.0);
    /// assert_eq!(center.dy, 120.0);
    /// ```
    #[must_use]
    #[inline]
    pub fn align_within(self, rect: Rect<f64>) -> Offset<f64> {
        let half_width = rect.width() * 0.5;
        let half_height = rect.height() * 0.5;
        let center_x = rect.left() + half_width;
        let center_y = rect.top() + half_height;
        Offset::new(
            center_x + half_width * self.x,
            center_y + half_height * self.y,
        )
    }
}

impl Default for Alignment {
    #[inline]
    fn default() -> Self {
        Self::CENTER
    }
}

impl From<(f64, f64)> for Alignment {
    #[inline]
    fn from((x, y): (f64, f64)) -> Self {
        Alignment::new(x, y)
    }
}

impl Add for Alignment {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Neg for Alignment {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y)
    }
}

/// An `Alignment` whose horizontal component depends on text direction.
///
/// Mirrors Flutter's `AlignmentDirectional`: `start` is the reading
/// edge (left in LTR, right in RTL) and must be resolved with
/// [`resolve`](Self::resolve) before use; the vertical axis matches
/// `Alignment`.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlignmentDirectional {
    /// Start alignment: -1.0 = start edge, 0.0 = center, 1.0 = end edge
    pub start: f64,
    /// Vertical alignment: -1.0 = top, 0.0 = center, 1.0 = bottom
    pub y: f64,
}

impl AlignmentDirectional {
    /// Create a new directional alignment.
    #[inline]
    pub const fn new(start: f64, y: f64) -> Self {
        Self { start, y }
    }

    /// Top start alignment (-1, -1).
    pub const TOP_START: Self = Self::new(-1.0, -1.0);

    /// Top center alignment (0, -1).
    pub const TOP_CENTER: Self = Self::new(0.0, -1.0);

    /// Top end alignment (1, -1).
    pub const TOP_END: Self = Self::new(1.0, -1.0);

    /// Center start alignment (-1, 0).
    pub const CENTER_START: Self = Self::new(-1.0, 0.0);

    /// Center alignment (0, 0).
    pub const CENTER: Self = Self::new(0.0, 0.0);

    /// Center end alignment (1, 0).
    pub const CENTER_END: Self = Self::new(1.0, 0.0);

    /// Bottom start alignment (-1, 1).
    pub const BOTTOM_START: Self = Self::new(-1.0, 1.0);

    /// Bottom center alignment (0, 1).
    pub const BOTTOM_CENTER: Self = Self::new(0.0, 1.0);

    /// Bottom end alignment (1, 1).
    pub const BOTTOM_END: Self = Self::new(1.0, 1.0);

    /// Resolve to absolute Alignment based on text direction.
    ///
    /// # Arguments
    ///
    /// * `is_ltr` - true for left-to-right, false for right-to-left
    #[inline]
    pub fn resolve(&self, is_ltr: bool) -> Alignment {
        if is_ltr {
            Alignment::new(self.start, self.y)
        } else {
            Alignment::new(-self.start, self.y)
        }
    }

    /// Linear interpolation between two directional alignments.
    ///
    /// Values of `t` outside `[0, 1]` extrapolate — they are **not** clamped,
    /// matching [`Alignment::lerp`] and Flutter's `AlignmentDirectional.lerp`
    /// so overshoot animation curves propagate without flattening.
    #[must_use]
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        Self::new(a.start + (b.start - a.start) * t, a.y + (b.y - a.y) * t)
    }
}

impl Default for AlignmentDirectional {
    #[inline]
    fn default() -> Self {
        Self::CENTER
    }
}

impl Add for AlignmentDirectional {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.start + rhs.start, self.y + rhs.y)
    }
}

impl Neg for AlignmentDirectional {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self::new(-self.start, -self.y)
    }
}

/// Either an absolute or a text-direction-relative alignment.
///
/// Mirrors Flutter's `AlignmentGeometry` base class as a Rust enum;
/// call [`resolve`](Self::resolve) to obtain an absolute `Alignment`.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AlignmentGeometry {
    /// Absolute alignment (x, y).
    Absolute(Alignment),
    /// Directional alignment (start, y).
    Directional(AlignmentDirectional),
}

impl AlignmentGeometry {
    /// Resolve to absolute Alignment based on text direction.
    ///
    /// # Arguments
    ///
    /// * `is_ltr` - true for left-to-right, false for right-to-left
    #[inline]
    pub fn resolve(&self, is_ltr: bool) -> Alignment {
        match self {
            AlignmentGeometry::Absolute(alignment) => *alignment,
            AlignmentGeometry::Directional(alignment) => alignment.resolve(is_ltr),
        }
    }
}

impl From<Alignment> for AlignmentGeometry {
    #[inline]
    fn from(alignment: Alignment) -> Self {
        AlignmentGeometry::Absolute(alignment)
    }
}

impl From<AlignmentDirectional> for AlignmentGeometry {
    #[inline]
    fn from(alignment: AlignmentDirectional) -> Self {
        AlignmentGeometry::Directional(alignment)
    }
}

impl Default for AlignmentGeometry {
    #[inline]
    fn default() -> Self {
        AlignmentGeometry::Absolute(Alignment::CENTER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- along_size tests (pre-existing) ----

    // ---- lerp tests (migrated from painting::alignment) ----

    #[test]
    fn lerp_midpoint_is_center() {
        let mid = Alignment::lerp(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT, 0.5);
        assert_eq!(mid, Alignment::CENTER);
    }

    // ---- align_within tests (migrated from painting::alignment + new) ----

    #[test]
    fn align_within_handles_offset_rect() {
        // 200×100 rect anchored at (10, 20).
        let r = Rect::from_ltwh(10.0, 20.0, 200.0, 100.0);
        assert_eq!(Alignment::TOP_LEFT.align_within(r).dx, 10.0);
        assert_eq!(Alignment::TOP_LEFT.align_within(r).dy, 20.0);
        assert_eq!(Alignment::CENTER.align_within(r).dx, 110.0);
        assert_eq!(Alignment::CENTER.align_within(r).dy, 70.0);
    }

    // ---- canonical constants (migrated from painting::alignment) ----
}
