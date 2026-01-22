//! Alignment types for layout widgets
//!
//! This module contains enums and utilities for aligning children
//! within parent containers, similar to Flutter's alignment system.

use std::ops::{Add, Neg};

use crate::{Offset, Size};

/// How much space should be occupied in the main axis.
///
/// Similar to Flutter's `MainAxisSize`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::MainAxisSize;
///
/// let min = MainAxisSize::Min;
/// let max = MainAxisSize::Max;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MainAxisSize {
    /// Minimize the amount of space occupied by the children.
    ///
    /// The widget will be as small as possible while still containing all children.
    Min,

    /// Maximize the amount of space occupied by the children.
    ///
    /// The widget will expand to fill all available space along the main axis.
    #[default]
    Max,
}

impl MainAxisSize {
    /// Check if this is Min.
    pub const fn is_min(self) -> bool {
        matches!(self, MainAxisSize::Min)
    }

    /// Check if this is Max.
    pub const fn is_max(self) -> bool {
        matches!(self, MainAxisSize::Max)
    }
}

/// How the children should be placed along the main axis in a flex layout.
///
/// This is similar to CSS `justify-content` property and Flutter's `MainAxisAlignment`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::MainAxisAlignment;
///
/// // Children at the start
/// let start = MainAxisAlignment::Start;
///
/// // Equal space between children
/// let space_between = MainAxisAlignment::SpaceBetween;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MainAxisAlignment {
    /// Place children at the start of the main axis.
    ///
    /// For Row: left side (in LTR)
    /// For Column: top side
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
    pub const fn requires_custom_spacing(self) -> bool {
        matches!(
            self,
            MainAxisAlignment::SpaceBetween
                | MainAxisAlignment::SpaceAround
                | MainAxisAlignment::SpaceEvenly
        )
    }

    /// Calculate spacing for children given total available space and number of children.
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

/// How the children should be placed along the cross axis in a flex layout.
///
/// This is similar to CSS `align-items` property and Flutter's `CrossAxisAlignment`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::CrossAxisAlignment;
///
/// // Children at the start of cross axis
/// let start = CrossAxisAlignment::Start;
///
/// // Children stretched to fill
/// let stretch = CrossAxisAlignment::Stretch;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CrossAxisAlignment {
    /// Place children at the start of the cross axis.
    ///
    /// For Row: top side
    /// For Column: left side (in LTR)
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
    pub const fn requires_custom_sizing(self) -> bool {
        matches!(
            self,
            CrossAxisAlignment::Stretch | CrossAxisAlignment::Baseline
        )
    }
}

/// How to align a child within its parent container.
///
/// This is similar to Flutter's `Alignment` class.
///
/// The coordinate system:
/// - x: -1.0 = left, 0.0 = center, 1.0 = right
/// - y: -1.0 = top, 0.0 = center, 1.0 = bottom
///
/// # Examples
///
/// ```
/// use flui_types::Alignment;
///
/// let top_left = Alignment::TOP_LEFT;
/// let center = Alignment::CENTER;
/// let custom = Alignment::new(0.5, -0.5); // Between center and top-right
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
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

    /// Calculate the offset for a child of given size within a parent of given size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Alignment, Size};
    ///
    /// let alignment = Alignment::CENTER;
    /// let child_size = Size::new(50.0, 30.0);
    /// let parent_size = Size::new(100.0, 60.0);
    ///
    /// let offset = alignment.calculate_offset(child_size, parent_size);
    /// assert_eq!(offset.dx, 25.0); // (100 - 50) / 2
    /// assert_eq!(offset.dy, 15.0); // (60 - 30) / 2
    /// ```
    #[inline]
    #[must_use]
    pub fn calculate_offset(self, child_size: Size<f32>, parent_size: Size<f32>) -> Offset<f32> {
        let available_space = Size::new(
            parent_size.width - child_size.width,
            parent_size.height - child_size.height,
        );

        Offset::new(
            available_space.width * (self.x + 1.0) / 2.0,
            available_space.height * (self.y + 1.0) / 2.0,
        )
    }

    /// Returns the offset that is this fraction in the direction of the given offset.
    ///
    /// This is the Flutter `alongOffset` method. It takes the available space as
    /// an Offset (parent_size - child_size) and returns the position offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Alignment, Offset};
    ///
    /// let alignment = Alignment::CENTER;
    /// // Available space: 50x30 (difference between parent and child)
    /// let available = Offset::new(50.0, 30.0);
    /// let offset = alignment.along_offset(available);
    /// assert_eq!(offset.dx, 25.0); // 50 / 2
    /// assert_eq!(offset.dy, 15.0); // 30 / 2
    ///
    /// let top_left = Alignment::TOP_LEFT;
    /// let offset = top_left.along_offset(available);
    /// assert_eq!(offset.dx, 0.0);
    /// assert_eq!(offset.dy, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn along_offset(self, other: Offset<f32>) -> Offset<f32> {
        let center_x = other.dx / 2.0;
        let center_y = other.dy / 2.0;
        Offset::new(center_x + self.x * center_x, center_y + self.y * center_y)
    }

    /// Returns the offset that is this fraction within the given size.
    ///
    /// This is the Flutter `alongSize` method.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Alignment, Size};
    ///
    /// let alignment = Alignment::CENTER;
    /// let size = Size::new(100.0, 60.0);
    /// let offset = alignment.along_size(size);
    /// assert_eq!(offset.dx, 50.0); // 100 / 2
    /// assert_eq!(offset.dy, 30.0); // 60 / 2
    /// ```
    #[inline]
    #[must_use]
    pub fn along_size(self, other: Size<f32>) -> Offset<f32> {
        let center_x = other.width / 2.0;
        let center_y = other.height / 2.0;
        Offset::new(center_x + self.x * center_x, center_y + self.y * center_y)
    }

    /// Linear interpolation between two alignments.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Alignment;
    ///
    /// let a = Alignment::TOP_LEFT;
    /// let b = Alignment::BOTTOM_RIGHT;
    /// let mid = Alignment::lerp(a, b, 0.5);
    ///
    /// assert_eq!(mid, Alignment::CENTER);
    /// ```
    #[inline]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
    }
}

