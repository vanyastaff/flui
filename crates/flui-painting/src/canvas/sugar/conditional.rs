//! Conditional drawing helpers.
//!
//! These avoid the boilerplate of `if condition { canvas.draw_*(...) }`
//! at call sites that toggle drawing based on a flag or `Option`.

use flui_types::geometry::{Pixels, Point, Rect};

use crate::canvas::Canvas;
use crate::display_list::Paint;

impl Canvas {
    /// Draws a rectangle only if the condition is true.
    #[inline]
    pub fn draw_rect_if(&mut self, condition: bool, rect: Rect<Pixels>, paint: &Paint) {
        if condition {
            self.draw_rect(rect, paint);
        }
    }

    /// Draws a circle only if the condition is true.
    #[inline]
    pub fn draw_circle_if(
        &mut self,
        condition: bool,
        center: Point<Pixels>,
        radius: Pixels,
        paint: &Paint,
    ) {
        if condition {
            self.draw_circle(center, radius, paint);
        }
    }

    /// Executes drawing closure only if the condition is true.
    #[inline]
    pub fn draw_if<F>(&mut self, condition: bool, f: F)
    where
        F: FnOnce(&mut Self),
    {
        if condition {
            f(self);
        }
    }

    /// Executes drawing closure only if the condition is false.
    #[inline]
    pub fn draw_unless<F>(&mut self, condition: bool, f: F)
    where
        F: FnOnce(&mut Self),
    {
        if !condition {
            f(self);
        }
    }

    /// Draws based on Option - draws if Some, skips if None.
    #[inline]
    pub fn draw_if_some<T, F>(&mut self, option: Option<T>, f: F)
    where
        F: FnOnce(&mut Self, T),
    {
        if let Some(value) = option {
            f(self, value);
        }
    }
}
