//! Wgpu-based CommandRenderer implementation
//!
//! Production rendering backend executing drawing commands via GPU
//! acceleration.

use flui_painting::{BlendMode, DisplayListCore, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, Transform, px},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

use std::sync::Arc;

use super::painter::WgpuPainter;
use crate::{
    commands::dispatch_command,
    traits::{CommandRenderer, LayerStateStack},
};

/// wgpu backend implementation of CommandRenderer.
///
/// # Lifetime parameter
///
/// `Backend<'frame>` borrows the current frame's `wgpu::TextureView` +
/// `wgpu::Texture` when [`bind_surface`](Self::bind_surface) is
/// called. The lifetime is internal to one render pass: `Renderer::render`
/// creates the Backend, binds the frame surface, dispatches the
/// `LayerTree`, then drops the Backend before the surface is
/// presented. Sites that don't need to flush mid-frame (shader-mask
/// offscreen rendering, tests) call [`Backend::new`] which leaves
/// the surface handles unbound; the
/// [`render_backdrop_filter`](CommandRenderer::render_backdrop_filter)
/// command-path falls back to passthrough when the handles are
/// `None` (cycle 4 U-8, U-9).
///
/// Per *Rust for Rustaceans* ch.2 "Variance and Lifetimes": the
/// `'frame` parameter encodes the borrow's scope so the compiler
/// enforces that no Backend outlives its bound surface.
///
/// Note: Debug is not derived because `WgpuPainter` contains wgpu types that
/// don't implement Debug.
#[allow(missing_debug_implementations)]
pub struct Backend<'frame> {
    painter: WgpuPainter,
    offscreen: Option<Arc<parking_lot::Mutex<super::offscreen::OffscreenRenderer>>>,
    /// Cached offscreen painter reused across shader mask invocations.
    /// Lazily created on first use, resized when dimensions change.
    offscreen_painter: Option<WgpuPainter>,
    /// Bound surface view for the current frame. `None` outside a
    /// frame, or when the construction site cannot supply it
    /// (e.g. shader-mask offscreen render). Backdrop-filter
    /// dispatch falls back to passthrough when `None`.
    surface_view: Option<&'frame wgpu::TextureView>,
    /// Bound surface texture for the current frame -- companion of
    /// [`surface_view`](Self::surface_view) for
    /// `COPY_TEXTURE_TO_TEXTURE` operations during backdrop-filter
    /// dispatch.
    surface_texture: Option<&'frame wgpu::Texture>,
}

impl<'frame> Backend<'frame> {
    /// Create a new Backend with the given painter.
    ///
    /// `surface_view` / `surface_texture` start unbound. Call
    /// [`bind_surface`](Self::bind_surface) when the frame surface
    /// is available to enable the DisplayList-backdrop-filter
    /// command path.
    pub fn new(painter: WgpuPainter) -> Self {
        Self {
            painter,
            offscreen: None,
            offscreen_painter: None,
            surface_view: None,
            surface_texture: None,
        }
    }

    /// Create a new Backend with the given painter and offscreen renderer.
    pub fn with_offscreen(
        painter: WgpuPainter,
        offscreen: Arc<parking_lot::Mutex<super::offscreen::OffscreenRenderer>>,
    ) -> Self {
        Self {
            painter,
            offscreen: Some(offscreen),
            offscreen_painter: None,
            surface_view: None,
            surface_texture: None,
        }
    }

    /// Bind the frame's surface handles.
    ///
    /// Must be called by [`Renderer::render`](super::renderer::Renderer::render)
    /// after constructing the Backend and before dispatching any
    /// `LayerTree` commands. Required for
    /// [`CommandRenderer::render_backdrop_filter`] to actually
    /// flush + blur the surface contents; without it the backdrop-
    /// filter path falls back to dispatching the child display list
    /// without applying the filter (visible regression vs Flutter).
    ///
    /// Cycle 4 E-2 / U-8.
    pub fn bind_surface(
        &mut self,
        view: &'frame wgpu::TextureView,
        texture: &'frame wgpu::Texture,
    ) {
        self.surface_view = Some(view);
        self.surface_texture = Some(texture);
    }

    /// Access the offscreen renderer (for shader mask, backdrop filter).
    pub fn offscreen(
        &self,
    ) -> Option<&Arc<parking_lot::Mutex<super::offscreen::OffscreenRenderer>>> {
        self.offscreen.as_ref()
    }

    /// Get a reference to the underlying painter.
    pub fn painter(&self) -> &WgpuPainter {
        &self.painter
    }

    /// Get a mutable reference to the underlying painter.
    pub fn painter_mut(&mut self) -> &mut WgpuPainter {
        &mut self.painter
    }

    /// Consume the renderer and return the underlying painter.
    ///
    /// The offscreen `Arc` is dropped here; ref-counting keeps it alive in Renderer.
    pub fn into_painter(self) -> WgpuPainter {
        self.painter
    }

