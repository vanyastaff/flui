//! Wgpu-based CommandRenderer implementation
//!
//! Production rendering backend executing drawing commands via GPU
//! acceleration.

use flui_painting::{BlendMode, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, px},
    painting::{Image, Path},
    styling::Color,
};
use smallvec::SmallVec;

use std::sync::Arc;

use crate::{
    command_ir::{GammaDirection, ImageFilterPass, ImageFilterSpec, LayerFilter, MorphOp},
    painter::WgpuPainter,
    state_stack::ResolvedClip,
};
use crate::{command_renderer::CommandRenderer, layer_state_stack::LayerStateStack};

/// wgpu backend implementation of CommandRenderer.
///
/// # Lifetime parameter
///
/// `LayerDispatcher<'frame>` borrows the current frame's painter (`&'frame mut
/// WgpuPainter`) and, when present, the `wgpu::TextureView` /
/// `wgpu::Texture` bound by [`bind_surface`](Self::bind_surface). The
/// lifetime is internal to one render pass: `Renderer::render` creates
/// the LayerDispatcher in a scoped block, dispatches the `LayerTree`, then lets
/// it drop before calling `painter.render()`. Sites that don't need to
/// flush mid-frame (shader-mask offscreen rendering, tests) call
/// [`LayerDispatcher::new`] which leaves the surface handles unbound.
///
/// Per *Rust for Rustaceans* ch.2 "Variance and Lifetimes": the
/// `'frame` parameter encodes the borrow's scope so the compiler
/// enforces that no LayerDispatcher outlives its bound resources.
///
/// Note: Debug is not derived because `WgpuPainter` contains wgpu types that
/// don't implement Debug.
// `missing_debug_implementations` is a crate-level `#[expect]`: these types
// hold `wgpu` handles, whose lack of `Debug` is the whole reason it exists.
pub(crate) struct LayerDispatcher<'frame> {
    painter: &'frame mut WgpuPainter,
    offscreen: Option<&'frame mut crate::offscreen::OffscreenRenderer>,
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
    /// The matrix that is currently applied to
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
    /// `LayerStateStack` method on `LayerDispatcher` (`push_clip_*`,
    /// `pop_clip`, `push_offset`, `push_transform`, `pop_transform`,
    /// `push_opacity`, `pop_opacity`, `push_color_filter`,
    /// `pop_color_filter`, `push_image_filter`, `pop_image_filter`).
    /// These `LayerStateStack` flush points are required: without
    /// them, a `push_clip → with_transform → pop_clip` sequence would pop
    /// the lazy save instead of the clip, corrupting state across sibling
    /// layers.
    ///
    /// `None` means the painter is at the default state and no
    /// balance is owed.
    ///
    /// The `Drop` impl provides a final safety-net flush: if a future
    /// code path forgets to call `flush_active_transform()` before
    /// the LayerDispatcher goes out of scope, Drop balances the deferred save
    /// so the borrowed painter is left in a clean state. The eager call
    /// sites are NOT replaced by Drop — they flush at precisely the right
    /// point for correctness; Drop is the backstop for any site that is
    /// missed.
    active_transform: Option<Matrix4>,
    /// One entry per clip layer that is currently open, innermost last.
    ///
    /// [`LayerStateStack::pop_clip`] serves all three `push_clip_*` variants
    /// and takes no argument, so nothing in the call itself can say whether the
    /// matching push opened an offscreen. This stack is the only thing that
    /// can, and [`LayerDispatcher::open_clip_frame`] is the only writer: it performs
    /// the open and records it in the same call, so a push cannot open a layer
    /// it did not record or record one it did not open.
    clip_frames: Vec<ClipFrame>,
}

/// What one `push_clip_*` did to the painter, and therefore what the matching
/// `pop_clip` must undo.
///
/// The variants name painter operations rather than clip modes on purpose:
/// `pop_clip` does not need to know which `Clip` was asked for, only what is
/// open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClipFrame {
    /// `painter.save()` — balanced by `painter.restore()`.
    Save,
    /// `painter.save()` then `painter.save_layer_clipped(..)` — balanced by
    /// `painter.restore_layer()` and then `painter.restore()`, in that order.
    SaveLayer,
}

