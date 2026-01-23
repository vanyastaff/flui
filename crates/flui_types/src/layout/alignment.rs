//! Alignment types for layout widgets
//!
//! This module contains enums and utilities for aligning children
//! within parent containers, similar to Flutter's alignment system.

use std::ops::{Add, Neg};

use crate::{Offset, Size};

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MainAxisSize {
    /// Minimize the amount of space occupied by the children.
    ///
    /// The widget will be as small as possible while still containing all children.
    Min,

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

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MainAxisAlignment {
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

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CrossAxisAlignment {
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

    #[must_use]

    #[must_use]

    #[must_use]

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
