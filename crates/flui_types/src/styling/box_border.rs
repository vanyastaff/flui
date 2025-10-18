//! Box border types for styling

use crate::styling::BorderSide;

/// A border of a box, comprised of four sides.
///
/// Similar to Flutter's `Border`.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{Border, BorderSide, Color};
///
/// // All sides with same border
/// let border = Border::all(BorderSide::new(Color::BLACK, 1.0, Default::default()));
///
/// // Different borders for each side
/// let border = Border::new(
///     Some(BorderSide::new(Color::RED, 2.0, Default::default())),
///     Some(BorderSide::new(Color::GREEN, 2.0, Default::default())),
///     Some(BorderSide::new(Color::BLUE, 2.0, Default::default())),
///     Some(BorderSide::new(Color::YELLOW, 2.0, Default::default())),
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Border {
    /// The top side of this border.
    pub top: Option<BorderSide>,

    /// The right side of this border.
    pub right: Option<BorderSide>,

    /// The bottom side of this border.
    pub bottom: Option<BorderSide>,

    /// The left side of this border.
    pub left: Option<BorderSide>,
}

impl Border {
    /// Creates a border with all sides having the same border side.
    pub const fn all(side: BorderSide) -> Self {
        Self {
            top: Some(side),
            right: Some(side),
            bottom: Some(side),
            left: Some(side),
        }
    }

    /// Creates a border with only the specified sides.
    pub const fn new(
        top: Option<BorderSide>,
        right: Option<BorderSide>,
        bottom: Option<BorderSide>,
        left: Option<BorderSide>,
    ) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Creates a border with symmetric vertical and horizontal sides.
    pub const fn symmetric(vertical: Option<BorderSide>, horizontal: Option<BorderSide>) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Creates a border with only the top side.
    pub const fn top_only(side: BorderSide) -> Self {
        Self {
            top: Some(side),
            right: None,
            bottom: None,
            left: None,
        }
    }

    /// Creates a border with only the right side.
    pub const fn right_only(side: BorderSide) -> Self {
        Self {
            top: None,
            right: Some(side),
            bottom: None,
            left: None,
        }
    }

    /// Creates a border with only the bottom side.
    pub const fn bottom_only(side: BorderSide) -> Self {
        Self {
            top: None,
            right: None,
            bottom: Some(side),
            left: None,
        }
    }

    /// Creates a border with only the left side.
    pub const fn left_only(side: BorderSide) -> Self {
        Self {
            top: None,
            right: None,
            bottom: None,
            left: Some(side),
        }
    }

    /// A border with no sides.
    pub const NONE: Self = Self {
        top: None,
        right: None,
        bottom: None,
        left: None,
    };

    /// Returns true if all sides of the border are identical and present.
    pub fn is_uniform(&self) -> bool {
        match (self.top, self.right, self.bottom, self.left) {
            (Some(t), Some(r), Some(b), Some(l)) => t == r && r == b && b == l,
            _ => false,
        }
    }

    /// Linearly interpolate between two borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);

        Self {
            top: match (a.top, b.top) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (Some(side), None) | (None, Some(side)) => Some(side.scale(if t < 0.5 { 1.0 - t * 2.0 } else { (t - 0.5) * 2.0 })),
                (None, None) => None,
            },
            right: match (a.right, b.right) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (Some(side), None) | (None, Some(side)) => Some(side.scale(if t < 0.5 { 1.0 - t * 2.0 } else { (t - 0.5) * 2.0 })),
                (None, None) => None,
            },
            bottom: match (a.bottom, b.bottom) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (Some(side), None) | (None, Some(side)) => Some(side.scale(if t < 0.5 { 1.0 - t * 2.0 } else { (t - 0.5) * 2.0 })),
                (None, None) => None,
            },
            left: match (a.left, b.left) {
                (Some(a_side), Some(b_side)) => Some(BorderSide::lerp(a_side, b_side, t)),
                (Some(side), None) | (None, Some(side)) => Some(side.scale(if t < 0.5 { 1.0 - t * 2.0 } else { (t - 0.5) * 2.0 })),
                (None, None) => None,
            },
        }
    }
}

impl Default for Border {
    fn default() -> Self {
        Self::NONE
    }
}

/// A border of a box that is expressed in terms of start and end rather than left and right.
///
/// Similar to Flutter's `BorderDirectional`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderDirectional {
    /// The top side of this border.
    pub top: Option<BorderSide>,

    /// The end side of this border (right in LTR, left in RTL).
    pub end: Option<BorderSide>,

    /// The bottom side of this border.
    pub bottom: Option<BorderSide>,

    /// The start side of this border (left in LTR, right in RTL).
    pub start: Option<BorderSide>,
}

impl BorderDirectional {
    /// Creates a directional border with all sides having the same border side.
    pub const fn all(side: BorderSide) -> Self {
        Self {
            top: Some(side),
            end: Some(side),
            bottom: Some(side),
            start: Some(side),
        }
    }