impl<'frame> LayerDispatcher<'frame> {
    /// Create a new LayerDispatcher that borrows the given painter for the frame.
    ///
    /// `surface_view` / `surface_texture` start unbound. Call
    /// [`bind_surface`](Self::bind_surface) when the frame surface
    /// is available to enable the DisplayList-backdrop-filter
    /// command path.
    pub(crate) fn new(painter: &'frame mut WgpuPainter) -> Self {
        Self {
            painter,
            offscreen: None,
            surface_view: None,
            surface_texture: None,
            active_transform: None,
            clip_frames: Vec::new(),
        }
    }

    /// Create a new LayerDispatcher that borrows the given painter and offscreen renderer.
    pub(crate) fn with_offscreen(
        painter: &'frame mut WgpuPainter,
        offscreen: &'frame mut crate::offscreen::OffscreenRenderer,
    ) -> Self {
        Self {
            painter,
            offscreen: Some(offscreen),
            surface_view: None,
            surface_texture: None,
            active_transform: None,
            clip_frames: Vec::new(),
        }
    }

    /// Bind the frame's surface handles.
    ///
    /// Must be called by [`Renderer::render_scene`](crate::renderer::Renderer::render_scene)
    /// after constructing the LayerDispatcher and before dispatching any
    /// `LayerTree` commands. These handles identify the target for mid-frame
    /// flushes before backdrop sampling.
    pub(crate) fn bind_surface(
        &mut self,
        view: &'frame wgpu::TextureView,
        texture: &'frame wgpu::Texture,
    ) {
        self.surface_view = Some(view);
        self.surface_texture = Some(texture);
    }

    /// Access the offscreen renderer mutably (for shader mask, backdrop filter).
    pub(crate) fn offscreen_mut(&mut self) -> Option<&mut crate::offscreen::OffscreenRenderer> {
        self.offscreen.as_deref_mut()
    }

    /// Get a reference to the underlying painter.
    pub(crate) fn painter(&self) -> &WgpuPainter {
        self.painter
    }

    /// Get a mutable reference to the underlying painter.
    pub(crate) fn painter_mut(&mut self) -> &mut WgpuPainter {
        &mut *self.painter
    }

    /// Whether this push will render its subtree into an offscreen.
    ///
    /// Answered ONCE per push, before anything is installed, because two halves
    /// depend on it and must agree: which clip call installs the content's clip
    /// (per draw, or the bounding scissor alone with the rounded coverage saved
    /// for the composite), and what [`LayerStateStack::pop_clip`] closes.
    /// Deciding it after installing would leave a refused layer's content
    /// clipped by its bounding box with the coverage dropped on the floor —
    /// worse than either answer.
    ///
    /// Two things must both hold:
    ///
    /// - the mode asks for it ([`clip_opens_a_layer`]);
    /// - no enclosing layer routes through a bounds-growing image filter. Those
    ///   layers discard nested `DrawItem::OpacityLayer`s, and everything already
    ///   flushed beside them, so opening one there deletes content rather than
    ///   improving an edge. Degrading the mode to per-draw coverage loses an
    ///   edge; opening the layer loses the subtree. See
    ///   `LayerCompositor::inside_image_filter_layer`.
    ///
    /// Every one of the four `push_clip_*` sites installs a clip, so there is no
    /// third condition asking whether one landed: a mode that asks for NO clip
    /// is refused a layer earlier, by [`clip_is_disabled`] on the canvas route
    /// and by the layer's own `clips()` gate on the layer route. See
    /// `ARCHITECTURE.md` for why that condition once existed and what removing
    /// it proved.
    fn opens_offscreen(&self, behavior: flui_types::painting::Clip) -> bool {
        clip_opens_a_layer(behavior) && !self.painter.inside_image_filter_layer()
    }

    /// Open the offscreen a clip asked for, if it asked for one, and record the
    /// frame [`LayerStateStack::pop_clip`] will close.
    ///
    /// `Some(clip)` means the push wants the offscreen and `clip` is the
    /// coverage its group composite applies — see
    /// [`WgpuPainter::save_layer_clipped`], and note that `ResolvedClip::NONE`
    /// is a legitimate payload. `None` means the push installed its clip per
    /// draw and wants no layer.
    ///
    /// The only writer of `clip_frames`. The open and the record happen here, on
    /// the two sides of one `if`, so they cannot be set apart: no caller can
    /// record a `SaveLayer` without opening one, or open one without recording
    /// it.
    fn open_clip_frame(&mut self, composite_clip: Option<ResolvedClip>) {
        let frame = if let Some(clip) = composite_clip {
            // The clip's own scissor, which the caller has just installed: the
            // clip's device-space bounds already intersected with every
            // ancestor's, and a bound on everything the offscreen can hold.
            let bounds = self.painter.clip_bounds();
            self.painter.save_layer_clipped(clip, bounds);
            ClipFrame::SaveLayer
        } else {
            ClipFrame::Save
        };
        self.clip_frames.push(frame);
    }

