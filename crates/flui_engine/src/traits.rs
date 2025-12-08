//! Abstract rendering traits
//!
//! This module defines the abstract traits that rendering backends must implement.
//! These traits enable multiple backend implementations (wgpu, skia, vello, software)
//! without changing the high-level rendering code.
//!
//! # Design Principles
//!
//! - **Backend Agnostic**: Traits define what to render, not how
//! - **Dependency Inversion**: High-level code depends on abstractions (SOLID)
//! - **Extensible**: New backends implement these traits

use flui_painting::{BlendMode, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect},
    painting::{Image, Path, TextureId},
    styling::Color,
    typography::TextStyle,
};

// ============================================================================
// COMMAND RENDERER TRAIT
// ============================================================================

/// Visitor interface for rendering DrawCommands
///
/// Backends implement this trait to provide concrete rendering logic.
/// Each method corresponds to one DrawCommand variant.
///
/// This trait enables:
/// - Multiple rendering backends without changing DisplayList
/// - Type-safe dispatch without giant match statements
/// - Easy testing via TestRenderer implementation
///
/// # Example
///
/// ```rust,ignore
/// pub struct WgpuBackend { /* ... */ }
///
/// impl CommandRenderer for WgpuBackend {
///     fn render_rect(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4) {
///         self.with_transform(transform, |painter| {
///             painter.rect(rect, paint);
///         });
///     }
///     // ... other methods
/// }
/// ```
pub trait CommandRenderer {
    // ===== Primitive Shapes =====

    /// Render a filled or stroked rectangle
    fn render_rect(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4);

    /// Render a rounded rectangle
    fn render_rrect(&mut self, rrect: RRect, paint: &Paint, transform: &Matrix4);

    /// Render a circle
    fn render_circle(&mut self, center: Point, radius: f32, paint: &Paint, transform: &Matrix4);

    /// Render an oval (ellipse)
    fn render_oval(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4);

    /// Render a line segment
    fn render_line(&mut self, p1: Point, p2: Point, paint: &Paint, transform: &Matrix4);

    /// Render an arbitrary path
    fn render_path(&mut self, path: &Path, paint: &Paint, transform: &Matrix4);

    // ===== Advanced Shapes =====