    /// Returns the current save stack depth.
    ///
    /// This is useful for tracking how many `save()` calls have been made
    /// by layer rendering so that the corresponding number of `restore()` calls
    /// can be issued after rendering children.
    pub fn save_count(&self) -> usize {
        self.painter.save_count()
    }

    /// Restores the most recently saved canvas state.
    ///
    /// This pops the transform and clip state from the save stack.
    /// Used to restore state after rendering layer children.
    pub fn restore(&mut self) {
        self.painter.restore();
    }

    /// Get or create a cached offscreen painter for shader mask rendering.
    ///
    /// On first call, creates a new `WgpuPainter` with shared device/queue.
    /// On subsequent calls, returns the cached painter, resizing if needed.
    fn get_or_create_offscreen_painter(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> &mut WgpuPainter {
        if let Some(ref painter) = self.offscreen_painter
            && painter.size() != size
        {
            // Size changed — drop and recreate
            self.offscreen_painter = None;
        }

        self.offscreen_painter.get_or_insert_with(|| {
            tracing::debug!(
                "Creating cached offscreen painter: size={}x{}, format={:?}",
                size.0,
                size.1,
                format
            );
            super::painter::WgpuPainter::with_shared_device(
                Arc::clone(device),
                Arc::clone(queue),
                format,
                size,
            )
        })
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
            self.painter.translate(Offset::new(px(tx), px(ty)));
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

impl CommandRenderer for Backend<'_> {
    fn render_rect(&mut self, rect: Rect<Pixels>, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.rect(rect, paint);
        });
    }

