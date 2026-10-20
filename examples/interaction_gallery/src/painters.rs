//! The two pictures the gallery paints itself: a street map under the
//! bottom sheet and a colour grid under the zoomable viewer.

use std::any::Any;

use flui::geometry::{Point, Rect, Size};
use flui::painting::{Canvas, CustomPainter, Paint};
use flui::prelude::*;

/// The side of one grid cell, in logical pixels.
pub(crate) const CELL: f64 = 40.0;
/// Cells along each side of the grid.
pub(crate) const CELLS: u8 = 24;

/// The colour of the grid cell in `column` and `row`: red rises along a
/// row and green down a column, so every cell, and so every zoom and pan
/// that brings another cell under a point, reads differently.
#[must_use]
pub(crate) fn cell_color(column: u8, row: u8) -> Color {
    Color::rgb(30 + 9 * column, 40 + 8 * row, 200 - 4 * column.min(row))
}

/// A grid of [`CELLS`] by [`CELLS`] cells of [`CELL`], each in its
/// [`cell_color`], with white hairlines between them.
#[derive(Debug)]
pub(crate) struct GridPainter;

impl CustomPainter for GridPainter {
    fn paint(&self, canvas: &mut Canvas, _size: Size) {
        for row in 0..CELLS {
            for column in 0..CELLS {
                let left = f64::from(column) * CELL;
                let top = f64::from(row) * CELL;
                canvas.draw_rect(
                    Rect::from_xywh(left, top, CELL - 1.0, CELL - 1.0),
                    &Paint::fill(cell_color(column, row)),
                );
            }
        }
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The grid's whole extent.
#[must_use]
pub(crate) fn grid_size() -> Size {
    let side = f64::from(CELLS) * CELL;
    Size::new(side, side)
}

/// Pale blocks between streets, as a map behind the sheet.
#[derive(Debug)]
pub(crate) struct MapPainter;

/// The map's ground.
pub(crate) const MAP_GROUND: Color = Color::rgb(229, 236, 222);

impl CustomPainter for MapPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, size.width, size.height),
            &Paint::fill(MAP_GROUND),
        );
        let street = Paint::stroke(Color::rgb(255, 255, 255), 10.0);
        let mut x = 60.0;
        while x < size.width {
            canvas.draw_line(Point::new(x, 0.0), Point::new(x, size.height), &street);
            x += 140.0;
        }
        let mut y = 40.0;
        while y < size.height {
            canvas.draw_line(Point::new(0.0, y), Point::new(size.width, y), &street);
            y += 110.0;
        }
        let park = Paint::fill(Color::rgb(187, 219, 172));
        canvas.draw_rect(Rect::from_xywh(75.0, 55.0, 110.0, 80.0), &park);
        canvas.draw_circle(
            Point::new(size.width * 0.6, size.height * 0.3),
            14.0,
            &Paint::fill(Color::rgb(214, 69, 65)),
        );
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