    /// Get or create a cached offscreen painter for shader mask rendering.
    ///
    /// On first call, creates a new `WgpuPainter` with shared device/queue.
    /// On subsequent calls, returns the cached painter, resizing if needed.
    ///
    /// Dispatch a draw closure under the given
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
    /// / pop_image_filter) and the `Drop` impl (so the borrowed painter is
    /// balanced when the LayerDispatcher leaves scope). See
    /// [`Self::active_transform`] for the full list of flush points and why
    /// each one is needed.
    ///
    /// Measured effect: a render pass batching 1000 same-transform
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
        // the new transform — the whole matrix, never a TRS
        // decomposition, which drops skew and perspective.
        self.flush_active_transform();
        self.painter.save();
        self.painter.transform(transform);

        self.active_transform = Some(*transform);
        draw_fn(self.painter);
    }

    /// Balance the deferred `save()` left by a
    /// prior `with_transform` run with a `restore()`, clearing
    /// `active_transform`. No-op if no transform is active.
    ///
    /// Called from every site that mutates the painter save stack
    /// outside the coalescing path: `with_transform`'s identity /
    /// mismatch arms, every `LayerStateStack` method on `LayerDispatcher`,
    /// and the `Drop` impl. See the
    /// [`active_transform`](Self::active_transform) field doc for
    /// the full list of flush points and why each one is needed.
    fn flush_active_transform(&mut self) {
        if self.active_transform.is_some() {
            self.painter.restore();
            self.active_transform = None;
        }
    }

    /// Blurs the surface region beneath a backdrop-filter layer.
    ///
    /// Steps: clamp `device_rect` to the surface extent → copy that region from
    /// the surface into a pooled blur-input → Dual-Kawase blur → queue the result
    /// for compositing at the **clamped** rect. The painter is flushed first so
    /// the pixels to be sampled are present (the flush + copy stay in one
    /// submission).
    ///
    /// Returns `true` if the blur was queued, `false` if it was skipped (no
    /// offscreen renderer, or the region is entirely off-screen). The caller
    /// renders the backdrop's children either way.
    ///
    /// `device_rect` is the filter bounds already mapped to device space by the
    /// caller using the accumulated layer transform.
    pub(crate) fn apply_backdrop_blur(
        &mut self,
        device_rect: Rect<Pixels>,
        sigma: f32,
        blend: BlendMode,
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
                .expect("BUG: apply_backdrop_blur returned above when self.offscreen was None; nothing clears it before this borrow");
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
        let x = device_rect.left().0.clamp(0.0, surface_w as f32).round() as u32;
        let y = device_rect.top().0.clamp(0.0, surface_h as f32).round() as u32;
        let right = device_rect.right().0.clamp(0.0, surface_w as f32).round() as u32;
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
            crate::render_target::RenderTarget::sampleable(surface_view, surface_texture);
        if let Err(e) = self.painter.render(flush_target, &mut flush_encoder) {
            tracing::error!("Backdrop flush failed: {}", e);
        }

        // Copy the clamped device region from the surface into a pooled blur input.
        let blur_input = self
            .offscreen
            .as_deref_mut()
            .expect("BUG: apply_backdrop_blur returned above when self.offscreen was None; nothing clears it before this borrow")
            .texture_pool_mut()
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
            .expect("BUG: apply_backdrop_blur returned above when self.offscreen was None; nothing clears it before this borrow")
            .render_blur(&blur_input, sigma);
        let clamped_composite_rect = Rect::from_xywh(
            Pixels(x as f32),
            Pixels(y as f32),
            Pixels(w as f32),
            Pixels(h as f32),
        );
        self.painter
            .queue_offscreen_result(blurred, clamped_composite_rect, blend);
        true
    }
}

