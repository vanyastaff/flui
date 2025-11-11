//! Wgpu-based CommandRenderer implementation
//!
//! Production rendering backend executing drawing commands via GPU acceleration.

use super::command_renderer::CommandRenderer;
use crate::painter::{Painter, WgpuPainter};
use flui_painting::{BlendMode, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect, Transform},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

pub struct WgpuRenderer {
    painter: WgpuPainter,
}

impl WgpuRenderer {
    pub fn new(painter: WgpuPainter) -> Self {
        Self { painter }
    }

    pub fn painter(&self) -> &WgpuPainter {
        &self.painter
    }

    pub fn painter_mut(&mut self) -> &mut WgpuPainter {
        &mut self.painter
    }

    /// Consume the renderer and return the underlying painter
    pub fn into_painter(self) -> WgpuPainter {
        self.painter
    }

    fn with_transform<F>(&mut self, transform: &Matrix4, draw_fn: F)
    where
        F: FnOnce(&mut WgpuPainter),
    {
        if transform.is_identity() {
            draw_fn(&mut self.painter);
            return;
        }

        self.painter.save();

        // Use centralized Transform::decompose() method (Phase 6 cleanup)
        let transform_enum = Transform::from(*transform);
        let (tx, ty, rotation, sx, sy) = transform_enum.decompose();

        if tx != 0.0 || ty != 0.0 {
            self.painter.translate(Offset::new(tx, ty));
        }
        if rotation.abs() > f32::EPSILON {
            self.painter.rotate(rotation);
        }
        if (sx - 1.0).abs() > f32::EPSILON || (sy - 1.0).abs() > f32::EPSILON {
            self.painter.scale(sx, sy);
        }

        draw_fn(&mut self.painter);
        self.painter.restore();
    }
}

impl CommandRenderer for WgpuRenderer {
    fn render_rect(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.rect(rect, paint);
        });
    }

    fn render_rrect(&mut self, rrect: RRect, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.rrect(rrect, paint);
        });
    }

    fn render_circle(&mut self, center: Point, radius: f32, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.circle(center, radius, paint);
        });
    }

    fn render_oval(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.oval(rect, paint);
        });
    }

    fn render_line(&mut self, p1: Point, p2: Point, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.line(p1, p2, paint);
        });
    }

    fn render_path(&mut self, path: &Path, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_path(path, paint);
        });
    }

    fn render_arc(&mut self, rect: Rect, start_angle: f32, sweep_angle: f32, use_center: bool, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_arc(rect, start_angle, sweep_angle, use_center, paint);
        });
    }

    fn render_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_drrect(outer, inner, paint);
        });
    }

    fn render_points(&mut self, mode: PointMode, points: &[Point], paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            match mode {
                PointMode::Points => {
                    let radius = paint.stroke_width / 2.0;
                    for point in points {
                        painter.circle(*point, radius, paint);
                    }
                }
                PointMode::Lines => {
                    for i in (0..points.len()).step_by(2) {
                        if i + 1 < points.len() {
                            painter.line(points[i], points[i + 1], paint);
                        }
                    }
                }
                PointMode::Polygon => {
                    for i in 0..points.len().saturating_sub(1) {
                        painter.line(points[i], points[i + 1], paint);
                    }
                    if points.len() > 2 {
                        painter.line(points[points.len() - 1], points[0], paint);
                    }
                }
            }
        });
    }

    fn render_text(&mut self, text: &str, offset: Offset, style: &TextStyle, _paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            let font_size = style.font_size.unwrap_or(14.0) as f32;
            let color = style.color.unwrap_or(Color::BLACK);
            let paint = Paint::fill(color);
            let position = Point::new(offset.dx, offset.dy);
            painter.text_styled(text, position, font_size, &paint);
        });
    }

    fn render_image(&mut self, image: &Image, dst: Rect, _paint: Option<&Paint>, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_image(image, dst);
        });
    }

    fn render_atlas(&mut self, image: &Image, sprites: &[Rect], transforms: &[Matrix4], colors: Option<&[Color]>, _blend_mode: BlendMode, _paint: Option<&Paint>, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_atlas(image, sprites, transforms, colors);
        });
    }

    fn render_shadow(&mut self, path: &Path, color: Color, elevation: f32, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_shadow(path, color, elevation);
        });
    }

    fn render_color(&mut self, color: Color, _blend_mode: BlendMode, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            let viewport_bounds = painter.viewport_bounds();
            let paint = Paint::fill(color);
            painter.rect(viewport_bounds, &paint);
        });
    }

    fn render_vertices(&mut self, vertices: &[Point], colors: Option<&[Color]>, tex_coords: Option<&[Point]>, indices: &[u16], paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_vertices(vertices, colors, tex_coords, indices, paint);
        });
    }

    fn clip_rect(&mut self, rect: Rect, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.clip_rect(rect);
        });
    }

    fn clip_rrect(&mut self, rrect: RRect, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.clip_rrect(rrect);
        });
    }

    fn clip_path(&mut self, _path: &Path, transform: &Matrix4) {
        self.with_transform(transform, |_painter| {
            #[cfg(debug_assertions)]
            tracing::warn!("WgpuRenderer: clip_path not fully implemented");
        });
    }

    fn viewport_bounds(&self) -> Rect {
        self.painter.viewport_bounds()
    }
}
