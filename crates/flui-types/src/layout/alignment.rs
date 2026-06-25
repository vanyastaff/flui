//! Alignment types for layout widgets
//!
//! This module contains enums and utilities for aligning children
//! within parent containers, similar to Flutter's alignment system.

use std::ops::{Add, Neg};

use crate::geometry::{Offset, Pixels, Rect, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MainAxisSize {
    /// Minimize the amount of space occupied by the children.
    ///
    /// The widget will be as small as possible while still containing all
    /// children.
    Min,

    #[default]
    Max,
}

impl MainAxisSize {
    /// Check if this is Min.
    #[inline]
    pub const fn is_min(self) -> bool {
        matches!(self, MainAxisSize::Min)
    }

    /// Check if this is Max.
    #[inline]
    pub const fn is_max(self) -> bool {
        matches!(self, MainAxisSize::Max)
    }
}

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MainAxisAlignment {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    #[default]
    Start,

    /// Place children at the end of the main axis.
    ///
    /// For Row: right side (in LTR)
    /// For Column: bottom side
    End,

    /// Place children in the center of the main axis.
    Center,

    /// Place children with equal space between them.
    ///
    /// The first child is at the start, the last child is at the end,
    /// and the remaining space is distributed evenly between children.
    ///
    /// If there's only one child, it behaves like Start.
    SpaceBetween,

    /// Place children with equal space around them.
    ///
    /// Each child has equal space on both sides, resulting in
    /// half-sized space at the start and end.
    SpaceAround,

    /// Place children with equal space around them, including start and end.
    ///
    /// All children have equal space between them and at the start/end.
    SpaceEvenly,
}

impl MainAxisAlignment {
    /// Check if this alignment requires custom spacing logic.
    ///
    /// Returns true for SpaceBetween, SpaceAround, and SpaceEvenly.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::MainAxisAlignment;
    ///
    /// assert!(!MainAxisAlignment::Start.requires_custom_spacing());
    /// assert!(MainAxisAlignment::SpaceBetween.requires_custom_spacing());
    /// ```
    #[inline]
    pub const fn requires_custom_spacing(self) -> bool {
        matches!(
            self,
            MainAxisAlignment::SpaceBetween
                | MainAxisAlignment::SpaceAround
                | MainAxisAlignment::SpaceEvenly
        )
    }

    /// Calculate spacing for children given total available space and number of
    /// children.
    ///
    /// Returns (leading_space, spacing_between).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::MainAxisAlignment;
    ///
    /// // 100px available, 3 children
    /// let (leading, between) = MainAxisAlignment::SpaceBetween.calculate_spacing(100.0, 3);
    /// assert_eq!(leading, 0.0);
    /// assert_eq!(between, 50.0); // 100 / 2 gaps
    /// ```
    #[inline]
    pub fn calculate_spacing(self, available_space: f32, child_count: usize) -> (f32, f32) {
        if child_count == 0 {
            return (0.0, 0.0);
        }

        match self {
            MainAxisAlignment::Start | MainAxisAlignment::End | MainAxisAlignment::Center => {
                (0.0, 0.0)
            }
            MainAxisAlignment::SpaceBetween => {
                if child_count == 1 {
                    (0.0, 0.0)
                } else {
                    let spacing = available_space / (child_count - 1) as f32;
                    (0.0, spacing)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let spacing = available_space / child_count as f32;
                (spacing / 2.0, spacing)
            }
            MainAxisAlignment::SpaceEvenly => {
                let spacing = available_space / (child_count + 1) as f32;
                (spacing, spacing)
            }
        }
    }
}

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CrossAxisAlignment {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    #[default]
    Start,

    /// Place children at the end of the cross axis.
    ///
    /// For Row: bottom side
    /// For Column: right side (in LTR)
    End,

    /// Place children in the center of the cross axis.
    Center,

    /// Stretch children to fill the cross axis.
    ///
    /// Children's cross-axis size will be set to the maximum.
    Stretch,

    /// Place children along the cross axis such that their baselines match.
    ///
    /// This only applies to text and requires baseline information.
    /// Falls back to Start if baseline is not available.
    Baseline,
}

impl CrossAxisAlignment {
    /// Check if this alignment requires custom sizing logic.
    ///
    /// Returns true for Stretch and Baseline.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::CrossAxisAlignment;
    ///
    /// assert!(!CrossAxisAlignment::Start.requires_custom_sizing());
    /// assert!(CrossAxisAlignment::Stretch.requires_custom_sizing());
    /// ```
    #[inline]
    pub const fn requires_custom_sizing(self) -> bool {
        matches!(
            self,
            CrossAxisAlignment::Stretch | CrossAxisAlignment::Baseline
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Alignment {
    /// Horizontal alignment: -1.0 = left, 0.0 = center, 1.0 = right
    pub x: f32,

    /// Vertical alignment: -1.0 = top, 0.0 = center, 1.0 = bottom
    pub y: f32,
}

impl Alignment {
    /// Create a new alignment with the given x and y values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Alignment;
    ///
    /// let alignment = Alignment::new(0.5, -0.5);
    /// assert_eq!(alignment.x, 0.5);
    /// assert_eq!(alignment.y, -0.5);
    /// ```
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
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
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
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
    /// use flui_types::{Alignment, Offset, Size, geometry::px};
    ///
    /// // Center: free space 100×50 → offset (50, 25)
    /// let offset = Alignment::CENTER.along_size(Size::new(px(100.0), px(50.0)));
    /// assert_eq!(offset, Offset::new(px(50.0), px(25.0)));
    /// ```
    #[must_use]
    #[inline]
    pub fn along_size(self, free_space: Size<Pixels>) -> Offset<Pixels> {
        Offset::new(
            free_space.width * (0.5 * (1.0 + self.x)),
            free_space.height * (0.5 * (1.0 + self.y)),
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
    /// use flui_types::{Alignment, Rect, geometry::px};
    ///
    /// let rect = flui_types::Rect::from_ltwh(px(10.0), px(20.0), px(100.0), px(200.0));
    /// // Center of a 100×200 rect anchored at (10, 20) is (60, 120).
    /// let center = Alignment::CENTER.align_within(rect);
    /// assert_eq!(center.dx, px(60.0));
    /// assert_eq!(center.dy, px(120.0));
    /// ```
    #[must_use]
    #[inline]
    pub fn align_within(self, rect: Rect<Pixels>) -> Offset<Pixels> {
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

impl From<(f32, f32)> for Alignment {
    #[inline]
    fn from((x, y): (f32, f32)) -> Self {
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

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlignmentDirectional {
    /// Start alignment: -1.0 = start edge, 0.0 = center, 1.0 = end edge
    pub start: f32,
    /// Vertical alignment: -1.0 = top, 0.0 = center, 1.0 = bottom
    pub y: f32,
}

impl AlignmentDirectional {
    /// Create a new directional alignment.
    #[inline]
    pub const fn new(start: f32, y: f32) -> Self {
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
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
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
    use crate::geometry::px;

    // ---- along_size tests (pre-existing) ----

    #[test]
    fn along_size_center_returns_half_free_space() {
        let offset = Alignment::CENTER.along_size(Size::new(px(100.0), px(50.0)));
        assert_eq!(offset, Offset::new(px(50.0), px(25.0)));
    }

    #[test]
    fn along_size_top_left_returns_zero_offset() {
        let offset = Alignment::TOP_LEFT.along_size(Size::new(px(100.0), px(50.0)));
        assert_eq!(offset, Offset::new(px(0.0), px(0.0)));
    }

    #[test]
    fn along_size_bottom_right_returns_full_free_space() {
        let offset = Alignment::BOTTOM_RIGHT.along_size(Size::new(px(100.0), px(50.0)));
        assert_eq!(offset, Offset::new(px(100.0), px(50.0)));
    }

    // ---- lerp tests (migrated from painting::alignment) ----

    #[test]
    fn lerp_endpoints_are_exact() {
        let a = Alignment::TOP_LEFT;
        let b = Alignment::BOTTOM_RIGHT;
        assert_eq!(Alignment::lerp(a, b, 0.0), a);
        assert_eq!(Alignment::lerp(a, b, 1.0), b);
    }

    #[test]
    fn lerp_midpoint_is_center() {
        let mid = Alignment::lerp(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT, 0.5);
        assert_eq!(mid, Alignment::CENTER);
    }

    /// Red→green anchor for the lerp fix: the old `lerp` clamped `t` to `[0, 1]`,
    /// which pinned `t = 1.5` to `1.0` and produced `(1.0, 1.0)`.
    /// Without the clamp, extrapolation yields `(2.0, 2.0)`.
    #[test]
    fn lerp_extrapolates_outside_unit_interval() {
        let out = Alignment::lerp(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT, 1.5);
        assert_eq!(out, Alignment::new(2.0, 2.0));
    }

    /// `AlignmentDirectional::lerp` mirrors the no-clamp contract: the old clamp
    /// pinned `t = 1.5` to `1.0` and produced `(1.0, 1.0)`; without it,
    /// `TOP_START..BOTTOM_END` at `t = 1.5` extrapolates to `(2.0, 2.0)`.
    #[test]
    fn directional_lerp_extrapolates_outside_unit_interval() {
        let out = AlignmentDirectional::lerp(
            AlignmentDirectional::TOP_START,
            AlignmentDirectional::BOTTOM_END,
            1.5,
        );
        assert_eq!(out, AlignmentDirectional::new(2.0, 2.0));
    }

    // ---- align_within tests (migrated from painting::alignment + new) ----

    #[test]
    fn align_within_corner_constants_match_rect_corners() {
        let r = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(200.0));
        assert_eq!(Alignment::TOP_LEFT.align_within(r).dx, px(0.0));
        assert_eq!(Alignment::TOP_LEFT.align_within(r).dy, px(0.0));
        assert_eq!(Alignment::BOTTOM_RIGHT.align_within(r).dx, px(100.0));
        assert_eq!(Alignment::BOTTOM_RIGHT.align_within(r).dy, px(200.0));
        assert_eq!(Alignment::CENTER.align_within(r).dx, px(50.0));
        assert_eq!(Alignment::CENTER.align_within(r).dy, px(100.0));
    }

    #[test]
    fn align_within_handles_offset_rect() {
        // 200×100 rect anchored at (10, 20).
        let r = Rect::from_ltwh(px(10.0), px(20.0), px(200.0), px(100.0));
        assert_eq!(Alignment::TOP_LEFT.align_within(r).dx, px(10.0));
        assert_eq!(Alignment::TOP_LEFT.align_within(r).dy, px(20.0));
        assert_eq!(Alignment::CENTER.align_within(r).dx, px(110.0));
        assert_eq!(Alignment::CENTER.align_within(r).dy, px(70.0));
    }

    #[test]
    fn align_within_accepts_values_outside_unit_range() {
        // Off-rectangle anchor — a follower-layer use case.
        // x=2.0 on a 100-wide rect: cx=50, result = 50 + 50*2.0 = 150.
        let r = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
        let a = Alignment::new(2.0, 0.0);
        assert_eq!(a.align_within(r).dx, px(150.0));
    }

    #[test]
    fn align_within_edge_midpoint_constants_lie_on_correct_edge() {
        let r = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(200.0));
        // CENTER_LEFT: x=-1 → left edge; y=0 → vertical center.
        assert_eq!(Alignment::CENTER_LEFT.align_within(r).dx, px(0.0));
        assert_eq!(Alignment::CENTER_LEFT.align_within(r).dy, px(100.0));
        // TOP_CENTER: x=0 → horizontal center; y=-1 → top edge.
        assert_eq!(Alignment::TOP_CENTER.align_within(r).dx, px(50.0));
        assert_eq!(Alignment::TOP_CENTER.align_within(r).dy, px(0.0));
    }

    #[test]
    fn alignment_is_copy_and_lightweight() {
        const fn requires_copy<T: Copy>() {}
        requires_copy::<Alignment>();
        assert_eq!(std::mem::size_of::<Alignment>(), 8);
    }

    /// `align_within` on a zero-origin rect of size = `free_space` must return
    /// the same `Offset` as `along_size(free_space)`.
    ///
    /// Invariant: a zero-origin rect with the given dimensions collapses
    /// `align_within` to the same formula as `along_size`.
    #[test]
    fn align_within_and_along_size_agree_on_free_space() {
        let free_space = Size::new(px(120.0), px(80.0));
        let zero_origin_rect =
            Rect::from_ltwh(px(0.0), px(0.0), free_space.width, free_space.height);
        for alignment in [
            Alignment::TOP_LEFT,
            Alignment::TOP_CENTER,
            Alignment::TOP_RIGHT,
            Alignment::CENTER_LEFT,
            Alignment::CENTER,
            Alignment::CENTER_RIGHT,
            Alignment::BOTTOM_LEFT,
            Alignment::BOTTOM_CENTER,
            Alignment::BOTTOM_RIGHT,
        ] {
            let via_along_size = alignment.along_size(free_space);
            let via_align_within = alignment.align_within(zero_origin_rect);
            assert_eq!(
                via_along_size, via_align_within,
                "along_size and align_within disagree for {alignment:?}"
            );
        }
    }

    // ---- canonical constants (migrated from painting::alignment) ----

    #[test]
    fn canonical_constants_match_quadrant() {
        assert_eq!(Alignment::TOP_LEFT, Alignment::new(-1.0, -1.0));
        assert_eq!(Alignment::CENTER, Alignment::new(0.0, 0.0));
        assert_eq!(Alignment::BOTTOM_RIGHT, Alignment::new(1.0, 1.0));
        assert_eq!(Alignment::CENTER_LEFT.x, -1.0);
        assert_eq!(Alignment::TOP_CENTER.y, -1.0);
    }

    #[test]
    fn default_is_center() {
        assert_eq!(Alignment::default(), Alignment::CENTER);
    }
}
