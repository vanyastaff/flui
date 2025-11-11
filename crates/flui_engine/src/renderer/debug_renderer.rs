//! Debug renderer - logs commands and validates state
//!
//! Useful for development, debugging, and testing rendering without GPU.

use super::command_renderer::CommandRenderer;
use flui_painting::{BlendMode, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

/// Debug renderer that logs all commands to tracing
pub struct DebugRenderer {
    viewport: Rect,
    command_count: usize,
}

impl DebugRenderer {
    pub fn new(viewport: Rect) -> Self {
        Self { viewport, command_count: 0 }
    }

    pub fn command_count(&self) -> usize {
        self.command_count
    }

    fn log_command(&mut self, name: &str, details: &str) {
        self.command_count += 1;
        tracing::debug!("[{}] {}: {}", self.command_count, name, details);
    }
}

impl CommandRenderer for DebugRenderer {
    fn render_rect(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_rect", &format!("rect={:?}, paint={:?}", rect, paint));
    }

    fn render_rrect(&mut self, rrect: RRect, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_rrect", &format!("rrect={:?}", rrect));
    }

    fn render_circle(&mut self, center: Point, radius: f32, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_circle", &format!("center={:?}, radius={}", center, radius));
    }

    fn render_oval(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_oval", &format!("rect={:?}", rect));
    }

    fn render_line(&mut self, p1: Point, p2: Point, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_line", &format!("p1={:?}, p2={:?}", p1, p2));
    }

    fn render_path(&mut self, path: &Path, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_path", &format!("commands={}", path.commands().len()));
    }

    fn render_arc(&mut self, rect: Rect, start_angle: f32, sweep_angle: f32, use_center: bool, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_arc", &format!("start={}, sweep={}", start_angle, sweep_angle));
    }

    fn render_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_drrect", "double rounded rect");
    }

    fn render_points(&mut self, mode: PointMode, points: &[Point], paint: &Paint, transform: &Matrix4) {
        self.log_command("render_points", &format!("mode={:?}, count={}", mode, points.len()));
    }

    fn render_text(&mut self, text: &str, offset: Offset, style: &TextStyle, paint: &Paint, transform: &Matrix4) {
        self.log_command("render_text", &format!("text='{}', offset={:?}", text, offset));
    }

    fn render_image(&mut self, image: &Image, dst: Rect, paint: Option<&Paint>, transform: &Matrix4) {
        self.log_command("render_image", &format!("dst={:?}", dst));
    }

    fn render_atlas(&mut self, image: &Image, sprites: &[Rect], transforms: &[Matrix4], colors: Option<&[Color]>, blend_mode: BlendMode, paint: Option<&Paint>, transform: &Matrix4) {
        self.log_command("render_atlas", &format!("sprites={}", sprites.len()));
    }

    fn render_shadow(&mut self, path: &Path, color: Color, elevation: f32, transform: &Matrix4) {
        self.log_command("render_shadow", &format!("color={:?}, elevation={}", color, elevation));
    }

    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4) {
        self.log_command("render_color", &format!("color={:?}", color));
    }

    fn render_vertices(&mut self, vertices: &[Point], colors: Option<&[Color]>, tex_coords: Option<&[Point]>, indices: &[u16], paint: &Paint, transform: &Matrix4) {
        self.log_command("render_vertices", &format!("vertices={}, indices={}", vertices.len(), indices.len()));
    }

    fn clip_rect(&mut self, rect: Rect, _transform: &Matrix4) {
        self.log_command("clip_rect", &format!("rect={:?}", rect));
    }

    fn clip_rrect(&mut self, rrect: RRect, _transform: &Matrix4) {
        self.log_command("clip_rrect", &format!("rrect={:?}", rrect));
    }

    fn clip_path(&mut self, path: &Path, _transform: &Matrix4) {
        self.log_command("clip_path", &format!("commands={}", path.commands().len()));
    }

    fn viewport_bounds(&self) -> Rect {
        self.viewport
    }
}