    fn render_rrect(&mut self, rrect: RRect, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.rrect(rrect, paint);
        });
    }

    fn render_circle(
        &mut self,
        center: Point<Pixels>,
        radius: f32,
        paint: &Paint,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.circle(center, radius, paint);
        });
    }

    fn render_oval(&mut self, rect: Rect<Pixels>, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.oval(rect, paint);
        });
    }

    fn render_line(
        &mut self,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        paint: &Paint,
        transform: &Matrix4,
    ) {
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
        rect: Rect<Pixels>,
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
        points: &[Point<Pixels>],
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
        offset: Offset<Pixels>,
        style: &TextStyle,
        _paint: &Paint,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            #[allow(clippy::cast_possible_truncation)]
            let font_size = style.font_size.unwrap_or(14.0) as f32;
            let color = style.color.unwrap_or(Color::BLACK);
            let paint = Paint::fill(color);
            let position = Point::new(offset.dx, offset.dy);
            painter.text(text, position, font_size, &paint);
        });
    }

    fn render_text_span(
        &mut self,
        span: &flui_types::typography::InlineSpan,
        offset: Offset<Pixels>,
        text_scale_factor: f64,
        transform: &Matrix4,
    ) {
        let text = span.to_plain_text();
        if text.is_empty() {
            return;
        }

        let style = span.style();
        #[allow(clippy::cast_possible_truncation)]
        let base_font_size = style.and_then(|s| s.font_size).unwrap_or(14.0) as f32;
        #[allow(clippy::cast_possible_truncation)]
        let font_size = base_font_size * (text_scale_factor as f32);
        let color = style.and_then(|s| s.color).unwrap_or(Color::BLACK);
        let paint = Paint::fill(color);
        let position = Point::new(offset.dx, offset.dy);

        self.with_transform(transform, |painter| {
            painter.text(&text, position, font_size, &paint);
        });
    }

    fn render_image(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
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
        sprites: &[Rect<Pixels>],
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
        dst: Rect<Pixels>,
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
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
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
        dst: Rect<Pixels>,
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
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
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
        shader: &flui_painting::Shader,
        bounds: Rect<Pixels>,
        blend_mode: BlendMode,
        _transform: &Matrix4,
    ) {
        // Try GPU shader mask pipeline
        if let Some(offscreen_arc) = self.offscreen.clone() {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let width = bounds.width().0.max(1.0) as u32;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let height = bounds.height().0.max(1.0) as u32;

            // Step 1: Get GPU resources from offscreen renderer
            let (device, queue, format, child_tex) = {
                let offscreen = offscreen_arc.lock();
                let device = Arc::clone(offscreen.device());
                let queue = Arc::clone(offscreen.queue());
                let format = offscreen.surface_format();
                let child_tex = offscreen.texture_pool().acquire(width, height, format);
                (device, queue, format, child_tex)
            };
            // Lock released here

            // Step 2: Get or create cached offscreen painter (avoids per-call allocation)
            // Ensure the cache is populated (creates or resizes as needed), then take
            // it out temporarily so we can wrap it in a Backend for command dispatch.
            let _ = self.get_or_create_offscreen_painter(&device, &queue, format, (width, height));
            let mut temp_painter = self
                .offscreen_painter
                .take()
                .expect("offscreen_painter was just populated by get_or_create");
            {
                let mut temp_backend = Backend::new(temp_painter);
                for command in child.commands() {
                    dispatch_command(command, &mut temp_backend);
                }
                temp_painter = temp_backend.into_painter();
            }

            // Step 4: Flush child content to offscreen texture
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ShaderMask Child Render"),
            });
            // Clear the child texture first
            {
                let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ShaderMask Child Clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: child_tex.view(),
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            // Render child batches to offscreen texture
            if let Err(e) = temp_painter.render(child_tex.view(), &mut encoder) {
                tracing::error!("Failed to render shader mask child content: {}", e);
            }
            queue.submit(std::iter::once(encoder.finish()));

            // Put the cached painter back for reuse
            self.offscreen_painter = Some(temp_painter);

            // Step 5: Apply shader mask via GPU pipeline
            let masked_texture = {
                let mut offscreen = offscreen_arc.lock();
                let result =
                    offscreen.render_masked(bounds, shader, blend_mode, child_tex.texture());
                result.into_texture()
            };

            // Step 6: Queue masked result for compositing on main target
            self.painter.queue_offscreen_result(masked_texture, bounds);

            tracing::debug!(
                "ShaderMask GPU pipeline complete: bounds={:?}, child_size={}x{}",
                bounds,
                width,
                height
            );
            return;
        }

        // Fallback: render child content without masking
        tracing::warn!("ShaderMask: no OffscreenRenderer, rendering child without mask");
        for command in child.commands() {
            dispatch_command(command, self);
        }
    }

    fn render_gradient(
        &mut self,
        rect: Rect<Pixels>,
        shader: &flui_painting::Shader,
        transform: &Matrix4,
    ) {
        use super::effects::GradientStop;

        self.with_transform(transform, |painter| {
            match shader {
                flui_painting::Shader::LinearGradient {
                    from,
                    to,
                    colors,
                    stops,
                    ..
                } => {
                    if colors.is_empty() {
                        return;
                    }

                    // Convert colors and stops to GradientStop array
                    let gradient_stops: Vec<GradientStop> = if let Some(stop_positions) = stops {
                        colors
                            .iter()
                            .zip(stop_positions.iter())
                            .map(|(color, pos)| GradientStop::new(*color, *pos))
                            .collect()
                    } else {
                        // Default evenly spaced stops
                        let count = colors.len();
                        colors
                            .iter()
                            .enumerate()
                            .map(|(i, color)| {
                                let pos = if count > 1 {
                                    i as f32 / (count - 1) as f32
                                } else {
                                    0.0
                                };
                                GradientStop::new(*color, pos)
                            })
                            .collect()
                    };

                    painter.gradient_rect(
                        rect,
                        glam::Vec2::new(from.dx.0, from.dy.0),
                        glam::Vec2::new(to.dx.0, to.dy.0),
                        &gradient_stops,
                        0.0, // No corner radius for rect
                    );
                }
                flui_painting::Shader::RadialGradient {
                    center,
                    radius,
                    colors,
                    stops,
                    ..
                } => {
                    if colors.is_empty() {
                        return;
                    }

                    // Convert colors and stops to GradientStop array
                    let gradient_stops: Vec<GradientStop> = if let Some(stop_positions) = stops {
                        colors
                            .iter()
                            .zip(stop_positions.iter())
                            .map(|(color, pos)| GradientStop::new(*color, *pos))
                            .collect()
                    } else {
                        // Default evenly spaced stops
                        let count = colors.len();
                        colors
                            .iter()
                            .enumerate()
                            .map(|(i, color)| {
                                let pos = if count > 1 {
                                    i as f32 / (count - 1) as f32
                                } else {
                                    0.0
                                };
                                GradientStop::new(*color, pos)
                            })
                            .collect()
                    };

                    painter.radial_gradient_rect(
                        rect,
                        glam::Vec2::new(center.dx.0, center.dy.0),
                        *radius,
                        &gradient_stops,
                        0.0, // No corner radius for rect
                    );
                }
                flui_painting::Shader::SweepGradient {
                    center,
                    start_angle,
                    end_angle,
                    colors,
                    stops,
                    ..
                } => {
                    if colors.is_empty() {
                        return;
                    }

                    // Convert colors and stops to GradientStop array
                    let gradient_stops: Vec<GradientStop> = if let Some(stop_positions) = stops {
                        colors
                            .iter()
                            .zip(stop_positions.iter())
                            .map(|(color, pos)| GradientStop::new(*color, *pos))
                            .collect()
                    } else {
                        let count = colors.len();
                        colors
                            .iter()
                            .enumerate()
                            .map(|(i, color)| {
                                let pos = if count > 1 {
                                    i as f32 / (count - 1) as f32
                                } else {
                                    0.0
                                };
                                GradientStop::new(*color, pos)
                            })
                            .collect()
                    };

                    painter.sweep_gradient_rect(
                        rect,
                        glam::Vec2::new(center.dx.0, center.dy.0),
                        *start_angle,
                        *end_angle,
                        &gradient_stops,
                        0.0, // No corner radius for rect
                    );
                }
                _ => {
                    // Image and other non-gradient shader types are not applicable
                    // for gradient rendering; skip silently.
                    tracing::debug!("render_gradient: unsupported shader variant, skipping");
                }
            }
        });
    }

    fn render_gradient_rrect(
        &mut self,
        rrect: RRect,
        shader: &flui_painting::Shader,
        transform: &Matrix4,
    ) {
        use super::effects::GradientStop;

        self.with_transform(transform, |painter| {
            // Get average corner radius
            let corner_radius =
                (rrect.top_left.x + rrect.top_right.x + rrect.bottom_left.x + rrect.bottom_right.x)
                    / px(4.0);

            match shader {
                flui_painting::Shader::LinearGradient {
                    from,
                    to,
                    colors,
                    stops,
                    ..
                } => {
                    if colors.is_empty() {
                        return;
                    }

                    // Convert colors and stops to GradientStop array
                    let gradient_stops: Vec<GradientStop> = if let Some(stop_positions) = stops {
                        colors
                            .iter()
                            .zip(stop_positions.iter())
                            .map(|(color, pos)| GradientStop::new(*color, *pos))
                            .collect()
                    } else {
                        // Default evenly spaced stops
                        let count = colors.len();
                        colors
                            .iter()
                            .enumerate()
                            .map(|(i, color)| {
                                let pos = if count > 1 {
                                    i as f32 / (count - 1) as f32
                                } else {
                                    0.0
                                };
                                GradientStop::new(*color, pos)
                            })
                            .collect()
                    };

                    painter.gradient_rect(
                        rrect.rect,
                        glam::Vec2::new(from.dx.0, from.dy.0),
                        glam::Vec2::new(to.dx.0, to.dy.0),
                        &gradient_stops,
                        corner_radius,
                    );
                }
                flui_painting::Shader::RadialGradient {
                    center,
                    radius,
                    colors,
                    stops,
                    ..
                } => {
                    if colors.is_empty() {
                        return;
                    }

                    // Convert colors and stops to GradientStop array
                    let gradient_stops: Vec<GradientStop> = if let Some(stop_positions) = stops {
                        colors
                            .iter()
                            .zip(stop_positions.iter())
                            .map(|(color, pos)| GradientStop::new(*color, *pos))
                            .collect()
                    } else {
                        // Default evenly spaced stops
                        let count = colors.len();
                        colors
                            .iter()
                            .enumerate()
                            .map(|(i, color)| {
                                let pos = if count > 1 {
                                    i as f32 / (count - 1) as f32
                                } else {
                                    0.0
                                };
                                GradientStop::new(*color, pos)
                            })
                            .collect()
                    };

                    painter.radial_gradient_rect(
                        rrect.rect,
                        glam::Vec2::new(center.dx.0, center.dy.0),
                        *radius,
                        &gradient_stops,
                        corner_radius,
                    );
                }
                flui_painting::Shader::SweepGradient {
                    center,
                    start_angle,
                    end_angle,
                    colors,
                    stops,
                    ..
                } => {
                    if colors.is_empty() {
                        return;
                    }

                    let gradient_stops: Vec<GradientStop> = if let Some(stop_positions) = stops {
                        colors
                            .iter()
                            .zip(stop_positions.iter())
                            .map(|(color, pos)| GradientStop::new(*color, *pos))
                            .collect()
                    } else {
                        let count = colors.len();
                        colors
                            .iter()
                            .enumerate()
                            .map(|(i, color)| {
                                let pos = if count > 1 {
                                    i as f32 / (count - 1) as f32
                                } else {
                                    0.0
                                };
                                GradientStop::new(*color, pos)
                            })
                            .collect()
                    };

                    painter.sweep_gradient_rect(
                        rrect.rect,
                        glam::Vec2::new(center.dx.0, center.dy.0),
                        *start_angle,
                        *end_angle,
                        &gradient_stops,
                        corner_radius,
                    );
                }
                _ => {
                    tracing::debug!("render_gradient_rrect: unsupported shader variant, skipping");
                }
            }
        });
    }

    fn render_color(&mut self, color: Color, _blend_mode: BlendMode, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            let viewport_bounds = painter.viewport_bounds();
            let paint = Paint::fill(color);
            painter.rect(viewport_bounds, &paint);
        });
    }

    fn render_paint(&mut self, paint: &Paint, transform: &Matrix4) {
        let paint = paint.clone();
        self.with_transform(transform, |painter| {
            let viewport_bounds = painter.viewport_bounds();
            painter.rect(viewport_bounds, &paint);
        });
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "backdrop-filter region bounds are f32 in physical pixels; coercion to u32 \
                  matches Path A's `Renderer::handle_backdrop_filter` (renderer.rs:903-906) \
                  which is the canonical reference"
    )]
    fn render_backdrop_filter(
        &mut self,
        child: Option<&flui_painting::DisplayList>,
        filter: &flui_painting::display_list::ImageFilter,
        bounds: Rect<Pixels>,
        _blend_mode: BlendMode,
        transform: &Matrix4,
    ) {
        use flui_painting::display_list::ImageFilter;

        // Helper: dispatch the child display list (or no-op when None)
        // without applying any backdrop filter. Used by every fall-back
        // branch below.
        //
        // PR #110 review feedback (F-W2-4): pre-fix this helper called
        // `this.with_transform(transform, |_painter| {})` before the
        // dispatch loop. `with_transform` save+applies the transform
        // then runs the closure body; the empty closure means save
        // is immediately balanced by restore, so the transform never
        // reached the subsequent `dispatch_command` loop. Effectively
        // a misleading no-op. Each `DrawCommand` in a `DisplayList`
        // carries its own pre-composited transform field (set during
        // display-list capture), so re-applying the outer
        // `render_backdrop_filter` transform here is redundant; Path A
        // (`Renderer::handle_backdrop_filter`) does the same -- it
        // just dispatches children without re-wrapping. The
        // `transform` arg is consumed by Stage 2 below (it transforms
        // `bounds` to device space).
        let passthrough = |this: &mut Self| {
            if let Some(child) = child {
                for command in child.commands() {
                    dispatch_command(command, this);
                }
            }
        };

        // Cycle 4 E-2 U-9: Path B (DisplayList-command-level) backdrop
        // filter. Mirrors Path A (`Renderer::handle_backdrop_filter`
        // at renderer.rs:845-960) which already works for the
        // layer-tree-level `BackdropFilterLayer`. The two paths converge
        // on the same offscreen pipeline:
        //
        //   1. Flush current painter batches to surface
        //   2. Apply `transform` to `bounds` -> device-space rect,
        //      clamp against surface extent
        //   3. COPY_TEXTURE_TO_TEXTURE the clamped region into a
        //      pooled blur-input texture
        //   4. Dual Kawase blur on the offscreen renderer
        //   5. Queue the blurred result for compositing on next flush
        //   6. Dispatch child display list on top of the blurred backdrop
        //
        // Non-blur filters + missing surface/offscreen handles fall
        // back to passthrough with a `tracing::warn!` so the gap is
        // observable (the pre-U-9 stub was silent for non-debug builds).

        let sigma = match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => f32::midpoint(*sigma_x, *sigma_y),
            other => {
                tracing::warn!(
                    "Backdrop filter type {:?} not supported in DisplayList path; passthrough",
                    other
                );
                passthrough(self);
                return;
            }
        };

        let Some(offscreen_arc) = self.offscreen.clone() else {
            tracing::warn!(
                "Backdrop filter: no OffscreenRenderer in DisplayList path; passthrough"
            );
            passthrough(self);
            return;
        };

        let (Some(surface_view), Some(surface_texture)) = (self.surface_view, self.surface_texture)
        else {
            tracing::warn!(
                "Backdrop filter: no surface bound via bind_surface(); passthrough \
                 (the surface handles are bound in `Renderer::render` only -- \
                 the shader-mask offscreen path does not bind them, which is expected)"
            );
            passthrough(self);
            return;
        };

        // Snapshot device/queue/format under a short lock; offscreen
        // mutation happens later through `render_blur`.
        let (device, queue, format) = {
            let off = offscreen_arc.lock();
            (
                Arc::clone(off.device()),
                Arc::clone(off.queue()),
                off.surface_format(),
            )
        };

        // Stage 2: apply `transform` to `bounds` -> device-space rect,
        // then clamp against the surface texture extent.
        //
        // PR #110 review feedback (F-W2-2): pre-fix `bounds` was used
        // directly as the copy source rect, ignoring `transform`.
        // `DrawCommand::BackdropFilter` stores `bounds` in local space
        // and `transform` as the outer transform stack; `paint_bounds()`
        // composes them via `transform.transform_rect(bounds)`. Using
        // untransformed `bounds` blurred the wrong region whenever the
        // canvas transform was non-identity.
        //
        // PR #110 review feedback (F-W2-1, P1): pre-fix `x/y/w/h` were
        // only lower-clamped (`max(0.0)`/`max(1.0)`). If a backdrop
        // filter is partially off-screen (negative origin or extent
        // beyond the frame), `copy_texture_to_texture` gets an
        // out-of-range region and wgpu validation panics at submit
        // time, dropping the frame. Clamp against the surface texture
        // extent before computing extents.
        let device_rect = transform.transform_rect(&bounds);
        let surface_extent = surface_texture.size();
        let surface_w = surface_extent.width;
        let surface_h = surface_extent.height;

        let x = device_rect.left().0.clamp(0.0, surface_w as f32) as u32;
        let y = device_rect.top().0.clamp(0.0, surface_h as f32) as u32;
        // Right/bottom likewise clamp, then derive width/height as the
        // difference. `saturating_sub` guards the corner case where
        // device_rect is entirely outside the surface (right <= x).
        let right = device_rect.right().0.clamp(0.0, surface_w as f32) as u32;
        let bottom = device_rect.bottom().0.clamp(0.0, surface_h as f32) as u32;
        let w = right.saturating_sub(x).max(1);
        let h = bottom.saturating_sub(y).max(1);

        // Refuse if the clamped region is empty (the backdrop region is
        // entirely off-screen). `copy_texture_to_texture` requires
        // non-zero extents; falling through to passthrough preserves
        // the child rendering without GPU validation panics.
        if right <= x || bottom <= y {
            tracing::warn!(
                bounds_l = bounds.left().0,
                bounds_t = bounds.top().0,
                bounds_r = bounds.right().0,
                bounds_b = bounds.bottom().0,
                surface_w,
                surface_h,
                "Backdrop filter: clamped region is empty (entirely off-screen); passthrough"
            );
            passthrough(self);
            return;
        }

        // Stage 1: flush painter batches to surface so the backdrop
        // pixels we are about to blur are present.
        let mut flush_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("DisplayList Backdrop Flush Encoder"),
        });
        if let Err(e) = self.painter.render(surface_view, &mut flush_encoder) {
            tracing::error!("DisplayList backdrop flush failed: {}", e);
        }

        // Stage 3: COPY_TEXTURE_TO_TEXTURE surface region -> pooled
        // blur-input. Acquired from offscreen's texture pool so the
        // allocation amortises across frames (Path A acquires the
        // same way).
        let blur_input = {
            let offscreen = offscreen_arc.lock();
            offscreen.texture_pool().acquire(w, h, format)
        };

        flush_encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: surface_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: blur_input.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(flush_encoder.finish()));

        // Stage 4: Dual Kawase blur on the offscreen renderer.
        let blurred = {
            let mut offscreen = offscreen_arc.lock();
            offscreen.render_blur(&blur_input, sigma)
        };

        // Stage 5: queue blurred result for compositing on next painter
        // flush. The blurred texture is laid down at the same
        // device-space rect we just sampled from -- using `bounds`
        // (local-space) here would composite the blur at the wrong
        // location whenever `transform` is non-identity.
        self.painter.queue_offscreen_result(blurred, device_rect);

        // Stage 6: dispatch the child display list on top of the blurred
        // backdrop. Each child `DrawCommand` carries its own
        // pre-composited transform from display-list capture, so no
        // outer transform wrap is needed here (Path A treats child
        // dispatch the same way).
        if let Some(child) = child {
            for command in child.commands() {
                dispatch_command(command, self);
            }
        }
    }

    fn render_vertices(
        &mut self,
        vertices: &[Point<Pixels>],
        colors: Option<&[Color]>,
        tex_coords: Option<&[Point<Pixels>]>,
        indices: &[u16],
        paint: &Paint,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.draw_vertices(vertices, colors, tex_coords, indices, paint);
        });
    }

    fn clip_rect(
        &mut self,
        rect: Rect<Pixels>,
        _clip_op: flui_types::painting::ClipOp,
        _clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        // ClipOp and Clip are stored in DrawCommand and available here for future
        // GPU-accelerated clip modes (e.g. stencil-based Difference, MSAA anti-aliased edges).
        // Current implementation uses simple scissor clipping (Intersect + HardEdge).
        self.with_transform(transform, |painter| {
            painter.clip_rect(rect);
        });
    }

    fn clip_rrect(
        &mut self,
        rrect: RRect,
        _clip_op: flui_types::painting::ClipOp,
        _clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.clip_rrect(rrect);
        });
    }

    fn clip_rsuperellipse(
        &mut self,
        rsuperellipse: flui_types::geometry::RSuperellipse,
        _clip_op: flui_types::painting::ClipOp,
        _clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        // Override the trait default (which routes to clip_rrect against an
        // approximating rounded rectangle). Delegate to the Painter's real
        // SDF clip, populating `current_rsuperellipse_clip` so subsequent
        // rect_instanced draws apply the iOS-squircle SDF (U9 wired the
        // per-instance kind=2 path).
        self.with_transform(transform, |painter| {
            painter.clip_rsuperellipse(rsuperellipse);
        });
    }

    fn superellipse_path(
        &mut self,
        rse: flui_types::geometry::RSuperellipse,
    ) -> flui_types::painting::Path {
        // Override the trait default (which freshly generates the path
        // every call, no caching). Delegate to the Painter-owned bounded
        // cache so identical superellipses across frames reuse the cached
        // tessellation. Cache eviction follows PathCache semantics
        // (`max_entries` + `last_used_frame`).
        self.painter.superellipse_path(&rse)
    }

    fn clip_path(
        &mut self,
        path: &Path,
        _clip_op: flui_types::painting::ClipOp,
        _clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        self.with_transform(transform, |painter| {
            painter.clip_path(path);
        });
    }

    fn viewport_bounds(&self) -> Rect<Pixels> {
        self.painter.viewport_bounds()
    }

    fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.save_layer(bounds, paint);
        });
    }

    fn restore_layer(&mut self, _transform: &Matrix4) {
        self.painter.restore_layer();
    }

    // ===== Layer Tree Operations split out =====
    //
    // Cycle 4 E-9: push_clip_* / push_offset / push_transform /
    // push_opacity / push_color_filter / push_image_filter + their
    // corresponding pop_* moved to `impl LayerStateStack for Backend`
    // (below). The visitor methods on this trait stay; the layer-tree
    // state-stack methods live on the dedicated `LayerStateStack`
    // trait. See traits.rs E-9 commentary.

    fn add_performance_overlay(
        &mut self,
        options_mask: u32,
        bounds: Rect<Pixels>,
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
            RRect::from_rect_and_radius(bounds, flui_types::geometry::Radius::circular(px(4.0)));
        self.painter.rrect(bg_rrect, &bg_paint);

        let x = bounds.left() + px(8.0);
        let x_val = bounds.left() + px(50.0);
        let mut y = bounds.top() + px(14.0);

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
            &format!("{fps:.0}"),
            Point::new(x_val, y),
            11.0,
            &Paint::fill(fps_color),
        );

        // FPS unit (dimmer)
        let gray = Color::rgba(130, 130, 130, 255);
        let fps_w = if fps >= 100.0 {
            px(24.0)
        } else if fps >= 10.0 {
            px(16.0)
        } else {
            px(8.0)
        };
        self.painter
            .text("FPS", Point::new(x_val + fps_w, y), 8.0, &Paint::fill(gray));
        y += px(14.0);

        // Frametime label (purple) + value
        let purple = Color::rgba(200, 100, 255, 255);
        self.painter
            .text("Frame", Point::new(x, y), 10.0, &Paint::fill(purple));

        let white = Color::rgba(220, 220, 220, 255);
        self.painter.text(
            &format!("{frame_time_ms:.1}"),
            Point::new(x_val, y),
            10.0,
            &Paint::fill(white),
        );
        self.painter.text(
            "ms",
            Point::new(x_val + px(22.0), y),
            8.0,
            &Paint::fill(gray),
        );

        let _ = total_frames;
    }
}