impl Drop for LayerDispatcher<'_> {
    /// Safety-net: balance any deferred lazy-coalescing save that was left on
    /// the painter stack by `with_transform`. Every `LayerStateStack` method and
    /// both arms of `with_transform` flush at the correct semantic point. This
    /// `Drop` impl is a backstop for any future call
    /// path that forgets to flush: when the LayerDispatcher goes out of scope the painter is
    /// left balanced and ready for its next use (`painter.render`,
    /// `end_frame_maintenance`, or the next frame's LayerDispatcher).
    fn drop(&mut self) {
        self.flush_active_transform();
    }
}

/// Whether a clip layer's mode wants a hard boundary.
///
/// `HardEdge` is the scissor for a rect, and a thresholded SDF for a rounded
/// one. `AntiAlias` feathers.
///
/// `None` is **not** handled here: it means "do not clip at all", and mapping
/// it to any rounding would clip. It is filtered by [`clip_is_disabled`] before
/// this is consulted. A render object choosing `Clip::None` pushes no clip
/// LAYER, which is what made this look unreachable — but `Canvas::clip_rect_ext`
/// and its siblings emit a `DrawCommand` unconditionally, so the canvas path
/// reaches it with any mode the caller passes.
///
/// `AntiAliasWithSaveLayer` answers `false`, and that is right as far as this
/// question goes — the mode is anti-aliased. What distinguishes it from
/// `AntiAlias` is decided by [`clip_opens_a_layer`] instead: its coverage is
/// applied once, to a group composite, rather than per draw. `push_clip_rrect`
/// therefore never consults this function for that mode.
///
/// The CANVAS clip commands (`clip_rect` / `clip_rrect` / `clip_rsuperellipse`
/// on the `CommandRenderer` impl, the `DrawCommand` path) still do, and for them
/// the mode really is approximated by `AntiAlias`. Flutter's own shape for it
/// on a canvas — clip anti-aliased, then `saveLayer` — would here be a
/// `save_layer_alpha(.., 255)`, but that layer composites at opacity 1.0 with a
/// white tint and `SrcOver`, which `LayerCompositor::pop_layer` reintegrates
/// rather than compositing, and a group composited that way is arithmetically
/// identical to drawing straight through. Tying the two commands together needs
/// a `DrawCommand` carrying the clip and the layer as one, which is a change to
/// the canvas vocabulary rather than to this backend; issue #848 stays open for
/// it. No in-repo caller reaches it: `RenderClip` and `RenderPhysicalModel` both
/// paint through `PaintCx::with_clip_*`, which pushes a clip LAYER.
/// Whether a clip layer's mode asks for an offscreen around the clipped
/// subtree.
///
/// `AntiAliasWithSaveLayer` and nothing else. Flutter renders the clipped
/// subtree into an offscreen so the group composites against the clip edge
/// ONCE; applying the coverage per draw instead makes the edge darker or more
/// opaque wherever the content overlaps itself, because each draw is attenuated
/// by the clip and then blended over an already-attenuated one. The offscreen
/// is also what isolates a destructive or advanced blend inside the clip from
/// the backdrop behind it — Flutter's own docs call that out as a semantic
/// change, not a side effect.
///
/// This answers only the MODE half of the question. Whether a layer is actually
/// opened is [`LayerDispatcher::opens_offscreen`], which also requires that no
/// enclosing image-filter layer would discard it.
const fn clip_opens_a_layer(behavior: flui_types::painting::Clip) -> bool {
    matches!(behavior, flui_types::painting::Clip::AntiAliasWithSaveLayer)
}

/// Whether a clip command asks for no clipping at all.
///
/// `Clip::None` reaching a clip call is not a contradiction: the layer path
/// never emits one, but `Canvas::clip_rect_ext` / `clip_rrect_ext` /
/// `clip_rsuperellipse_ext` push their command whatever mode they are given.
/// Honouring it means applying NO clip — the alternative, treating it as the
/// cheapest clip, clips content the caller asked to leave alone.
const fn clip_is_disabled(behavior: flui_types::painting::Clip) -> bool {
    matches!(behavior, flui_types::painting::Clip::None)
}

