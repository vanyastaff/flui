//! Wgpu-based CommandRenderer implementation
//!
//! Production rendering backend executing drawing commands via GPU
//! acceleration.

use flui_painting::{BlendMode, DisplayListCore, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, Size, Transform, px},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};
use smallvec::SmallVec;

use std::sync::Arc;

use super::{
    command_ir::{GammaDirection, ImageFilterPass, ImageFilterSpec, LayerFilter, MorphOp},
    painter::WgpuPainter,
};
use crate::{
    commands::dispatch_command,
    traits::{CommandRenderer, LayerStateStack},
};

/// Builds a gradient stop array from a color slice and optional explicit stop
/// positions, with no heap allocation for the common case of ≤ 8 stops.
///
/// When `stop_positions` is `None`, stops are spaced evenly across [0, 1].
/// The return type derefs to `&[GradientStop]`, satisfying every painter call site.
fn build_gradient_stops(
    colors: &[Color],
    stop_positions: Option<&Vec<f32>>,
) -> SmallVec<[super::effects::GradientStop; 8]> {
    use super::effects::GradientStop;
    if let Some(positions) = stop_positions {
        colors
            .iter()
            .zip(positions.iter())
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
    }
}

/// wgpu backend implementation of CommandRenderer.
///
/// # Lifetime parameter
///
/// `Backend<'frame>` borrows the current frame's painter (`&'frame mut
/// WgpuPainter`) and, when present, the `wgpu::TextureView` /
/// `wgpu::Texture` bound by [`bind_surface`](Self::bind_surface). The
/// lifetime is internal to one render pass: `Renderer::render` creates
/// the Backend in a scoped block, dispatches the `LayerTree`, then lets
/// it drop before calling `painter.render()`. Sites that don't need to
/// flush mid-frame (shader-mask offscreen rendering, tests) call
/// [`Backend::new`] which leaves the surface handles unbound; the
/// [`render_backdrop_filter`](CommandRenderer::render_backdrop_filter)
/// command-path falls back to passthrough when the handles are
/// `None` (cycle 4 U-8, U-9).
///
/// Per *Rust for Rustaceans* ch.2 "Variance and Lifetimes": the
/// `'frame` parameter encodes the borrow's scope so the compiler
/// enforces that no Backend outlives its bound resources.
///
/// Note: Debug is not derived because `WgpuPainter` contains wgpu types that
/// don't implement Debug.
#[allow(missing_debug_implementations)]
pub struct Backend<'frame> {
    painter: &'frame mut WgpuPainter,
    offscreen: Option<&'frame mut super::offscreen::OffscreenRenderer>,
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
    /// Cycle 4 wave 5 E-13: matrix that is currently applied to
    /// [`painter`](Self::painter) via a `save() + apply` pair that
    /// has not yet been balanced with `restore()`. `with_transform`
    /// uses this to coalesce consecutive same-matrix calls into a
    /// single push/pop: when the incoming transform equals
    /// `active_transform`, the draw closure runs directly on the
    /// already-applied state rather than paying another stack push.
    ///
    /// [`flush_active_transform`](Self::flush_active_transform)
    /// balances the deferred `restore()`. It is called eagerly at every
    /// point where the painter save stack could be mutated outside
    /// `with_transform`'s coalescing path -- the identity /
    /// transform-mismatch arms inside `with_transform` itself, every
    /// `LayerStateStack` method on `Backend` (`push_clip_*`,
    /// `pop_clip`, `push_offset`, `push_transform`, `pop_transform`,
    /// `push_opacity`, `pop_opacity`, `push_color_filter`,
    /// `pop_color_filter`, `push_image_filter`, `pop_image_filter`),
    /// and the explicit [`Backend::restore`](Self::restore) escape
    /// hatch. PR #117 review (Codex P1) added the LayerStateStack
    /// flush points after the initial wave-5 ship; without them, a
    /// `push_clip → with_transform → pop_clip` sequence would pop the
    /// lazy save instead of the clip, corrupting state across sibling
    /// layers.
    ///
    /// `None` means the painter is at the default state and no
    /// balance is owed.
    ///
    /// The `Drop` impl provides a final safety-net flush: if a future
    /// code path forgets to call `flush_active_transform()` before
    /// the Backend goes out of scope, Drop balances the deferred save
    /// so the borrowed painter is left in a clean state. The 21 eager
    /// call sites above are NOT replaced by Drop — they flush at
    /// precisely the right point for correctness; Drop is the backstop
    /// for any site that is missed.
    active_transform: Option<Matrix4>,
}

impl<'frame> Backend<'frame> {
    /// Create a new Backend that borrows the given painter for the frame.
    ///
    /// `surface_view` / `surface_texture` start unbound. Call
    /// [`bind_surface`](Self::bind_surface) when the frame surface
    /// is available to enable the DisplayList-backdrop-filter
    /// command path.
    pub fn new(painter: &'frame mut WgpuPainter) -> Self {
        Self {
            painter,
            offscreen: None,
            offscreen_painter: None,
            surface_view: None,
            surface_texture: None,
            active_transform: None,
        }
    }

    /// Create a new Backend that borrows the given painter and offscreen renderer.
    pub fn with_offscreen(
        painter: &'frame mut WgpuPainter,
        offscreen: &'frame mut super::offscreen::OffscreenRenderer,
    ) -> Self {
        Self {
            painter,
            offscreen: Some(offscreen),
            offscreen_painter: None,
            surface_view: None,
            surface_texture: None,
            active_transform: None,
        }
    }

    /// Bind the frame's surface handles.
    ///
    /// Must be called by [`Renderer::render_scene`](super::renderer::Renderer::render_scene)
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

    /// Access the offscreen renderer mutably (for shader mask, backdrop filter).
    pub fn offscreen_mut(&mut self) -> Option<&mut super::offscreen::OffscreenRenderer> {
        self.offscreen.as_deref_mut()
    }

    /// Get a reference to the underlying painter.
    pub fn painter(&self) -> &WgpuPainter {
        self.painter
    }

