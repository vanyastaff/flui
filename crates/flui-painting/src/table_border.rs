//! [`paint_table_border`] — draws a [`TableBorder`] around and inside a
//! table's cell grid.
//!
//! Paint order: interior vertical lines
//! (`vertical_inside`, one column per entry in `columns`), then interior
//! horizontal lines (`horizontal_inside`, one row per entry in `rows`), then
//! the outer border (`top`/`right`/`bottom`/`left`) — painted last so it sits
//! on top of the interior grid lines.
//!
//! [`TableBorder::border_radius`] rounds the outer border when it is uniform:
//! the corners are handed to [`crate::decoration`]'s border painter as an
//! [`RRect`], which already rounds the uniform outer path and ignores the
//! radius on the non-uniform (four-edge) path, so only a uniform outer edge
//! is rounded.

use crate::{
    paint::{Paint, Path},
    styling::{BorderStyle, TableBorder},
};
use flui_foundation::geometry::{Point, RRect, Rect};

use crate::canvas::Canvas;
use crate::decoration::paint_border;

/// Paints `border` around `rect`, with interior lines at the given `rows`
/// (vertical offsets between rows, relative to `rect.min.y`) and `columns`
/// (horizontal offsets between columns, relative to `rect.min.x`).
///
/// `rows`/`columns` hold only the INTERIOR boundaries — a 2-row table passes
/// one entry in `rows` (the line between row 0 and row 1), not the table's
/// top/bottom edges (those are `border.top`/`border.bottom`).
pub fn paint_table_border(
    canvas: &mut Canvas,
    rect: Rect<f64>,
    rows: &[f64],
    columns: &[f64],
    border: &TableBorder,
) {
    if !columns.is_empty() && border.vertical_inside.style == BorderStyle::Solid {
        let mut path = Path::new();
        for &x in columns {
            path.move_to(Point::new(rect.min.x + x, rect.min.y));
            path.line_to(Point::new(rect.min.x + x, rect.max.y));
        }
        let paint = Paint::stroke(border.vertical_inside.color, border.vertical_inside.width);
        canvas.draw_path(&path, &paint);
    }

    if !rows.is_empty() && border.horizontal_inside.style == BorderStyle::Solid {
        let mut path = Path::new();
        for &y in rows {
            path.move_to(Point::new(rect.min.x, rect.min.y + y));
            path.line_to(Point::new(rect.max.x, rect.min.y + y));
        }
        let paint = Paint::stroke(
            border.horizontal_inside.color,
            border.horizontal_inside.width,
        );
        canvas.draw_path(&path, &paint);
    }

    // Outer border painted last (on top of the interior grid). Its corners
    // come from `border.border_radius` (square when zero): a uniform outer
    // edge rounds to this `RRect`, while `paint_border` ignores the radius on
    // the non-uniform four-edge path, so only a uniform outer edge rounds.
    let outer_rrect = RRect::from_rect_and_corners(
        rect,
        border.border_radius.top_left,
        border.border_radius.top_right,
        border.border_radius.bottom_right,
        border.border_radius.bottom_left,
    );
    paint_border(canvas, rect, Some(outer_rrect), &border.outer_border());
}
