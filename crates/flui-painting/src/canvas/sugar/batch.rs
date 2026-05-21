//! Batch drawing helpers: loop wrappers around the single-shape
//! primary `draw_*` methods.
//!
//! Each method here takes a slice (or pair-slice) of shapes plus one
//! shared [`Paint`] and emits one `DrawCommand` per element. Use them
//! when you have a homogeneous batch with shared paint state — they
//! are no faster than the primary methods (no batching at the
//! `DrawCommand` level), but they keep call-site noise down.

use flui_types::{
    geometry::{Pixels, Point, RRect, Rect},
    painting::Path,
};

use crate::canvas::Canvas;
use crate::display_list::Paint;

impl Canvas {
    /// Draws multiple rectangles with the same paint.
    #[inline]
    pub fn draw_rects(&mut self, rects: &[Rect<Pixels>], paint: &Paint) {
        for rect in rects {
            self.draw_rect(*rect, paint);
        }
    }

    /// Draws multiple circles with the same paint.
    #[inline]
    pub fn draw_circles(&mut self, circles: &[(Point<Pixels>, f32)], paint: &Paint) {
        for (center, radius) in circles {
            self.draw_circle(*center, Pixels(*radius), paint);
        }
    }

    /// Draws multiple lines with the same paint.
    #[inline]
    pub fn draw_lines(&mut self, lines: &[(Point<Pixels>, Point<Pixels>)], paint: &Paint) {
        for (p1, p2) in lines {
            self.draw_line(*p1, *p2, paint);
        }
    }

    /// Draws multiple rounded rectangles with the same paint.
    #[inline]
    pub fn draw_rrects(&mut self, rrects: &[RRect], paint: &Paint) {
        for rrect in rrects {
            self.draw_rrect(*rrect, paint);
        }
    }

    /// Draws multiple paths with the same paint.
    #[inline]
    pub fn draw_paths(&mut self, paths: &[&Path], paint: &Paint) {
        for path in paths {
            self.draw_path(path, paint);
        }
    }
}
