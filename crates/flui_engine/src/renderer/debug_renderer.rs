//! Debug renderer - logs commands and validates state
//!
//! Useful for development, debugging, and testing rendering without GPU.

use super::command_renderer::CommandRenderer;
use flui_painting::{BlendMode, DisplayListCore, Paint, PointMode};
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
        Self {
            viewport,
            command_count: 0,
        }
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
    fn render_rect(&mut self, rect: Rect, paint: &Paint, _transform: &Matrix4) {
        self.log_command(
            "render_rect",
            &format!("rect={:?}, paint={:?}", rect, paint),
        );
    }

    fn render_rrect(&mut self, rrect: RRect, _paint: &Paint, _transform: &Matrix4) {
        self.log_command("render_rrect", &format!("rrect={:?}", rrect));
    }

    fn render_circle(&mut self, center: Point, radius: f32, _paint: &Paint, _transform: &Matrix4) {
        self.log_command(
            "render_circle",
            &format!("center={:?}, radius={}", center, radius),
        );
    }

    fn render_oval(&mut self, rect: Rect, _paint: &Paint, _transform: &Matrix4) {
        self.log_command("render_oval", &format!("rect={:?}", rect));
    }

    fn render_line(&mut self, p1: Point, p2: Point, _paint: &Paint, _transform: &Matrix4) {
        self.log_command("render_line", &format!("p1={:?}, p2={:?}", p1, p2));
    }

    fn render_path(&mut self, path: &Path, _paint: &Paint, _transform: &Matrix4) {
        self.log_command(
            "render_path",
            &format!("commands={}", path.commands().len()),
        );
    }

    fn render_arc(
        &mut self,
        _rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        _use_center: bool,
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_arc",
            &format!("start={}, sweep={}", start_angle, sweep_angle),
        );
    }

    fn render_drrect(
        &mut self,
        _outer: RRect,
        _inner: RRect,
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command("render_drrect", "double rounded rect");
    }

    fn render_points(
        &mut self,
        mode: PointMode,
        points: &[Point],
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_points",
            &format!("mode={:?}, count={}", mode, points.len()),
        );
    }

    fn render_text(
        &mut self,
        text: &str,
        offset: Offset,
        _style: &TextStyle,
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_text",
            &format!("text='{}', offset={:?}", text, offset),
        );
    }

    fn render_image(
        &mut self,
        _image: &Image,
        dst: Rect,
        _paint: Option<&Paint>,
        _transform: &Matrix4,
    ) {
        self.log_command("render_image", &format!("dst={:?}", dst));
    }

    fn render_atlas(
        &mut self,
        _image: &Image,
        sprites: &[Rect],
        _transforms: &[Matrix4],
        _colors: Option<&[Color]>,
        _blend_mode: BlendMode,
        _paint: Option<&Paint>,
        _transform: &Matrix4,
    ) {
        self.log_command("render_atlas", &format!("sprites={}", sprites.len()));
    }

    fn render_image_repeat(
        &mut self,
        _image: &Image,
        dst: Rect,
        repeat: flui_painting::display_list::ImageRepeat,
        _paint: Option<&Paint>,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_image_repeat",
            &format!("dst={:?}, repeat={:?}", dst, repeat),
        );
    }

    fn render_image_nine_slice(
        &mut self,
        _image: &Image,
        center_slice: Rect,
        dst: Rect,
        _paint: Option<&Paint>,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_image_nine_slice",
            &format!("center_slice={:?}, dst={:?}", center_slice, dst),
        );
    }

    fn render_image_filtered(
        &mut self,
        _image: &Image,
        dst: Rect,
        filter: flui_painting::display_list::ColorFilter,
        _paint: Option<&Paint>,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_image_filtered",
            &format!("dst={:?}, filter={:?}", dst, filter),
        );
    }

    fn render_texture(
        &mut self,
        texture_id: flui_types::painting::TextureId,
        dst: Rect,
        src: Option<Rect>,
        filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_texture",
            &format!(
                "texture_id={}, dst={:?}, src={:?}, filter={:?}, opacity={}",
                texture_id.get(),
                dst,
                src,
                filter_quality,
                opacity
            ),
        );
    }

    fn render_shadow(&mut self, _path: &Path, color: Color, elevation: f32, _transform: &Matrix4) {
        self.log_command(
            "render_shadow",
            &format!("color={:?}, elevation={}", color, elevation),
        );
    }

    fn render_shader_mask(
        &mut self,
        child: &flui_painting::DisplayList,
        shader: &flui_painting::Shader,
        bounds: Rect,
        blend_mode: BlendMode,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_shader_mask",
            &format!(
                "shader={:?}, bounds={:?}, blend_mode={:?}, child_commands={}",
                shader,
                bounds,
                blend_mode,
                child.commands().count()
            ),
        );
    }

    fn render_gradient(
        &mut self,
        rect: Rect,
        shader: &flui_painting::Shader,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_gradient",
            &format!("rect={:?}, shader={:?}", rect, shader),
        );
    }

    fn render_gradient_rrect(
        &mut self,
        rrect: RRect,
        shader: &flui_painting::Shader,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_gradient_rrect",
            &format!("rrect={:?}, shader={:?}", rrect, shader),
        );
    }

    fn render_color(&mut self, color: Color, _blend_mode: BlendMode, _transform: &Matrix4) {
        self.log_command("render_color", &format!("color={:?}", color));
    }

    fn render_backdrop_filter(
        &mut self,
        child: Option<&flui_painting::DisplayList>,
        filter: &flui_painting::display_list::ImageFilter,
        bounds: Rect,
        blend_mode: BlendMode,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_backdrop_filter",
            &format!(
                "filter={:?}, bounds={:?}, blend_mode={:?}, child_commands={}",
                filter,
                bounds,
                blend_mode,
                child.map(|c| c.commands().count()).unwrap_or(0)
            ),
        );
    }

    fn render_vertices(
        &mut self,
        vertices: &[Point],
        _colors: Option<&[Color]>,
        _tex_coords: Option<&[Point]>,
        indices: &[u16],
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_vertices",
            &format!("vertices={}, indices={}", vertices.len(), indices.len()),
        );
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

    fn save_layer(&mut self, bounds: Option<Rect>, paint: &Paint, _transform: &Matrix4) {
        self.log_command(
            "save_layer",
            &format!("bounds={:?}, paint={:?}", bounds, paint),
        );
    }

    fn restore_layer(&mut self, _transform: &Matrix4) {
        self.log_command("restore_layer", "");
    }
}
