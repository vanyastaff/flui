//! Convenience shape methods: compound shapes assembled from the
//! primary primitives (rounded rectangles with per-corner radii,
//! pills, rings).

use flui_types::{
    geometry::{Pixels, Point, RRect, Radius, Rect, Size, px},
    painting::Paint,
};

use crate::canvas::Canvas;

impl Canvas {
    /// Draws a rounded rectangle with uniform corner radius.
    #[inline]
    pub fn draw_rounded_rect(&mut self, rect: Rect<Pixels>, radius: Pixels, paint: &Paint) {
        let rrect = RRect::from_rect_circular(rect, radius);
        self.draw_rrect(rrect, paint);
    }

    /// Draws a rectangle with different corner radii.
    #[inline]
    pub fn draw_rounded_rect_corners(
        &mut self,
        rect: Rect<Pixels>,
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
        paint: &Paint,
    ) {
        let rrect = RRect::from_rect_and_corners(
            rect,
            Radius::circular(px(top_left)),
            Radius::circular(px(top_right)),
            Radius::circular(px(bottom_right)),
            Radius::circular(px(bottom_left)),
        );
        self.draw_rrect(rrect, paint);
    }

    /// Draws a pill shape (fully rounded rectangle).
    ///
    /// The corner radius is automatically set to half the smaller
    /// dimension.
    #[inline]
    pub fn draw_pill(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        let radius = rect.width().min(rect.height()).0 / 2.0;
        self.draw_rounded_rect(rect, Pixels(radius), paint);
    }

    /// Draws a ring (donut shape).
    #[inline]
    pub fn draw_ring(
        &mut self,
        center: Point<Pixels>,
        outer_radius: f32,
        inner_radius: f32,
        paint: &Paint,
    ) {
        let outer = RRect::from_rect_circular(
            Rect::from_center_size(
                center,
                Size::new(px(outer_radius * 2.0), px(outer_radius * 2.0)),
            ),
            px(outer_radius),
        );
        let inner = RRect::from_rect_circular(
            Rect::from_center_size(
                center,
                Size::new(px(inner_radius * 2.0), px(inner_radius * 2.0)),
            ),
            px(inner_radius),
        );
        self.draw_drrect(outer, inner, paint);
    }
}
