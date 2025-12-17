//! Wgpu-based CommandRenderer implementation
//!
//! Production rendering backend executing drawing commands via GPU acceleration.

use super::commands::dispatch_command;
use super::painter::WgpuPainter;
use crate::traits::{CommandRenderer, Painter};
use flui_painting::{BlendMode, DisplayListCore, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect, Transform},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

/// wgpu backend implementation of CommandRenderer.
///
/// Note: Debug is not derived because `WgpuPainter` contains wgpu types that don't implement Debug.
#[allow(missing_debug_implementations)]
pub struct Backend {
    painter: WgpuPainter,
}

impl Backend {
    /// Create a new Backend with the given painter.
    pub fn new(painter: WgpuPainter) -> Self {
        Self { painter }
    }

    /// Get a reference to the underlying painter.
    pub fn painter(&self) -> &WgpuPainter {
        &self.painter
    }

    /// Get a mutable reference to the underlying painter.
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

impl CommandRenderer for Backend {
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

    fn render_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_arc(rect, start_angle, sweep_angle, use_center, paint);
        });
    }

    fn render_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_drrect(outer, inner, paint);
        });
    }

    fn render_points(
        &mut self,
        mode: PointMode,
        points: &[Point],
        paint: &Paint,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| match mode {
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
        });
    }

    fn render_text(
        &mut self,
        text: &str,
        offset: Offset,
        style: &TextStyle,
        _paint: &Paint,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            let font_size = style.font_size.unwrap_or(14.0) as f32;
            let color = style.color.unwrap_or(Color::BLACK);
            let paint = Paint::fill(color);
            let position = Point::new(offset.dx, offset.dy);
            painter.text_styled(text, position, font_size, &paint);
        });
    }

    fn render_text_span(
        &mut self,
        _span: &flui_types::typography::InlineSpan,
        offset: Offset,
        _text_scale_factor: f64,
        transform: &Matrix4,
    ) {
        // TODO: Implement rich text span rendering
        // For now, just log that we received a text span
        self.with_transform(transform, |_painter| {
            tracing::debug!(
                offset_x = offset.dx,
                offset_y = offset.dy,
                "render_text_span: rich text span rendering not yet implemented"
            );
        });
    }

    fn render_image(
        &mut self,
        image: &Image,
        dst: Rect,
        _paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_image(image, dst);
        });
    }

    fn render_atlas(
        &mut self,
        image: &Image,
        sprites: &[Rect],
        transforms: &[Matrix4],
        colors: Option<&[Color]>,
        _blend_mode: BlendMode,
        _paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_atlas(image, sprites, transforms, colors);
        });
    }

    fn render_image_repeat(
        &mut self,
        image: &Image,
        dst: Rect,
        repeat: flui_painting::display_list::ImageRepeat,
        _paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_image_repeat(image, dst, repeat);
        });
    }

    fn render_image_nine_slice(
        &mut self,
        image: &Image,
        center_slice: Rect,
        dst: Rect,
        _paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_image_nine_slice(image, center_slice, dst);
        });
    }

    fn render_image_filtered(
        &mut self,
        image: &Image,
        dst: Rect,
        filter: flui_painting::display_list::ColorFilter,
        _paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_image_filtered(image, dst, filter);
        });
    }

    fn render_texture(
        &mut self,
        texture_id: flui_types::painting::TextureId,
        dst: Rect,
        src: Option<Rect>,
        filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_texture(texture_id, dst, src, filter_quality, opacity);
        });
    }

    fn render_shadow(&mut self, path: &Path, color: Color, elevation: f32, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_shadow(path, color, elevation);
        });
    }

    fn render_shader_mask(
        &mut self,
        child: &flui_painting::DisplayList,
        _shader: &flui_painting::Shader,
        _bounds: Rect,
        _blend_mode: BlendMode,
        _transform: &Matrix4,
    ) {
        // TODO: Implement full shader mask rendering
        //
        // Current architecture limitation: WgpuRenderer wraps WgpuPainter which doesn't
        // have access to OffscreenRenderer (lives in GpuRenderer).
        //
        // For full implementation, we need to either:
        // 1. Pass OffscreenRenderer to WgpuRenderer constructor, OR
        // 2. Move shader mask handling to GpuRenderer level (render Layer directly), OR
        // 3. Refactor to give WgpuPainter access to GPU resources
        //
        // For now, just render child content without masking as fallback
        tracing::warn!(
            "ShaderMask rendering via DisplayList not yet fully wired - rendering child without mask"
        );

        // Render child content without masking (fallback behavior)
        for command in child.commands() {
            dispatch_command(command, self);
        }
    }

    fn render_gradient(&mut self, rect: Rect, shader: &flui_painting::Shader, transform: &Matrix4) {
        // Sample gradient center for fallback solid color until GPU gradient shader is implemented
        let color = <WgpuPainter as Painter>::sample_gradient_center(shader, rect);
        let paint = Paint::fill(color);
        self.with_transform(transform, |painter| {
            painter.rect(rect, &paint);
        });
    }

    fn render_gradient_rrect(
        &mut self,
        rrect: RRect,
        shader: &flui_painting::Shader,
        transform: &Matrix4,
    ) {
        // Sample gradient center for fallback solid color until GPU gradient shader is implemented
        let color = <WgpuPainter as Painter>::sample_gradient_center(shader, rrect.rect);
        let paint = Paint::fill(color);
        self.with_transform(transform, |painter| {
            painter.rrect(rrect, &paint);
        });
    }

    fn render_color(&mut self, color: Color, _blend_mode: BlendMode, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            let viewport_bounds = painter.viewport_bounds();
            let paint = Paint::fill(color);
            painter.rect(viewport_bounds, &paint);
        });
    }

    fn render_backdrop_filter(
        &mut self,
        child: Option<&flui_painting::DisplayList>,
        _filter: &flui_painting::display_list::ImageFilter,
        _bounds: Rect,
        _blend_mode: BlendMode,
        _transform: &Matrix4,
    ) {
        // TODO: Implement full backdrop filter rendering
        //
        // Current architecture limitation: WgpuRenderer wraps WgpuPainter which doesn't
        // have access to OffscreenRenderer (lives in GpuRenderer).
        //
        // For full implementation, we need to either:
        // 1. Capture backdrop into offscreen texture
        // 2. Apply image filter (blur, color adjustment, etc.)
        // 3. Composite filtered backdrop with optional child content
        //
        // For now, just render child content without filtering as fallback
        tracing::warn!(
            "BackdropFilter rendering not yet fully implemented - rendering child without filter"
        );

        // Render child content without filtering (fallback behavior)
        if let Some(child) = child {
            for command in child.commands() {
                dispatch_command(command, self);
            }
        }
    }

    fn render_vertices(
        &mut self,
        vertices: &[Point],
        colors: Option<&[Color]>,
        tex_coords: Option<&[Point]>,
        indices: &[u16],
        paint: &Paint,
        transform: &Matrix4,
    ) {
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

    fn clip_path(&mut self, path: &Path, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.clip_path(path);
        });
    }

    fn viewport_bounds(&self) -> Rect {
        self.painter.viewport_bounds()
    }

    fn save_layer(&mut self, bounds: Option<Rect>, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.save_layer(bounds, paint);
        });
    }

    fn restore_layer(&mut self, _transform: &Matrix4) {
        self.painter.restore_layer();
    }

    // ===== Layer Tree Operations =====

    fn push_clip_rect(&mut self, rect: &Rect, _clip_behavior: flui_types::painting::Clip) {
        self.painter.save();
        self.painter.clip_rect(*rect);
    }

    fn push_clip_rrect(&mut self, rrect: &RRect, _clip_behavior: flui_types::painting::Clip) {
        self.painter.save();
        self.painter.clip_rrect(*rrect);
    }

    fn push_clip_path(&mut self, path: &Path, _clip_behavior: flui_types::painting::Clip) {
        self.painter.save();
        self.painter.clip_path(path);
    }

    fn pop_clip(&mut self) {
        self.painter.restore();
    }

    fn push_offset(&mut self, offset: Offset) {
        self.painter.save();
        self.painter.translate(offset);
    }

    fn push_transform(&mut self, transform: &Matrix4) {
        self.painter.save();

        // Decompose and apply transform components
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
    }

    fn pop_transform(&mut self) {
        self.painter.restore();
    }

    fn push_opacity(&mut self, alpha: f32) {
        // Create a layer with opacity
        let alpha_u8 = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let paint = Paint::fill(Color::WHITE).with_alpha(alpha_u8);
        self.painter.save_layer(None, &paint);
    }

    fn pop_opacity(&mut self) {
        self.painter.restore_layer();
    }

    fn push_color_filter(&mut self, _filter: &flui_types::painting::ColorMatrix) {
        // TODO: Implement color filter via GPU shader
        // For now, save state to maintain push/pop balance
        self.painter.save();
        tracing::trace!("push_color_filter: GPU color matrix filter not yet implemented");
    }

    fn pop_color_filter(&mut self) {
        self.painter.restore();
    }

    fn push_image_filter(&mut self, filter: &flui_painting::display_list::ImageFilter) {
        // TODO: Implement image filter via GPU compute shader
        // For now, save state to maintain push/pop balance
        self.painter.save();
        tracing::trace!(
            "push_image_filter: GPU image filter not yet implemented - filter={:?}",
            filter
        );
    }

    fn pop_image_filter(&mut self) {
        self.painter.restore();
    }

    fn add_performance_overlay(
        &mut self,
        options_mask: u32,
        bounds: Rect,
        fps: f32,
        frame_time_ms: f32,
        total_frames: u64,
    ) {
        use flui_layer::PerformanceOverlayOption;

        let _options = PerformanceOverlayOption::from_mask(options_mask);

        // Semi-transparent dark background (MangoHud style)
        let bg_color = Color::rgba(10, 10, 15, 200);
        let bg_paint = Paint::fill(bg_color);
        let bg_rrect =
            RRect::from_rect_and_radius(bounds, flui_types::geometry::Radius::circular(4.0));
        self.painter.rrect(bg_rrect, &bg_paint);

        let x = bounds.left() + 8.0;
        let x_val = bounds.left() + 50.0;
        let mut y = bounds.top() + 14.0;

        // GPU label (cyan) + FPS value
        let cyan = Color::rgba(0, 200, 200, 255);
        self.painter
            .text("GPU", Point::new(x, y), 11.0, &Paint::fill(cyan));

        // FPS with color coding
        let fps_color = if fps >= 55.0 {
            Color::rgba(170, 255, 170, 255) // Light green
        } else if fps >= 30.0 {
            Color::rgba(255, 255, 130, 255) // Light yellow
        } else {
            Color::rgba(255, 130, 130, 255) // Light red
        };
        self.painter.text(
            &format!("{:.0}", fps),
            Point::new(x_val, y),
            11.0,
            &Paint::fill(fps_color),
        );

        // FPS unit (dimmer)
        let gray = Color::rgba(130, 130, 130, 255);
        let fps_w = if fps >= 100.0 {
            24.0
        } else if fps >= 10.0 {
            16.0
        } else {
            8.0
        };
        self.painter
            .text("FPS", Point::new(x_val + fps_w, y), 8.0, &Paint::fill(gray));
        y += 14.0;

        // Frametime label (purple) + value
        let purple = Color::rgba(200, 100, 255, 255);
        self.painter
            .text("Frame", Point::new(x, y), 10.0, &Paint::fill(purple));

        let white = Color::rgba(220, 220, 220, 255);
        self.painter.text(
            &format!("{:.1}", frame_time_ms),
            Point::new(x_val, y),
            10.0,
            &Paint::fill(white),
        );
        self.painter
            .text("ms", Point::new(x_val + 22.0, y), 8.0, &Paint::fill(gray));

        let _ = total_frames;
    }
}