    /// Get a mutable reference to the underlying painter.
    pub fn painter_mut(&mut self) -> &mut WgpuPainter {
        &mut *self.painter
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
    ///
    /// Cycle 4 wave 5 E-13 PR #117 review (Codex P1): flushes any
    /// lazy `with_transform` save first so the explicit `restore()`
    /// pops the caller's matched `save()`, not the lazy transform
    /// that happens to be the top of the painter stack.
    pub fn restore(&mut self) {
        self.flush_active_transform();
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

    /// Cycle 4 wave 5 E-13: dispatch a draw closure under the given
    /// transform, coalescing consecutive same-matrix calls so that
    /// the `painter.save()` + matrix-decompose + apply + restore
    /// pipeline runs once per RUN of identical transforms rather
    /// than once per shape.
    ///
    /// Three fast paths plus the cold path:
    /// 1. `transform.is_identity()` -- if a non-identity transform
    ///    is still active from a prior run, balance the deferred
    ///    `restore()` first; then dispatch on a clean painter.
    /// 2. `Some(transform) == active_transform` -- the painter is
    ///    already in the right state; just run the closure (one
    ///    bit-exact `Matrix4` compare = 16 floats, well under the
    ///    cost of a stack push).
    /// 3. Transform changed -- balance the prior active (if any),
    ///    save, decompose + apply, mark active. The next call with
    ///    the same matrix will hit path 2.
    ///
    /// The lazy save is balanced at every site that mutates the
    /// painter save stack outside this method: each `LayerStateStack`
    /// trait method (push_clip_* / pop_clip / push_offset /
    /// push_transform / pop_transform / push_opacity / pop_opacity
    /// / push_color_filter / pop_color_filter / push_image_filter
    /// / pop_image_filter), the public `Backend::restore` escape
    /// hatch, and the `Drop` impl (so the borrowed painter is
    /// balanced when the Backend leaves scope). See [`Self::active_transform`] for
    /// the full list and the PR #117 review (Codex P1) context.
    ///
    /// Audit context: a render pass batching 1000 same-transform
    /// shapes used to pay 2000 stack ops + 1000 mat-decomposes
    /// (each pair `save + apply + restore`). After this change the
    /// run pays one `save + apply` plus one `restore` at the next
    /// transform change -- (N-1) push/pops eliminated per run.
    fn with_transform<F>(&mut self, transform: &Matrix4, draw_fn: F)
    where
        F: FnOnce(&mut WgpuPainter),
    {
        if transform.is_identity() {
            self.flush_active_transform();
            draw_fn(self.painter);
            return;
        }

        if self.active_transform.as_ref() == Some(transform) {
            // Path 2: same matrix as the currently-applied one --
            // skip the push entirely; the painter is already in the
            // right state.
            draw_fn(self.painter);
            return;
        }

        // Path 3: incoming transform differs from active (or no
        // active). Balance the prior `save()` if any, then push
        // the new transform.
        self.flush_active_transform();
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

        self.active_transform = Some(*transform);
        draw_fn(self.painter);
    }

    /// Cycle 4 wave 5 E-13: balance the deferred `save()` left by a
    /// prior `with_transform` run with a `restore()`, clearing
    /// `active_transform`. No-op if no transform is active.
    ///
    /// Called from every site that mutates the painter save stack
    /// outside the coalescing path: `with_transform`'s identity /
    /// mismatch arms, every `LayerStateStack` method on `Backend`,
    /// the public `Backend::restore`, and the `Drop` impl. See the
    /// [`active_transform`](Self::active_transform) field doc for
    /// the full list and the PR #117 review (Codex P1) context.
    fn flush_active_transform(&mut self) {
        if self.active_transform.is_some() {
            self.painter.restore();
            self.active_transform = None;
        }
    }

    /// Shared backdrop-filter blur pipeline used by both backdrop entry points:
    /// the layer-tree path (`Renderer::handle_backdrop_filter`, "Path A") and the
    /// display-list path ([`render_backdrop_filter`](CommandRenderer::render_backdrop_filter),
    /// "Path B"). Consolidating it here is the single-source-of-truth fix for a
    /// duplication that had the off-screen clamp bug fixed twice and silently
    /// drift afterward (Path B truncated where Path A rounded).
    ///
    /// Steps: clamp `device_rect` to the surface extent → copy that region from
    /// the surface into a pooled blur-input → Dual-Kawase blur → queue the result
    /// for compositing at the **clamped** rect. The painter is flushed first so
    /// the pixels to be sampled are present (the flush + copy stay in one
    /// submission — "Fix #11 ordering").
    ///
    /// Returns `true` if the blur was queued, `false` if it was skipped (no
    /// offscreen renderer, or the region is entirely off-screen). The caller
    /// renders the backdrop's children either way — child dispatch differs per
    /// path (layer-tree recursion vs display-list command dispatch) and stays at
    /// the call site.
    ///
    /// `device_rect` is the filter bounds already mapped to device space by the
    /// caller (the transform source differs: the layer-walk CTM vs the
    /// display-list command transform).
    pub(crate) fn apply_backdrop_blur(
        &mut self,
        device_rect: Rect<Pixels>,
        sigma: f32,
        surface_texture: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
    ) -> bool {
        if self.offscreen.is_none() {
            // No offscreen renderer → no blur, and no mid-frame flush either: any
            // painter batches queued before this backdrop still draw in the
            // frame-end flush (the painter's `draw_order` is an explicit ordered
            // list, so pre-backdrop content precedes the caller's children
            // regardless of submit boundaries). Don't "restore" a mid-frame flush
            // here — it would only split one submit into two with no blur to feed.
            tracing::warn!("Backdrop blur skipped: no offscreen renderer available");
            return false;
        }

        // device/queue/format come from the offscreen renderer (the same device
        // the surface was created on); later mutation borrows `offscreen` again
        // sequentially for the texture pool and the blur.
        let (device, queue, format) = {
            let off = self
                .offscreen
                .as_deref_mut()
                .expect("offscreen is_some checked above");
            (
                Arc::clone(off.device()),
                Arc::clone(off.queue()),
                off.surface_format(),
            )
        };

        // Clamp the device rect to the surface extent. `.round()` before
        // truncation avoids a 1-device-pixel undersize on sub-pixel boundaries
        // (DPR ≠ 1 or fractional-offset CTMs) — the canonical clamp both paths
        // now share (Path B previously truncated here, undersizing fractional
        // backdrops). Edges are kept ≥ 0 by the prior `clamp`.
        let surface_extent = surface_texture.size();
        let surface_w = surface_extent.width;
        let surface_h = surface_extent.height;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let x = device_rect.left().0.clamp(0.0, surface_w as f32).round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let y = device_rect.top().0.clamp(0.0, surface_h as f32).round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let right = device_rect.right().0.clamp(0.0, surface_w as f32).round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bottom = device_rect.bottom().0.clamp(0.0, surface_h as f32).round() as u32;
        let w = right.saturating_sub(x).max(1);
        let h = bottom.saturating_sub(y).max(1);

        // Entirely off-screen after clamping → no copyable region.
        if right <= x || bottom <= y {
            tracing::warn!(
                rect_l = device_rect.left().0,
                rect_t = device_rect.top().0,
                rect_r = device_rect.right().0,
                rect_b = device_rect.bottom().0,
                surface_w,
                surface_h,
                "Backdrop blur skipped: clamped device region is empty (entirely off-screen)"
            );
            return false;
        }

        // Flush painter batches so the backdrop pixels are present on the surface
        // before the copy. The copy is recorded into the same encoder, keeping
        // flush → copy in one submission.
        //
        // PROFILER-SKIP: this backdrop-flush encoder is intentionally absent from
        // the GpuFrameProfiler. Backdrop GPU time is not threaded through here
        // (neither backdrop entry point has a profiler handle); the clear-pass and
        // final-render scopes in `render_scene` cover the primary frame timing.
        // This is an explicit trade-off, not an oversight.
        let mut flush_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Backdrop Flush Encoder"),
        });
        let flush_target =
            super::render_target::RenderTarget::sampleable(surface_view, surface_texture);
        if let Err(e) = self.painter.render(flush_target, &mut flush_encoder) {
            tracing::error!("Backdrop flush failed: {}", e);
        }

        // Copy the clamped device region from the surface into a pooled blur input.
        let blur_input = self
            .offscreen
            .as_deref_mut()
            .expect("offscreen is_some checked above")
            .texture_pool()
            .acquire(w, h, format);
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

        // Dual-Kawase blur, then queue for compositing at the CLAMPED rect — the
        // copy used origin (x,y) extent (w,h), so the composite rect must match
        // exactly or the smaller blurred texture would stretch across an
        // unclamped (edge-crossing) `device_rect`.
        let blurred = self
            .offscreen
            .as_deref_mut()
            .expect("offscreen is_some checked above")
            .render_blur(&blur_input, sigma);
        let clamped_composite_rect = Rect::from_xywh(
            Pixels(x as f32),
            Pixels(y as f32),
            Pixels(w as f32),
            Pixels(h as f32),
        );
        self.painter
            .queue_offscreen_result(blurred, clamped_composite_rect);
        true
    }
}