impl Default for Alignment {
    fn default() -> Self {
        Self::CENTER
    }
}

impl From<(f32, f32)> for Alignment {
    fn from((x, y): (f32, f32)) -> Self {
        Alignment::new(x, y)
    }
}

impl Add for Alignment {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Neg for Alignment {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_main_axis_size() {
        assert!(MainAxisSize::Min.is_min());
        assert!(!MainAxisSize::Min.is_max());
        assert!(MainAxisSize::Max.is_max());
        assert!(!MainAxisSize::Max.is_min());
    }

    #[test]
    fn test_main_axis_alignment_custom_spacing() {
        assert!(!MainAxisAlignment::Start.requires_custom_spacing());
        assert!(!MainAxisAlignment::End.requires_custom_spacing());
        assert!(!MainAxisAlignment::Center.requires_custom_spacing());
        assert!(MainAxisAlignment::SpaceBetween.requires_custom_spacing());
        assert!(MainAxisAlignment::SpaceAround.requires_custom_spacing());
        assert!(MainAxisAlignment::SpaceEvenly.requires_custom_spacing());
    }

    #[test]
    fn test_main_axis_alignment_spacing_calculation() {
        // SpaceBetween with 3 children, 100px available
        let (leading, between) = MainAxisAlignment::SpaceBetween.calculate_spacing(100.0, 3);
        assert_eq!(leading, 0.0);
        assert_eq!(between, 50.0); // 100 / 2 gaps

        // SpaceAround with 3 children, 90px available
        let (leading, between) = MainAxisAlignment::SpaceAround.calculate_spacing(90.0, 3);
        assert_eq!(leading, 15.0); // 30 / 2
        assert_eq!(between, 30.0); // 90 / 3

        // SpaceEvenly with 3 children, 100px available
        let (leading, between) = MainAxisAlignment::SpaceEvenly.calculate_spacing(100.0, 3);
        assert_eq!(leading, 25.0); // 100 / 4
        assert_eq!(between, 25.0); // 100 / 4

        // Single child with SpaceBetween behaves like Start
        let (leading, between) = MainAxisAlignment::SpaceBetween.calculate_spacing(100.0, 1);
        assert_eq!(leading, 0.0);
        assert_eq!(between, 0.0);
    }

    #[test]
    fn test_cross_axis_alignment_custom_sizing() {
        assert!(!CrossAxisAlignment::Start.requires_custom_sizing());
        assert!(!CrossAxisAlignment::End.requires_custom_sizing());
        assert!(!CrossAxisAlignment::Center.requires_custom_sizing());
        assert!(CrossAxisAlignment::Stretch.requires_custom_sizing());
        assert!(CrossAxisAlignment::Baseline.requires_custom_sizing());
    }

    #[test]
    fn test_alignment_constants() {
        assert_eq!(Alignment::TOP_LEFT.x, -1.0);
        assert_eq!(Alignment::TOP_LEFT.y, -1.0);

        assert_eq!(Alignment::CENTER.x, 0.0);
        assert_eq!(Alignment::CENTER.y, 0.0);

        assert_eq!(Alignment::BOTTOM_RIGHT.x, 1.0);
        assert_eq!(Alignment::BOTTOM_RIGHT.y, 1.0);
    }

