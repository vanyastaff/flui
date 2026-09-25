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

use crate::{
    geometry::Pixels,
    styling::{BorderRadius, BorderRadiusExt, BorderSide},
};

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
            border_radius: BorderRadius::ZERO,
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
            border_radius: BorderRadius::ZERO,
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

    fn sides(b: &TableBorder) -> [BorderSide<Pixels>; 6] {
        [
            b.top,
            b.right,
            b.bottom,
            b.left,
            b.horizontal_inside,
            b.vertical_inside,
        ]
    }

    #[test]
    fn new_places_each_side() {
        let s: [BorderSide<Pixels>; 6] = std::array::from_fn(|i| solid(i as f32 + 1.0));
        let b = TableBorder::new(s[0], s[1], s[2], s[3], s[4], s[5]);
        assert_eq!(sides(&b), s);
        assert_eq!(b.border_radius, BorderRadius::ZERO);
        let rounded = b.with_border_radius(BorderRadius::circular(px(4.0)));
        assert_eq!(rounded.border_radius, BorderRadius::circular(px(4.0)));
        let outer = b.outer_border();
        assert_eq!(
            [outer.top, outer.right, outer.bottom, outer.left],
            [Some(s[0]), Some(s[1]), Some(s[2]), Some(s[3])]
        );
    }

    /// Uniform means every side, inside ones included, shares color, width
    /// and style; any one side differing in any one of them breaks it.
    #[test]
    fn is_uniform_checks_every_side_and_attribute() {
        let base = solid(1.0);
        assert!(TableBorder::all(base).is_uniform());
        assert!(TableBorder::NONE.is_uniform());
        // Neither the radius nor a side's stroke alignment is part of it.
        assert!(
            TableBorder::all(base)
                .with_border_radius(BorderRadius::circular(px(2.0)))
                .is_uniform()
        );
        let variants = [
            base.with_color(Color::RED),
            base.with_width(px(2.0)),
            base.with_style(BorderStyle::None),
        ];
        for odd in variants {
            for i in 0..6 {
                let mut s = [base; 6];
                s[i] = odd;
                let b = TableBorder::new(s[0], s[1], s[2], s[3], s[4], s[5]);
                assert!(!b.is_uniform(), "side {i}: {odd:?}");
            }
        }
        let mut aligned = [base; 6];
        aligned[3] = base.with_stroke_alignment(1.0);
        assert!(
            TableBorder::new(
                aligned[0], aligned[1], aligned[2], aligned[3], aligned[4], aligned[5]
            )
            .is_uniform()
        );
    }
}
