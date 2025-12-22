//! Implementation of `Painter` trait for `Canvas`.
//!
//! This module bridges the abstract `Painter` trait from `flui-foundation`
//! with the concrete `Canvas` implementation, enabling backend-agnostic
//! painting through the protocol system.

use crate::canvas::Canvas;
use crate::display_list::Paint;
use flui_foundation::painting::{PaintImage, PaintParagraph, Painter};
use flui_types::geometry::{Matrix4, Offset, Point, RRect, Rect};
use flui_types::painting::Path;

impl Painter for Canvas {
    // ════════════════════════════════════════════════════════════════════════
    // STATE MANAGEMENT
    // ════════════════════════════════════════════════════════════════════════

    #[inline]
    fn save(&mut self) {
        Canvas::save(self);
    }

    #[inline]
    fn restore(&mut self) {
        Canvas::restore(self);
    }

    #[inline]
    fn save_count(&self) -> usize {
        Canvas::save_count(self)
    }

    fn restore_to_count(&mut self, count: usize) {
        Canvas::restore_to_count(self, count);
    }

    // ════════════════════════════════════════════════════════════════════════
    // TRANSFORMATIONS
    // ════════════════════════════════════════════════════════════════════════

    #[inline]
    fn translate(&mut self, dx: f32, dy: f32) {
        Canvas::translate(self, dx, dy);
    }

    #[inline]
    fn rotate(&mut self, radians: f32) {
        Canvas::rotate(self, radians);
    }

    #[inline]
    fn scale(&mut self, sx: f32, sy: f32) {
        Canvas::scale_xy(self, sx, sy);
    }

    #[inline]
    fn skew(&mut self, sx: f32, sy: f32) {
        Canvas::skew(self, sx, sy);
    }

    #[inline]
    fn transform(&mut self, matrix: &Matrix4) {
        Canvas::transform(self, *matrix);
    }

    #[inline]
    fn reset_transform(&mut self) {
        Canvas::set_transform(self, Matrix4::identity());
    }

    #[inline]
    fn get_transform(&self) -> Matrix4 {
        Canvas::transform_matrix(self)
    }

    // ════════════════════════════════════════════════════════════════════════
    // CLIPPING
    // ════════════════════════════════════════════════════════════════════════

    #[inline]
    fn clip_rect(&mut self, rect: Rect) {
        Canvas::clip_rect(self, rect);
    }

    #[inline]
    fn clip_rrect(&mut self, rrect: RRect) {
        Canvas::clip_rrect(self, rrect);
    }

    #[inline]
    fn clip_path(&mut self, path: &Path) {
        Canvas::clip_path(self, path);
    }

    // ════════════════════════════════════════════════════════════════════════
    // DRAWING PRIMITIVES
    // ════════════════════════════════════════════════════════════════════════

    #[inline]
    fn draw_rect(&mut self, rect: Rect, paint: &flui_types::painting::Paint) {
        // Convert flui_types::Paint to display_list::Paint
        let dl_paint = convert_paint(paint);
        Canvas::draw_rect(self, rect, &dl_paint);
    }

    #[inline]
    fn draw_rrect(&mut self, rrect: RRect, paint: &flui_types::painting::Paint) {
        let dl_paint = convert_paint(paint);
        Canvas::draw_rrect(self, rrect, &dl_paint);
    }

    #[inline]
    fn draw_circle(&mut self, center: Point, radius: f32, paint: &flui_types::painting::Paint) {
        let dl_paint = convert_paint(paint);
        Canvas::draw_circle(self, center, radius, &dl_paint);
    }

    #[inline]
    fn draw_oval(&mut self, rect: Rect, paint: &flui_types::painting::Paint) {
        let dl_paint = convert_paint(paint);
        Canvas::draw_oval(self, rect, &dl_paint);
    }