    #[test]
    fn test_alignment_calculate_offset() {
        let alignment = Alignment::CENTER;
        let child_size = Size::new(50.0, 30.0);
        let parent_size = Size::new(100.0, 60.0);

        let offset = alignment.calculate_offset(child_size, parent_size);
        assert_eq!(offset.dx, 25.0); // (100 - 50) / 2
        assert_eq!(offset.dy, 15.0); // (60 - 30) / 2

        let top_left = Alignment::TOP_LEFT;
        let offset = top_left.calculate_offset(child_size, parent_size);
        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 0.0);

        let bottom_right = Alignment::BOTTOM_RIGHT;
        let offset = bottom_right.calculate_offset(child_size, parent_size);
        assert_eq!(offset.dx, 50.0); // 100 - 50
        assert_eq!(offset.dy, 30.0); // 60 - 30
    }

    #[test]
    fn test_alignment_lerp() {
        let a = Alignment::TOP_LEFT;
        let b = Alignment::BOTTOM_RIGHT;

        let start = Alignment::lerp(a, b, 0.0);
        assert_eq!(start, a);

        let end = Alignment::lerp(a, b, 1.0);
        assert_eq!(end, b);

        let mid = Alignment::lerp(a, b, 0.5);
        assert_eq!(mid, Alignment::CENTER);
    }

    #[test]
    fn test_alignment_add() {
        let a = Alignment::new(0.5, 0.0);
        let b = Alignment::new(0.0, 0.5);
        let combined = a + b;

        assert_eq!(combined.x, 0.5);
        assert_eq!(combined.y, 0.5);
    }

    #[test]
    fn test_alignment_negate() {
        let top_left = Alignment::TOP_LEFT;
        let bottom_right = -top_left;

        assert_eq!(bottom_right, Alignment::BOTTOM_RIGHT);

        let custom = Alignment::new(0.5, -0.5);
        let negated = -custom;
        assert_eq!(negated.x, -0.5);
        assert_eq!(negated.y, 0.5);
    }

    #[test]
    fn test_alignment_from_tuple() {
        let alignment: Alignment = (0.5, -0.5).into();
        assert_eq!(alignment.x, 0.5);
        assert_eq!(alignment.y, -0.5);
    }

    #[test]
    fn test_alignment_default() {
        let default = Alignment::default();
        assert_eq!(default, Alignment::CENTER);
    }
}

/// Directional alignment that adapts to text direction (LTR vs RTL).
///
/// Instead of using absolute x coordinate, uses `start` which maps to:
/// - left in LTR (Left-To-Right) layouts
/// - right in RTL (Right-To-Left) layouts
///
/// Similar to Flutter's `AlignmentDirectional`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlignmentDirectional {
    /// Start alignment: -1.0 = start edge, 0.0 = center, 1.0 = end edge
    pub start: f32,
    /// Vertical alignment: -1.0 = top, 0.0 = center, 1.0 = bottom
    pub y: f32,
}

impl AlignmentDirectional {
    /// Create a new directional alignment.
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
    pub fn resolve(&self, is_ltr: bool) -> Alignment {
        if is_ltr {
            Alignment::new(self.start, self.y)
        } else {
            Alignment::new(-self.start, self.y)
        }
    }

    /// Linear interpolation between two directional alignments.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self::new(a.start + (b.start - a.start) * t, a.y + (b.y - a.y) * t)
    }
}

impl Default for AlignmentDirectional {
    fn default() -> Self {
        Self::CENTER
    }
}

impl Add for AlignmentDirectional {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.start + rhs.start, self.y + rhs.y)
    }
}

impl Neg for AlignmentDirectional {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.start, -self.y)
    }
}

/// Base class for Alignment and AlignmentDirectional.
///
/// This enum allows working with both absolute and directional alignments uniformly.
/// Similar to Flutter's `AlignmentGeometry`.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub fn resolve(&self, is_ltr: bool) -> Alignment {
        match self {
            AlignmentGeometry::Absolute(alignment) => *alignment,
            AlignmentGeometry::Directional(alignment) => alignment.resolve(is_ltr),
        }
    }

    /// Calculate offset for a child within a parent, respecting text direction.
    pub fn calculate_offset(&self, child_size: Size<f32>, parent_size: Size<f32>, is_ltr: bool) -> Offset<f32> {
        self.resolve(is_ltr)
            .calculate_offset(child_size, parent_size)
    }
}

impl From<Alignment> for AlignmentGeometry {
    fn from(alignment: Alignment) -> Self {
        AlignmentGeometry::Absolute(alignment)
    }
}

impl From<AlignmentDirectional> for AlignmentGeometry {
    fn from(alignment: AlignmentDirectional) -> Self {
        AlignmentGeometry::Directional(alignment)
    }
}

impl Default for AlignmentGeometry {
    fn default() -> Self {
        AlignmentGeometry::Absolute(Alignment::CENTER)
    }
}