/// Whether this clip op can be expressed by the primitives this backend has.
///
/// `ClipOp::Difference` keeps the shape's COMPLEMENT. A scissor cannot express
/// a complement — it is one rectangle — and the per-draw SDF slot evaluates the
/// shape rather than its inverse, so no clip primitive here can honour it.
///
/// Installing the shape as an intersect instead is not an approximation of the
/// request, it is its inverse: a caller punching a hole gets everything OUTSIDE
/// the hole erased. That is destructive, where refusing is merely permissive —
/// the caller sees content the clip should have removed, rather than losing
/// content it asked to keep. Three of the four shapes did the first thing until
/// issue #941; `clip_path` already did the second, because #934 forced the
/// question for a bounding-box clip whose complement's bounding box is the
/// whole surface.
///
/// Honouring it needs the machinery an exact path clip needs: a stencil pass,
/// or a shader carrying a clip STACK that can evaluate `1 − coverage`. Both are
/// tracked with path clipping itself.
const fn clip_op_is_expressible(clip_op: flui_types::painting::ClipOp) -> bool {
    matches!(clip_op, flui_types::painting::ClipOp::Intersect)
}

/// Report a clip this backend cannot express, and go on without installing it.
///
/// Release level, not debug: an unhonoured clip renders content the caller
/// asked to remove, which is a visible defect a production scrape must be able
/// to see — the same reasoning that raised `WgpuPainter::clip_path`'s own
/// message from `trace!` to `warn!`.
///
/// Per call rather than latched. `ClipOp::Difference` has no in-tree producer,
/// so this cannot spam a frame loop today; if one appears, the fix is the
/// once-per-painter latch `clip_path` already carries, not a level downgrade.
fn warn_unexpressible_clip_op(shape: &str) {
    tracing::warn!(
        "LayerDispatcher::{shape}: ClipOp::Difference keeps the shape's complement, \
         which no clip primitive here can express; the clip is NOT applied. \
         Installing the shape as an intersect instead would invert the request \
         and erase the content it asked to keep. Honouring it needs the stencil \
         pass exact path clipping needs."
    );
}

