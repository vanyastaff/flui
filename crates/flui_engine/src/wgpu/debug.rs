//! Debug renderer - logs commands and validates state
//!
//! Useful for development, debugging, and testing rendering without GPU.

use super::commands::CommandRenderer;
use flui_painting::{BlendMode, DisplayListCore, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect, Pixels},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

/// Debug backend that logs all commands to tracing.
#[derive(Debug)]
pub struct DebugBackend {
    viewport: Rect<Pixels>,
    command_count: usize,
}

impl DebugBackend {
    /// Create a new debug backend with the given viewport.
    pub fn new(viewport: Rect<Pixels>) -> Self {
        Self {
            viewport,
            command_count: 0,
        }
    }

    /// Get the total number of commands processed.
    pub fn command_count(&self) -> usize {
        self.command_count
    }

    fn log_command(&mut self, name: &str, details: &str) {
        self.command_count += 1;
        tracing::trace!("[{}] {}: {}", self.command_count, name, details);
    }
}

impl CommandRenderer for DebugBackend {
    fn render_rect(&mut self, rect: Rect<Pixels>, paint: &Paint, _transform: &Matrix4) {
        self.log_command(
            "render_rect",
            &format!("rect={:?}, paint={:?}", rect, paint),
        );
    }

    fn render_rrect(&mut self, rrect: RRect<Pixels>, _paint: &Paint, _transform: &Matrix4) {
        self.log_command("render_rrect", &format!("rrect={:?}", rrect));
    }

    fn render_circle(&mut self, center: Point<Pixels>, radius: f32, _paint: &Paint, _transform: &Matrix4) {
        self.log_command(
            "render_circle",
            &format!("center={:?}, radius={}", center, radius),
        );
    }

    fn render_oval(&mut self, rect: Rect<Pixels>, _paint: &Paint, _transform: &Matrix4) {
        self.log_command("render_oval", &format!("rect={:?}", rect));
    }

    fn render_line(&mut self, p1: Point<Pixels>, p2: Point<Pixels>, _paint: &Paint, _transform: &Matrix4) {
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
        _rect: Rect<Pixels>,
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
        _outer: RRect<Pixels>,
        _inner: RRect<Pixels>,
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command("render_drrect", "double rounded rect");
    }

    fn render_points(
        &mut self,
        mode: PointMode,
        points: &[Point<Pixels>],
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
        offset: Offset<Pixels>,
        _style: &TextStyle,
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_text",
            &format!("text='{}', offset={:?}", text, offset),
        );
    }

    fn render_text_span(
        &mut self,
        _span: &flui_types::typography::InlineSpan,
        offset: Offset<Pixels>,
        text_scale_factor: f64,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_text_span",
            &format!("offset={:?}, scale={}", offset, text_scale_factor),
        );
    }

    fn render_image(
        &mut self,
        _image: &Image,
        dst: Rect<Pixels>,
        _paint: Option<&Paint>,
        _transform: &Matrix4,
    ) {
        self.log_command("render_image", &format!("dst={:?}", dst));
    }

    fn render_atlas(
        &mut self,
        _image: &Image,
        sprites: &[Rect<Pixels>],
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
        dst: Rect<Pixels>,
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
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
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
        dst: Rect<Pixels>,
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
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
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
        bounds: Rect<Pixels>,
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
        rect: Rect<Pixels>,
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
        rrect: RRect<Pixels>,
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
        bounds: Rect<Pixels>,
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
        vertices: &[Point<Pixels>],
        _colors: Option<&[Color]>,
        _tex_coords: Option<&[Point<Pixels>]>,
        indices: &[u16],
        _paint: &Paint,
        _transform: &Matrix4,
    ) {
        self.log_command(
            "render_vertices",
            &format!("vertices={}, indices={}", vertices.len(), indices.len()),
        );
    }

    fn clip_rect(&mut self, rect: Rect<Pixels>, _transform: &Matrix4) {
        self.log_command("clip_rect", &format!("rect={:?}", rect));
    }

    fn clip_rrect(&mut self, rrect: RRect<Pixels>, _transform: &Matrix4) {
        self.log_command("clip_rrect", &format!("rrect={:?}", rrect));
    }

    fn clip_path(&mut self, path: &Path, _transform: &Matrix4) {
        self.log_command("clip_path", &format!("commands={}", path.commands().len()));
    }

    fn viewport_bounds(&self) -> Rect {
        self.viewport
    }

    fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint, _transform: &Matrix4) {
        self.log_command(
            "save_layer",
            &format!("bounds={:?}, paint={:?}", bounds, paint),
        );
    }

    fn restore_layer(&mut self, _transform: &Matrix4) {
        self.log_command("restore_layer", "");
    }

    // ===== Layer Tree Operations =====

    fn push_clip_rect(&mut self, rect: &Rect, clip_behavior: flui_types::painting::Clip) {
        self.log_command(
            "push_clip_rect",
            &format!("rect={:?}, behavior={:?}", rect, clip_behavior),
        );
    }

    fn push_clip_rrect(&mut self, rrect: &RRect, clip_behavior: flui_types::painting::Clip) {
        self.log_command(
            "push_clip_rrect",
            &format!("rrect={:?}, behavior={:?}", rrect, clip_behavior),
        );
    }

    fn push_clip_path(&mut self, path: &Path, clip_behavior: flui_types::painting::Clip) {
        self.log_command(
            "push_clip_path",
            &format!(
                "commands={}, behavior={:?}",
                path.commands().len(),
                clip_behavior
            ),
        );
    }

    fn pop_clip(&mut self) {
        self.log_command("pop_clip", "");
    }

    fn push_offset(&mut self, offset: Offset<Pixels>) {
        self.log_command("push_offset", &format!("offset={:?}", offset));
    }

    fn push_transform(&mut self, transform: &Matrix4) {
        self.log_command("push_transform", &format!("transform={:?}", transform));
    }

    fn pop_transform(&mut self) {
        self.log_command("pop_transform", "");
    }

    fn push_opacity(&mut self, alpha: f32) {
        self.log_command("push_opacity", &format!("alpha={}", alpha));
    }

    fn pop_opacity(&mut self) {
        self.log_command("pop_opacity", "");
    }

    fn push_color_filter(&mut self, filter: &flui_types::painting::ColorMatrix) {
        self.log_command("push_color_filter", &format!("filter={:?}", filter));
    }

    fn pop_color_filter(&mut self) {
        self.log_command("pop_color_filter", "");
    }

    fn push_image_filter(&mut self, filter: &flui_painting::display_list::ImageFilter) {
        self.log_command("push_image_filter", &format!("filter={:?}", filter));
    }

    fn pop_image_filter(&mut self) {
        self.log_command("pop_image_filter", "");
    }

    fn add_performance_overlay(
        &mut self,
        options_mask: u32,
        bounds: Rect<Pixels>,
        fps: f32,
        frame_time_ms: f32,
        total_frames: u64,
    ) {
        self.log_command(
            "add_performance_overlay",
            &format!(
                "options_mask={}, bounds={:?}, fps={:.1}, frame_time={:.2}ms, total_frames={}",
                options_mask, bounds, fps, frame_time_ms, total_frames
            ),
        );
    }
}