    #[inline]
    fn draw_line(&mut self, p1: Point, p2: Point, paint: &flui_types::painting::Paint) {
        let dl_paint = convert_paint(paint);
        Canvas::draw_line(self, p1, p2, &dl_paint);
    }

    #[inline]
    fn draw_path(&mut self, path: &Path, paint: &flui_types::painting::Paint) {
        let dl_paint = convert_paint(paint);
        Canvas::draw_path(self, path, &dl_paint);
    }

    fn draw_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &flui_types::painting::Paint,
    ) {
        let dl_paint = convert_paint(paint);
        Canvas::draw_arc(self, rect, start_angle, sweep_angle, use_center, &dl_paint);
    }

    fn draw_points(&mut self, points: &[Point], paint: &flui_types::painting::Paint) {
        let dl_paint = convert_paint(paint);
        // Canvas has draw_points which takes radius, use small radius for points
        Canvas::draw_points(self, points, 1.0, &dl_paint);
    }

    // ════════════════════════════════════════════════════════════════════════
    // COMPLEX DRAWING
    // ════════════════════════════════════════════════════════════════════════

    fn draw_image(
        &mut self,
        _image: &dyn PaintImage,
        _offset: Offset,
        _paint: &flui_types::painting::Paint,
    ) {
        // TODO: Implement when we have a way to convert PaintImage to Image
        // This requires either:
        // 1. Downcast to concrete Image type
        // 2. Add image data extraction methods to PaintImage trait
        tracing::warn!("draw_image via Painter trait not yet implemented");
    }

    fn draw_image_rect(
        &mut self,
        _image: &dyn PaintImage,
        _src: Rect,
        _dst: Rect,
        _paint: &flui_types::painting::Paint,
    ) {
        // TODO: Implement when we have image conversion
        tracing::warn!("draw_image_rect via Painter trait not yet implemented");
    }

    fn draw_image_nine(
        &mut self,
        _image: &dyn PaintImage,
        _center: Rect,
        _dst: Rect,
        _paint: &flui_types::painting::Paint,
    ) {
        // TODO: Implement when we have image conversion
        tracing::warn!("draw_image_nine via Painter trait not yet implemented");
    }

    fn draw_paragraph(&mut self, _paragraph: &dyn PaintParagraph, _offset: Offset) {
        // TODO: Implement when we have paragraph conversion
        // This requires converting PaintParagraph to InlineSpan or TextPainter
        tracing::warn!("draw_paragraph via Painter trait not yet implemented");
    }

    // ════════════════════════════════════════════════════════════════════════
    // LAYER OPERATIONS
    // ════════════════════════════════════════════════════════════════════════

    fn save_layer(&mut self, bounds: Option<Rect>, paint: Option<&flui_types::painting::Paint>) {
        let dl_paint = paint.map(convert_paint).unwrap_or_else(Paint::default);
        Canvas::save_layer(self, bounds, &dl_paint);
    }

    fn save_layer_alpha(&mut self, bounds: Option<Rect>, alpha: u8) {
        Canvas::save_layer_alpha(self, bounds, alpha);
    }

    // ════════════════════════════════════════════════════════════════════════
    // MISCELLANEOUS
    // ════════════════════════════════════════════════════════════════════════

    fn clear(&mut self, color: u32) {
        use crate::display_list::BlendMode;
        use flui_types::styling::Color;

        let color = Color::from_argb(color);
        Canvas::draw_color(self, color, BlendMode::Src);
    }

    fn draw_shadow(
        &mut self,
        path: &Path,
        color: u32,
        elevation: f32,
        _transparent_occluder: bool,
    ) {
        use flui_types::styling::Color;

        let color = Color::from_argb(color);
        Canvas::draw_shadow(self, path, color, elevation);
    }
}