impl CommandRenderer for LayerDispatcher<'_> {
    fn render_rect(&mut self, rect: Rect<Pixels>, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_rect(rect, paint);
        });
    }

    fn render_rrect(&mut self, rrect: RRect, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_rrect(rrect, paint);
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
            painter.draw_circle(center, radius, paint);
        });
    }

    fn render_oval(&mut self, rect: Rect<Pixels>, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.draw_oval(rect, paint);
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
            painter.draw_line(p1, p2, paint);
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
                    painter.draw_circle(*point, radius, paint);
                }
            }
            PointMode::Lines => {
                for i in (0..points.len()).step_by(2) {
                    if i + 1 < points.len() {
                        painter.draw_line(points[i], points[i + 1], paint);
                    }
                }
            }
            PointMode::Polygon => {
                for i in 0..points.len().saturating_sub(1) {
                    painter.draw_line(points[i], points[i + 1], paint);
                }
                if points.len() > 2 {
                    painter.draw_line(points[points.len() - 1], points[0], paint);
                }
            }
        });
    }

    fn render_paragraph(
        &mut self,
        layout: &Arc<flui_painting::TextLayout>,
        offset: Offset<Pixels>,
        color: Color,
        transform: &Matrix4,
    ) {
        let position = Point::new(offset.dx, offset.dy);
        self.with_transform(transform, |painter| {
            painter.draw_paragraph(Arc::clone(layout), position, color);
        });
    }

    fn render_image(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        // Thread paint.blend_mode to the GPU-level composite.
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
        // DrawItem::AdvancedShape. SrcOver takes the
        // per-sprite cached_images path unchanged.
        self.with_transform(transform, |painter| {
            painter.draw_atlas(image, sprites, transforms, colors, blend_mode);
        });
    }

    fn render_image_repeat(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        repeat: flui_types::painting::image::ImageRepeat,
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
        filter: flui_types::painting::image::ColorFilter,
        paint: Option<&Paint>,
        transform: &Matrix4,
    ) {
        // Thread paint.blend_mode as the GPU-level composite mode.
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

    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            let viewport_bounds = painter.viewport_bounds();
            // Carry the command's blend mode so the full-viewport fill composites
            // correctly (e.g. `DrawColor` with `Clear` punches out the layer).
            let paint = Paint::fill(color).with_blend_mode(blend_mode);
            painter.draw_rect(viewport_bounds, &paint);
        });
    }

    fn render_paint(&mut self, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            let viewport_bounds = painter.viewport_bounds();
            painter.draw_rect(viewport_bounds, paint);
        });
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
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        if clip_is_disabled(clip_behavior) {
            return;
        }
        if !clip_op_is_expressible(clip_op) {
            warn_unexpressible_clip_op("clip_rect");
            return;
        }
        // The `Clip` MODE rides through; the painter decides how each shape
        // honours it (see `WgpuPainter::clip_rect` for the one that cannot).
        self.with_transform(transform, |painter| {
            painter.clip_rect(rect, clip_behavior);
        });
    }

    fn clip_rrect(
        &mut self,
        rrect: RRect,
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        if clip_is_disabled(clip_behavior) {
            return;
        }
        if !clip_op_is_expressible(clip_op) {
            warn_unexpressible_clip_op("clip_rrect");
            return;
        }
        self.with_transform(transform, |painter| {
            painter.clip_rrect(rrect, clip_behavior);
        });
    }

    fn clip_rsuperellipse(
        &mut self,
        rsuperellipse: flui_types::geometry::RSuperellipse,
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        // Override the trait default (which routes to clip_rrect against an
        // approximating rounded rectangle). Delegate to the Painter's real
        // SDF clip, populating `current_rsuperellipse_clip` so subsequent
        // rect_instanced draws apply the iOS-squircle SDF via the
        // per-instance kind=2 path.
        if clip_is_disabled(clip_behavior) {
            return;
        }
        if !clip_op_is_expressible(clip_op) {
            warn_unexpressible_clip_op("clip_rsuperellipse");
            return;
        }
        self.with_transform(transform, |painter| {
            painter.clip_rsuperellipse(rsuperellipse, clip_behavior);
        });
    }

    fn clip_path(
        &mut self,
        path: &Path,
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        // `Clip::None` asks for no clipping, and this is where the three sibling
        // shapes honour it. `Canvas`'s whole `_ext` family refuses the mode
        // before recording a command, so none of the four guards is reachable
        // from the only producer in the tree today — they are the second half
        // of a two-sided invariant, held here so a future producer of
        // `DrawOp::ClipPath` cannot make this the one entry point that
        // honours "do not clip" by clipping. Omitting it from this arm alone
        // would be the asymmetry, not the guard.
        if clip_is_disabled(clip_behavior) {
            return;
        }
        if !clip_op_is_expressible(clip_op) {
            warn_unexpressible_clip_op("clip_path");
            return;
        }
        self.with_transform(transform, |painter| {
            painter.clip_path(path);
        });
    }

    fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint, transform: &Matrix4) {
        self.with_transform(transform, |painter| {
            painter.save_layer(bounds, paint);
        });
    }

    fn restore_layer(&mut self, _transform: &Matrix4) {
        self.painter.restore_layer();
    }

    fn save_state(&mut self) {
        // Both of these push/pop the SAME painter stack the lazy
        // `active_transform` save uses, so the deferred save has to be settled
        // before the scope moves the stack under it. Otherwise a later
        // `flush_active_transform` pops whichever save happens to be on top —
        // the scope's, not its own — and the absolute matrix lands on the wrong
        // CTM while the scope silently never closes.
        self.flush_active_transform();
        self.painter.save();
    }

    fn restore_state(&mut self) {
        // Symmetric, and the ordering matters in the other direction: the lazy
        // save was pushed *inside* this scope, so it must come off first or the
        // pop below takes it and leaves `active_transform` pointing at a save
        // that is already gone.
        self.flush_active_transform();
        self.painter.restore();
    }

    // ===== Layer Tree Operations split out =====
    //
    // push_clip_* / push_offset / push_transform / push_opacity /
    // push_color_filter / push_image_filter and their corresponding pop_*
    // methods live in `impl LayerStateStack for LayerDispatcher` (below), not in
    // this `CommandRenderer` impl. The visitor methods stay on this trait;
    // the layer-tree state-stack methods live on the dedicated
    // `LayerStateStack` trait. See the doc comment on that trait in
    // traits.rs for why.

    // `options` is accepted but not yet honoured: every readout is drawn
    // regardless (`flui-app`'s config documents the option set as having no
    // observable effect yet).
    fn add_performance_overlay(
        &mut self,
        _options: flui_layer::PerformanceOverlayOption,
        bounds: Rect<Pixels>,
        fps: f32,
        frame_time_ms: f32,
        total_frames: u64,
        diagnostic_line: Option<&str>,
    ) {
        // Semi-transparent dark background (MangoHud style)
        let bg_color = Color::rgba(10, 10, 15, 200);
        let bg_paint = Paint::fill(bg_color);
        let bg_rrect =
            RRect::from_rect_and_radius(bounds, flui_types::geometry::Radius::circular(px(4.0)));
        self.painter.draw_rrect(bg_rrect, &bg_paint);

        let x = bounds.left() + px(8.0);
        let x_val = bounds.left() + px(50.0);
        let mut y = bounds.top() + px(14.0);

        // GPU label (cyan) + FPS value
        let cyan = Color::rgba(0, 200, 200, 255);
        self.painter
            .draw_text("GPU", Point::new(x, y), 11.0, &Paint::fill(cyan));

        // FPS with color coding
        let fps_color = if fps >= 55.0 {
            Color::rgba(170, 255, 170, 255) // Light green
        } else if fps >= 30.0 {
            Color::rgba(255, 255, 130, 255) // Light yellow
        } else {
            Color::rgba(255, 130, 130, 255) // Light red
        };
        self.painter.draw_text(
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
            .draw_text("FPS", Point::new(x_val + fps_w, y), 8.0, &Paint::fill(gray));
        y += px(14.0);

        // Frametime label (purple) + value
        let purple = Color::rgba(200, 100, 255, 255);
        self.painter
            .draw_text("Frame", Point::new(x, y), 10.0, &Paint::fill(purple));

        let white = Color::rgba(220, 220, 220, 255);
        self.painter.draw_text(
            &format!("{frame_time_ms:.1}"),
            Point::new(x_val, y),
            10.0,
            &Paint::fill(white),
        );
        self.painter.draw_text(
            "ms",
            Point::new(x_val + px(22.0), y),
            8.0,
            &Paint::fill(gray),
        );

        if let Some(line) = diagnostic_line {
            y += px(14.0);
            // This is the highest-density row in the overlay. Keep it brighter
            // and slightly larger than unit suffixes so the diagnostic signal
            // remains legible after glyph antialiasing and display scaling.
            let diagnostic_color = Color::rgba(205, 205, 210, 255);
            self.painter
                .draw_text(line, Point::new(x, y), 9.0, &Paint::fill(diagnostic_color));
        }

        let _ = total_frames;
    }
}