    /// Render an arc segment
    fn render_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
        transform: &Matrix4,
    );

    /// Render a double rounded rectangle (ring/border)
    fn render_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint, transform: &Matrix4);

    /// Render a set of points
    fn render_points(
        &mut self,
        mode: PointMode,
        points: &[Point],
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Text =====

    /// Render text with given style
    fn render_text(
        &mut self,
        text: &str,
        offset: Offset,
        style: &TextStyle,
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Images =====

    /// Render an image to destination rectangle
    fn render_image(
        &mut self,
        image: &Image,
        dst: Rect,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render a texture atlas with sprites
    #[allow(clippy::too_many_arguments)]
    fn render_atlas(
        &mut self,
        image: &Image,
        sprites: &[Rect],
        transforms: &[Matrix4],
        colors: Option<&[Color]>,
        blend_mode: BlendMode,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with repeat/tiling
    fn render_image_repeat(
        &mut self,
        image: &Image,
        dst: Rect,
        repeat: flui_painting::display_list::ImageRepeat,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with 9-slice/9-patch scaling
    fn render_image_nine_slice(
        &mut self,
        image: &Image,
        center_slice: Rect,
        dst: Rect,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with a color filter applied
    fn render_image_filtered(
        &mut self,
        image: &Image,
        dst: Rect,
        filter: flui_painting::display_list::ColorFilter,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render a GPU texture referenced by ID
    fn render_texture(
        &mut self,
        texture_id: TextureId,
        dst: Rect,
        src: Option<Rect>,
        filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
        transform: &Matrix4,
    );

    // ===== Effects =====

    /// Render a shadow for a path
    fn render_shadow(&mut self, path: &Path, color: Color, elevation: f32, transform: &Matrix4);

    /// Apply a shader as a mask to child content
    fn render_shader_mask(
        &mut self,
        child: &flui_painting::DisplayList,
        shader: &flui_painting::Shader,
        bounds: Rect,
        blend_mode: BlendMode,
        transform: &Matrix4,
    );

    // ===== Gradients =====

    /// Render a gradient-filled rectangle
    fn render_gradient(&mut self, rect: Rect, shader: &flui_painting::Shader, transform: &Matrix4);

    /// Render a gradient-filled rounded rectangle
    fn render_gradient_rrect(
        &mut self,
        rrect: RRect,
        shader: &flui_painting::Shader,
        transform: &Matrix4,
    );

    /// Fill entire viewport with color
    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4);

    /// Render backdrop filter effect (blur, color adjustments, etc.)
    fn render_backdrop_filter(
        &mut self,
        child: Option<&flui_painting::DisplayList>,
        filter: &flui_painting::display_list::ImageFilter,
        bounds: Rect,
        blend_mode: BlendMode,
        transform: &Matrix4,
    );

    // ===== Custom Geometry =====

    /// Render custom vertex geometry
    fn render_vertices(
        &mut self,
        vertices: &[Point],
        colors: Option<&[Color]>,
        tex_coords: Option<&[Point]>,
        indices: &[u16],
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Clipping =====

    /// Set rectangular clip region
    fn clip_rect(&mut self, rect: Rect, transform: &Matrix4);

    /// Set rounded rectangular clip region
    fn clip_rrect(&mut self, rrect: RRect, transform: &Matrix4);

    /// Set arbitrary path clip region
    fn clip_path(&mut self, path: &Path, transform: &Matrix4);

    // ===== Viewport Information =====

    /// Get the viewport bounds
    fn viewport_bounds(&self) -> Rect;

    // ===== Layer Operations =====

    /// Save canvas state and create a new compositing layer
    fn save_layer(&mut self, bounds: Option<Rect>, paint: &Paint, transform: &Matrix4);

    /// Restore canvas state and composite the saved layer
    fn restore_layer(&mut self, transform: &Matrix4);

    // ===== Layer Tree Operations =====

    /// Push a rectangular clip onto the clip stack
    fn push_clip_rect(&mut self, rect: &Rect, clip_behavior: flui_types::painting::Clip);

    /// Push a rounded rectangular clip onto the clip stack
    fn push_clip_rrect(&mut self, rrect: &RRect, clip_behavior: flui_types::painting::Clip);

    /// Push an arbitrary path clip onto the clip stack
    fn push_clip_path(&mut self, path: &Path, clip_behavior: flui_types::painting::Clip);

    /// Pop the most recent clip from the clip stack
    fn pop_clip(&mut self);

    /// Push a translation offset onto the transform stack
    fn push_offset(&mut self, offset: Offset);

    /// Push a full matrix transformation onto the transform stack
    fn push_transform(&mut self, transform: &Matrix4);

    /// Pop the most recent transform from the transform stack
    fn pop_transform(&mut self);

    /// Push an opacity value onto the effect stack
    fn push_opacity(&mut self, alpha: f32);

    /// Pop the most recent opacity from the effect stack
    fn pop_opacity(&mut self);

    /// Push a color filter onto the effect stack
    fn push_color_filter(&mut self, filter: &flui_types::painting::ColorMatrix);

    /// Pop the most recent color filter from the effect stack
    fn pop_color_filter(&mut self);

    /// Push an image filter onto the effect stack
    fn push_image_filter(&mut self, filter: &flui_painting::display_list::ImageFilter);

    /// Pop the most recent image filter from the effect stack
    fn pop_image_filter(&mut self);
}

// ============================================================================
// PAINTER TRAIT
// ============================================================================

/// Abstract 2D drawing interface
///
/// Provides a high-level API for common drawing operations. Backends implement
/// this trait to provide hardware-accelerated or software rendering.
///
/// # Example
///
/// ```rust,ignore
/// fn draw_button(painter: &mut impl Painter, rect: Rect, color: Color) {
///     let paint = Paint::fill(color);
///     painter.rect(rect, &paint);
/// }
/// ```
pub trait Painter {
    // ===== Core Drawing Methods =====

    /// Draw a filled or stroked rectangle
    fn rect(&mut self, rect: Rect, paint: &Paint);

    /// Draw a rounded rectangle
    fn rrect(&mut self, rrect: RRect, paint: &Paint);

    /// Draw a circle
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint);

    /// Draw a line segment
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint);

    /// Draw text at a position
    fn text(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint);

    /// Draw a texture by ID (abstract platform-independent ID)
    fn texture(&mut self, texture_id: TextureId, dst_rect: Rect);

    // ===== Transform Stack =====

    /// Save the current canvas state
    fn save(&mut self);

    /// Restore the previously saved canvas state
    fn restore(&mut self);

    /// Apply a translation offset
    fn translate(&mut self, offset: Offset);

    /// Apply a rotation (in radians)
    fn rotate(&mut self, angle: f32);

    /// Apply a scale transformation
    fn scale(&mut self, sx: f32, sy: f32);

    // ===== Clipping =====

    /// Set rectangular clip region
    fn clip_rect(&mut self, rect: Rect);

    /// Set rounded rectangular clip region
    fn clip_rrect(&mut self, rrect: RRect);

    /// Set arbitrary path clip region
    fn clip_path(&mut self, path: &Path);

    // ===== Viewport Information =====

    /// Get the viewport bounds
    fn viewport_bounds(&self) -> Rect;

    // ===== Gradient Helpers =====

    /// Sample a gradient shader at the center of a rect to get a representative color
    ///
    /// This is a fallback for when full GPU gradient rendering is not available.
    fn sample_gradient_center(shader: &flui_painting::Shader, rect: Rect) -> Color
    where
        Self: Sized,
    {
        use flui_painting::Shader;

        match shader {
            Shader::LinearGradient {
                from,
                to,
                colors,
                stops,
                ..
            } => {
                if colors.is_empty() {
                    return Color::TRANSPARENT;
                }
                if colors.len() == 1 {
                    return colors[0];
                }

                let center_x = rect.left() + rect.width() / 2.0;
                let center_y = rect.top() + rect.height() / 2.0;

                let dx = to.dx - from.dx;
                let dy = to.dy - from.dy;
                let len_sq = dx * dx + dy * dy;

                let t = if len_sq > f32::EPSILON {
                    let px = center_x - from.dx;
                    let py = center_y - from.dy;
                    ((px * dx + py * dy) / len_sq).clamp(0.0, 1.0)
                } else {
                    0.5
                };

                Self::interpolate_gradient_color(colors, stops.as_deref(), t)
            }
            Shader::RadialGradient {
                center,
                radius,
                colors,
                stops,
                ..
            } => {
                if colors.is_empty() {
                    return Color::TRANSPARENT;
                }
                if colors.len() == 1 {
                    return colors[0];
                }

                let rect_center_x = rect.left() + rect.width() / 2.0;
                let rect_center_y = rect.top() + rect.height() / 2.0;

                let dx = rect_center_x - center.dx;
                let dy = rect_center_y - center.dy;
                let dist = (dx * dx + dy * dy).sqrt();

                let t = if *radius > f32::EPSILON {
                    (dist / radius).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                Self::interpolate_gradient_color(colors, stops.as_deref(), t)
            }
            Shader::SweepGradient {
                center,
                colors,
                stops,
                start_angle,
                end_angle,
                ..
            } => {
                if colors.is_empty() {
                    return Color::TRANSPARENT;
                }
                if colors.len() == 1 {
                    return colors[0];
                }

                let rect_center_x = rect.left() + rect.width() / 2.0;
                let rect_center_y = rect.top() + rect.height() / 2.0;

                let dx = rect_center_x - center.dx;
                let dy = rect_center_y - center.dy;
                let angle = dy.atan2(dx);

                let angle_range = end_angle - start_angle;
                let t = if angle_range.abs() > f32::EPSILON {
                    ((angle - start_angle) / angle_range).clamp(0.0, 1.0)
                } else {
                    0.5
                };

                Self::interpolate_gradient_color(colors, stops.as_deref(), t)
            }
            Shader::Image(_) => Color::WHITE,
            _ => Color::WHITE,
        }
    }

    /// Interpolate between gradient colors at a given t value
    fn interpolate_gradient_color(colors: &[Color], stops: Option<&[f32]>, t: f32) -> Color
    where
        Self: Sized,
    {
        if colors.is_empty() {
            return Color::TRANSPARENT;
        }
        if colors.len() == 1 {
            return colors[0];
        }

        let default_stops: Vec<f32> = (0..colors.len())
            .map(|i| i as f32 / (colors.len() - 1) as f32)
            .collect();
        let stops = stops.unwrap_or(&default_stops);

        let mut idx = 0;
        for (i, &stop) in stops.iter().enumerate() {
            if t <= stop {
                idx = i;
                break;
            }
            idx = i;
        }

        if idx == 0 {
            return colors[0];
        }

        let prev_stop = stops[idx - 1];
        let next_stop = stops[idx];
        let local_t = if (next_stop - prev_stop).abs() > f32::EPSILON {
            (t - prev_stop) / (next_stop - prev_stop)
        } else {
            0.5
        };

        let c1 = &colors[idx - 1];
        let c2 = &colors[idx.min(colors.len() - 1)];

        Color::rgba(
            (c1.r as f32 + (c2.r as f32 - c1.r as f32) * local_t) as u8,
            (c1.g as f32 + (c2.g as f32 - c1.g as f32) * local_t) as u8,
            (c1.b as f32 + (c2.b as f32 - c1.b as f32) * local_t) as u8,
            (c1.a as f32 + (c2.a as f32 - c1.a as f32) * local_t) as u8,
        )
    }

    // ===== Advanced Methods with Default Implementations =====

    /// Save canvas state for backdrop capture
    fn save_layer_backdrop(&mut self) {
        self.save();
    }

    /// Draw an arbitrary path
    fn draw_path(&mut self, _path: &Path, _paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_path: not implemented");
    }

    /// Draw an oval (ellipse)
    fn oval(&mut self, _rect: Rect, _paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::oval: not implemented");
    }

    /// Draw an arc
    fn draw_arc(
        &mut self,
        _rect: Rect,
        _start_angle: f32,
        _sweep_angle: f32,
        _use_center: bool,
        _paint: &Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_arc: not implemented");
    }

    /// Draw a double rounded rectangle (ring/border)
    fn draw_drrect(&mut self, _outer: RRect, _inner: RRect, _paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_drrect: not implemented");
    }

    /// Draw a shadow for a path
    fn draw_shadow(&mut self, _path: &Path, _color: Color, _elevation: f32) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_shadow: not implemented");
    }

    /// Draw custom vertex geometry
    fn draw_vertices(
        &mut self,
        _vertices: &[Point],
        _colors: Option<&[Color]>,
        _tex_coords: Option<&[Point]>,
        _indices: &[u16],
        _paint: &Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_vertices: not implemented");
    }

    /// Draw a texture atlas
    fn draw_atlas(
        &mut self,
        _image: &Image,
        _sprites: &[Rect],
        _transforms: &[Matrix4],
        _colors: Option<&[Color]>,
    ) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_atlas: not implemented");
    }

    /// Draw text with style
    fn text_styled(&mut self, text: &str, position: Point, font_size: f32, paint: &Paint) {
        self.text(text, position, font_size, paint);
    }

    /// Draw an image
    fn draw_image(&mut self, _image: &Image, _dst_rect: Rect) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_image: not implemented");
    }

    /// Draw a GPU texture by ID with full options
    fn draw_texture(
        &mut self,
        _texture_id: TextureId,
        _dst: Rect,
        _src: Option<Rect>,
        _filter_quality: flui_types::painting::FilterQuality,
        _opacity: f32,
    ) {
        #[cfg(debug_assertions)]
        tracing::warn!("Painter::draw_texture: not implemented");
    }

    /// Draw text with shadow
    #[allow(clippy::too_many_arguments)]
    fn text_with_shadow(
        &mut self,
        text: &str,
        position: Point,
        font_size: f32,
        paint: &Paint,
        _shadow_offset: Offset,
        _shadow_blur: f32,
        _shadow_color: Color,
    ) {
        self.text(text, position, font_size, paint);
    }

    /// Draw rounded rect with shadow
    fn rrect_with_shadow(
        &mut self,
        rrect: RRect,
        paint: &Paint,
        _shadow_offset: Offset,
        _shadow_blur: f32,
        _shadow_color: Color,
    ) {
        self.rrect(rrect, paint);
    }

    /// Draw rect with shadow
    fn rect_with_shadow(
        &mut self,
        rect: Rect,
        paint: &Paint,
        _shadow_offset: Offset,
        _shadow_blur: f32,
        _shadow_color: Color,
    ) {
        self.rect(rect, paint);
    }

    // ===== Image Extensions =====

    /// Draw an image with repeat/tiling
    fn draw_image_repeat(
        &mut self,
        image: &Image,
        dst: Rect,
        _repeat: flui_painting::display_list::ImageRepeat,
    ) {
        self.draw_image(image, dst);
        #[cfg(debug_assertions)]
        tracing::debug!("Painter::draw_image_repeat: using fallback (no tiling)");
    }

    /// Draw an image with 9-slice/9-patch scaling
    fn draw_image_nine_slice(&mut self, image: &Image, _center_slice: Rect, dst: Rect) {
        self.draw_image(image, dst);
        #[cfg(debug_assertions)]
        tracing::debug!("Painter::draw_image_nine_slice: using fallback (no 9-slice)");
    }

    /// Draw an image with a color filter
    fn draw_image_filtered(
        &mut self,
        image: &Image,
        dst: Rect,
        _filter: flui_painting::display_list::ColorFilter,
    ) {
        self.draw_image(image, dst);
        #[cfg(debug_assertions)]
        tracing::debug!("Painter::draw_image_filtered: using fallback (no filter)");
    }

    // ===== Layer Operations =====

    /// Save canvas state and create a compositing layer
    fn save_layer(&mut self, _bounds: Option<Rect>, _paint: &Paint) {
        self.save();
        #[cfg(debug_assertions)]
        tracing::debug!("Painter::save_layer: using fallback (no offscreen compositing)");
    }

    /// Restore canvas state and composite the layer
    fn restore_layer(&mut self) {
        self.restore();
        #[cfg(debug_assertions)]
        tracing::debug!("Painter::restore_layer: using fallback restore");
    }
}