// ============================================================================
// LAYER-STATE-STACK IMPL (cycle 4 E-9 split)
// ============================================================================
//
// The 13 push_/pop_ methods below moved out of the `impl CommandRenderer
// for Backend` block in cycle 4 E-9 to live on the dedicated
// `LayerStateStack` trait. Bodies + behavior unchanged; only the
// receiving trait differs. See `crates/flui-engine/src/traits.rs`
// for the trait-split rationale.

impl LayerStateStack for Backend<'_> {
    fn push_clip_rect(&mut self, rect: &Rect<Pixels>, _clip_behavior: flui_types::painting::Clip) {
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

    fn push_offset(&mut self, offset: Offset<Pixels>) {
        self.painter.save();
        self.painter.translate(offset);
    }

    fn push_transform(&mut self, transform: &Matrix4) {
        self.painter.save();

        // Decompose and apply transform components
        let transform_enum = Transform::from(*transform);
        let (tx, ty, rotation, sx, sy) = transform_enum.decompose();

        if tx != 0.0 || ty != 0.0 {
            self.painter.translate(Offset::new(px(tx), px(ty)));
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
        // Create a layer with opacity (clamped to [0, 255])
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let alpha_u8 = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let paint = Paint::fill(Color::WHITE).with_alpha(alpha_u8);
        self.painter.save_layer(None, &paint);
    }

    fn pop_opacity(&mut self) {
        self.painter.restore_layer();
    }

    fn push_color_filter(&mut self, filter: &flui_types::painting::ColorMatrix) {
        // Check if the matrix is identity (no transformation needed)
        let identity = flui_types::painting::ColorMatrix::identity();
        // Exact f32 array comparison is intentional: ColorMatrix::identity()
        // is built from bit-exact 0.0/1.0 literals, so a transitive equality
        // check correctly fast-paths the no-op case without ULP slop.
        #[expect(
            clippy::float_cmp,
            reason = "identity matrix is bit-exact (0.0/1.0 literals); exact comparison is correct"
        )]
        if filter.values == identity.values {
            // Identity matrix: use a plain save so pop_color_filter stays balanced
            self.painter.save_layer(None, &Paint::fill(Color::WHITE));
            tracing::trace!("push_color_filter: identity matrix, no-op layer");
            return;
        }

        // Pragmatic approximation: extract opacity and tint from the color matrix.
        let alpha_scale = filter.values[18].clamp(0.0, 1.0);
        let alpha_offset = filter.values[19].clamp(0.0, 1.0);
        let effective_alpha = (alpha_scale + alpha_offset).clamp(0.0, 1.0);
        let tinted = filter.apply([1.0, 1.0, 1.0, 1.0]);

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let tint_color = Color::rgba(
            (tinted[0] * 255.0) as u8,
            (tinted[1] * 255.0) as u8,
            (tinted[2] * 255.0) as u8,
            (effective_alpha * 255.0) as u8,
        );

        let paint = Paint::fill(tint_color);
        self.painter.save_layer(None, &paint);

        tracing::debug!(
            "push_color_filter: approximation tint=({},{},{}) alpha={:.2}",
            tint_color.r,
            tint_color.g,
            tint_color.b,
            effective_alpha
        );
    }

    fn pop_color_filter(&mut self) {
        self.painter.restore_layer();
    }

    fn push_image_filter(&mut self, filter: &flui_painting::display_list::ImageFilter) {
        use flui_painting::display_list::ImageFilter;

        // Pragmatic approximation: full GPU image filters (blur, dilate, erode)
        // require render-to-texture + compute shader post-processing which needs
        // offscreen infrastructure not yet available. Instead, we use save_layer
        // to isolate the filtered content for future GPU pass integration.
        match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                let paint = Paint::fill(Color::WHITE);
                self.painter.save_layer(None, &paint);
                tracing::debug!(
                    "push_image_filter(Blur): save_layer for blur sigma_x={:.2}, sigma_y={:.2} \
                     (GPU blur not yet implemented)",
                    sigma_x,
                    sigma_y,
                );
            }
            ImageFilter::Dilate { radius } => {
                let paint = Paint::fill(Color::WHITE);
                self.painter.save_layer(None, &paint);
                tracing::debug!(
                    "push_image_filter(Dilate): save_layer for dilate radius={:.2} \
                     (GPU morphology not yet implemented)",
                    radius,
                );
            }
            ImageFilter::Erode { radius } => {
                let paint = Paint::fill(Color::WHITE);
                self.painter.save_layer(None, &paint);
                tracing::debug!(
                    "push_image_filter(Erode): save_layer for erode radius={:.2} \
                     (GPU morphology not yet implemented)",
                    radius,
                );
            }
            ImageFilter::Matrix(matrix) => {
                let alpha_scale = matrix.values[18].clamp(0.0, 1.0);
                let alpha_offset = matrix.values[19].clamp(0.0, 1.0);
                let effective_alpha = (alpha_scale + alpha_offset).clamp(0.0, 1.0);
                let tinted = matrix.apply([1.0, 1.0, 1.0, 1.0]);

                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let tint_color = Color::rgba(
                    (tinted[0] * 255.0) as u8,
                    (tinted[1] * 255.0) as u8,
                    (tinted[2] * 255.0) as u8,
                    (effective_alpha * 255.0) as u8,
                );
                let paint = Paint::fill(tint_color);
                self.painter.save_layer(None, &paint);
                tracing::debug!(
                    "push_image_filter(Matrix): approximation tint=({},{},{}) alpha={:.2}",
                    tint_color.r,
                    tint_color.g,
                    tint_color.b,
                    effective_alpha,
                );
            }
            ImageFilter::ColorAdjust(adjustment) => {
                let paint = Paint::fill(Color::WHITE);
                self.painter.save_layer(None, &paint);
                tracing::debug!(
                    "push_image_filter(ColorAdjust): save_layer for {:?} \
                     (GPU color adjust not yet implemented)",
                    adjustment,
                );
            }
            ImageFilter::Compose(filters) => {
                let paint = Paint::fill(Color::WHITE);
                self.painter.save_layer(None, &paint);
                tracing::debug!(
                    "push_image_filter(Compose): save_layer for {} chained filters \
                     (GPU compose not yet implemented)",
                    filters.len(),
                );
            }
            #[cfg(debug_assertions)]
            ImageFilter::OverflowIndicator {
                overflow_h,
                overflow_v,
                ..
            } => {
                let paint = Paint::fill(Color::WHITE);
                self.painter.save_layer(None, &paint);
                tracing::debug!(
                    "push_image_filter(OverflowIndicator): h={:.1}, v={:.1}",
                    overflow_h,
                    overflow_v,
                );
            }
        }
    }

    fn pop_image_filter(&mut self) {
        self.painter.restore_layer();
    }
}
