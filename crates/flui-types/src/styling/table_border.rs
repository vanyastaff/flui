//! [`TableBorder`] — border specification for `Table`/`RenderTable`.
//!
//! Like [`Border`](crate::styling::Border), but with two additional interior
//! sides: the horizontal lines between rows and the vertical lines between
//! columns.
//!
//! Flutter parity: `rendering/table_border.dart` `TableBorder`.
//!
//! `border_radius` is intentionally **not** part of this first slice — only
//! the zero-radius uniform and non-uniform outer-border paths are supported
//! (see `flui_painting::paint_table_border`).

use crate::{geometry::Pixels, styling::BorderSide};

/// Border specification for a `Table`/`RenderTable`: four outer sides plus
/// two interior sides (between rows, between columns).
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableBorder {
    /// The top side of the outer border.
    pub top: BorderSide<Pixels>,

    /// The right side of the outer border.
    pub right: BorderSide<Pixels>,

    /// The bottom side of the outer border.
    pub bottom: BorderSide<Pixels>,

    /// The left side of the outer border.
    pub left: BorderSide<Pixels>,

    /// The interior side drawn between rows.
    pub horizontal_inside: BorderSide<Pixels>,

    /// The interior side drawn between columns.
    pub vertical_inside: BorderSide<Pixels>,
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
    };

    /// Creates a border with explicit per-side styling.
    #[inline]
    pub const fn new(
        top: BorderSide<Pixels>,
        right: BorderSide<Pixels>,
        bottom: BorderSide<Pixels>,
        left: BorderSide<Pixels>,
        horizontal_inside: BorderSide<Pixels>,
        vertical_inside: BorderSide<Pixels>,
    ) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
            horizontal_inside,
            vertical_inside,
        }
    }

    /// A uniform border: every side (outer and interior) uses `side`.
    #[inline]
    pub const fn all(side: BorderSide<Pixels>) -> Self {
        Self {
            top: side,
            right: side,
            bottom: side,
            left: side,
            horizontal_inside: side,
            vertical_inside: side,
        }
    }

    /// A border where every outer side uses `outside` and every interior side
    /// uses `inside`.
    #[inline]
    pub const fn symmetric(inside: BorderSide<Pixels>, outside: BorderSide<Pixels>) -> Self {
        Self {
            top: outside,
            right: outside,
            bottom: outside,
            left: outside,
            horizontal_inside: inside,
            vertical_inside: inside,
        }
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
    pub fn outer_border(&self) -> crate::styling::Border<Pixels> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::px,
        styling::{BorderStyle, Color},
    };

    fn solid(width: f32) -> BorderSide<Pixels> {
        BorderSide::new(Color::BLACK, px(width), BorderStyle::Solid)
    }

    #[test]
    fn default_is_none_on_every_side() {
        let border = TableBorder::default();
        assert_eq!(border.top, BorderSide::NONE);
        assert_eq!(border.horizontal_inside, BorderSide::NONE);
        assert_eq!(border.vertical_inside, BorderSide::NONE);
    }

    #[test]
    fn all_sets_every_side_including_interior() {
        let side = solid(2.0);
        let border = TableBorder::all(side);
        assert_eq!(border.top, side);
        assert_eq!(border.left, side);
        assert_eq!(border.horizontal_inside, side);
        assert_eq!(border.vertical_inside, side);
        assert!(border.is_uniform());
    }

    #[test]
    fn symmetric_splits_inside_and_outside() {
        let inside = solid(1.0);
        let outside = solid(3.0);
        let border = TableBorder::symmetric(inside, outside);
        assert_eq!(border.top, outside);
        assert_eq!(border.right, outside);
        assert_eq!(border.horizontal_inside, inside);
        assert_eq!(border.vertical_inside, inside);
        assert!(!border.is_uniform());
        assert!(border.outer_border().is_uniform());
    }
}