// ============================================================================
// LAYER-STATE-STACK IMPL
// ============================================================================
//
// The 13 push_/pop_ methods below live on the dedicated `LayerStateStack`
// trait rather than in the `impl CommandRenderer for LayerDispatcher` block.
// Bodies and behavior are unchanged from before the split; only the
// receiving trait differs. See `crates/flui-engine/src/traits.rs`
// for the trait-split rationale.

impl LayerStateStack for LayerDispatcher<'_> {
    // Every method on this trait must call `self.flush_active_transform()`
    // BEFORE any `painter.save` / `painter.restore` / `painter.save_layer`
    // / `painter.restore_layer` op. `with_transform`'s coalescing leaves
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

    fn push_clip_rect(&mut self, rect: &Rect<Pixels>, clip_behavior: flui_types::painting::Clip) {
        self.flush_active_transform();
        self.painter.save();
        self.painter.clip_rect(*rect, clip_behavior);
        // A rect clip is the hardware scissor under every mode, and the scissor
        // has already clipped every draw that goes into the offscreen. It is
        // binary, so re-applying it to the group would change nothing — hence
        // `ResolvedClip::NONE`. The layer is still opened, for the half of the
        // mode a scissor cannot give: isolation from the backdrop.
        let composite_clip = self
            .opens_offscreen(clip_behavior)
            .then_some(ResolvedClip::NONE);
        self.open_clip_frame(composite_clip);
    }

    fn push_clip_rrect(&mut self, rrect: &RRect, clip_behavior: flui_types::painting::Clip) {
        self.flush_active_transform();
        self.painter.save();
        // Decided BEFORE installing anything: the two calls below clip the
        // content differently, and picking the wrong one because the layer was
        // refused afterwards would drop the rounded coverage entirely.
        let composite_clip = if self.opens_offscreen(clip_behavior) {
            // Bounding scissor only: the rounded coverage is what the group
            // composite applies, once. Installing the SDF slot as well would
            // apply it a second time, per draw — the defect the mode exists to
            // avoid.
            Some(self.painter.clip_rrect_at_composite(*rrect))
        } else {
            self.painter.clip_rrect(*rrect, clip_behavior);
            None
        };
        self.open_clip_frame(composite_clip);
    }

    fn push_clip_rsuperellipse(
        &mut self,
        rse: &flui_types::geometry::RSuperellipse,
        clip_behavior: flui_types::painting::Clip,
    ) {
        self.flush_active_transform();
        self.painter.save();
        // The rrect shape exactly: decided BEFORE installing anything, because
        // the two calls below clip the content differently and picking the
        // wrong one because the layer was refused afterwards would drop the
        // squircle coverage entirely.
        //
        // The SDF is the shape itself, not an approximation of it: the
        // rounded rectangle sharing this squircle's outer rect and radii is
        // INSCRIBED in it, so substituting one would clip corner content the
        // squircle keeps.
        let composite_clip = if self.opens_offscreen(clip_behavior) {
            Some(self.painter.clip_rsuperellipse_at_composite(*rse))
        } else {
            self.painter.clip_rsuperellipse(*rse, clip_behavior);
            None
        };
        self.open_clip_frame(composite_clip);
    }

    fn push_clip_path(&mut self, path: &Path, clip_behavior: flui_types::painting::Clip) {
        self.flush_active_transform();
        self.painter.save();
        // The clip installed is the path's BOUNDING BOX, not the path — see
        // `WgpuPainter::clip_path`. That makes this exactly the rect case: the
        // scissor is binary and has already clipped every draw going into the
        // offscreen, so re-applying it to the group would change nothing, hence
        // `ResolvedClip::NONE`. The layer is still opened for the half a
        // scissor cannot give — isolation from the backdrop.
        self.painter.clip_path(path);
        let composite_clip = self
            .opens_offscreen(clip_behavior)
            .then_some(ResolvedClip::NONE);
        self.open_clip_frame(composite_clip);
    }

    fn pop_clip(&mut self) {
        self.flush_active_transform();
        match self.clip_frames.pop() {
            Some(ClipFrame::SaveLayer) => {
                // Inside out: the layer was opened after the save, so it closes
                // before it. Reversing these leaves the composite reading the
                // parent's clip state instead of the clip's own.
                self.painter.restore_layer();
                self.painter.restore();
            }
            Some(ClipFrame::Save) => self.painter.restore(),
            None => {
                // An unmatched pop, and it must NOT reach the painter.
                //
                // `WgpuPainter::restore` leaves state unchanged only when its
                // own stack is EMPTY. Here the clip stack ran out while the
                // painter's may not have: an enclosing `push_offset` or
                // `push_transform` has a save on it, and restoring would pop
                // THAT, silently dropping a transform every later draw depends
                // on. The clip stack is the one that underflowed, so the clip
                // stack is the only one that reports it.
                tracing::warn!(
                    "LayerDispatcher::pop_clip: no clip frame is open; the layer walk emitted a \
                     pop_clip without a matching push_clip_* -- painter state left untouched"
                );
            }
        }
    }

    fn push_offset(&mut self, offset: Offset<Pixels>) {
        self.flush_active_transform();
        self.painter.save();
        self.painter.translate(offset);
    }

    fn push_transform(&mut self, transform: &Matrix4) {
        self.flush_active_transform();
        self.painter.save();
        self.painter.transform(transform);
    }

    fn pop_transform(&mut self) {
        self.flush_active_transform();
        self.painter.restore();
    }

    fn push_opacity(&mut self, alpha: f32) {
        self.flush_active_transform();
        // Create a layer with opacity (clamped to [0, 255]).
        // Blend mode defaults to SrcOver via Paint::fill.
        let alpha_u8 = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let paint = Paint::fill(Color::WHITE).with_alpha(alpha_u8);
        self.painter.save_layer(None, &paint);
    }

    fn push_opacity_blend(&mut self, alpha: f32, blend: flui_types::painting::BlendMode) {
        self.flush_active_transform();
        // Propagate the explicit blend mode into the saveLayer paint so the
        // compositor reads it from `paint.blend_mode` and routes the layer
        // through the dst-read advanced compositor path when needed.
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

    fn push_image_filter(&mut self, filter: &flui_types::painting::effects::ImageFilter) {
        use flui_types::painting::effects::ImageFilter;

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
    filters: &[flui_types::painting::effects::ImageFilter],
    out: &mut SmallVec<[ImageFilterPass; 4]>,
) {
    use flui_types::painting::effects::ImageFilter;
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
