//! [`paint_table_border`] — draws a [`TableBorder`] around and inside a
//! table's cell grid.
//!
//! Flutter parity: `rendering/table_border.dart` `TableBorder.paint`. Paint
//! order (oracle, `table_border.dart:296-329`): interior vertical lines
//! (`vertical_inside`, one column per entry in `columns`), then interior
//! horizontal lines (`horizontal_inside`, one row per entry in `rows`), then
//! the outer border (`top`/`right`/`bottom`/`left`) — painted last so it sits
//! on top of the interior grid lines.
//!
//! [`TableBorder::border_radius`] rounds the outer border when it is uniform:
//! the corners are handed to [`crate::decoration`]'s border painter as an
//! [`RRect`], which already rounds the uniform outer path and ignores the
//! radius on the non-uniform (four-edge) path — matching the oracle, which
//! only rounds a uniform outer edge (`table_border.dart:143-156`).

use flui_types::{
    Pixels, Point, RRect, Rect,
    painting::{Paint, Path},
    styling::{BorderStyle, TableBorder},
};

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
    rect: Rect<Pixels>,
    rows: &[Pixels],
    columns: &[Pixels],
    border: &TableBorder,
) {
    if !columns.is_empty() && border.vertical_inside.style == BorderStyle::Solid {
        let mut path = Path::new();
        for &x in columns {
            path.move_to(Point::new(rect.min.x + x, rect.min.y));
            path.line_to(Point::new(rect.min.x + x, rect.max.y));
        }
        let paint = Paint::stroke(
            border.vertical_inside.color,
            border.vertical_inside.width.get(),
        );
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
            border.horizontal_inside.width.get(),
        );
        canvas.draw_path(&path, &paint);
    }

    // Outer border painted last (on top of the interior grid). Its corners
    // come from `border.border_radius` (square when zero): a uniform outer
    // edge rounds to this `RRect`, while `paint_border` ignores the radius on
    // the non-uniform four-edge path — matching the oracle, which rounds a
    // uniform outer edge only (`table_border.dart:143-156`).
    let outer_rrect = RRect::from_rect_and_corners(
        rect,
        border.border_radius.top_left,
        border.border_radius.top_right,
        border.border_radius.bottom_right,
        border.border_radius.bottom_left,
    );
    paint_border(canvas, rect, Some(outer_rrect), &border.outer_border());
}

#[cfg(test)]
mod tests {
    use flui_types::{
        Color, Point as GeomPoint, geometry::px, painting::PathCommand, styling::BorderSide,
    };

    use super::*;
    use crate::{DisplayListCore, DrawCommand};

    fn rect() -> Rect<Pixels> {
        Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(60.0))
    }

    fn solid(width: f32, color: Color) -> BorderSide<Pixels> {
        BorderSide::new(color, px(width), BorderStyle::Solid)
    }

    #[test]
    fn no_border_and_no_lines_paints_nothing() {
        let mut canvas = Canvas::new();
        paint_table_border(&mut canvas, rect(), &[], &[], &TableBorder::NONE);
        let list = canvas.finish();
        assert!(list.is_empty(), "expected no draw commands, got {list:?}");
    }

    #[test]
    fn interior_lines_use_the_inside_sides_and_outer_border_paints_last() {
        let mut canvas = Canvas::new();
        let border = TableBorder {
            vertical_inside: solid(1.0, Color::RED),
            horizontal_inside: solid(2.0, Color::GREEN),
            ..TableBorder::all(solid(3.0, Color::BLUE))
        };
        paint_table_border(&mut canvas, rect(), &[px(30.0)], &[px(50.0)], &border);
        let list = canvas.finish();
        // Vertical interior line, then horizontal interior line, then the
        // (uniform) outer border as a DrawDRRect — three draw calls, in
        // that order.
        assert_eq!(list.len(), 3, "commands: {list:?}");

        let cmds: Vec<_> = list.iter().collect();
        #[allow(clippy::panic)] // Test assertion
        let DrawCommand::DrawPath { path, .. } = &cmds[0] else {
            panic!("expected the first command to be the vertical interior line path");
        };
        assert_eq!(
            path.commands(),
            &[
                PathCommand::MoveTo(GeomPoint::new(px(50.0), px(0.0))),
                PathCommand::LineTo(GeomPoint::new(px(50.0), px(60.0))),
            ][..]
        );

        #[allow(clippy::panic)] // Test assertion
        let DrawCommand::DrawPath { path, .. } = &cmds[1] else {
            panic!("expected the second command to be the horizontal interior line path");
        };
        assert_eq!(
            path.commands(),
            &[
                PathCommand::MoveTo(GeomPoint::new(px(0.0), px(30.0))),
                PathCommand::LineTo(GeomPoint::new(px(100.0), px(30.0))),
            ][..]
        );

        assert!(
            matches!(cmds[2], DrawCommand::DrawDRRect { .. }),
            "expected the outer border to paint last as a uniform DrawDRRect; got {:?}",
            cmds[2]
        );
    }

    #[test]
    fn uniform_outer_border_rounds_to_the_border_radius() {
        use flui_types::geometry::Radius;
        use flui_types::styling::{BorderRadius, BorderRadiusExt};

        let mut canvas = Canvas::new();
        let border = TableBorder::all(solid(2.0, Color::BLACK))
            .with_border_radius(BorderRadius::circular(px(8.0)));
        paint_table_border(&mut canvas, rect(), &[], &[], &border);
        let list = canvas.finish();
        let cmds: Vec<_> = list.iter().collect();

        // The single (uniform) outer border rounds its OUTER rrect to the
        // requested 8px corners — the deferred-edge feature working end to end.
        #[allow(clippy::panic)] // Test assertion
        let DrawCommand::DrawDRRect { outer, .. } = &cmds[0] else {
            panic!("expected a single uniform outer DrawDRRect; got {:?}", cmds);
        };
        assert_eq!(outer.top_left, Radius::circular(px(8.0)));
        assert_eq!(outer.bottom_right, Radius::circular(px(8.0)));
    }

    #[test]
    fn zero_border_radius_leaves_the_outer_corners_square() {
        use flui_types::geometry::Radius;

        let mut canvas = Canvas::new();
        // No `with_border_radius` -> default `BorderRadius::ZERO`.
        let border = TableBorder::all(solid(2.0, Color::BLACK));
        paint_table_border(&mut canvas, rect(), &[], &[], &border);
        let list = canvas.finish();
        let cmds: Vec<_> = list.iter().collect();

        #[allow(clippy::panic)] // Test assertion
        let DrawCommand::DrawDRRect { outer, .. } = &cmds[0] else {
            panic!("expected a single uniform outer DrawDRRect; got {:?}", cmds);
        };
        assert_eq!(
            outer.top_left,
            Radius::circular(px(0.0)),
            "square by default"
        );
    }

    #[test]
    fn non_solid_inside_style_skips_the_interior_lines() {
        let mut canvas = Canvas::new();
        let mut border = TableBorder::all(solid(1.0, Color::BLACK));
        border.vertical_inside.style = BorderStyle::None;
        border.horizontal_inside.style = BorderStyle::None;
        paint_table_border(&mut canvas, rect(), &[px(30.0)], &[px(50.0)], &border);
        let list = canvas.finish();
        // Only the outer border remains.
        assert_eq!(list.len(), 1, "commands: {list:?}");
    }
}