impl Drop for Backend<'_> {
    /// Safety-net: balance any deferred lazy-coalescing save that was left on
    /// the painter stack by `with_transform`. The 21 eager `flush_active_transform`
    /// call sites throughout the impl (every `LayerStateStack` method, the identity
    /// / mismatch arms of `with_transform`, and the `restore` escape hatch) flush at
    /// the correct semantic point. This `Drop` impl is a backstop for any future call
    /// path that forgets to flush: when the Backend goes out of scope the painter is
    /// left balanced and ready for its next use (`painter.render`,
    /// `end_frame_maintenance`, or the next frame's Backend).
    fn drop(&mut self) {
        self.flush_active_transform();
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
        wrap_width: Option<f32>,
        transform: &Matrix4,
    ) {
        // Resolve the buffer-level defaults from the root span's style.
        let root_style = span.style();
        #[allow(clippy::cast_possible_truncation)] // f64 font-size fits in f32 at UI scales
        let base_font_size = root_style.and_then(|s| s.font_size).unwrap_or(14.0) as f32;
        #[allow(clippy::cast_possible_truncation)]
        let scaled_font_size = base_font_size * (text_scale_factor as f32);
        let base_color = root_style
            .and_then(|s| s.foreground.or(s.color))
            .unwrap_or(Color::BLACK);

        // Flatten the span tree into per-run (text, merged style) pairs with
        // text_scale_factor baked into every effective font size.
        // Average and worst case O(total spans + text bytes): one pre-order walk.
        #[allow(clippy::cast_possible_truncation)] // same truncation guard as above
        let runs = crate::wgpu::text::collect_styled_spans(span, text_scale_factor as f32);

        if runs.is_empty() {
            return;
        }

        let position = Point::new(offset.dx, offset.dy);
        self.with_transform(transform, |painter| {
            painter.rich_text(&runs, position, scaled_font_size, base_color, wrap_width);
        });
    }

    fn render_image(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        // Thread paint.blend_mode to the GPU-level composite (PR-5).
        // SrcOver is the correct default when no Paint is supplied.
        let blend_mode = paint.map_or(flui_painting::BlendMode::SrcOver, |p| p.blend_mode);
        self.with_transform(transform, |painter| {
            painter.draw_image(image, dst, blend_mode);
        });
    }

    fn render_atlas(
        &mut self,
        image: &Image,
        sprites: &[Rect<Pixels>],
        transforms: &[Matrix4],
        colors: Option<&[Color]>,
        blend_mode: BlendMode,
        _paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        // Thread blend_mode to the painter so advanced modes divert to
        // DrawItem::AdvancedShape (PR-5, condition 3). SrcOver takes the
        // per-sprite cached_images path unchanged.
        self.with_transform(transform, |painter| {
            painter.draw_atlas(image, sprites, transforms, colors, blend_mode);
        });
    }

    fn render_image_repeat(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        repeat: flui_painting::display_list::ImageRepeat,
        paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        let blend_mode = paint.map_or(flui_painting::BlendMode::SrcOver, |p| p.blend_mode);
        self.with_transform(transform, |painter| {
            painter.draw_image_repeat(image, dst, repeat, blend_mode);
        });
    }

    fn render_image_nine_slice(
        &mut self,
        image: &Image,
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
        paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        let blend_mode = paint.map_or(flui_painting::BlendMode::SrcOver, |p| p.blend_mode);
        self.with_transform(transform, |painter| {
            painter.draw_image_nine_slice(image, center_slice, dst, blend_mode);
        });
    }

    fn render_image_filtered(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        filter: flui_painting::display_list::ColorFilter,
        paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        // Thread paint.blend_mode as the GPU-level composite mode (PR-5).
        // ColorFilter bakes pixels CPU-side; paint.blend_mode composites the
        // result GPU-side against the framebuffer. These two modes are independent.
        // See DrawBatcher::draw_image_filtered for the boundary contract.
        let paint_blend_mode = paint.map_or(flui_painting::BlendMode::SrcOver, |p| p.blend_mode);
        self.with_transform(transform, |painter| {
            painter.draw_image_filtered(image, dst, filter, paint_blend_mode);
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
        // Flush any deferred-coalesced transform from the prior command before
        // reading the CTM. The Backend defers transforms lazily (see
        // `with_transform` / `active_transform`); without this call
        // `current_transform_matrix()` / `current_max_scale()` below would read
        // the PRIOR command's unrelated transform and size/position the offscreen
        // from stale state. Every other method that reads painter transform state
        // calls `flush_active_transform()` first (push_opacity, push_color_filter,
        // push_image_filter, all LayerStateStack impls); shader-mask must match.
        self.flush_active_transform();

        // Try GPU shader mask pipeline
        if self.offscreen.is_some() {
            // SAFETY of subsequent `expect` calls: `is_some()` was true above; each
            // `as_deref_mut()` is a separate sequential borrow that does not overlap
            // with the painter borrows interleaved between them.
            // The live device-pixel ratio rides in the painter's current
            // transform: the `RenderView` root pushes `scale(dpr)` and the paint
            // walk accumulates it into the CTM. The `_transform` argument is
            // identity on the paint path, so it is NOT the DPR source — read the
            // active CTM here, before `reset_frame_state` below clears it.
            //
            // Sizing the offscreen child/result textures from the logical
            // `bounds` would allocate the masked layer at half resolution on a
            // 2x display and composite it at half coordinates / quarter area
            // (Flutter/Impeller `ShaderMaskLayer::Paint` runs the child + masked
            // saveLayer under the accumulated device matrix, so the offscreen is
            // device-resolution and composited in device space).
            let transform = self.painter.current_transform_matrix();
            let dpr_scale = self.painter.current_max_scale().max(1.0);

            // Device-resolution offscreen dimensions: logical extent × DPR.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let dev_width = (bounds.width().0 * dpr_scale).round().max(1.0) as u32;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let dev_height = (bounds.height().0 * dpr_scale).round().max(1.0) as u32;

            // Composite rect in device space — mirrors Path B (backdrop filter):
            // map the logical bounds through the CTM rather than scaling by DPR
            // alone, so any translation in the CTM is honored too.
            let device_bounds = transform.transform_rect(&bounds);

            // Step 1: Get GPU resources from offscreen renderer
            let (device, queue, format, child_tex) = {
                let offscreen = self
                    .offscreen
                    .as_deref_mut()
                    .expect("checked is_some above");
                let device = Arc::clone(offscreen.device());
                let queue = Arc::clone(offscreen.queue());
                let format = offscreen.surface_format();
                let child_tex = offscreen
                    .texture_pool()
                    .acquire(dev_width, dev_height, format);
                (device, queue, format, child_tex)
            };

            // Step 2: Get or create cached offscreen painter (avoids per-call allocation)
            // Ensure the cache is populated (creates or resizes as needed), then borrow
            // it for command dispatch. No take/put-back needed: the Backend borrows
            // `&mut WgpuPainter` directly from `self.offscreen_painter`, and the Drop
            // impl on the temp Backend guarantees `flush_active_transform()` runs when
            // the dispatch scope ends — leaving the cached painter balanced for its next
            // use (render call below, or the next ShaderMask in this frame).
            // The cached painter's render target is the device-sized child texture,
            // so it must be sized at device resolution too.
            let _ = self.get_or_create_offscreen_painter(
                &device,
                &queue,
                format,
                (dev_width, dev_height),
            );
            {
                let offscreen_painter = self
                    .offscreen_painter
                    .as_mut()
                    .expect("offscreen_painter was just populated by get_or_create");
                // Reset per-frame clip/transform/opacity state before rendering
                // into this painter.  Without this, a clip_rect command from a
                // previous ShaderMask call in the same frame leaks
                // `current_scissor` / `current_rrect_clip` into the next one,
                // causing the second ShaderMask's child content to be silently
                // clipped to the prior mask's scissor region.
                offscreen_painter.reset_frame_state();
                // After reset the CTM is identity. The child DisplayList carries
                // logical coordinates, so scale by the DPR to bake it into the
                // device-sized offscreen — without this the child renders into
                // the top-left logical quadrant of the device texture.
                if (dpr_scale - 1.0).abs() > f32::EPSILON {
                    offscreen_painter.scale(dpr_scale, dpr_scale);
                }
                let mut temp_backend = Backend::new(offscreen_painter);
                for command in child.commands() {
                    dispatch_command(command, &mut temp_backend);
                }
                // temp_backend drops here → Drop impl calls flush_active_transform(),
                // balancing any deferred lazy-coalescing save on the offscreen painter.
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
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            // Render child batches to the sampleable offscreen texture.
            // `child_tex` is a pooled texture that carries COPY_SRC, so passing
            // `sampleable` here lets any advanced-blend op inside the child
            // display list dst-read from the child's own offscreen as backdrop,
            // producing correct Multiply/Screen/etc. output rather than falling
            // back to SrcOver.
            let child_target = super::render_target::RenderTarget::sampleable(
                child_tex.view(),
                child_tex.texture(),
            );
            let offscreen_painter = self
                .offscreen_painter
                .as_mut()
                .expect("offscreen_painter was populated in step 2 and not moved");
            if let Err(e) = offscreen_painter.render(child_target, &mut encoder) {
                tracing::error!("Failed to render shader mask child content: {}", e);
            }
            queue.submit(std::iter::once(encoder.finish()));

            // Step 5: Apply shader mask via GPU pipeline. The result texture is
            // sized at device resolution; `bounds` stays logical so the shader's
            // gradient-endpoint normalization (scale-invariant) lands correctly.
            let result_size = Size::new(Pixels(dev_width as f32), Pixels(dev_height as f32));
            let masked_texture = {
                let offscreen = self
                    .offscreen
                    .as_deref_mut()
                    .expect("checked is_some above");
                let result = offscreen.render_masked(
                    bounds,
                    result_size,
                    shader,
                    blend_mode,
                    child_tex.texture(),
                );
                result.into_texture()
            };

            // Step 6: Queue masked result for compositing on main target at the
            // device-space rect (logical `bounds` would composite at half
            // scale/position on a HiDPI frame).
            self.painter
                .queue_offscreen_result(masked_texture, device_bounds);

            tracing::debug!(
                "ShaderMask GPU pipeline complete: bounds={:?}, device_bounds={:?}, \
                 dpr_scale={}, child_size={}x{}",
                bounds,
                device_bounds,
                dpr_scale,
                dev_width,
                dev_height
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
        // SrcOver-by-contract: DrawGradient / DrawGradientRRect display-list commands
        // carry no Paint upstream, so advanced blend is unreachable for gradient-rect
        // draws via this path. Shape-with-shader-paint (the gradient advanced blend
        // producer) is handled by dispatch_shader_rect in batches/gradients.rs.
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

                    let gradient_stops = build_gradient_stops(colors, stops.as_ref());
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

                    let gradient_stops = build_gradient_stops(colors, stops.as_ref());
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

                    let gradient_stops = build_gradient_stops(colors, stops.as_ref());
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
        // SrcOver-by-contract: DrawGradientRRect carries no Paint upstream, so advanced
        // blend is unreachable here. See render_gradient for the full rationale.
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

                    let gradient_stops = build_gradient_stops(colors, stops.as_ref());
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

                    let gradient_stops = build_gradient_stops(colors, stops.as_ref());
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

                    let gradient_stops = build_gradient_stops(colors, stops.as_ref());
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

    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            let viewport_bounds = painter.viewport_bounds();
            // Carry the command's blend mode so the full-viewport fill composites
            // correctly (e.g. `DrawColor` with `Clear` punches out the layer).
            let paint = Paint::fill(color).with_blend_mode(blend_mode);
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
        // `_blend_mode` is intentionally dropped here. Advanced blend on a
        // BackdropFilter is a separate future Path-A backdrop-compositor seam,
        // out of PR-5 scope. PR-5 covers shape/gradient/image producers only.
        use flui_painting::display_list::ImageFilter;

        // Dispatch the child display list (or no-op when None). Used both as the
        // fall-back when no blur is applied (non-blur filter / no surface) AND as
        // the success-path child dispatch after `apply_backdrop_blur` — children
        // always render, with or without a backdrop behind them.
        //
        // Each `DrawCommand` carries its own pre-composited transform from
        // display-list capture, so the outer `transform` is NOT re-applied here
        // (it is consumed only to map `bounds` to device space below). Re-applying
        // it would double-transform; Path A (`Renderer::handle_backdrop_filter`)
        // dispatches children the same way. (PR #110 F-W2-4: an earlier version
        // wrapped this in `with_transform(transform, |_| {})`, whose empty closure
        // balanced save with restore immediately — a misleading no-op.)
        let dispatch_children = |this: &mut Self| {
            if let Some(child) = child {
                for command in child.commands() {
                    dispatch_command(command, this);
                }
            }
        };

        // Path B (DisplayList-command level) backdrop filter. Shares the
        // offscreen blur pipeline with Path A (`Renderer::handle_backdrop_filter`,
        // layer-tree level) via `Backend::apply_backdrop_blur` — the clamp +
        // copy + blur + composite live there once, so the off-screen-clamp
        // handling can no longer drift between the two paths. Non-blur filters
        // and a missing surface fall back to passthrough with a `warn!` so the
        // gap stays observable.
        let sigma = match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => f32::midpoint(*sigma_x, *sigma_y),
            other => {
                tracing::warn!(
                    "Backdrop filter type {:?} not supported in DisplayList path; passthrough",
                    other
                );
                dispatch_children(self);
                return;
            }
        };

        let (Some(surface_view), Some(surface_texture)) = (self.surface_view, self.surface_texture)
        else {
            tracing::warn!(
                "Backdrop filter: no surface bound via bind_surface(); passthrough \
                 (the surface handles are bound in `Renderer::render` only -- \
                 the shader-mask offscreen path does not bind them, which is expected)"
            );
            dispatch_children(self);
            return;
        };

        // Map local `bounds` to device space (the command carries the outer
        // transform), then run the shared blur (no-op return if off-screen or no
        // offscreen renderer — children still render below either way).
        let device_rect = transform.transform_rect(&bounds);
        self.apply_backdrop_blur(device_rect, sigma, surface_texture, surface_view);

        // Dispatch the child display list on top of the (maybe-)blurred backdrop.
        // Each child `DrawCommand` carries its own pre-composited transform, so
        // no outer transform wrap is needed (Path A dispatches children the same).
        dispatch_children(self);
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
    ) -> std::sync::Arc<flui_types::painting::Path> {
        // Override the trait default (which freshly generates the path
        // every call, no caching). Delegate to the Painter-owned bounded
        // cache so identical superellipses across frames reuse the cached
        // tessellation. Cache hits pay only for an Arc::clone; the
        // ~256-command path is never deep-copied. Cache eviction follows
        // PathCache semantics (`max_entries` + `last_used_frame`).
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
    // Cycle 4 wave 5 PR #117 review (Codex P1): every method on
    // this trait must call `self.flush_active_transform()` BEFORE
    // any `painter.save` / `painter.restore` / `painter.save_layer`
    // / `painter.restore_layer` op. E-13's `with_transform` leaves
    // a deferred `save()` active across consecutive same-matrix
    // calls; if a layer-tree boundary (push_clip etc.) intervened
    // without flushing first, the layer's matched
    // `pop_clip`/`pop_layer` would pop the lazy save instead of
    // its own, leaking state across sibling layers. Flushing here
    // re-establishes the invariant that `active_transform == Some`
    // implies the painter has that transform at the TOP of its
    // save stack.
    //
    // The flush is a no-op when no lazy transform is active, so
    // the cost is one branch per layer-stack call -- negligible
    // versus the save_layer/clip_path GPU work that follows.

    fn push_clip_rect(&mut self, rect: &Rect<Pixels>, _clip_behavior: flui_types::painting::Clip) {
        self.flush_active_transform();
        self.painter.save();
        self.painter.clip_rect(*rect);
    }

    fn push_clip_rrect(&mut self, rrect: &RRect, _clip_behavior: flui_types::painting::Clip) {
        self.flush_active_transform();
        self.painter.save();
        self.painter.clip_rrect(*rrect);
    }

    fn push_clip_path(&mut self, path: &Path, _clip_behavior: flui_types::painting::Clip) {
        self.flush_active_transform();
        self.painter.save();
        self.painter.clip_path(path);
    }

    fn pop_clip(&mut self) {
        self.flush_active_transform();
        self.painter.restore();
    }

    fn push_offset(&mut self, offset: Offset<Pixels>) {
        self.flush_active_transform();
        self.painter.save();
        self.painter.translate(offset);
    }

    fn push_transform(&mut self, transform: &Matrix4) {
        self.flush_active_transform();
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
        self.flush_active_transform();
        self.painter.restore();
    }

    fn push_opacity(&mut self, alpha: f32) {
        self.flush_active_transform();
        // Create a layer with opacity (clamped to [0, 255]).
        // Blend mode defaults to SrcOver via Paint::fill.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let alpha_u8 = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let paint = Paint::fill(Color::WHITE).with_alpha(alpha_u8);
        self.painter.save_layer(None, &paint);
    }

    fn push_opacity_blend(&mut self, alpha: f32, blend: flui_types::painting::BlendMode) {
        self.flush_active_transform();
        // Propagate the explicit blend mode into the saveLayer paint so the
        // compositor reads it from `paint.blend_mode` and routes the layer
        // through the dst-read advanced compositor path when needed.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let alpha_u8 = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let paint = Paint::fill(Color::WHITE)
            .with_alpha(alpha_u8)
            .with_blend_mode(blend);
        self.painter.save_layer(None, &paint);
    }

    fn pop_opacity(&mut self) {
        self.flush_active_transform();
        self.painter.restore_layer();
    }

    fn push_color_filter(&mut self, filter: &flui_types::painting::ColorFilter) {
        use flui_types::painting::ColorFilter;

        self.flush_active_transform();

        match filter {
            ColorFilter::Matrix(m) => {
                // Identity fast-path: no-op layer keeps push/pop balanced.
                // Exact f32 comparison is correct: `ColorMatrix::identity()` is
                // built from bit-exact 0.0/1.0 literals, so a transitive equality
                // check correctly skips the GPU pass without ULP slop.
                let identity = flui_types::painting::effects::ColorMatrix::identity();
                #[expect(
                    clippy::float_cmp,
                    reason = "identity matrix is bit-exact (0.0/1.0 literals); exact comparison is correct"
                )]
                if m.values == identity.values {
                    self.painter.save_layer(None, &Paint::fill(Color::WHITE));
                    tracing::trace!("push_color_filter: identity matrix — no-op layer");
                    return;
                }
                // Real color-matrix: open a filter layer.  The GPU shader applies the
                // full 5×4 matrix per-pixel (unpremul → matrix → clamp → repremul).
                self.painter
                    .save_layer_with_filter(None, LayerFilter::ColorMatrix(m.values));
                tracing::trace!(
                    matrix = ?m.values,
                    "push_color_filter: GPU color-matrix filter layer"
                );
            }
            ColorFilter::Mode { color, blend_mode } => {
                // Blend a solid filter color over each layer pixel via the
                // Porter-Duff / W3C blend equation.  Unpremul → blend → clamp →
                // repremul is applied by the GPU shader.
                self.painter.save_layer_with_filter(
                    None,
                    LayerFilter::Mode {
                        color: color.to_f32_array(),
                        blend_mode: *blend_mode,
                    },
                );
                tracing::trace!(
                    ?color,
                    ?blend_mode,
                    "push_color_filter: GPU mode filter layer"
                );
            }
            ColorFilter::LinearToSrgbGamma => {
                // Linear-light → sRGB-encoded transfer per RGB channel; alpha
                // passes through unchanged.
                self.painter
                    .save_layer_with_filter(None, LayerFilter::Gamma(GammaDirection::LinearToSrgb));
                tracing::trace!("push_color_filter: GPU LinearToSrgb gamma filter layer");
            }
            ColorFilter::SrgbToLinearGamma => {
                // sRGB-encoded → linear-light transfer per RGB channel; alpha
                // passes through unchanged.
                self.painter
                    .save_layer_with_filter(None, LayerFilter::Gamma(GammaDirection::SrgbToLinear));
                tracing::trace!("push_color_filter: GPU SrgbToLinear gamma filter layer");
            }
            // `ColorFilter` is `#[non_exhaustive]`; a wildcard arm is required by
            // the compiler.  Open a balanced no-op layer so `pop_color_filter` has a
            // matching restore, and warn once so unknown variants surface in logs.
            _ => {
                tracing::warn!(
                    "push_color_filter: unknown ColorFilter variant (future extension?) \
                     — falling back to no-op layer to keep push/pop balanced"
                );
                self.painter.save_layer(None, &Paint::fill(Color::WHITE));
            }
        }
    }

    fn pop_color_filter(&mut self) {
        self.flush_active_transform();
        self.painter.restore_layer();
    }

    fn push_image_filter(&mut self, filter: &flui_painting::display_list::ImageFilter) {
        use flui_painting::display_list::ImageFilter;

        self.flush_active_transform();

        match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                // Full GPU separable Gaussian blur via two H/V sub-passes
                // (PINNED #2: premultiplied-direct, sRGB-encoded, √3·σ kernel).
                self.painter
                    .save_layer_with_image_filter(ImageFilterSpec::Blur {
                        sigma_x: *sigma_x,
                        sigma_y: *sigma_y,
                    });
                tracing::trace!(
                    sigma_x,
                    sigma_y,
                    "push_image_filter(Blur): GPU Gaussian blur layer opened"
                );
            }
            ImageFilter::Dilate { radius } => {
                self.painter
                    .save_layer_with_image_filter(ImageFilterSpec::Morph {
                        radius: *radius,
                        op: MorphOp::Dilate,
                    });
                tracing::trace!(
                    radius,
                    "push_image_filter(Dilate): GPU morphology dilate layer opened"
                );
            }
            ImageFilter::Erode { radius } => {
                self.painter
                    .save_layer_with_image_filter(ImageFilterSpec::Morph {
                        radius: *radius,
                        op: MorphOp::Erode,
                    });
                tracing::trace!(
                    radius,
                    "push_image_filter(Erode): GPU morphology erode layer opened"
                );
            }
            ImageFilter::Matrix(matrix) => {
                // Full GPU color-matrix pass — no approximation.
                self.painter
                    .save_layer_with_filter(None, LayerFilter::ColorMatrix(matrix.values));
                tracing::trace!(
                    matrix = ?matrix.values,
                    "push_image_filter(Matrix): GPU color-matrix filter layer"
                );
            }
            ImageFilter::ColorAdjust(adjustment) => {
                // Promote to a color matrix so the same GPU pass handles it.
                let matrix = adjustment.to_color_matrix();
                self.painter
                    .save_layer_with_filter(None, LayerFilter::ColorMatrix(matrix.values));
                tracing::trace!(
                    ?adjustment,
                    "push_image_filter(ColorAdjust): GPU color-matrix filter layer"
                );
            }
            ImageFilter::Compose(filters) => {
                // Flatten the AST at record time (depth-first, left-to-right =
                // inner-first, PINNED #4).  The resulting flat `passes` vec is
                // carried by `ImageFilterSpec::Chain` and handed to `restore_layer`,
                // which emits a single `DrawItem::Filter` with `cumulative_growth`
                // bounds and the full ordered chain.
                let mut passes: SmallVec<[ImageFilterPass; 4]> = SmallVec::new();
                flatten_compose(filters, &mut passes);

                if passes.is_empty() {
                    // Degenerate empty Compose — no filter math to apply.  Open a
                    // plain group layer so that save/restore remains balanced without
                    // emitting a `DrawItem::Filter` for a zero-length chain.
                    let paint = Paint::fill(Color::WHITE);
                    self.painter.save_layer(None, &paint);
                    tracing::trace!(
                        "push_image_filter(Compose): empty Compose — opened plain group"
                    );
                } else {
                    self.painter
                        .save_layer_with_image_filter(ImageFilterSpec::Chain(passes));
                    tracing::trace!(
                        "push_image_filter(Compose): {} flattened passes",
                        filters.len(),
                    );
                }
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
        self.flush_active_transform();
        self.painter.restore_layer();
    }
}

// ─── Compose flatten ──────────────────────────────────────────────────────────

/// Flatten a `Compose(Vec<ImageFilter>)` AST into an ordered `ImageFilterPass` vec.
///
/// Traverses `filters` depth-first, left-to-right (index 0 = innermost = applied
/// first, PINNED #4 verified against Flutter `dl_compose_image_filter.cc:33-51`).
/// Nested `Compose` nodes are recursed into at record time — the resulting `out`
/// vec is flat with no GPU-side recursion and no nested IR.
///
/// ## Variant mapping
///
/// - `Blur{σx,σy}` → [`ImageFilterPass::Blur`]
/// - `Dilate{r}` → [`ImageFilterPass::Morph`] (op: Dilate)
/// - `Erode{r}` → [`ImageFilterPass::Morph`] (op: Erode)
/// - `Matrix(m)` → [`ImageFilterPass::ColorMatrix`] (`m.values`)
/// - `ColorAdjust(a)` → [`ImageFilterPass::ColorMatrix`] (`a.to_color_matrix().values`)
/// - nested `Compose(inner)` → recurse (depth-first, index order preserved)
/// - `OverflowIndicator` (debug only) → [`ImageFilterPass::Identity`] + `tracing::debug!`
///   (index-faithful: never elide, never shift sibling positions)
///
/// ## No `_` catch-all
///
/// The inner `match` is exhaustive: adding a new `ImageFilter` variant forces
/// a compile error here, ensuring the flatten stays up-to-date.
pub(crate) fn flatten_compose(
    filters: &[flui_painting::display_list::ImageFilter],
    out: &mut SmallVec<[ImageFilterPass; 4]>,
) {
    use flui_painting::display_list::ImageFilter;
    for filter in filters {
        match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                out.push(ImageFilterPass::Blur {
                    sigma_x: *sigma_x,
                    sigma_y: *sigma_y,
                });
            }
            ImageFilter::Dilate { radius } => {
                out.push(ImageFilterPass::Morph {
                    radius: *radius,
                    op: MorphOp::Dilate,
                });
            }
            ImageFilter::Erode { radius } => {
                out.push(ImageFilterPass::Morph {
                    radius: *radius,
                    op: MorphOp::Erode,
                });
            }
            ImageFilter::Matrix(matrix) => {
                out.push(ImageFilterPass::ColorMatrix(matrix.values));
            }
            ImageFilter::ColorAdjust(adjustment) => {
                out.push(ImageFilterPass::ColorMatrix(
                    adjustment.to_color_matrix().values,
                ));
            }
            ImageFilter::Compose(inner_filters) => {
                // Depth-first recursion: inner_filters[0] is innermost at this level.
                flatten_compose(inner_filters, out);
            }
            #[cfg(debug_assertions)]
            ImageFilter::OverflowIndicator {
                overflow_h,
                overflow_v,
                ..
            } => {
                // Push Identity to preserve index positions — eliding would shift
                // sibling filter positions, changing the fold order (PINNED #4).
                out.push(ImageFilterPass::Identity);
                tracing::debug!(
                    overflow_h,
                    overflow_v,
                    "flatten_compose(OverflowIndicator): no GPU pass for overflow \
                     indicator inside Compose; pushing Identity to preserve chain indices"
                );
            }
        }
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;
    use crate::traits::CommandRenderer;

    /// Acquire a real device/queue. Returns `None` when no GPU adapter exists
    /// (CI without a GPU), so the test skips gracefully.
    fn test_device_and_queue() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ShaderMask HiDPI Test Device"),
            ..Default::default()
        }))
        .ok()?;
        Some((Arc::new(device), Arc::new(queue)))
    }

    /// BUG 2 (HiDPI shader mask): the offscreen child/result textures must be
    /// allocated at DEVICE resolution (`bounds * dpr`) and the masked result
    /// composited at the device-space rect, sourcing the DPR from the live
    /// painter CTM (not the identity `_transform` paint-path argument).
    ///
    /// Under a `scale(2)` CTM a `ShaderMask` over logical bounds (0,0,100,100)
    /// must allocate a 200x200 offscreen and composite at device (0,0,200,200).
    /// Red before the fix: 100x100 offscreen composited at (0,0,100,100)
    /// (half resolution, quarter area).
    #[test]
    fn shader_mask_offscreen_is_device_sized_under_dpr() {
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU here; skip gracefully.
        };

        let format = wgpu::TextureFormat::Bgra8Unorm;

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (800, 800),
        );
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        // Simulate the `RenderView` DPR root transform on the live CTM.
        backend.painter_mut().scale(2.0, 2.0);

        // Child display list: a single 100x100 rect, built via the public Canvas.
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            &Paint::fill(Color::WHITE),
        );
        let child = canvas.finish();

        let shader = flui_painting::Shader::solid(Color::WHITE);
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        // `_transform` is identity on the paint path — the DPR comes from the CTM.
        backend.render_shader_mask(
            &child,
            &shader,
            bounds,
            BlendMode::SrcOver,
            &Matrix4::IDENTITY,
        );

        let results = backend.painter().offscreen_results_for_test();
        assert_eq!(
            results.len(),
            1,
            "shader mask must queue exactly one offscreen composite"
        );
        let (composite_rect, tex_w, tex_h) = results[0];

        // Result texture allocated at device resolution.
        assert_eq!(
            (tex_w, tex_h),
            (200, 200),
            "shader-mask offscreen must be device-sized (200x200) under DPR=2; \
             got {tex_w}x{tex_h} (100x100 means the CTM scale was dropped)"
        );

        // Composited at the device-space rect (0,0,200,200).
        assert!(
            composite_rect.left().0.abs() < 0.5
                && composite_rect.top().0.abs() < 0.5
                && (composite_rect.right().0 - 200.0).abs() < 0.5
                && (composite_rect.bottom().0 - 200.0).abs() < 0.5,
            "shader-mask composite rect must span device (0,0,200,200) under DPR=2; \
             got {composite_rect:?}"
        );
    }

    /// Locks the gradient-uniform / device-sizing split under HiDPI.
    ///
    /// `render_masked` receives `child_bounds` in LOGICAL pixels (for gradient
    /// normalization) but `result_size` in DEVICE pixels. A regression that passes
    /// device-sized bounds as `child_bounds` would misplace gradient stop positions;
    /// a regression that passes logical size as `result_size` would under-allocate.
    ///
    /// This test uses a LINEAR-GRADIENT shader (not solid) to exercise the
    /// gradient-uniform branch. Under DPR=2 over logical bounds (0,0,100,100):
    /// - result texture must be 200×200 (device-sized)
    /// - composite rect must cover device (0,0,200,200)
    /// - the dispatch must not panic (gradient uniforms stay in logical space)
    ///
    /// Red before the fix: result texture is 100×100 (logical size fed as
    /// `result_size`) and composite rect is (0,0,100,100).
    #[test]
    fn shader_mask_gradient_is_device_sized_under_dpr() {
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use flui_types::painting::TileMode;

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU here; skip gracefully.
        };

        let format = wgpu::TextureFormat::Bgra8Unorm;

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (800, 800),
        );
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        // Simulate DPR=2 on the live CTM.
        backend.painter_mut().scale(2.0, 2.0);

        // Child display list: a white rect matching the logical mask bounds.
        let mut canvas = flui_painting::Canvas::new();
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        canvas.draw_rect(bounds, &Paint::fill(Color::WHITE));
        let child = canvas.finish();

        // Linear gradient: black-to-white left-to-right across the logical bounds.
        // `to_mask_uniform_data` normalizes endpoints as (endpoint - origin) / extent;
        // both endpoints and `child_bounds` are logical, so normalization is correct.
        let shader = flui_painting::Shader::linear_gradient(
            Offset::new(px(0.0), px(0.0)),
            Offset::new(px(100.0), px(0.0)),
            vec![Color::BLACK, Color::WHITE],
            None,
            TileMode::Clamp,
        );

        backend.render_shader_mask(
            &child,
            &shader,
            bounds,
            BlendMode::SrcOver,
            &Matrix4::IDENTITY,
        );

        let results = backend.painter().offscreen_results_for_test();
        assert_eq!(
            results.len(),
            1,
            "gradient shader mask must queue exactly one offscreen composite"
        );
        let (composite_rect, tex_w, tex_h) = results[0];

        // Result texture must be device-sized: 200×200 under DPR=2.
        assert_eq!(
            (tex_w, tex_h),
            (200, 200),
            "gradient shader-mask offscreen must be device-sized (200×200) under DPR=2; \
             got {tex_w}×{tex_h} — logical sizing means the CTM scale was ignored for \
             result_size"
        );

        // Composite must cover device (0,0,200,200).
        assert!(
            composite_rect.left().0.abs() < 0.5
                && composite_rect.top().0.abs() < 0.5
                && (composite_rect.right().0 - 200.0).abs() < 0.5
                && (composite_rect.bottom().0 - 200.0).abs() < 0.5,
            "gradient shader-mask composite rect must be (0,0,200,200) under DPR=2; \
             got {composite_rect:?}"
        );
    }

    /// Locks P2 #1: `render_shader_mask` must flush the backend's deferred
    /// transform coalescing state before reading the painter CTM.
    ///
    /// The Backend's `with_transform` mechanism batches consecutive same-matrix
    /// draw calls by leaving a `save()+apply` on the painter stack between calls,
    /// clearing it lazily on the next transform change. If `render_shader_mask`
    /// reads `current_transform_matrix()` / `current_max_scale()` WITHOUT first
    /// calling `flush_active_transform()`, it reads the PRIOR command's transform
    /// stacked on top of the DPR root, and sizes/positions the offscreen from
    /// stale state.
    ///
    /// Scenario:
    ///   1. Main painter CTM = scale(2) (DPR root, pushed by the test before
    ///      any commands, simulating RenderView).
    ///   2. `render_rect` is called with a non-identity per-command transform
    ///      `scale(3, 3, 1)`. `with_transform` leaves `active_transform =
    ///      Some(scale3)` on the stack (the painter's current transform matrix
    ///      is now scale(2)*scale(3) = scale(6) in the lazy-save layer).
    ///   3. `render_shader_mask` is called for bounds (0,0,100,100) under the
    ///      DPR=2 root CTM (no additional per-mask transform, _transform=identity).
    ///
    ///   Without the flush: `current_max_scale()` returns ≈6.0 (scale(6) from
    ///   the still-active rect transform), device_size ≈ 600, composite rect ≈
    ///   (0,0,600,600).
    ///   With the flush (fix): `flush_active_transform()` restores the lazy save,
    ///   `current_max_scale()` returns ≈2.0 (the DPR root only), device_size =
    ///   200, composite rect = (0,0,200,200).
    ///
    /// Red-before: tex 600×600, composite (0,0,600,600).
    /// Green-after: tex 200×200, composite (0,0,200,200).
    #[test]
    fn shader_mask_uses_own_transform_not_prior_command() {
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU here; skip gracefully.
        };

        let format = wgpu::TextureFormat::Bgra8Unorm;

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (800, 800),
        );
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        // Step 1: push the DPR root scale(2) into the main painter CTM — this
        // happens before any command dispatch in a real frame.
        backend.painter_mut().scale(2.0, 2.0);

        // Step 2: dispatch a rect with a non-identity per-command transform. This
        // calls `with_transform(scale3_matrix, …)` which leaves
        // `active_transform = Some(scale3)` and the painter's CTM temporarily at
        // scale(2)*scale(3) = scale(6).
        let scale3_matrix = Matrix4::scaling(3.0, 3.0, 1.0);
        let prior_rect_bounds = Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0));
        backend.render_rect(prior_rect_bounds, &Paint::fill(Color::RED), &scale3_matrix);

        // Step 3: build the ShaderMask child display list and issue the mask
        // under the DPR=2 root CTM (_transform = identity, as on the paint path).
        let mut canvas = flui_painting::Canvas::new();
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        canvas.draw_rect(bounds, &Paint::fill(Color::WHITE));
        let child = canvas.finish();
        let shader = flui_painting::Shader::solid(Color::WHITE);

        backend.render_shader_mask(
            &child,
            &shader,
            bounds,
            BlendMode::SrcOver,
            &Matrix4::IDENTITY,
        );

        let results = backend.painter().offscreen_results_for_test();
        assert_eq!(
            results.len(),
            1,
            "shader mask must queue exactly one offscreen composite"
        );
        let (composite_rect, tex_w, tex_h) = results[0];

        // Must be sized at DPR=2 (200×200), NOT at scale(2)*scale(3)=scale(6)
        // (600×600) which would be the result if the deferred transform was not
        // flushed before reading the CTM.
        assert_eq!(
            (tex_w, tex_h),
            (200, 200),
            "shader-mask offscreen must be 200×200 (DPR=2 only); got {tex_w}×{tex_h} — \
             600×600 indicates the prior rect's deferred scale(3) was not flushed \
             before the mask read the CTM (stale active_transform leak)"
        );

        // Composite rect must also derive from the DPR=2 CTM, not scale(6).
        assert!(
            composite_rect.left().0.abs() < 0.5
                && composite_rect.top().0.abs() < 0.5
                && (composite_rect.right().0 - 200.0).abs() < 0.5
                && (composite_rect.bottom().0 - 200.0).abs() < 0.5,
            "shader-mask composite rect must be (0,0,200,200) under DPR=2; \
             got {composite_rect:?} — (0,0,600,600) indicates stale prior-rect \
             transform leaked into the mask CTM read"
        );
    }

    // ── ShaderMask child with advanced blend: Multiply applies, no panic ─────

    /// Regression test for BLOCKER 2a: a ShaderMask whose child DisplayList
    /// contains an advanced-blend shape (Multiply) must dst-read from the child's
    /// own offscreen — NOT fall back to SrcOver and NOT panic.
    ///
    /// **Setup:**
    /// - Child display list: (1) opaque blue rect (SrcOver), (2) opaque orange
    ///   rect with `BlendMode::Multiply`.  The sampleable child texture means
    ///   the Multiply shape dst-reads the blue backdrop that step (1) laid down.
    /// - Solid-white shader mask (passthrough alpha) — the mask does not change
    ///   the color, so the composited result on the main surface should reflect
    ///   the advanced blend outcome.
    ///
    /// **What this proves:**
    /// - Pre-fix (`view_only`): advanced shape fell back to SrcOver → orange.
    /// - Post-fix (`sampleable`): advanced shape dst-reads blue → Multiply(orange,
    ///   blue) = darker value in R and G channels.
    ///
    /// The test asserts no panic, and that the composited center pixel is NOT
    /// orange-like (SrcOver), but darker (Multiply).
    #[test]
    fn shader_mask_child_advanced_blend_uses_sampleable_not_srcover() {
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;
        use flui_types::{Color, Rect, geometry::Pixels, painting::BlendMode};

        // Use Rgba8Unorm so readback values are straightforward.
        const W: u32 = 64;
        const H: u32 = 64;

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let bounds = Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(W as f32), Pixels(H as f32));

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (W, H),
        );

        // Build child display list:
        //   Step 1 — opaque blue rect (SrcOver): establishes child backdrop.
        //   Step 2 — opaque orange rect (Multiply): must dst-read the blue backdrop.
        let blue_color = Color::rgba(40, 60, 220, 255);
        let orange_color = Color::rgba(200, 120, 40, 255);
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_rect(bounds, &Paint::fill(blue_color));
        canvas.draw_rect(
            bounds,
            &Paint::fill(orange_color).with_blend_mode(BlendMode::Multiply),
        );
        let child = canvas.finish();

        // Solid-white shader: passthrough — mask does not alter child colors.
        let shader = flui_painting::Shader::solid(Color::WHITE);

        // Call the method under test — must not panic.
        {
            let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);
            backend.render_shader_mask(
                &child,
                &shader,
                bounds,
                BlendMode::SrcOver,
                &Matrix4::IDENTITY,
            );
            // backend drops here → Drop impl calls flush_active_transform() so
            // the painter borrow is released cleanly before painter.render below.
        }

        // Render the painter onto a sampleable main surface to composite the
        // queued ShaderMask offscreen result.
        let main_surface = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ShaderMask Advanced Test Surface"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let main_view = main_surface.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ShaderMask Advanced Test Encoder"),
        });
        // Pre-clear main surface to black (so non-opaque composites are visible).
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Surface Clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &main_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        // Composite the ShaderMask result onto the main surface.
        // The backend was dropped above (releasing the borrow); painter is
        // now exclusively accessible for the final render call.
        let render_target = RenderTarget::sampleable(&main_view, &main_surface);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        // Readback center pixel of the main surface.
        let pixel = {
            let bytes_per_pixel = 4u32;
            let unpadded = W * bytes_per_pixel;
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let padded = unpadded.div_ceil(align) * align;
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ShaderMask Advanced Readback"),
                size: u64::from(padded * H),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut copy_enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ShaderMask Advanced Readback Encoder"),
            });
            copy_enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &main_surface,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(H),
                    },
                },
                wgpu::Extent3d {
                    width: W,
                    height: H,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(std::iter::once(copy_enc.finish()));
            let slice = buf.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .expect("readback poll must complete");
            let data = slice.get_mapped_range();
            let center = (H / 2) as usize * padded as usize + (W / 2) as usize * 4;
            let px = [
                data[center],
                data[center + 1],
                data[center + 2],
                data[center + 3],
            ];
            drop(data);
            buf.unmap();
            px
        };

        // CPU oracle: Multiply(orange, blue).
        let blend_result = orange_color.blend(blue_color, BlendMode::Multiply);
        let [br, bg, bb, ba] = blend_result.to_f32_array();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped [0,1]*255; truncation safe"
        )]
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let multiply_oracle = [to_u8(br * ba), to_u8(bg * ba), to_u8(bb * ba), to_u8(ba)];

        // SrcOver of orange over blue: opaque orange wins (R≈200, G≈120, B≈40).
        let srcover_result = orange_color.blend(blue_color, BlendMode::SrcOver);
        let [sr, sg, sb, sa] = srcover_result.to_f32_array();
        let srcover_oracle = [to_u8(sr * sa), to_u8(sg * sa), to_u8(sb * sa), to_u8(sa)];

        let tol = 6i16; // ±6: absorbs shader-mask pipeline quantization
        let within = |a: u8, b: u8| (i16::from(a) - i16::from(b)).abs() <= tol;
        let matches_multiply = pixel
            .iter()
            .zip(multiply_oracle.iter())
            .all(|(&a, &b)| within(a, b));
        let matches_srcover = pixel
            .iter()
            .zip(srcover_oracle.iter())
            .all(|(&a, &b)| within(a, b));

        // Must not be SrcOver (the pre-fix fallback).
        assert!(
            !matches_srcover,
            "ShaderMask child advanced Multiply must NOT fall back to SrcOver; \
             pixel={pixel:?} matches srcover_oracle={srcover_oracle:?}. \
             Indicates `child_target` is still `view_only` instead of `sampleable`."
        );

        // Must match the Multiply oracle (or at least be non-trivially different from SrcOver).
        assert!(
            matches_multiply,
            "ShaderMask child advanced Multiply must match the CPU Multiply oracle; \
             pixel={pixel:?}, multiply_oracle={multiply_oracle:?}, srcover_oracle={srcover_oracle:?}."
        );
    }

    // ── Backdrop filter: composite rect is clamped when crossing edge ─────────

    /// Path B (`render_backdrop_filter`) must composite the blurred texture at the
    /// CLAMPED rect that matches the copy source, not at the unclamped `device_rect`.
    ///
    /// Setup: 64×64 surface, backdrop `bounds` covering (32,32)→(100,100) in local
    /// space with identity `transform`.  After transform: `device_rect` spans
    /// x=32..100, y=32..100 (w=68, h=68), but the surface is only 64 wide/tall, so
    /// the clamped copy region is x=32, y=32, w=32, h=32.
    ///
    /// Pre-fix: `queue_offscreen_result` receives the unclamped `device_rect`
    /// (left=32, width=68) → composite_rect.width() ≈ 68 → test FAILS.
    /// Post-fix: `queue_offscreen_result` receives the clamped rect
    /// (left=32, width=32) → composite_rect.width() ≈ 32 → test PASSES.
    #[test]
    fn backdrop_filter_composites_at_clamped_rect_when_crossing_edge() {
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use flui_painting::display_list::ImageFilter;

        const SURFACE_W: u32 = 64;
        const SURFACE_H: u32 = 64;

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Bgra8Unorm;

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (SURFACE_W, SURFACE_H),
        );

        // Create a minimal surface texture that `bind_surface` can reference.
        // Needs RENDER_ATTACHMENT (for the flush render pass), TEXTURE_BINDING,
        // COPY_SRC (copy_texture_to_texture source), and COPY_DST.
        let surface_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Backdrop Edge Test Surface"),
            size: wgpu::Extent3d {
                width: SURFACE_W,
                height: SURFACE_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Bounds in local space: (32,32) → (100,100).  With identity transform,
        // device_rect == bounds == left=32, top=32, right=100, bottom=100.
        // Clamped to 64×64 surface: right→64, bottom→64, so w=32, h=32.
        // The discriminating assertion: composite_rect.width() must be 32, not 68.
        let filter_bounds = Rect::from_xywh(px(32.0), px(32.0), px(68.0), px(68.0));
        let filter = ImageFilter::Blur {
            sigma_x: 4.0,
            sigma_y: 4.0,
        };

        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);
        backend.bind_surface(&surface_view, &surface_texture);

        // `child = None` — we only care about the composite rect queued, not child dispatch.
        backend.render_backdrop_filter(
            None,
            &filter,
            filter_bounds,
            BlendMode::SrcOver,
            &Matrix4::IDENTITY,
        );

        let results = backend.painter().offscreen_results_for_test();
        assert_eq!(
            results.len(),
            1,
            "render_backdrop_filter must queue exactly one offscreen composite; got {}",
            results.len()
        );
        let (composite_rect, _tex_w, _tex_h) = results[0];

        // The clamped rect: left=32, top=32, width=32 (64-32), height=32 (64-32).
        // Pre-fix: width ≈ 68 (unclamped device_rect width). Post-fix: width ≈ 32.
        let expected_width = (SURFACE_W - 32) as f32; // 32.0
        let expected_height = (SURFACE_H - 32) as f32; // 32.0
        let expected_left = 32.0_f32;
        let expected_top = 32.0_f32;

        assert!(
            (composite_rect.left().0 - expected_left).abs() < 0.5,
            "composite_rect.left must be {expected_left} (clamped x); got {}",
            composite_rect.left().0
        );
        assert!(
            (composite_rect.top().0 - expected_top).abs() < 0.5,
            "composite_rect.top must be {expected_top} (clamped y); got {}",
            composite_rect.top().0
        );
        assert!(
            (composite_rect.width().0 - expected_width).abs() < 0.5,
            "composite_rect.width must be {expected_width} (clamped w=64-32=32), not 68 \
             (unclamped device_rect width); got {}. \
             Pre-fix failure: Path B passes device_rect to queue_offscreen_result instead \
             of the clamped rect derived from x,y,w,h.",
            composite_rect.width().0
        );
        assert!(
            (composite_rect.height().0 - expected_height).abs() < 0.5,
            "composite_rect.height must be {expected_height} (clamped h=64-32=32), not 68; got {}",
            composite_rect.height().0
        );
    }
}
