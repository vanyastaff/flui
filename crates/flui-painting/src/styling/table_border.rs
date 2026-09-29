//! [`TableBorder`] — border specification for `Table`/`RenderTable`.
//!
//! Like [`Border`](crate::styling::Border), but with two additional interior
//! sides: the horizontal lines between rows and the vertical lines between
//! columns.
//!
//! Flutter parity: `rendering/table_border.dart` `TableBorder`.
//!
//! [`TableBorder::border_radius`] rounds the outer border when it is uniform
//! (matching the oracle, which only rounds a uniform outer edge); see
//! `flui_painting::paint_table_border`.

use crate::styling::{BorderRadius, BorderRadiusExt, BorderSide};

/// Border specification for a `Table`/`RenderTable`: four outer sides plus
/// two interior sides (between rows, between columns).
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableBorder {
    /// The top side of the outer border.
    pub top: BorderSide<f64>,

    /// The right side of the outer border.
    pub right: BorderSide<f64>,

    /// The bottom side of the outer border.
    pub bottom: BorderSide<f64>,

    /// The left side of the outer border.
    pub left: BorderSide<f64>,

    /// The interior side drawn between rows.
    pub horizontal_inside: BorderSide<f64>,

    /// The interior side drawn between columns.
    pub vertical_inside: BorderSide<f64>,

    /// Corner rounding for the outer border.
    ///
    /// Only takes effect when the outer border is uniform (Flutter rounds a
    /// uniform outer edge only); a non-uniform outer border paints square
    /// regardless. Defaults to [`BorderRadius::ZERO`] (square corners).
    pub border_radius: BorderRadius,
}

impl TableBorder {
    /// A border with every side set to [`BorderSide::NONE`] (no border drawn).
    pub const NONE: Self = Self {
        top: BorderSide::NONE,
        right: BorderSide::NONE,
        bottom: BorderSide::NONE,
        left: BorderSide::NONE,
        horizontal_inside: BorderSide::NONE,
        vertical_inside: BorderSide::NONE,
        border_radius: BorderRadius::ZERO,
    };

    /// Creates a border with explicit per-side styling.
    #[inline]
    pub const fn new(
        top: BorderSide<f64>,
        right: BorderSide<f64>,
        bottom: BorderSide<f64>,
        left: BorderSide<f64>,
        horizontal_inside: BorderSide<f64>,
        vertical_inside: BorderSide<f64>,
    ) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
            horizontal_inside,
            vertical_inside,
            border_radius: BorderRadius::ZERO,
        }
    }

    /// A uniform border: every side (outer and interior) uses `side`.
    #[inline]
    pub const fn all(side: BorderSide<f64>) -> Self {
        Self {
            top: side,
            right: side,
            bottom: side,
            left: side,
            horizontal_inside: side,
            vertical_inside: side,
            border_radius: BorderRadius::ZERO,
        }
    }

    /// A border where every outer side uses `outside` and every interior side
    /// uses `inside`.
    #[inline]
    pub const fn symmetric(inside: BorderSide<f64>, outside: BorderSide<f64>) -> Self {
        Self {
            top: outside,
            right: outside,
            bottom: outside,
            left: outside,
            horizontal_inside: inside,
            vertical_inside: inside,
            border_radius: BorderRadius::ZERO,
        }
    }

    /// Builder: round the outer border by `border_radius` (takes effect only
    /// when the outer border is uniform — see [`Self::border_radius`]).
    #[must_use]
    pub const fn with_border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Whether every side (outer and interior) has an identical color, width,
    /// and style.
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        let sides = [
            self.top,
            self.right,
            self.bottom,
            self.left,
            self.horizontal_inside,
            self.vertical_inside,
        ];
        sides.windows(2).all(|pair| {
            pair[0].color == pair[1].color
                && pair[0].width == pair[1].width
                && pair[0].style == pair[1].style
        })
    }

    /// The outer four sides (`top`/`right`/`bottom`/`left`) as a plain
    /// [`Border`](crate::styling::Border), for reuse by the shared
    /// outer-border paint routine (`flui_painting::paint_table_border`
    /// delegates the outer edge to the same uniform/non-uniform logic
    /// `paint_box_decoration`'s border already uses).
    #[must_use]
    pub fn outer_border(&self) -> crate::styling::Border<f64> {
        crate::styling::Border {
            top: Some(self.top),
            right: Some(self.right),
            bottom: Some(self.bottom),
            left: Some(self.left),
        }
    }
}

impl Default for TableBorder {
    /// All sides default to [`BorderSide::NONE`] — Flutter parity
    /// (`TableBorder`'s constructor defaults, `table_border.dart:22-28`).
    #[inline]
    fn default() -> Self {
        Self::NONE
    }
}
