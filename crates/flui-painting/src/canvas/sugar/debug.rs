//! Debug-visualization helpers.
//!
//! Cheap diagnostic overlays for layout / hit-region / transform
//! debugging. Each method composes the primary `draw_line` /
//! `draw_rect` / `draw_circle` calls — there is no separate
//! "debug" `DrawCommand` variant.

use flui_types::{
    geometry::{Pixels, Point, Rect, px},
    styling::Color,
};

use crate::canvas::Canvas;
use crate::display_list::Paint;

impl Canvas {
    /// Draws a debug rectangle showing bounds (outline only).
    #[inline]
    pub fn debug_rect(&mut self, rect: Rect<Pixels>, color: Color) {
        let paint = Paint::stroke(color, 1.0);
        self.draw_rect(rect, &paint);
    }

    /// Draws a debug cross at the specified point.
    #[inline]
    pub fn debug_point(&mut self, point: Point<Pixels>, size: f32, color: Color) {
        let half = size / 2.0;
        let paint = Paint::stroke(color, 1.0);
        self.draw_line(
            Point::new(Pixels(point.x.0 - half), point.y),
            Point::new(Pixels(point.x.0 + half), point.y),
            &paint,
        );
        self.draw_line(
            Point::new(point.x, Pixels(point.y.0 - half)),
            Point::new(point.x, Pixels(point.y.0 + half)),
            &paint,
        );
    }

    /// Draws debug visualization of the current transform origin.
    #[inline]
    pub fn debug_axes(&mut self, length: f32) {
        let origin = Point::new(Pixels(0.0), Pixels(0.0));

        self.draw_line(
            origin,
            Point::new(Pixels(length), Pixels(0.0)),
            &Paint::stroke(Color::RED, 2.0),
        );

        self.draw_line(
            origin,
            Point::new(Pixels(0.0), Pixels(length)),
            &Paint::stroke(Color::GREEN, 2.0),
        );

        self.draw_circle(origin, px(3.0), &Paint::fill(Color::BLUE));
    }

    /// Draws a debug grid overlay.
    ///
    /// # Panics
    ///
    /// Panics if `spacing` is non-positive (`<= 0.0`), NaN, or
    /// `INFINITY` — otherwise the `while x += spacing` loops below
    /// run forever (or step by zero).
    pub fn debug_grid(&mut self, bounds: Rect<Pixels>, spacing: f32, color: Color) {
        assert!(
            spacing > 0.0 && spacing.is_finite(),
            "Canvas::debug_grid spacing must be positive and finite, got {spacing}"
        );

        let paint = Paint::stroke(color, 0.5);

        let mut x = bounds.left();
        while x <= bounds.right() {
            self.draw_line(
                Point::new(x, bounds.top()),
                Point::new(x, bounds.bottom()),
                &paint,
            );
            x += Pixels(spacing);
        }

        let mut y = bounds.top();
        while y <= bounds.bottom() {
            self.draw_line(
                Point::new(bounds.left(), y),
                Point::new(bounds.right(), y),
                &paint,
            );
            y += Pixels(spacing);
        }
    }
}