/// Converts `flui_types::Paint` to `display_list::Paint`.
///
/// This is a temporary bridge until we unify the Paint types.
fn convert_paint(paint: &flui_types::painting::Paint) -> Paint {
    use crate::display_list::BlendMode as DlBlendMode;
    use flui_types::painting::BlendMode;

    let blend_mode = match paint.blend_mode {
        BlendMode::Clear => DlBlendMode::Clear,
        BlendMode::Src => DlBlendMode::Src,
        BlendMode::Dst => DlBlendMode::Dst,
        BlendMode::SrcOver => DlBlendMode::SrcOver,
        BlendMode::DstOver => DlBlendMode::DstOver,
        BlendMode::SrcIn => DlBlendMode::SrcIn,
        BlendMode::DstIn => DlBlendMode::DstIn,
        BlendMode::SrcOut => DlBlendMode::SrcOut,
        BlendMode::DstOut => DlBlendMode::DstOut,
        BlendMode::SrcATop => DlBlendMode::SrcATop,
        BlendMode::DstATop => DlBlendMode::DstATop,
        BlendMode::Xor => DlBlendMode::Xor,
        BlendMode::Plus => DlBlendMode::Plus,
        BlendMode::Modulate => DlBlendMode::Modulate,
        BlendMode::Screen => DlBlendMode::Screen,
        BlendMode::Overlay => DlBlendMode::Overlay,
        BlendMode::Darken => DlBlendMode::Darken,
        BlendMode::Lighten => DlBlendMode::Lighten,
        BlendMode::ColorDodge => DlBlendMode::ColorDodge,
        BlendMode::ColorBurn => DlBlendMode::ColorBurn,
        BlendMode::HardLight => DlBlendMode::HardLight,
        BlendMode::SoftLight => DlBlendMode::SoftLight,
        BlendMode::Difference => DlBlendMode::Difference,
        BlendMode::Exclusion => DlBlendMode::Exclusion,
        BlendMode::Multiply => DlBlendMode::Multiply,
        BlendMode::Hue => DlBlendMode::Hue,
        BlendMode::Saturation => DlBlendMode::Saturation,
        BlendMode::Color => DlBlendMode::Color,
        BlendMode::Luminosity => DlBlendMode::Luminosity,
    };

    Paint {
        color: paint.color,
        blend_mode,
        stroke_width: paint.stroke_width,
        anti_alias: paint.anti_alias,
        ..Paint::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::painting::Painter;
    use flui_types::geometry::Rect;
    use flui_types::painting::Paint as TypesPaint;
    use flui_types::styling::Color;

    #[test]
    fn test_painter_save_restore() {
        let mut canvas = Canvas::new();

        assert_eq!(Painter::save_count(&canvas), 1);

        Painter::save(&mut canvas);
        assert_eq!(Painter::save_count(&canvas), 2);

        Painter::save(&mut canvas);
        assert_eq!(Painter::save_count(&canvas), 3);

        Painter::restore(&mut canvas);
        assert_eq!(Painter::save_count(&canvas), 2);

        Painter::restore_to_count(&mut canvas, 1);
        assert_eq!(Painter::save_count(&canvas), 1);
    }

    #[test]
    fn test_painter_transforms() {
        let mut canvas = Canvas::new();

        Painter::translate(&mut canvas, 10.0, 20.0);
        let transform = Painter::get_transform(&canvas);
        // Check translation component
        assert!((transform.translation().0 - 10.0).abs() < 0.001);
        assert!((transform.translation().1 - 20.0).abs() < 0.001);

        Painter::reset_transform(&mut canvas);
        let transform = Painter::get_transform(&canvas);
        assert!(transform.is_identity());
    }

    #[test]
    fn test_painter_draw_rect() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let paint = TypesPaint::fill(Color::RED);

        Painter::draw_rect(&mut canvas, rect, &paint);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 1);
    }

    #[test]
    fn test_painter_clipping() {
        let mut canvas = Canvas::new();
        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);

        Painter::clip_rect(&mut canvas, rect);

        let display_list = canvas.finish();
        assert_eq!(display_list.len(), 1);
    }
}