    /// Creates a directional border with only the specified sides.
    pub const fn new(
        top: Option<BorderSide>,
        end: Option<BorderSide>,
        bottom: Option<BorderSide>,
        start: Option<BorderSide>,
    ) -> Self {
        Self {
            top,
            end,
            bottom,
            start,
        }
    }

    /// A border with no sides.
    pub const NONE: Self = Self {
        top: None,
        end: None,
        bottom: None,
        start: None,
    };

    /// Converts this directional border to a regular border.
    ///
    /// # Arguments
    ///
    /// * `ltr` - If true, uses left-to-right layout. If false, uses right-to-left.
    pub const fn resolve(self, ltr: bool) -> Border {
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

impl Default for BorderDirectional {
    fn default() -> Self {
        Self::NONE
    }
}

/// Base trait for box borders.
///
/// Similar to Flutter's `BoxBorder`.
pub trait BoxBorder {
    /// Returns true if this border is uniform (all sides are identical).
    fn is_uniform(&self) -> bool;

    /// Linearly interpolate between two box borders.
    fn lerp_border(a: &Self, b: &Self, t: f32) -> Self
    where
        Self: Sized;
}

impl BoxBorder for Border {
    fn is_uniform(&self) -> bool {
        self.is_uniform()
    }

    fn lerp_border(a: &Self, b: &Self, t: f32) -> Self {
        Border::lerp(*a, *b, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styling::{BorderStyle, Color};

    fn test_side() -> BorderSide {
        BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid)
    }

    #[test]
    fn test_border_all() {
        let border = Border::all(test_side());
        assert_eq!(border.top, Some(test_side()));
        assert_eq!(border.right, Some(test_side()));
        assert_eq!(border.bottom, Some(test_side()));
        assert_eq!(border.left, Some(test_side()));
    }

    #[test]
    fn test_border_symmetric() {
        let vert = BorderSide::new(Color::RED, 2.0, BorderStyle::Solid);
        let horiz = BorderSide::new(Color::BLUE, 3.0, BorderStyle::Solid);

        let border = Border::symmetric(Some(vert), Some(horiz));
        assert_eq!(border.top, Some(vert));
        assert_eq!(border.bottom, Some(vert));
        assert_eq!(border.left, Some(horiz));
        assert_eq!(border.right, Some(horiz));
    }

    #[test]
    fn test_border_only_methods() {
        let side = test_side();

        let top = Border::top_only(side);
        assert_eq!(top.top, Some(side));
        assert_eq!(top.right, None);

        let right = Border::right_only(side);
        assert_eq!(right.right, Some(side));
        assert_eq!(right.top, None);

        let bottom = Border::bottom_only(side);
        assert_eq!(bottom.bottom, Some(side));
        assert_eq!(bottom.top, None);

        let left = Border::left_only(side);
        assert_eq!(left.left, Some(side));
        assert_eq!(left.top, None);
    }

    #[test]
    fn test_border_is_uniform() {
        let side = test_side();
        let uniform = Border::all(side);
        assert!(uniform.is_uniform());

        let non_uniform = Border::new(
            Some(side),
            Some(BorderSide::new(Color::RED, 2.0, BorderStyle::Solid)),
            Some(side),
            Some(side),
        );
        assert!(!non_uniform.is_uniform());

        let partial = Border::top_only(side);
        assert!(!partial.is_uniform());
    }

    #[test]
    fn test_border_lerp() {
        let a = Border::all(BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid));
        let b = Border::all(BorderSide::new(Color::WHITE, 3.0, BorderStyle::Solid));

        let mid = Border::lerp(a, b, 0.5);
        assert!(mid.top.is_some());
        let top = mid.top.unwrap();
        assert_eq!(top.width, 2.0);
    }

    #[test]
    fn test_border_directional_resolve_ltr() {
        let side = test_side();
        let directional = BorderDirectional::all(side);
        let resolved = directional.resolve(true);

        assert_eq!(resolved.top, Some(side));
        assert_eq!(resolved.right, Some(side));
        assert_eq!(resolved.bottom, Some(side));
        assert_eq!(resolved.left, Some(side));
    }

    #[test]
    fn test_border_directional_resolve_rtl() {
        let start_side = BorderSide::new(Color::RED, 2.0, BorderStyle::Solid);
        let end_side = BorderSide::new(Color::BLUE, 3.0, BorderStyle::Solid);

        let directional = BorderDirectional::new(None, Some(end_side), None, Some(start_side));
        let resolved = directional.resolve(false);

        assert_eq!(resolved.left, Some(end_side));
        assert_eq!(resolved.right, Some(start_side));
    }

    #[test]
    fn test_box_border_trait() {
        let border = Border::all(test_side());
        assert!(border.is_uniform());

        let a = Border::all(BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid));
        let b = Border::all(BorderSide::new(Color::WHITE, 3.0, BorderStyle::Solid));
        let _lerped = Border::lerp_border(&a, &b, 0.5);
    }
}
