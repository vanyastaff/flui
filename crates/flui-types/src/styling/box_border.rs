//! Box border types for styling

use crate::geometry::traits::{NumericUnit, Unit};
use crate::styling::BorderSide;

/// A border for a box, with a separate side for each edge.
///
/// Generic over unit type `T` for full type safety. Use `Border<Pixels>` for UI borders.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{Border, BorderSide, BorderStyle, Color};
/// use flui_types::geometry::px;
///
/// // All sides the same
/// let border = Border::all(BorderSide::new(Color::BLACK, px(2.0), BorderStyle::Solid));
///
/// // Symmetric horizontal/vertical
/// let border = Border::symmetric(
///     Some(BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid)),  // top/bottom
///     Some(BorderSide::new(Color::GRAY, px(2.0), BorderStyle::Solid)),   // left/right
/// );
///
/// // Custom per-side
/// let border = Border::new(
///     Some(BorderSide::new(Color::RED, px(2.0), BorderStyle::Solid)),    // top
///     Some(BorderSide::new(Color::BLUE, px(2.0), BorderStyle::Solid)),   // right
///     Some(BorderSide::new(Color::GREEN, px(2.0), BorderStyle::Solid)),  // bottom
///     Some(BorderSide::new(Color::YELLOW, px(2.0), BorderStyle::Solid)), // left
/// );
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Border<T: Unit> {
    /// The top side of this border.
    pub top: Option<BorderSide<T>>,

    /// The right side of this border.
    pub right: Option<BorderSide<T>>,

    /// The bottom side of this border.
    pub bottom: Option<BorderSide<T>>,

    /// The left side of this border.
    pub left: Option<BorderSide<T>>,
}

impl<T: Unit> Border<T> {
    /// Creates a border with all sides having the same border side.
    #[inline]
    pub const fn all(side: BorderSide<T>) -> Self {
        Self {
            top: Some(side),
            right: Some(side),
            bottom: Some(side),
            left: Some(side),
        }
    }

    /// Creates a border with only the specified sides.
    #[inline]
    pub const fn new(
        top: Option<BorderSide<T>>,
        right: Option<BorderSide<T>>,
        bottom: Option<BorderSide<T>>,
        left: Option<BorderSide<T>>,
    ) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Creates a border with symmetric vertical and horizontal sides.
    #[inline]
    pub const fn symmetric(
        vertical: Option<BorderSide<T>>,
        horizontal: Option<BorderSide<T>>,
    ) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Creates a border with no sides.
    #[inline]
    pub const fn none() -> Self {
        Self {
            top: None,
            right: None,
            bottom: None,
            left: None,
        }
    }

    /// Returns true if all sides are None.
    #[inline]
    pub const fn is_none(&self) -> bool {
        self.top.is_none() && self.right.is_none() && self.bottom.is_none() && self.left.is_none()
    }

    /// Returns true if all sides are uniform (same BorderSide).
    #[inline]
    pub fn is_uniform(&self) -> bool {
        match (self.top, self.right, self.bottom, self.left) {
            (Some(t), Some(r), Some(b), Some(l)) => t == r && r == b && b == l,
            (None, None, None, None) => true,
            _ => false,
        }
    }
}

impl<T: Unit> Default for Border<T> {
    #[inline]
    fn default() -> Self {
        Self::none()
    }
}

impl<T: NumericUnit> Border<T>
where
    T: std::ops::Mul<f32, Output = T>,
{
    /// Linearly interpolates between two borders.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            top: match (a.top, b.top) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (None, Some(b_side)) => Some(BorderSide::lerp(BorderSide::none(), b_side, t)),
                (Some(a_side), None) => Some(BorderSide::lerp(a_side, BorderSide::none(), t)),
                (None, None) => None,
            },
            right: match (a.right, b.right) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (None, Some(b_side)) => Some(BorderSide::lerp(BorderSide::none(), b_side, t)),
                (Some(a_side), None) => Some(BorderSide::lerp(a_side, BorderSide::none(), t)),
                (None, None) => None,
            },
            bottom: match (a.bottom, b.bottom) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (None, Some(b_side)) => Some(BorderSide::lerp(BorderSide::none(), b_side, t)),
                (Some(a_side), None) => Some(BorderSide::lerp(a_side, BorderSide::none(), t)),
                (None, None) => None,
            },
            left: match (a.left, b.left) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (None, Some(b_side)) => Some(BorderSide::lerp(BorderSide::none(), b_side, t)),
                (Some(a_side), None) => Some(BorderSide::lerp(a_side, BorderSide::none(), t)),
                (None, None) => None,
            },
        }
    }
}

/// A border for a box, with directional sides that adapt to text direction.
///
/// Generic over unit type `T` for full type safety.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{BorderDirectional, BorderSide, BorderStyle, Color};
/// use flui_types::geometry::px;
///
/// let border = BorderDirectional::all(BorderSide::new(Color::BLACK, px(2.0), BorderStyle::Solid));
///
/// // Resolve to physical border based on text direction
/// let ltr_border = border.resolve(true);   // left-to-right
/// let rtl_border = border.resolve(false);  // right-to-left
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderDirectional<T: Unit> {
    /// The start side of this border (left in LTR, right in RTL).
    pub start: Option<BorderSide<T>>,

    /// The top side of this border.
    pub top: Option<BorderSide<T>>,

    /// The end side of this border (right in LTR, left in RTL).
    pub end: Option<BorderSide<T>>,

    /// The bottom side of this border.
    pub bottom: Option<BorderSide<T>>,
}

impl<T: Unit> BorderDirectional<T> {
    /// Creates a directional border with all sides having the same border side.
    #[inline]
    pub const fn all(side: BorderSide<T>) -> Self {
        Self {
            start: Some(side),
            top: Some(side),
            end: Some(side),
            bottom: Some(side),
        }
    }

    /// Creates a directional border with only the specified sides.
    #[inline]
    pub const fn new(
        start: Option<BorderSide<T>>,
        top: Option<BorderSide<T>>,
        end: Option<BorderSide<T>>,
        bottom: Option<BorderSide<T>>,
    ) -> Self {
        Self {
            start,
            top,
            end,
            bottom,
        }
    }

    /// Creates a border with no sides.
    #[inline]
    pub const fn none() -> Self {
        Self {
            start: None,
            top: None,
            end: None,
            bottom: None,
        }
    }

    /// Resolves this directional border to a regular border based on text direction.
    ///
    /// # Arguments
    ///
    /// * `ltr` - If true, uses left-to-right layout. If false, uses right-to-left.
    #[inline]
    pub const fn resolve(self, ltr: bool) -> Border<T> {
        if ltr {
            Border {
                top: self.top,
                right: self.end,
                bottom: self.bottom,
                left: self.start,
            }
        } else {
            Border {
                top: self.top,
                right: self.start,
                bottom: self.bottom,
                left: self.end,
            }
        }
    }
}

impl<T: Unit> Default for BorderDirectional<T> {
    #[inline]
    fn default() -> Self {
        Self::none()
    }
}

/// Common trait for border types.
pub trait BoxBorder<T: Unit> {
    /// Returns true if this border is uniform (all sides the same).
    fn is_uniform(&self) -> bool;
}

impl<T: Unit> BoxBorder<T> for Border<T> {
    #[inline]
    fn is_uniform(&self) -> bool {
        Border::is_uniform(self)
    }
}
