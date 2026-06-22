//! GPU-accelerated 2D painter using wgpu + glyphon + lyon
//!
//! This is the unified painter implementation that combines:
//! - Shape rendering via vertex batching
//! - Text rendering via glyphon
//! - Path tessellation via lyon
//! - Transform stack for coordinate transformations
//!
//! Follows SOLID and KISS principles with clean separation of concerns.

use std::sync::Arc;

use smallvec::smallvec;

use super::{
    command_ir::{
        DrawItem, DrawSegment, FilterOp, ImageFilterPass, ImageFilterSpec, LayerFilter,
        LayerFilterChain, PendingOffscreenTexture, PendingOpacityLayer, ScissorRect,
    },
    layer_compositor::{LayerCompositor, RestoreOutcome},
    pipelines::PipelineSet,
    replay::GpuReplay,
    resources::GpuResources,
    state_stack::GpuStateStack,
    text::TextRenderer,
};
use flui_painting::Paint;
use flui_types::{
    Offset, Point, Rect,
    geometry::{Pixels, RRect, px},
    painting::{Path, TextureId},
};

/// GPU painter for wgpu-based rendering.
///
/// Manages instanced batching, tessellation, text rendering, and offscreen compositing.
pub struct WgpuPainter {
    // ===== GPU State =====
    /// wgpu device (Arc for sharing with text renderer)
    device: Arc<wgpu::Device>,

    /// wgpu queue (Arc for sharing with text renderer)
    queue: Arc<wgpu::Queue>,

    /// Surface texture format (needed for offscreen pipeline creation)
    surface_format: wgpu::TextureFormat,

    /// Viewport size (width, height)
    size: (u32, u32),

    // ===== GPU Resource Managers =====
    /// Facade owning BufferPool, TextureCache, TexturePool, and ExternalTextureRegistry.
    resources: GpuResources,

    // ===== Pipeline Collection =====
    /// All render pipelines used by this painter: nine named instanced/gradient/shadow
    /// pipelines + the on-demand shape pipeline cache. See `PipelineSet` for the full
    /// field map from previous painter fields to sub-fields.
    pipelines: PipelineSet,

    // ===== Segment-flush replay / GPU plumbing =====
    /// Owns the five static GPU plumbing fields (`viewport_buffer`,
    /// `viewport_bind_group`, `unit_quad_buffer`, `unit_quad_index_buffer`,
    /// `default_sampler`), the per-frame texture-instance scratch batch, all
    /// six segment-flush methods, the top-level `submit` dispatch loop, and
    /// opacity-layer recursion (`flush_opacity_layer`).  Separated so the
    /// flush path can borrow `&mut replay` independently of the remaining
    /// painter fields.
    replay: GpuReplay,

    // ===== Record-side draw batcher =====
    /// Owns the tessellator, path cache, and superellipse cache — the three
    /// mutable-but-non-GPU assets used only during draw recording.
    ///
    /// Separated from the flush-side fields so the borrow checker can split
    /// `&mut batcher` from `&mut current_segment`, `&mut draw_order`, and
    /// `&state` in the same call.  See `batches.rs` for the borrow seam contract.
    batcher: super::batches::DrawBatcher,

    // ===== Text Rendering =====
    /// Glyphon-based text renderer
    text_renderer: TextRenderer,

    // ===== GPU Draw-State Stack =====
    /// Owns the four parallel transform/scissor/SDF-clip stacks and their
    /// cached current values. All save/restore/translate/rotate/scale and
    /// clip operations delegate through this.
    state: GpuStateStack,

    // ===== Opacity/Layer Compositing =====
    /// Owns the opacity/layer save-state: `opacity_stack`, `current_opacity`,
    /// and `layer_stack`.  All save-layer book-keeping delegates here;
    /// GPU emission and draw-record mutation stay on `WgpuPainter`.
    compositor: LayerCompositor,

    // ===== Segmented Draw Order =====
    /// Current draw segment accumulating batched commands
    current_segment: DrawSegment,

    /// Ordered list of completed draw items (segments and offscreen textures)
    draw_order: Vec<DrawItem>,
}

// GPU rendering routinely converts between numeric types for pixel coordinates,
// color channels, buffer indices, and instance counts.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl WgpuPainter {
    /// Create a new GPU painter
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `queue` - wgpu queue
    /// * `surface_format` - Surface texture format
    /// * `size` - Initial viewport size (width, height)
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        Self::with_shared_device(Arc::new(device), Arc::new(queue), surface_format, size)
    }

    /// Create a WgpuPainter with shared device and queue.
    ///
    /// Use this when the device/queue are already wrapped in Arc
    /// (e.g., shared with Renderer).
    pub fn with_shared_device(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::new: format={:?}, size=({}, {})",
            surface_format,
            size.0,
            size.1
        );

        // ===== Pipeline collection (all 9 named pipelines + shape cache) =====
        //
        // `PipelineSet::new` creates the viewport bind-group layout internally
        // and exposes it via `viewport_bind_group_layout()`.  `GpuReplay::new`
        // below passes `&pipelines` so the viewport bind group it creates is
        // built against that exact layout object, satisfying the wgpu identity
        // requirement between bind groups and pipelines.
        let pipelines = PipelineSet::new(&device, surface_format);

        // ===== Replay / GPU plumbing (viewport buffer/bind-group, unit quad, sampler) =====
        let replay = GpuReplay::new(&device, &pipelines, size.0, size.1);

        // ===== Text renderer =====
        let text_renderer = TextRenderer::new(&device, &queue, surface_format);

        // ===== Resource managers =====
        let resources = GpuResources::new(Arc::clone(&device), Arc::clone(&queue));

        Self {
            device,
            queue,
            surface_format,
            size,
            resources,
            pipelines,
            replay,
            batcher: super::batches::DrawBatcher::new(),
            text_renderer,
            state: GpuStateStack::new(),
            compositor: LayerCompositor::new(),
            current_segment: DrawSegment::new(),
            draw_order: Vec::new(),
        }
    }

    // ===== Accessors =====

    /// Returns a reference to the wgpu device.
    #[must_use]
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// Returns a reference to the wgpu queue.
    #[must_use]
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    /// Returns the surface texture format.
    #[must_use]
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    // ===== Frame Lifecycle =====

    /// Reset all per-frame clip/transform/opacity/layer state to pristine values.
    ///
    /// Must be called at the **start** of every frame, before any damage scissor
    /// or other per-frame setup, so that state from frame N is never visible in
    /// frame N+1.
    ///
    /// Without this call the damage-scissor that was intersected into
    /// `current_scissor` during a partial-damage frame leaks into the next
    /// frame, causing full-repaint frames to silently clip to the previous
    /// damage rect.
    pub fn reset_frame_state(&mut self) {
        // Assert save/restore balance at the frame boundary BEFORE clearing.
        //
        // Not placed in `GpuStateStack::Drop` because the Backend
        // implicit-single-save (a lazy `active_transform` save left when a
        // Backend is dropped without calling `into_painter`) must not
        // false-positive-panic, and a Drop panic during unwind aborts the process.
        //
        // The assertion logic lives in `GpuStateStack::debug_assert_balanced`
        // so it can be exercised by unit tests without a GPU.
        self.state.debug_assert_balanced();
        self.compositor.debug_assert_balanced();

        self.state.reset();
        self.compositor.reset();

        tracing::trace!("WgpuPainter::reset_frame_state: per-frame state cleared");
    }

    /// Returns the current scissor rect for testing purposes.
    ///
    /// Gated to match its sole consumer (`reset_frame_state_clears_damage_scissor`)
    /// so it is never dead code in either build configuration.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn current_scissor_for_test(&self) -> Option<(u32, u32, u32, u32)> {
        self.state.current_scissor()
    }

    /// Returns the `dst_rect` field `[x, y, w, h]` of each pending external-image
    /// instance in the current segment.  Used by regression tests to verify that
    /// `draw_texture` transforms the destination rect through `current_transform`.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn external_image_rects_for_test(&self) -> Vec<[f32; 4]> {
        self.current_segment
            .external_images
            .iter()
            .map(|(_, inst, _)| inst.dst_rect)
            .collect()
    }

    /// Returns the scissor stored alongside each pending external-image instance.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn external_image_scissors_for_test(&self) -> Vec<ScissorRect> {
        self.current_segment
            .external_images
            .iter()
            .map(|(_, _, scissor)| *scissor)
            .collect()
    }

    /// Returns a copy of the tessellated vertex positions accumulated in the
    /// current segment.  Used by the transform-baking regression test to verify
    /// that `submit_transformed_geometry` is applied exactly once.
    ///
    /// Gated to `#[cfg(all(test, feature = "enable-wgpu-tests"))]` so it is
    /// never dead code in production builds.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn tess_vertices_for_test(&self) -> Vec<[f32; 2]> {
        self.current_segment
            .vertices
            .iter()
            .map(|v| v.position)
            .collect()
    }

    /// The tessellator's current flatten scale — to assert a draw call primed it.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn tessellator_max_scale_for_test(&self) -> f32 {
        self.batcher.tessellator.max_scale()
    }

    /// Force a stale tessellator scale to set up the prime-on-draw regression.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn set_tessellator_max_scale_for_test(&mut self, scale: f32) {
        self.batcher.tessellator.set_max_scale(scale);
    }

    /// The composite `bounds` and backing texture pixel size of every pending
    /// [`DrawItem::OffscreenTexture`] in the draw order, in draw order. Used by
    /// the HiDPI shader-mask / backdrop regression tests to assert an offscreen
    /// result is allocated at device resolution (`extent * dpr`) and composited
    /// at the device-space rect (`bounds * dpr`), not the logical rect.
    ///
    /// Returns `(bounds, texture_width, texture_height)`.
    /// Return all `DrawItem::AdvancedShape` operations in the current draw order.
    ///
    /// Used by routing unit tests (I1-I5, GI8) to assert that image/atlas advanced
    /// blend draws produce exactly one `AdvancedShape` per call rather than zero
    /// (silent SrcOver fall-through) or more than one (per-tile leak).
    ///
    /// Gated to test builds; must never be called from production code.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn advanced_shapes_for_test(&self) -> Vec<&super::command_ir::AdvancedShapeOp> {
        self.draw_order
            .iter()
            .filter_map(|item| match item {
                DrawItem::AdvancedShape(op) => Some(op),
                _ => None,
            })
            .collect()
    }

    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn offscreen_results_for_test(&self) -> Vec<(Rect<Pixels>, u32, u32)> {
        self.draw_order
            .iter()
            .filter_map(|item| match item {
                DrawItem::OffscreenTexture(p) => {
                    Some((p.bounds, p.texture.width(), p.texture.height()))
                }
                _ => None,
            })
            .collect()
    }

    /// Finalise the current segment and drain all recorded draw items, returning them
    /// as cloned [`DrawSegment`] values.
    ///
    /// Only `DrawItem::Segment` variants are exposed; `OffscreenTexture` and
    /// `OpacityLayer` variants (which carry live GPU handles in `PooledTexture`) are
    /// skipped because they are not cloneable.  The deterministic-replay test uses a
    /// draw scene that produces only `Segment` items, so all items in the drain are
    /// returned.
    ///
    /// This accessor exists solely to feed the T11 C5-gate tests.  It is gated to
    /// `#[cfg(all(test, feature = "enable-wgpu-tests"))]` and must never be called
    /// from production code.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn drain_segments_for_test(&mut self) -> Vec<DrawSegment> {
        self.finish_current_segment();
        self.draw_order
            .drain(..)
            .filter_map(|item| match item {
                DrawItem::Segment(seg) => Some(seg),
                // SsaaPath carries the path's tessellated DrawSegment internally;
                // surface it so the T11 deterministic-replay drain covers path
                // geometry too (rather than silently omitting it if a future test
                // scene adds a SrcOver arbitrary-path fill).
                DrawItem::SsaaPath(op) => Some(op.segment),
                DrawItem::OffscreenTexture(_)
                | DrawItem::OpacityLayer(_)
                | DrawItem::AdvancedShape(_) => None,
                // Surface the filter's input segment so drain covers Filter
                // geometry; the grown_bounds / passes are test-infrastructure
                // concerns and are not needed by the deterministic-replay drain.
                DrawItem::Filter(op) => Some(op.input),
            })
            .collect()
    }

    /// Return clones of all [`FilterOp`]s in the current draw order.
    ///
    /// Used by structural tests (flatten-nesting, cumulative-bounds) to inspect
    /// the `passes` and `grown_bounds` fields emitted by `restore_layer` without
    /// needing GPU execution.  Finalises the current segment first so that any
    /// in-progress content is in the draw order.
    ///
    /// Gated to test builds; must never be called from production code.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn filter_ops_for_test(&mut self) -> Vec<super::command_ir::FilterOp> {
        self.finish_current_segment();
        self.draw_order
            .iter()
            .filter_map(|item| match item {
                DrawItem::Filter(op) => Some(op.clone()),
                _ => None,
            })
            .collect()
    }

    /// Replay a caller-supplied list of `DrawItem`s onto `view` using `encoder`.
    ///
    /// This is a thin wrapper around `GpuReplay::submit` that exposes the replay
    /// path to the T11 deterministic-replay test.  Two independent calls with
    /// two independent encoders + views and the **same logical IR** (same content,
    /// different clones) must produce byte-identical pixel outputs — that is the
    /// C5 gate assertion.
    ///
    /// Production code does not call this: `WgpuPainter::render` drives the
    /// normal path.  This is gated to `#[cfg(all(test, feature = "enable-wgpu-tests"))]`.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn replay_items_for_test(
        &mut self,
        items: Vec<DrawItem>,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> crate::error::EngineResult<()> {
        self.replay.submit(
            items,
            self.size,
            self.surface_format,
            &self.device,
            &self.queue,
            &mut self.pipelines,
            &mut self.resources,
            &mut self.text_renderer,
            encoder,
            super::render_target::RenderTarget::view_only(view),
        )
    }

    // ===== Offscreen Compositing =====

    /// Queue an offscreen-rendered texture for compositing into the main render target.
    ///
    /// This finalizes the current draw segment and inserts the offscreen texture
    /// into the draw order. Content drawn before this call will render before
    /// the offscreen texture, and content drawn after will render after it,
    /// preserving correct Z-ordering.
    pub fn queue_offscreen_result(
        &mut self,
        texture: super::texture_pool::PooledTexture,
        bounds: Rect<Pixels>,
    ) {
        // Finalize the current segment and start a new one
        self.finish_current_segment();
        self.draw_order
            .push(DrawItem::OffscreenTexture(PendingOffscreenTexture {
                texture,
                bounds,
            }));
    }

    /// Render all batched geometry to a texture view.
    ///
    /// Called once per frame after all drawing operations.  Draw items are
    /// replayed in the order they were recorded, with offscreen textures
    /// interleaved at the correct Z-position.
    ///
    /// The dispatch loop and opacity-layer recursion live in
    /// `GpuReplay::submit` (see `replay.rs`); `render` is responsible only for
    /// the record-finish steps (cache advance, stats trace,
    /// `finish_current_segment`) and the post-submit buffer-pool reset.
    ///
    /// # Arguments
    /// * `view`    - Texture view to render to
    /// * `encoder` - Command encoder
    #[tracing::instrument(level = "trace", skip_all)]
    #[must_use = "errors must be propagated or handled"]
    pub(crate) fn render(
        &mut self,
        target: super::render_target::RenderTarget<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> crate::error::EngineResult<()> {
        // Advance batcher cache frame counters and evict stale entries.
        self.batcher.path_cache.advance_frame();
        self.batcher.superellipse_cache.advance_frame();

        // Log rendering stats before finalising (so counts reflect pre-drain state).
        let text_count = self.text_renderer.text_count();
        let rect_count = self.current_segment.rect_batch.len();
        let circle_count = self.current_segment.circle_batch.len();
        let buffer_stats = self.resources.buffer_pool_mut().stats();

        tracing::trace!(
            vertices = self.current_segment.vertices.len(),
            indices = self.current_segment.indices.len(),
            text_count,
            rects = rect_count,
            circles = circle_count,
            segments = self.draw_order.len(),
            cache_hit_rate = format!("{:.0}%", buffer_stats.reuse_rate * 100.0),
            "Drawing commands"
        );

        // Finalise the current segment and drain the draw order into a local
        // vec.  The drain is a pure move — no per-frame alloc beyond the vec
        // header (capacity was already allocated by the record side).
        self.finish_current_segment();
        let items: Vec<DrawItem> = self.draw_order.drain(..).collect();

        // Dispatch all items + text via GpuReplay::submit.
        // text_renderer.render is the final phase inside submit.
        self.replay.submit(
            items,
            self.size,
            self.surface_format,
            &self.device,
            &self.queue,
            &mut self.pipelines,
            &mut self.resources,
            &mut self.text_renderer,
            encoder,
            target,
        )?;

        // Reset buffer pool for next frame.
        self.resources.buffer_pool_mut().reset();

        // NOTE: texture-cache maintenance is intentionally NOT done here.
        // `render` runs multiple times per frame — each backdrop-filter flush
        // (backend.rs / renderer.rs) plus the final flush — on the SAME cache.
        // Resetting use-counters here would mis-classify textures used in an
        // earlier pass as unused and evict / atlas-reset them mid-frame.  The
        // Renderer calls `end_frame_maintenance` exactly once per frame instead.

        Ok(())
    }

    /// Convenience wrapper: render to a plain `TextureView` with no backdrop
    /// sampling back-reference (write-only target).
    ///
    /// Use this for benchmarks and callers that do not own a backing
    /// `wgpu::Texture` to supply.  Internal callers should prefer
    /// `WgpuPainter::render` directly so they can pass a sampleable
    /// `RenderTarget` when available.
    #[must_use = "errors must be propagated or handled"]
    pub fn render_to_view(
        &mut self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> crate::error::EngineResult<()> {
        self.render(super::render_target::RenderTarget::view_only(view), encoder)
    }

    /// Run end-of-frame texture-cache maintenance: evict over-budget textures,
    /// reclaim a full atlas that holds stale entries, then reset use-counters.
    ///
    /// Call EXACTLY ONCE per frame, after the final `WgpuPainter::render` flush.
    /// `render` must not do this itself — it runs once per pass (backdrop-filter
    /// flushes invoke it mid-frame), so per-call maintenance would reset
    /// use-counters between passes and drop textures still in use this frame.
    pub fn end_frame_maintenance(&mut self) {
        let maint = self.resources.texture_cache_mut().end_frame_maintenance();
        if maint.evicted > 0 || maint.atlas_reset {
            tracing::debug!(
                evicted = maint.evicted,
                atlas_reset = maint.atlas_reset,
                memory_bytes = self.resources.texture_cache().memory_bytes(),
                "Texture cache maintenance"
            );
        }
    }

    /// Returns the current viewport size as `(width, height)`.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Resize the viewport.
    ///
    /// Call this when the window is resized.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.size = (width, height);
        // Delegate the GPU uniform-buffer write to GpuReplay, which owns the
        // buffer.  The write is byte-identical: [width, height, 0.0, 0.0].
        self.replay.update_viewport(&self.queue, width, height);
    }

    /// Returns the current save stack depth.
    ///
    /// Delegates to `GpuStateStack::depth` — the single source of truth is
    /// `transform_stack.len()` inside the stack; no parallel counter is
    /// maintained.
    pub fn save_count(&self) -> usize {
        self.state.depth()
    }

    // ===== External Texture Registry Access =====

    /// Get a reference to the external texture registry
    ///
    /// Use this to register external textures (video frames, camera preview,
    /// etc.) that can be rendered via `Canvas::draw_texture()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::painting::TextureId;
    ///
    /// let texture_id = TextureId::new(42);
    /// painter.external_texture_registry()
    ///     .register(texture_id, gpu_texture, 1920, 1080, true, true);
    /// ```
    pub fn external_texture_registry(
        &self,
    ) -> &super::external_texture_registry::ExternalTextureRegistry {
        self.resources.external_texture_registry()
    }

    /// Get a mutable reference to the external texture registry
    ///
    /// Use this to register, update, or unregister external textures.
    pub fn external_texture_registry_mut(
        &mut self,
    ) -> &mut super::external_texture_registry::ExternalTextureRegistry {
        self.resources.external_texture_registry_mut()
    }

    // ===== Helper Methods =====

    /// Maximum basis length of the current transform's 2D linear part.
    ///
    /// Mirrors Impeller's `Matrix::GetMaxBasisLengthXY`: the larger of the two
    /// column-vector lengths of the upper-left 2x2. The tessellator divides its
    /// device-space chord-error budget by this so curves are subdivided finely
    /// enough at the magnification they will be baked and drawn at — see
    /// [`Tessellator::set_max_scale`](super::tessellator::Tessellator::set_max_scale).
    ///
    /// Also consulted by `Backend::render_shader_mask` to size the shader-mask
    /// offscreen at device resolution: on a HiDPI frame the live device-pixel
    /// ratio rides in the painter CTM (the `RenderView` root pushes
    /// `scale(dpr)`), so the offscreen child/result textures must be allocated
    /// `bounds * dpr` to avoid rendering the masked layer at half resolution.
    pub(crate) fn current_max_scale(&self) -> f32 {
        self.state.max_scale()
    }

    /// The accumulated current transform (CTM) as a [`flui_types::Matrix4`].
    ///
    /// The painter stores its CTM as a `glam::Mat4`; both `glam::Mat4` and
    /// `Matrix4` are column-major `[f32; 16]`, so this is a direct reinterpret
    /// of the 16 floats.
    ///
    /// Consumed by `Renderer::handle_backdrop_filter` (layer-tree "Path A") to
    /// map a layer's local-space `bounds` into device space before sampling /
    /// compositing. The layer walk pushes the `RenderView` root `scale(dpr)`
    /// (and every intervening `TransformLayer`/`OffsetLayer`) onto this CTM via
    /// `push_transform`/`push_offset`, so reading it here is the same source of
    /// truth the display-list backdrop path ("Path B") receives as its
    /// `transform` argument.
    pub(crate) fn current_transform_matrix(&self) -> flui_types::Matrix4 {
        self.state.current_transform_matrix()
    }

    /// Seal the current segment and start a fresh one.
    ///
    /// Forwards to `DrawBatcher::finish_current_segment`.  Called explicitly
    /// from `queue_offscreen_result` when an offscreen texture must be
    /// interleaved at the correct Z-position, and from the flush path to
    /// finalize the last segment before GPU submission.
    fn finish_current_segment(&mut self) {
        super::batches::DrawBatcher::finish_current_segment(
            &mut self.current_segment,
            &mut self.draw_order,
        );
    }

    /// Flush the texture instance batch with straight-alpha blending.
    ///
    /// Thin forwarder to `GpuReplay::flush_texture_batch`; `GpuReplay` owns
    /// all GPU plumbing fields.  Accumulated instances in `replay.texture_batch`
    /// are submitted and the batch is cleared.
    ///
    /// Opacity-layer and offscreen compositing use the premultiplied variant,
    /// which is now called directly on `GpuReplay` inside `submit`.  This
    /// straight-alpha forwarder remains `pub` for callers (e.g. `backend.rs`)
    /// that draw external textures with straight alpha.
    ///
    /// # Arguments
    /// * `encoder`       - Command encoder
    /// * `view`          - Render target view
    /// * `texture_view`  - Texture to use for all instances in this batch
    /// * `scissor`       - Optional scissor rect `(x, y, w, h)`; `None` = full viewport
    pub fn flush_texture_batch(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
    ) {
        self.replay.flush_texture_batch(
            &self.device,
            &self.queue,
            &self.pipelines,
            &mut self.resources,
            self.size,
            encoder,
            view,
            texture_view,
            scissor,
        );
    }
}
// ===== Public Drawing API =====
//
// These methods used to be the `impl Painter for WgpuPainter` trait impl;
// the `Painter` trait was deleted in Mythos U5 (1 production impl, 6 default
// `tracing::warn!("not implemented")` impls, no second backend planned).
// The methods stay as inherent on `WgpuPainter` for direct use by `Backend`
// (the CommandRenderer impl) and external callers like `examples/painting_demo`.

// GPU rendering routinely converts between f32/u8/u32/i32 for pixel
// coordinates, color channels, and buffer indices. These truncations are
// intentional.
//
// `missing_docs` is allowed on this impl block: the methods were originally
// trait methods carrying their docs on the trait declaration; redocumenting
// every one here is deferred to a follow-up doc-sweep (recorded in
// crates/flui-engine/ARCHITECTURE.md `## Outstanding refactors`).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    missing_docs
)]
impl WgpuPainter {
    pub fn rect(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::rect: rect={:?}, paint={:?}", rect, paint);

        let opacity = self.compositor.current_opacity();
        self.batcher.rect(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rect,
            paint,
        );
    }

    pub fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        let opacity = self.compositor.current_opacity();
        self.batcher.rrect(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rrect,
            paint,
        );
    }

    pub fn circle(&mut self, center: Point<Pixels>, radius: f32, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::circle: center={:?}, radius={}, paint={:?}",
            center,
            radius,
            paint
        );

        let opacity = self.compositor.current_opacity();
        self.batcher.circle(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            center,
            radius,
            paint,
        );
    }

    pub fn oval(&mut self, rect: Rect<Pixels>, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!("WgpuPainter::oval: rect={:?}, paint={:?}", rect, paint);

        let opacity = self.compositor.current_opacity();
        self.batcher.oval(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rect,
            paint,
        );
    }

    pub fn draw_arc(
        &mut self,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_arc: rect={:?}, start={}, sweep={}, use_center={}, paint={:?}",
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint
        );

        let opacity = self.compositor.current_opacity();
        self.batcher.draw_arc(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            opacity,
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
        );
    }

    pub fn draw_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_drrect: outer={:?}, inner={:?}, paint={:?}",
            outer,
            inner,
            paint
        );

        self.batcher.draw_drrect(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            outer,
            inner,
            paint,
        );
    }

    pub fn line(&mut self, p1: Point<Pixels>, p2: Point<Pixels>, paint: &Paint) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::line: p1={:?}, p2={:?}, paint={:?}",
            p1,
            p2,
            paint
        );

        self.batcher.line(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            p1,
            p2,
            paint,
        );
    }

    pub fn text(&mut self, text: &str, position: Point<Pixels>, font_size: f32, paint: &Paint) {
        tracing::trace!(
            text,
            ?position,
            font_size,
            color = ?paint.color,
            "WgpuPainter::text"
        );
        let transformed_position = self.state.apply_transform(position);
        self.text_renderer
            .add_text(text, transformed_position, font_size, paint.color);
    }

    /// Renders a sequence of styled runs as rich text.
    ///
    /// `runs` is the flattened output of `collect_styled_spans`: each entry is
    /// `(text_fragment, merged_style)` with `text_scale_factor` already baked
    /// into `style.font_size`.  `base_font_size` is the buffer-level default
    /// for runs with no explicit size; `base_color` is the fallback for runs
    /// with no color.
    pub fn rich_text(
        &mut self,
        runs: &[(String, Option<flui_types::typography::TextStyle>)],
        position: Point<Pixels>,
        base_font_size: f32,
        base_color: flui_types::styling::Color,
        wrap_width: Option<f32>,
    ) {
        tracing::trace!(
            run_count = runs.len(),
            ?position,
            base_font_size,
            ?base_color,
            ?wrap_width,
            "WgpuPainter::rich_text"
        );
        let transformed_position = self.state.apply_transform(position);
        self.text_renderer.add_rich_text(
            runs,
            transformed_position,
            base_font_size,
            base_color,
            wrap_width,
        );
    }

    pub fn texture(&mut self, texture_id: TextureId, dst_rect: Rect<Pixels>) {
        super::batches::DrawBatcher::texture(
            &mut self.current_segment,
            &self.state,
            texture_id,
            dst_rect,
        );
    }

    pub fn draw_path(&mut self, path: &flui_types::painting::path::Path, paint: &Paint) {
        self.batcher.draw_path(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            path,
            paint,
        );
    }

    /// Draw an image with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default compositing behaviour (byte-identical
    /// to pre-PR-5).  When `blend_mode.is_advanced()` the draw is isolated into a
    /// `DrawItem::AdvancedShape` so `flush_advanced_layer` can dst-read the backdrop.
    pub fn draw_image(
        &mut self,
        image: &flui_types::painting::Image,
        dst_rect: Rect<Pixels>,
        blend_mode: flui_painting::BlendMode,
    ) {
        super::batches::DrawBatcher::draw_image(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            dst_rect,
            blend_mode,
        );
    }

    /// Draw a tiled image with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default tiling behaviour.  When
    /// `blend_mode.is_advanced()` ALL tiles are collected into ONE
    /// `DrawItem::AdvancedShape` so every tile reads the original backdrop.
    pub fn draw_image_repeat(
        &mut self,
        image: &flui_types::painting::Image,
        dst: Rect<Pixels>,
        repeat: flui_painting::display_list::ImageRepeat,
        blend_mode: flui_painting::BlendMode,
    ) {
        super::batches::DrawBatcher::draw_image_repeat(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            dst,
            repeat,
            blend_mode,
        );
    }

    /// Draw a nine-slice image with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default nine-slice behaviour.  When
    /// `blend_mode.is_advanced()` ALL nine regions are collected into ONE
    /// `DrawItem::AdvancedShape`.
    pub fn draw_image_nine_slice(
        &mut self,
        image: &flui_types::painting::Image,
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
        blend_mode: flui_painting::BlendMode,
    ) {
        super::batches::DrawBatcher::draw_image_nine_slice(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            center_slice,
            dst,
            blend_mode,
        );
    }

    /// Draw a color-filtered image with an explicit GPU-level blend mode.
    ///
    /// `filter` bakes a per-pixel CPU operation first; `blend_mode` composites the
    /// result against the framebuffer (GPU).  Pass `BlendMode::SrcOver` for the
    /// default behaviour — the two blend modes are independent (see
    /// `DrawBatcher::draw_image_filtered` for the boundary contract).
    pub fn draw_image_filtered(
        &mut self,
        image: &flui_types::painting::Image,
        dst: Rect<Pixels>,
        filter: flui_painting::display_list::ColorFilter,
        blend_mode: flui_painting::BlendMode,
    ) {
        super::batches::DrawBatcher::draw_image_filtered(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            dst,
            filter,
            blend_mode,
        );
    }

    pub fn draw_shadow(
        &mut self,
        path: &flui_types::painting::path::Path,
        color: flui_types::styling::Color,
        elevation: f32,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "WgpuPainter::draw_shadow: elevation={}, color={:?}",
            elevation,
            color
        );

        self.batcher.draw_shadow(
            &mut self.current_segment,
            &mut self.draw_order,
            &mut self.state,
            path,
            color,
            elevation,
        );
    }

    /// Draw indexed triangle geometry with per-vertex color + uv.
    ///
    /// # `tex_coords` parameter
    ///
    /// Cycle 4 E-12: the per-vertex uv extraction IS implemented (the
    /// `tex_coords` slice is consumed at the per-vertex loop, copied into
    /// `Vertex::tex_coord`, and baked into the GPU vertex buffer).  What is
    /// NOT yet wired is the **texture-binding pipeline path**:
    /// `pipeline_key_from_paint(paint)` returns a solid-color pipeline today,
    /// so the uv values reach the vertex shader but the fragment shader has no
    /// texture to sample.  A textured pipeline-key variant is a follow-up
    /// audit item; until then `tex_coords` callers pre-populate the vertex
    /// stream for forward-compat (the data path is correct, only the pipeline
    /// binding is missing).
    pub fn draw_vertices(
        &mut self,
        vertices: &[Point<Pixels>],
        colors: Option<&[flui_types::styling::Color]>,
        tex_coords: Option<&[Point<Pixels>]>,
        indices: &[u16],
        paint: &Paint,
    ) {
        super::batches::DrawBatcher::draw_vertices(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            vertices,
            colors,
            tex_coords,
            indices,
            paint,
        );
    }

    /// Draw a sprite atlas with an explicit blend mode.
    ///
    /// Pass `BlendMode::SrcOver` for the default per-sprite compositing behaviour.
    /// When `blend_mode.is_advanced()` ALL sprites are collected into ONE
    /// `DrawItem::AdvancedShape` so every sprite reads the original backdrop.
    pub fn draw_atlas(
        &mut self,
        image: &flui_types::painting::Image,
        sprites: &[Rect<Pixels>],
        transforms: &[flui_types::Matrix4],
        colors: Option<&[flui_types::styling::Color]>,
        blend_mode: flui_painting::BlendMode,
    ) {
        // Convert Matrix4 transforms to pixel-space origins here, at the
        // painter boundary, so the batcher stays Matrix4-free (C4 rule).
        // Each transform is column-major; m[12] = x translation, m[13] = y.
        let sprite_origins: Vec<Offset<Pixels>> = transforms
            .iter()
            .map(|t| Offset {
                dx: px(t.m[12]),
                dy: px(t.m[13]),
            })
            .collect();
        super::batches::DrawBatcher::draw_atlas(
            &mut self.current_segment,
            &mut self.draw_order,
            &self.state,
            self.resources.texture_cache_mut(),
            image,
            sprites,
            &sprite_origins,
            colors,
            blend_mode,
        );
    }

    pub fn draw_texture(
        &mut self,
        texture_id: flui_types::painting::TextureId,
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
        filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
    ) {
        // Read dimensions only when a `src` sub-rect was supplied, so the
        // batcher can normalize pixel coordinates to UV in [0,1].  The
        // TextureView stays in the registry until replay time.
        let src_dimensions = src.and_then(|_| {
            self.resources
                .external_texture_registry()
                .get(texture_id)
                .map(|entry| (entry.width, entry.height))
        });
        super::batches::DrawBatcher::draw_texture(
            &mut self.current_segment,
            &self.state,
            src_dimensions,
            texture_id,
            dst,
            src,
            filter_quality,
            opacity,
        );
    }

    // ===== Transform Stack =====

    pub fn save(&mut self) {
        self.state.save();
    }

    pub fn restore(&mut self) {
        self.state.restore();
    }

    pub fn translate(&mut self, offset: Offset<Pixels>) {
        self.state.translate(offset);
    }

    pub fn rotate(&mut self, angle: f32) {
        self.state.rotate(angle);
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.state.scale(sx, sy);
    }

    // ===== Clipping =====

    pub fn clip_rect(&mut self, rect: Rect<Pixels>) {
        self.state.clip_rect(rect, self.size);
    }

    #[allow(
        clippy::similar_names,
        reason = "r_tl/r_tr/r_br/r_bl mirror the rrect-corner field names; renaming would obscure intent"
    )]
    pub fn clip_rrect(&mut self, rrect: RRect) {
        self.state.clip_rrect(rrect, self.size);
    }

    /// Look up or generate a tessellated superellipse path via the
    /// Painter-owned bounded cache.
    ///
    /// Consulted by `Backend::superellipse_path` (the `CommandRenderer`
    /// trait override) so `ClipSuperellipseLayer::render`'s layer-tree
    /// clip path benefits from frame-bounded caching. On a miss the path
    /// is generated via `generate_superellipse_path` (the iOS-squircle
    /// math) and inserted; eviction follows PathCache semantics
    /// (`max_entries` + `last_used_frame`).
    pub(crate) fn superellipse_path(
        &mut self,
        rse: &flui_types::geometry::RSuperellipse,
    ) -> flui_types::painting::Path {
        let key = super::superellipse_cache::SuperellipseKey::from_superellipse(rse);
        if let Some(path) = self.batcher.superellipse_cache.get(&key) {
            return path;
        }
        let path = super::layer_render::generate_superellipse_path(rse);
        self.batcher.superellipse_cache.insert(key, path.clone());
        path
    }

    /// Set an SDF rounded-superellipse clip (iOS-squircle).
    ///
    /// Parallel to [`Self::clip_rrect`]: populates `current_rsuperellipse_clip`
    /// with the bounding rect + per-corner radii, applies a bounding-rect
    /// scissor for early rasterizer rejection, and relies on
    /// `rect_instanced.wgsl`'s per-pixel SDF evaluation to clip pixels
    /// outside the iOS-squircle curve (wired in U9 / U10).
    #[allow(
        clippy::similar_names,
        reason = "tl_r/tr_r/br_r/bl_r mirror the rsuperellipse-corner field names; renaming would obscure intent"
    )]
    pub fn clip_rsuperellipse(&mut self, rse: flui_types::geometry::RSuperellipse) {
        self.state.clip_rsuperellipse(rse, self.size);
    }

    pub fn clip_path(&mut self, _path: &Path) {
        // Path clipping requires stencil buffer or path tessellation
        // This is a complex feature that needs:
        // 1. Stencil buffer configuration in render pass
        // 2. Tessellate path and render to stencil buffer
        // 3. Enable stencil test for subsequent draws
        // 4. Stack management for nested clips
        // 5. Handle even-odd vs non-zero fill rules
        //
        // Additionally, Path::bounds() requires &mut Path for caching,
        // but we only have &Path in this context.
        //
        // For now, this is a no-op. Applications should use ClipRect or ClipRRect
        // for hardware-accelerated clipping. Path clipping will be implemented
        // in a future version with proper stencil buffer support.

        // Cycle 4 E-1: pre-cycle this path emitted a debug-only
        // `tracing::trace!` and returned silently. Production scrapes
        // never saw the missing clip — content rendered without the
        // intended clip. Upgrade to release-build `tracing::warn!` so
        // any consumer that hits the path gets a visible signal.
        tracing::warn!(
            "WgpuPainter::clip_path: path clipping not implemented; \
             content will render without the intended clip. \
             Use ClipRect or ClipRRect for hardware-accelerated clipping. \
             Path clipping requires stencil-buffer support (cycle 4 E-1)"
        );
    }

    // ===== Viewport Information =====

    pub fn viewport_bounds(&self) -> Rect<Pixels> {
        Rect::from_ltrb(
            px(0.0),
            px(0.0),
            px(self.size.0 as f32),
            px(self.size.1 as f32),
        )
    }

    // ===== Layer Operations (Opacity) =====

    pub fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint) {
        let paint_alpha = f32::from(paint.color.a) / 255.0;
        let layer_opacity = self.compositor.effective_layer_opacity(paint_alpha);

        // A saveLayer paint's RGB is NOT a compositing tint. Per Flutter
        // semantics the layer's group opacity comes from the paint's *alpha*,
        // and chroma comes only from an explicit ColorFilter — never from
        // `paint.color`'s RGB. The public canvas opacity helpers build
        // alpha-only layer paints as `Paint::fill(Color::TRANSPARENT)
        // .with_opacity(..)` (RGB `[0,0,0]`, see flui-painting
        // `canvas/state.rs`), so reading RGB here would tint group-opacity
        // layers black. Always use a white (no-op) chroma; ColorFilter chroma
        // arrives explicitly via `save_layer_with_tint` from
        // `push_color_filter`.
        //
        // The blend mode IS propagated: an advanced blend mode (e.g. Multiply)
        // on the saveLayer paint means the entire layer composites onto its
        // parent with that mode — the dominant real-world use case for
        // advanced blend.
        self.save_layer_impl(
            bounds,
            layer_opacity,
            [1.0, 1.0, 1.0],
            paint.blend_mode,
            LayerFilterChain::new(),
        );
    }

    /// Like [`Self::save_layer`] but applies an explicit per-channel chroma
    /// `tint_rgb` to the composited layer.
    ///
    /// Used by the ColorFilter layer path (`push_color_filter`), which
    /// approximates a color matrix as a single multiply tint
    /// (`filter.apply([1,1,1,1])`). `opacity` is the layer's effective alpha in
    /// `[0, 1]`; `tint_rgb` components are clamped to `[0, 1]`. The composite
    /// applies `(C.r*O, C.g*O, C.b*O, O)` to the premultiplied offscreen, so a
    /// hue shift survives compositing — see `flush_opacity_layer`.
    pub fn save_layer_with_tint(
        &mut self,
        bounds: Option<Rect<Pixels>>,
        opacity: f32,
        tint_rgb: [f32; 3],
    ) {
        let layer_opacity = self
            .compositor
            .effective_layer_opacity(opacity.clamp(0.0, 1.0));
        let tint = [
            tint_rgb[0].clamp(0.0, 1.0),
            tint_rgb[1].clamp(0.0, 1.0),
            tint_rgb[2].clamp(0.0, 1.0),
        ];
        // ColorFilter tint layers always use SrcOver — chroma is encoded via
        // the tint, not the blend mode.
        self.save_layer_impl(
            bounds,
            layer_opacity,
            tint,
            flui_types::painting::BlendMode::SrcOver,
            LayerFilterChain::new(), // no filter — tint carries the color
        );
    }

    /// Like [`Self::save_layer`] but routes the layer through a per-pixel GPU
    /// filter (currently only [`LayerFilter::ColorMatrix`]) before compositing.
    ///
    /// The filter is applied AFTER `render_layer_to_offscreen` and BEFORE the
    /// composite step, so it receives the fully-rendered premultiplied offscreen
    /// and emits a filtered premultiplied texture.  Opacity and blend mode carry
    /// through normally.
    ///
    /// Used by `push_color_filter` and the `Matrix`/`ColorAdjust` branches of
    /// `push_image_filter` in `backend.rs`.
    pub(crate) fn save_layer_with_filter(
        &mut self,
        bounds: Option<Rect<Pixels>>,
        filter: LayerFilter,
    ) {
        // Filter layers composite with white tint and SrcOver.  `effective_layer_opacity(1.0)`
        // multiplies 1.0 by the current ancestor opacity, so a filter layer nested inside an
        // outer opacity layer correctly inherits that opacity — matching Flutter semantics where
        // a color-filter saveLayer respects the parent's opacity.
        let layer_opacity = self.compositor.effective_layer_opacity(1.0);
        self.save_layer_impl(
            bounds,
            layer_opacity,
            [1.0, 1.0, 1.0],
            flui_types::painting::BlendMode::SrcOver,
            smallvec![filter],
        );
    }

    /// Open a bounds-GROWING image filter layer.
    ///
    /// Unlike `save_layer` (which closes over an offscreen with group opacity) and
    /// `save_layer_with_filter` (which applies a `LayerFilter::ColorMatrix` that
    /// does NOT grow bounds), this method routes the layer's offscreen content
    /// through a `DrawItem::Filter` at `restore_layer` time instead of
    /// `DrawItem::OpacityLayer`.  The `FilterOp` carries the pass chain derived
    /// from `spec` and a `grown_bounds` rect that expands beyond the content AABB,
    /// allowing morphology/blur to composite at a larger area than the input.
    ///
    /// The layer is pushed with opacity=inherited (so any outer group opacity still
    /// applies), white tint, SrcOver, and empty color-filter chain — identical to
    /// `save_layer_with_filter`.  The `image_filter` field on the top `SavedLayer`
    /// is then set so `restore_layer` can detect the bounds-growing path.
    ///
    /// Used by `push_image_filter` in `backend.rs` for `Dilate`, `Erode`, `Blur`,
    /// and `Compose` (the latter via a pre-flattened `ImageFilterSpec::Chain`).
    pub(crate) fn save_layer_with_image_filter(&mut self, spec: ImageFilterSpec) {
        // Inherit the current ancestor opacity (same as `save_layer_with_filter`).
        let layer_opacity = self.compositor.effective_layer_opacity(1.0);
        self.save_layer_impl(
            None, // bounds determined at restore time from content AABB + radius
            layer_opacity,
            [1.0, 1.0, 1.0],
            flui_types::painting::BlendMode::SrcOver,
            LayerFilterChain::new(), // no color-filter chain (image filter is separate)
        );
        // Mark the freshly-pushed SavedLayer with the image filter spec so that
        // `restore_layer` knows to emit DrawItem::Filter instead of OpacityLayer.
        // Log before the move so that the trace can capture `?spec` without needing
        // `Copy` on `ImageFilterSpec` (which was removed when `Chain` was added).
        tracing::trace!(
            ?spec,
            "WgpuPainter::save_layer_with_image_filter: image filter layer opened"
        );
        self.compositor.set_top_image_filter(spec);
    }

    /// Shared implementation for [`Self::save_layer`] /
    /// [`Self::save_layer_with_tint`] / [`Self::save_layer_with_filter`] /
    /// [`Self::save_layer_with_image_filter`]:
    /// snapshot the draw state and push a layer with the given composite
    /// `layer_opacity`, `layer_tint_rgb`, `layer_blend`, and color-filter chain.
    fn save_layer_impl(
        &mut self,
        bounds: Option<Rect<Pixels>>,
        layer_opacity: f32,
        layer_tint_rgb: [f32; 3],
        layer_blend: flui_types::painting::BlendMode,
        filters: LayerFilterChain,
    ) {
        // Convert bounds to [x, y, w, h] if provided.
        let bounds_array = bounds.map(|r| [r.left().0, r.top().0, r.width().0, r.height().0]);

        // Hand the current draw-record accumulators to the compositor; it wraps
        // them in a SavedLayer and resets current_opacity to 1.0 for the subtree.
        let saved_draw_order = std::mem::take(&mut self.draw_order);
        let saved_segment = std::mem::replace(&mut self.current_segment, DrawSegment::new());
        tracing::trace!(
            "WgpuPainter::save_layer: layer_opacity={:.3}, tint={:?}, blend={:?}, \
             filters={:?}, bounds={:?}",
            layer_opacity,
            layer_tint_rgb,
            layer_blend,
            filters,
            bounds_array
        );
        self.compositor.push_layer(
            saved_draw_order,
            saved_segment,
            layer_opacity,
            layer_tint_rgb,
            layer_blend,
            bounds_array,
            filters, // moved here after the trace
        );
    }

    pub fn restore_layer(&mut self) {
        // Capture the offscreen content drawn since save_layer.
        let offscreen_final_segment =
            std::mem::replace(&mut self.current_segment, DrawSegment::new());
        let offscreen_items = std::mem::take(&mut self.draw_order);

        // Determine compositing bounds before calling pop_layer so the painter
        // can resolve the viewport fallback using its own `size` field.
        // We need the SavedLayer bounds — peek at the top without popping.
        // The compositor's pop_layer needs the already-resolved Rect, so we
        // resolve it here using the pattern from the original restore_layer.
        // We peek the bounds from the top of the layer_stack before delegating.
        let composite_bounds = self.compositor.peek_layer_bounds().map_or_else(
            || self.viewport_bounds(),
            |b| Rect::from_ltrb(px(b[0]), px(b[1]), px(b[0] + b[2]), px(b[1] + b[3])),
        );

        let outcome =
            self.compositor
                .pop_layer(offscreen_final_segment, offscreen_items, composite_bounds);

        match outcome {
            RestoreOutcome::Composite {
                offscreen_items,
                offscreen_final_segment,
                layer_opacity,
                tint_rgb,
                composite_bounds,
                layer_blend,
                layer_filter,
                image_filter,
                saved_segment,
                saved_draw_order,
            } => {
                // Restore the parent draw-record accumulators.
                self.current_segment = saved_segment;
                self.draw_order = saved_draw_order;

                // Finalize the current parent segment so the new draw item is
                // inserted at the correct Z-position in the draw order.
                let parent_segment =
                    std::mem::replace(&mut self.current_segment, DrawSegment::new());
                if !parent_segment.is_empty() {
                    self.draw_order.push(DrawItem::Segment(parent_segment));
                }

                // Route to DrawItem::Filter for bounds-growing image filters
                // (Morph/Blur); fall through to DrawItem::OpacityLayer for
                // plain opacity/tint/blend-mode layers.
                match image_filter {
                    Some(ImageFilterSpec::Morph { radius, op }) => {
                        // Package the offscreen content as a FilterOp.
                        //
                        // `FilterOp::input` is a flat `DrawSegment` consumed by
                        // `render_segment_to_offscreen` at replay time.  For a
                        // morphology layer opened with `save_layer_with_image_filter`,
                        // callers do not nest opacity layers inside, so
                        // `offscreen_items` is empty and `offscreen_final_segment`
                        // holds all the content.  If `offscreen_items` is non-empty
                        // (e.g., a nested texture from a draw_image call), log a
                        // debug trace — the items are silently ignored because the
                        // current FilterOp::input is a single DrawSegment; a future
                        // task can extend FilterOp::input to Vec<DrawItem> if needed.
                        if !offscreen_items.is_empty() {
                            tracing::debug!(
                                item_count = offscreen_items.len(),
                                "restore_layer(Morph): offscreen_items discarded; \
                                 FilterOp::input only captures the final DrawSegment. \
                                 Nested opacity layers inside a morphology layer are \
                                 not yet supported."
                            );
                        }
                        // `_ = layer_opacity` — morphology is applied as a DrawItem::Filter
                        // that composites directly; the opacity field is inherited via
                        // `effective_layer_opacity(1.0)` in `save_layer_with_image_filter`
                        // and is already baked into the save-layer setup.  The composite
                        // step (flush_texture_batch_premultiplied) uses REPLACE blend, so
                        // the group opacity is effectively 1.0 at this stage.
                        let _ = (layer_opacity, tint_rgb, layer_blend, layer_filter);

                        // Growth via the shared helper (one source of truth for Morph).
                        let single_pass = ImageFilterPass::Morph { radius, op };
                        let growth_px = px(cumulative_growth(std::slice::from_ref(&single_pass)));
                        let grown = composite_bounds.expand(growth_px);
                        let viewport_rect = self.viewport_bounds();
                        let grown_bounds =
                            grown.intersect(&viewport_rect).unwrap_or(composite_bounds);

                        tracing::trace!(
                            radius,
                            op = ?op,
                            content_bounds = ?composite_bounds,
                            grown_bounds = ?grown_bounds,
                            "WgpuPainter::restore_layer: queued DrawItem::Filter (Morph)"
                        );
                        self.draw_order.push(DrawItem::Filter(FilterOp {
                            input: offscreen_final_segment,
                            passes: smallvec![single_pass],
                            content_bounds: composite_bounds,
                            grown_bounds,
                        }));
                    }
                    Some(ImageFilterSpec::Blur { sigma_x, sigma_y }) => {
                        // Gaussian blur via two H/V sub-passes (separable, anisotropic).
                        // Identical seam to Morph: grow by kernel_radius(max(σx,σy))
                        // on each side, clip to viewport, emit DrawItem::Filter.
                        //
                        // Growth via the shared `cumulative_growth` helper (one source
                        // of truth for Blur; `kernel_radius` uses Impeller's √3·σ rule).
                        if !offscreen_items.is_empty() {
                            tracing::debug!(
                                item_count = offscreen_items.len(),
                                "restore_layer(Blur): offscreen_items discarded; \
                                 FilterOp::input only captures the final DrawSegment. \
                                 Nested opacity layers inside a blur layer are not yet supported."
                            );
                        }
                        let _ = (layer_opacity, tint_rgb, layer_blend, layer_filter);

                        let single_pass = ImageFilterPass::Blur { sigma_x, sigma_y };
                        let halo_px = px(cumulative_growth(std::slice::from_ref(&single_pass)));
                        let grown = composite_bounds.expand(halo_px);
                        let viewport_rect = self.viewport_bounds();
                        let grown_bounds =
                            grown.intersect(&viewport_rect).unwrap_or(composite_bounds);

                        tracing::trace!(
                            sigma_x,
                            sigma_y,
                            content_bounds = ?composite_bounds,
                            grown_bounds = ?grown_bounds,
                            "WgpuPainter::restore_layer: queued DrawItem::Filter (Blur)"
                        );
                        self.draw_order.push(DrawItem::Filter(FilterOp {
                            input: offscreen_final_segment,
                            passes: smallvec![single_pass],
                            content_bounds: composite_bounds,
                            grown_bounds,
                        }));
                    }
                    Some(ImageFilterSpec::Chain(passes)) => {
                        // Multi-pass Compose chain: the passes vec is already flattened
                        // at record time by `flatten_compose` in `backend.rs`.
                        //
                        // Identical offscreen_items guard as Morph/Blur arms above.
                        if !offscreen_items.is_empty() {
                            tracing::debug!(
                                item_count = offscreen_items.len(),
                                pass_count = passes.len(),
                                "restore_layer(Chain): offscreen_items discarded; \
                                 FilterOp::input only captures the final DrawSegment. \
                                 Nested opacity layers inside a Compose chain layer are \
                                 not yet supported."
                            );
                        }
                        let _ = (layer_opacity, tint_rgb, layer_blend, layer_filter);

                        // Cumulative growth = Σ per-pass radii (ColorMatrix/Identity = 0).
                        let growth_px = px(cumulative_growth(&passes));
                        let grown = composite_bounds.expand(growth_px);
                        let viewport_rect = self.viewport_bounds();
                        let grown_bounds =
                            grown.intersect(&viewport_rect).unwrap_or(composite_bounds);

                        tracing::trace!(
                            pass_count = passes.len(),
                            content_bounds = ?composite_bounds,
                            grown_bounds = ?grown_bounds,
                            "WgpuPainter::restore_layer: queued DrawItem::Filter (Chain)"
                        );
                        self.draw_order.push(DrawItem::Filter(FilterOp {
                            input: offscreen_final_segment,
                            passes,
                            content_bounds: composite_bounds,
                            grown_bounds,
                        }));
                    }
                    None => {
                        // Plain opacity/tint/blend-mode composite — existing path.
                        tracing::trace!(
                            "WgpuPainter::restore_layer: queued OpacityLayer \
                             (opacity={:.3}, tint_rgb={:?}, blend={:?}, filters={:?}, bounds={:?})",
                            layer_opacity,
                            tint_rgb,
                            layer_blend,
                            layer_filter,
                            composite_bounds
                        );
                        self.draw_order
                            .push(DrawItem::OpacityLayer(PendingOpacityLayer {
                                items: offscreen_items,
                                final_segment: offscreen_final_segment,
                                opacity: layer_opacity,
                                tint_rgb,
                                bounds: composite_bounds,
                                blend: layer_blend,
                                filters: layer_filter,
                            }));
                    }
                }
            }
            RestoreOutcome::Reintegrate {
                offscreen_items,
                offscreen_final_segment,
                saved_segment,
                saved_draw_order,
            } => {
                // Restore the parent draw-record accumulators.
                self.current_segment = saved_segment;
                self.draw_order = saved_draw_order;

                // Opacity is ~1.0 AND tint is white — no compositing needed.
                // Finalize the parent's pre-save content into the draw order
                // BEFORE re-integrating the offscreen items so that parent
                // content renders beneath the layer subtree (correct Z-order).
                let parent_segment =
                    std::mem::replace(&mut self.current_segment, DrawSegment::new());
                if !parent_segment.is_empty() {
                    self.draw_order.push(DrawItem::Segment(parent_segment));
                }
                super::replay::GpuReplay::reintegrate_offscreen_content(
                    offscreen_final_segment,
                    offscreen_items,
                    1.0,
                    &mut self.draw_order,
                );
            }
            RestoreOutcome::Empty {
                saved_segment,
                saved_draw_order,
            } => {
                // Layer was empty — restore draw-record state, emit nothing.
                self.current_segment = saved_segment;
                self.draw_order = saved_draw_order;
            }
            RestoreOutcome::Underflow {
                current_segment,
                draw_order,
            } => {
                // Compositor already logged the warning and handled the
                // legacy opacity_stack fallback.
                //
                // Restore the records that were unconditionally captured before
                // the pop_layer call, so the frame's in-flight draws are not
                // wiped.  This matches the original painter behaviour where the
                // mem::take was guarded inside the `if let Some(saved)` block.
                self.current_segment = current_segment;
                self.draw_order = draw_order;
            }
        }

        tracing::trace!(
            "WgpuPainter::restore_layer: restored opacity={:.3}",
            self.compositor.current_opacity(),
        );
    }
}

// =============================================================================
// Advanced Effects API (Gradients, Shadows, Blur)
// =============================================================================

#[allow(clippy::cast_possible_truncation)]
impl WgpuPainter {
    /// Draw a rectangle with a linear gradient.
    ///
    /// # Arguments
    /// * `bounds`          - Rectangle bounds
    /// * `gradient_start`  - Gradient start point (local coordinates)
    /// * `gradient_end`    - Gradient end point (local coordinates)
    /// * `stops`           - Gradient color stops (max 8)
    /// * `corner_radius`   - Corner radius (uniform, 0.0 = sharp corners)
    ///
    /// # Example
    /// ```ignore
    /// // Vertical gradient from red to blue
    /// painter.gradient_rect(
    ///     Rect::from_ltrb(10.0, 10.0, 210.0, 110.0),
    ///     glam::Vec2::new(0.0, 0.0),   // Top
    ///     glam::Vec2::new(0.0, 100.0), // Bottom
    ///     &[
    ///         GradientStop::start(Color::RED),
    ///         GradientStop::end(Color::BLUE),
    ///     ],
    ///     12.0, // Rounded corners
    /// );
    /// ```
    pub fn gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        gradient_start: glam::Vec2,
        gradient_end: glam::Vec2,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        super::batches::DrawBatcher::gradient_rect(
            &mut self.current_segment,
            &self.state,
            bounds,
            gradient_start,
            gradient_end,
            stops,
            corner_radius,
        );
    }

    /// Draw a rectangle with a radial gradient.
    ///
    /// # Arguments
    /// * `bounds`         - Rectangle bounds
    /// * `center`         - Gradient center point (local coordinates)
    /// * `radius`         - Gradient radius
    /// * `stops`          - Gradient color stops (max 8)
    /// * `corner_radius`  - Corner radius (uniform, 0.0 = sharp corners)
    ///
    /// # Example
    /// ```ignore
    /// // Radial gradient from white center to transparent edge
    /// painter.radial_gradient_rect(
    ///     Rect::from_ltrb(10.0, 10.0, 110.0, 110.0),
    ///     glam::Vec2::new(50.0, 50.0), // Center
    ///     50.0,                         // Radius
    ///     &[
    ///         GradientStop::start(Color::WHITE),
    ///         GradientStop::end(Color::TRANSPARENT),
    ///     ],
    ///     0.0, // Sharp corners
    /// );
    /// ```
    pub fn radial_gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        radius: f32,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        super::batches::DrawBatcher::radial_gradient_rect(
            &mut self.current_segment,
            &self.state,
            bounds,
            center,
            radius,
            stops,
            corner_radius,
        );
    }

    /// Draw a rectangle with a sweep (angular/conic) gradient.
    ///
    /// # Arguments
    /// * `bounds`        - Rectangle bounds
    /// * `center`        - Gradient center point (local coordinates)
    /// * `start_angle`   - Start angle in radians
    /// * `end_angle`     - End angle in radians
    /// * `stops`         - Gradient color stops (max 8)
    /// * `corner_radius` - Corner radius (uniform, 0.0 = sharp corners)
    pub fn sweep_gradient_rect(
        &mut self,
        bounds: Rect<Pixels>,
        center: glam::Vec2,
        start_angle: f32,
        end_angle: f32,
        stops: &[super::effects::GradientStop],
        corner_radius: f32,
    ) {
        super::batches::DrawBatcher::sweep_gradient_rect(
            &mut self.current_segment,
            &self.state,
            bounds,
            center,
            start_angle,
            end_angle,
            stops,
            corner_radius,
        );
    }

    /// Draw a shadow for a rectangle.
    ///
    /// Renders an analytical shadow using Evan Wallace's technique.
    /// Single-pass O(1) rendering with quality indistinguishable from real
    /// Gaussian.
    ///
    /// # Arguments
    /// * `rect_pos`       - Rectangle position [x, y]
    /// * `rect_size`      - Rectangle size [width, height]
    /// * `corner_radius`  - Corner radius (uniform)
    /// * `params`         - Shadow parameters (offset, blur, color)
    ///
    /// # Example
    /// ```ignore
    /// use flui_engine::painter::effects::ShadowParams;
    /// use flui_types::styling::Color;
    /// use glam::Vec2;
    ///
    /// // Material Design elevation 2 shadow (offset.y=2, sigma=4, ~0.16 alpha)
    /// painter.shadow_rect(
    ///     [10.0, 10.0],
    ///     [200.0, 100.0],
    ///     12.0,
    ///     &ShadowParams::new(Vec2::new(0.0, 2.0), 4.0, Color::rgba(0, 0, 0, 41)),
    /// );
    /// ```
    pub fn shadow_rect(
        &mut self,
        rect_pos: [f32; 2],
        rect_size: [f32; 2],
        corner_radius: f32,
        params: &super::effects::ShadowParams,
    ) {
        super::batches::DrawBatcher::shadow_rect(
            &mut self.current_segment,
            rect_pos,
            rect_size,
            corner_radius,
            params,
        );
    }
}

// ─── Shared growth helper ─────────────────────────────────────────────────────

/// Compute the total grown-bounds expansion in pixels for a pass chain.
///
/// Each growing pass expands the filter halo by its radius; bounds-preserving
/// passes contribute 0.  Summing the per-pass contributions is the correct
/// conservative bound: each growing pass enlarges the halo of the result of all
/// prior passes, so radii compose additively (matches Flutter
/// `dl_compose_image_filter.cc:33-51` inner→outer bounds chaining).
///
/// ## Exhaustiveness
///
/// The `match` has **no `_` catch-all** — the compiler forces a new arm here
/// whenever a new [`ImageFilterPass`] variant is added (same discipline as
/// `apply_image_filter_passes` in `opacity_layer.rs`).
///
/// ## Formulas
///
/// - [`ImageFilterPass::Blur`] → `kernel_radius(max(sigma_x, sigma_y)) as f32`
///   (the conservative per-axis pad used by the standalone Blur arm, PINNED #2).
/// - [`ImageFilterPass::Morph`] → `radius.ceil()` (pixel expansion per `restore_layer`).
/// - [`ImageFilterPass::ColorMatrix`] → `0.0` (full-viewport REPLACE, no growth).
/// - [`ImageFilterPass::Identity`] → `0.0` (passthrough, no growth).
#[allow(
    clippy::cast_precision_loss,
    reason = "kernel_radius returns u32 ≤ a few thousand for any realistic sigma; \
              cast to f32 is exact for values this small"
)]
pub(super) fn cumulative_growth(passes: &[ImageFilterPass]) -> f32 {
    passes
        .iter()
        .map(|pass| match pass {
            ImageFilterPass::Blur { sigma_x, sigma_y } => {
                super::effects::kernel_radius(sigma_x.max(*sigma_y)) as f32
            }
            ImageFilterPass::Morph { radius, .. } => radius.ceil(),
            // Both are bounds-PRESERVING: neither grows the filter extent.
            ImageFilterPass::ColorMatrix(_) | ImageFilterPass::Identity => 0.0,
        })
        .sum()
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use std::sync::Arc;

    use flui_painting::BlendMode;
    use flui_types::{Point, Rect, Size, geometry::px};

    use super::WgpuPainter;

    /// Headless GPU device + queue for painter tests.
    fn test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter for painter tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Painter Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device for painter tests");
        (Arc::new(device), Arc::new(queue))
    }

    /// Regression: tessellated vertices must be baked through `current_transform`
    /// exactly once by `submit_transformed_geometry`.
    ///
    /// Draw the same line under identity and under scale(2,2) and assert that
    /// the baked vertex x-extent (max_x − min_x) is approximately 2× larger
    /// under the scaled transform.  A double-transform bug would produce ~4×;
    /// a missing transform would produce ~1× regardless of scale.
    #[test]
    fn tessellated_line_bakes_current_transform() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let black = flui_types::Color::rgba(0, 0, 0, 255);

        // --- Identity pass ---
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (200, 200),
        );
        // current_transform == IDENTITY at construction
        painter.line(
            Point::new(px(10.0), px(0.0)),
            Point::new(px(20.0), px(0.0)),
            &Paint::stroke(black, 2.0),
        );
        let verts_identity = painter.tess_vertices_for_test();
        assert!(
            !verts_identity.is_empty(),
            "tessellated line must produce vertices"
        );
        let min_x_id = verts_identity.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
        let max_x_id = verts_identity.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        let extent_identity = max_x_id - min_x_id;

        // --- Scale(2, 2) pass ---
        let mut painter2 = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (200, 200),
        );
        painter2.scale(2.0, 2.0);
        painter2.line(
            Point::new(px(10.0), px(0.0)),
            Point::new(px(20.0), px(0.0)),
            &Paint::stroke(black, 2.0),
        );
        let verts_scaled = painter2.tess_vertices_for_test();
        assert!(
            !verts_scaled.is_empty(),
            "tessellated line under scale(2) must produce vertices"
        );
        let min_x_sc = verts_scaled.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
        let max_x_sc = verts_scaled.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        let extent_scaled = max_x_sc - min_x_sc;

        // Under scale(2,2) the x-extent should be ~2× the identity extent.
        // We allow ±10% tolerance to accommodate stroke-cap geometry.
        // A missing-transform bug yields ratio ≈ 1.0; a double-transform bug
        // yields ratio ≈ 4.0; the correct fix yields ratio ≈ 2.0.
        let ratio = extent_scaled / extent_identity;
        assert!(
            (ratio - 2.0).abs() < 0.2,
            "expected x-extent ratio ≈ 2.0 (transform baked once), got {ratio:.3} \
             (identity_extent={extent_identity:.2}, scaled_extent={extent_scaled:.2})"
        );
    }

    /// P1 regression: `draw_texture` must apply `current_transform` to the
    /// destination rect before queuing the instance.
    ///
    /// Draw the same texture under identity and under `scale(2, 2)` and verify
    /// that the queued instance width/height are 2× larger under the scale.
    /// Before the fix, `draw_texture` passed the raw `dst` rect straight to
    /// `TextureInstance::with_uv`, so HiDPI scale and any widget transform were
    /// silently ignored.
    #[test]
    fn draw_texture_bakes_current_transform() {
        let (device, queue) = test_device_and_queue();
        let tex_id = flui_types::painting::TextureId::new(1);

        // Helper: create a minimal 1×1 external texture.
        let make_tex = |device: &wgpu::Device| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("test external texture"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };

        let dst = Rect::from_xywh(px(10.0), px(20.0), px(50.0), px(30.0));

        // --- Identity pass ---
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (400, 400),
        );
        painter.external_texture_registry_mut().register(
            tex_id,
            make_tex(&device),
            1,
            1,
            false,
            false,
        );
        painter.draw_texture(
            tex_id,
            dst,
            None,
            flui_types::painting::FilterQuality::None,
            1.0,
        );
        let rects_id = painter.external_image_rects_for_test();
        assert_eq!(rects_id.len(), 1, "expected one queued instance");
        let [x_id, y_id, w_id, h_id] = rects_id[0];

        // --- Scale(2, 2) pass ---
        let mut painter2 = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (400, 400),
        );
        painter2.external_texture_registry_mut().register(
            tex_id,
            make_tex(&device),
            1,
            1,
            false,
            false,
        );
        painter2.scale(2.0, 2.0);
        painter2.draw_texture(
            tex_id,
            dst,
            None,
            flui_types::painting::FilterQuality::None,
            1.0,
        );
        let rects_sc = painter2.external_image_rects_for_test();
        assert_eq!(
            rects_sc.len(),
            1,
            "expected one queued instance under scale"
        );
        let [x_sc, y_sc, w_sc, h_sc] = rects_sc[0];

        // Under scale(2,2) origin and size must both be 2×.
        // A missing-transform bug produces identical rects regardless of scale.
        assert!(
            (x_sc - x_id * 2.0).abs() < 0.5,
            "expected x ≈ {:.1}, got {x_sc:.1}",
            x_id * 2.0
        );
        assert!(
            (y_sc - y_id * 2.0).abs() < 0.5,
            "expected y ≈ {:.1}, got {y_sc:.1}",
            y_id * 2.0
        );
        // Width and height: from_ltrb stores (right-left, bottom-top) so we check
        // that the scaled instance covers a 2× larger extent.
        assert!(
            (w_sc - w_id * 2.0).abs() < 0.5,
            "expected w ≈ {:.1}, got {w_sc:.1} (transform not applied to draw_texture dst)",
            w_id * 2.0
        );
        assert!(
            (h_sc - h_id * 2.0).abs() < 0.5,
            "expected h ≈ {:.1}, got {h_sc:.1} (transform not applied to draw_texture dst)",
            h_id * 2.0
        );
    }

    /// P2 regression: texture instances must carry the active scissor so that
    /// `flush_texture_batch` can enforce the clip region.
    ///
    /// Call `clip_rect`, then `draw_texture`, and verify that the queued
    /// external-image entry stores the clip.  Without the fix the scissor field
    /// was absent and all texture draws rendered unclipped.
    #[test]
    fn draw_texture_captures_scissor() {
        let (device, queue) = test_device_and_queue();
        let tex_id = flui_types::painting::TextureId::new(2);

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (400, 400),
        );

        // Create a minimal external texture.
        let gpu_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test external texture scissor"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        painter
            .external_texture_registry_mut()
            .register(tex_id, gpu_tex, 1, 1, false, false);

        // Establish a clip region, then draw the texture inside it.
        painter.clip_rect(Rect::from_xywh(px(10.0), px(10.0), px(80.0), px(60.0)));
        let scissor_before = painter.current_scissor_for_test();
        assert!(
            scissor_before.is_some(),
            "clip_rect must set current_scissor"
        );

        let dst = Rect::from_xywh(px(20.0), px(20.0), px(40.0), px(30.0));
        painter.draw_texture(
            tex_id,
            dst,
            None,
            flui_types::painting::FilterQuality::None,
            1.0,
        );

        let scissors = painter.external_image_scissors_for_test();
        assert_eq!(scissors.len(), 1, "expected one queued instance");
        assert_eq!(
            scissors[0], scissor_before,
            "external image must carry the active scissor at draw time"
        );
    }

    /// P3 regression: `draw_arc` fast path must fall back to tessellation when
    /// the current transform includes a reflection (negative determinant).
    ///
    /// A reflection like `scale(-1, 1)` satisfies `is_axis_aligned()`
    /// (off-diagonals are zero) but would mirror the wedge direction, producing
    /// an arc on the wrong side.  The fix guards on `det >= 0`; reflected arcs
    /// must be routed to `tessellate_arc` which bakes the full matrix.
    ///
    /// We verify by comparing tessellated-vertex x-extents: under `scale(-1, 1)`
    /// (reflection across y-axis) the vertices must be negated relative to
    /// identity, which is only possible if the tessellation path was taken.
    #[test]
    fn draw_arc_reflection_takes_tessellation_path() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();

        // Draw a filled arc under identity — fast path, no tessellated vertices.
        let mut painter_id = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (400, 400),
        );
        let rect = Rect::from_xywh(px(100.0), px(100.0), px(80.0), px(80.0));
        painter_id.draw_arc(
            rect,
            0.0,
            std::f32::consts::PI,
            true,
            &Paint::fill(flui_types::Color::rgba(255, 0, 0, 255)),
        );
        // Identity + no rotation: fast path used → no tessellated geometry.
        let verts_id = painter_id.tess_vertices_for_test();
        assert!(
            verts_id.is_empty(),
            "identity arc must use fast path (no tessellated verts)"
        );

        // Draw the same arc under scale(-1, 1) — reflection, det = -1.
        // Must fall through to tessellation.
        let mut painter_ref = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (400, 400),
        );
        painter_ref.scale(-1.0, 1.0);
        painter_ref.draw_arc(
            rect,
            0.0,
            std::f32::consts::PI,
            true,
            &Paint::fill(flui_types::Color::rgba(255, 0, 0, 255)),
        );
        let verts_ref = painter_ref.tess_vertices_for_test();
        assert!(
            !verts_ref.is_empty(),
            "reflected arc must fall back to tessellation (det < 0 guard not applied)"
        );

        // The tessellated vertices should be in negative-x territory (the reflection
        // maps x → -x, so a rect at x=100..180 becomes x=-180..-100).
        let max_x = verts_ref.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        assert!(
            max_x < 0.0,
            "reflected arc vertices must have max_x < 0 (got {max_x:.1}), \
             indicating the reflection was actually applied via tessellation"
        );
    }

    /// Regression for the damage-scissor leak: the painter is reused across
    /// frames, so `reset_frame_state` MUST clear a per-frame scissor or it
    /// would clip subsequent frames to a stale damage rect.
    #[test]
    fn reset_frame_state_clears_damage_scissor() {
        let (device, queue) = test_device_and_queue();
        let mut painter = WgpuPainter::with_shared_device(
            device,
            queue,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            (100, 100),
        );

        // Simulate the per-frame damage clip the Renderer applies (unpaired).
        painter.clip_rect(Rect::from_origin_size(
            Point::ZERO,
            Size::new(px(50.0), px(50.0)),
        ));
        assert!(
            painter.current_scissor_for_test().is_some(),
            "clip_rect must set the current scissor"
        );

        painter.reset_frame_state();
        assert!(
            painter.current_scissor_for_test().is_none(),
            "reset_frame_state must clear the scissor so it cannot leak into the next frame"
        );
    }

    // ===== Color-readback helpers (BUG 1/2/3 regression tests) =====

    /// Format used for all color-readback tests: plain UNorm so the stored bytes
    /// equal the sRGB-encoded bytes the shader emits 1:1 (no OETF on store),
    /// matching the production surface format chosen by `select_surface_format`
    /// and Flutter/Impeller's onscreen convention.
    const READBACK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    /// Render `draw` into a `size`×`size` UNorm offscreen target cleared to
    /// `clear`, then read back the center texel as `[r, g, b, a]` bytes.
    ///
    /// Mirrors the production frame: the painter records draw commands and
    /// `render()` flushes them, including offscreen opacity/ColorFilter layers,
    /// onto the offscreen target. The center pixel is well inside any full-size
    /// fill so we avoid AA-edge ambiguity.
    fn render_and_read_center(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        size: u32,
        clear: wgpu::Color,
        draw: impl FnOnce(&mut WgpuPainter),
    ) -> [u8; 4] {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("readback target"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: READBACK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(device),
            Arc::clone(queue),
            READBACK_FORMAT,
            (size, size),
        );

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // Clear the target to the requested background colour.
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("readback clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        draw(&mut painter);
        painter
            .render_to_view(&target_view, &mut encoder)
            .expect("painter.render must succeed for readback");

        // Copy the target into a CPU-readable buffer. `bytes_per_row` must be a
        // multiple of 256; for the small square targets here a single padded row
        // covers the full width.
        let bytes_per_pixel = 4u32;
        let unpadded = size * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded.div_ceil(align) * align;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback buffer"),
            size: u64::from(padded_bytes_per_row) * u64::from(size),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(size),
                },
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| {
            r.expect("buffer mapping must succeed");
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete the readback copy");

        let data = slice.get_mapped_range();
        let center = size / 2;
        let row = center as usize * padded_bytes_per_row as usize;
        let col = center as usize * bytes_per_pixel as usize;
        let off = row + col;
        let px = [data[off], data[off + 1], data[off + 2], data[off + 3]];
        drop(data);
        buffer.unmap();
        px
    }

    /// Render `draw` into a `size`×`size` UNorm target cleared to `clear`, then
    /// return the tightly-packed RGBA bytes (`size*size*4`, row stride
    /// `size*4`). Use [`pixel_at`] to sample an individual texel. Unlike
    /// [`render_and_read_center`] this exposes every pixel so edge/column
    /// sampling (e.g. atlas-bleed checks) is possible.
    fn render_to_rgba(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        size: u32,
        clear: wgpu::Color,
        draw: impl FnOnce(&mut WgpuPainter),
    ) -> Vec<u8> {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("readback target (full)"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: READBACK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(device),
            Arc::clone(queue),
            READBACK_FORMAT,
            (size, size),
        );

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("readback clear (full)"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        draw(&mut painter);
        painter
            .render_to_view(&target_view, &mut encoder)
            .expect("painter.render must succeed for readback");

        let bytes_per_pixel = 4u32;
        let unpadded = size * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded.div_ceil(align) * align;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback buffer (full)"),
            size: u64::from(padded_bytes_per_row) * u64::from(size),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(size),
                },
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| {
            r.expect("buffer mapping must succeed");
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete the readback copy");

        let data = slice.get_mapped_range();
        let stride = padded_bytes_per_row as usize;
        let row_bytes = (size * bytes_per_pixel) as usize;
        let mut out = Vec::with_capacity(row_bytes * size as usize);
        for y in 0..size as usize {
            let start = y * stride;
            out.extend_from_slice(&data[start..start + row_bytes]);
        }
        drop(data);
        buffer.unmap();
        out
    }

    /// Sample one RGBA texel from a tightly-packed buffer produced by
    /// [`render_to_rgba`].
    fn pixel_at(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let off = (y as usize * size as usize + x as usize) * 4;
        [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
    }

    /// BUG 1 (sRGB double-encode): a mid-tone `Color::rgb(128,128,128)` filled
    /// over an opaque target must read back ~128 per channel on the UNorm
    /// surface format, NOT ~188.
    ///
    /// On an sRGB target the GPU treats the shader's already-sRGB 0.502 as
    /// *linear* and applies the linear->sRGB OETF on store, brightening 0x80 to
    /// ~0xBC (188). Primaries (0/255) are OETF fixed points, so geometry tests
    /// never caught this — only a mid-tone readback does. This fails on the old
    /// sRGB-preferring format and passes on UNorm (Impeller parity).
    #[test]
    fn midtone_fill_is_not_srgb_double_encoded() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0)),
                &Paint::fill(flui_types::Color::rgb(128, 128, 128)),
            );
        });

        for (i, label) in ["R", "G", "B"].iter().enumerate() {
            let v = i32::from(px[i]);
            assert!(
                (v - 128).abs() <= 3,
                "channel {label} = {v}, expected ~128 (UNorm 1:1 store). \
                 ~188 indicates an sRGB target double-encoding the color. \
                 full pixel = {px:?}"
            );
        }
    }

    /// BUG 2 (opacity-layer premultiplied double-multiply): a translucent rect
    /// `rgba(255,0,0,128)` drawn inside a `save_layer` of opacity 0.5 over an
    /// opaque WHITE background must composite as premultiplied source-over.
    ///
    /// The offscreen texel is premultiplied (`rgb = 255*0.502 = 128`, `a=128`).
    /// Pre-scaled by the group tint `(0.5,0.5,0.5,0.5)` it is `(0.251,0,0,0.251)`;
    /// premultiplied-OVER white yields R ≈ 255, G ≈ B ≈ 191. The OLD straight-
    /// alpha composite re-multiplies rgb by alpha, dropping R to ~223. So R is
    /// the discriminating channel: this fails (~223) before the fix and passes
    /// (~255) after.
    #[test]
    fn opacity_layer_composites_premultiplied() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |painter| {
            painter.save_layer(None, &Paint::fill(flui_types::Color::WHITE).with_alpha(128));
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0)),
                &Paint::fill(flui_types::Color::rgba(255, 0, 0, 128)),
            );
            painter.restore_layer();
        });

        let (r, g, b) = (i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
        // Premultiplied-OVER white: R ≈ 255 (fixed). The straight-alpha bug
        // gives R ≈ 223. Use a tolerance that excludes the buggy value.
        assert!(
            (r - 255).abs() <= 4,
            "R = {r}, expected ~255 (premultiplied composite). \
             R ≈ 223 indicates the straight-alpha double-multiply bug. pixel = {px:?}"
        );
        assert!(
            (g - 191).abs() <= 6 && (b - 191).abs() <= 6,
            "G,B = {g},{b}, expected ~191. pixel = {px:?}"
        );
    }

    /// Depth-2 nested opacity: `flush_opacity_layer` recurses correctly.
    ///
    /// Two nested `save_layer` calls each carry opacity 0.5 over an opaque BLACK
    /// background. The innermost content is a full-coverage opaque RED rect.
    ///
    /// ## Compositing derivation (premultiplied SrcOver throughout)
    ///
    /// **Inner layer (opacity 0.5):**
    /// - Offscreen cleared to TRANSPARENT; RED fill is premultiplied `(1,0,0,1)`.
    /// - Group-opacity tint `(0.5,0.5,0.5,0.5)` applied at composite time:
    ///   effective premultiplied source = `(0.5, 0, 0, 0.5)`.
    /// - Composited onto TRANSPARENT outer offscreen (SrcOver premul):
    ///   outer offscreen = `(0.5, 0, 0, 0.5)` pmul ≡ straight `(1,0,0,0.5)`.
    ///
    /// **Outer layer (opacity 0.5):**
    /// - Outer offscreen contains pmul `(0.5, 0, 0, 0.5)`.
    /// - Group-opacity tint `(0.5,0.5,0.5,0.5)` → scaled pmul `(0.25, 0, 0, 0.25)`.
    /// - SrcOver onto opaque BLACK `(0,0,0,1)`:
    ///   `dst = src + dst*(1−src.a)` = `(0.25,0,0,0.25) + (0,0,0,1)*0.75`
    ///   = `(0.25, 0, 0, 1.0)`.
    /// - In `[0,255]`: **R ≈ 64, G = 0, B = 0**.
    ///
    /// ## Discriminating power
    ///
    /// | Failure mode                                 | Expected R |
    /// |----------------------------------------------|------------|
    /// | Recursion dropped — only outer 0.5 applied  | ~128       |
    /// | Inner texture leaked / pool not cleared      | ~255       |
    /// | Correct depth-2 (this test)                  | ~64        |
    ///
    /// The assertion band `[40, 90]` excludes both failure modes.
    #[test]
    fn nested_opacity_layers_compose_at_depth_2() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let center_pixel =
            render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
                // Outer group opacity 0.5 — opaque-RGB paint; alpha drives layer opacity.
                painter.save_layer(None, &Paint::fill(flui_types::Color::WHITE).with_alpha(128));
                // Inner group opacity 0.5 nested inside the outer.
                painter.save_layer(None, &Paint::fill(flui_types::Color::WHITE).with_alpha(128));
                // Opaque RED fills the full canvas (center pixel fully covered).
                painter.rect(
                    Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0)),
                    &Paint::fill(flui_types::Color::rgba(255, 0, 0, 255)),
                );
                painter.restore_layer(); // inner → composites at depth-1 offscreen
                painter.restore_layer(); // outer → composites to main surface
            });

        let (r, g, b) = (
            i32::from(center_pixel[0]),
            i32::from(center_pixel[1]),
            i32::from(center_pixel[2]),
        );
        // Depth-2 composite: 0.5 × 0.5 = 0.25 effective opacity → R ≈ 64.
        // Depth-1 only (missed recursion) gives R ≈ 128.
        // Leaked inner texture gives R ≈ 255.
        assert!(
            (40..=90).contains(&r),
            "R = {r}, expected ~64 (doubly-attenuated RED at 0.5×0.5 over BLACK). \
             R ≈ 128 means the inner `flush_opacity_layer` recursion was skipped; \
             R ≈ 255 means the inner offscreen leaked to the outer composite. \
             pixel = {center_pixel:?}"
        );
        assert!(
            g <= 20 && b <= 20,
            "G = {g}, B = {b}, expected ~0 (no green/blue in doubly-attenuated RED). \
             pixel = {center_pixel:?}"
        );
    }

    /// BUG 3 (dropped ColorFilter tint): a `save_layer` whose paint carries a
    /// non-white chroma (blue at alpha 0.5) must shift the composited hue, not
    /// merely attenuate alpha.
    ///
    /// An opaque WHITE rect inside the layer becomes premultiplied
    /// `(255,255,255,255)`; the chroma tint `(0,0,1)*0.5 = (0,0,0.502)` with
    /// `a=0.502` selects only the blue channel. Premultiplied-OVER BLACK yields
    /// `(0,0,128)`. The OLD path hardcoded a white tint (chroma discarded), so
    /// the result would be gray `(128,128,128)` — B not dominant. The assertion
    /// `B >> R,G` fails before the fix and passes after.
    #[test]
    fn color_filter_layer_shifts_hue() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        // Blue chroma at opacity 0.5 via the explicit tint entry point — exactly
        // what `Backend::push_color_filter` now calls for a white->blue
        // ColorMatrix. (`save_layer` deliberately ignores paint RGB, so chroma
        // must come through `save_layer_with_tint`.)
        let px = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
            painter.save_layer_with_tint(None, 0.5, [0.0, 0.0, 1.0]);
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0)),
                &Paint::fill(flui_types::Color::WHITE),
            );
            painter.restore_layer();
        });

        let (r, g, b) = (i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
        assert!(
            b > r + 40 && b > g + 40,
            "expected blue-dominant composite (hue shift present): \
             B={b} must exceed R={r} and G={g} substantially. \
             A gray result (~128,128,128) means the ColorFilter chroma was dropped. \
             pixel = {px:?}"
        );
        assert!(
            b > 100,
            "B = {b}, expected ~128 (blue chroma at 0.5 over black). pixel = {px:?}"
        );
    }

    /// P1 regression: an alpha-only saveLayer paint with non-white RGB must NOT
    /// tint the layer.
    ///
    /// The public canvas opacity helpers (`Canvas::save_layer_alpha` /
    /// `save_layer_opacity`, flui-painting `canvas/state.rs`) build their layer
    /// paint as `Paint::fill(Color::TRANSPARENT).with_opacity(O)` — RGB
    /// `[0,0,0]`, alpha `O`. If `save_layer` treated paint RGB as a composite
    /// tint, those layers would composite with `(0,0,0,O)` and render the
    /// contents BLACK instead of applying group opacity. `save_layer` must
    /// normalize to a white (no-op) chroma; only `save_layer_with_tint` carries
    /// chroma.
    ///
    /// Opaque WHITE content in a 0.5 layer (black-RGB paint) over BLACK must
    /// composite to mid-gray ≈128, not 0.
    #[test]
    fn alpha_only_layer_paint_does_not_tint_black() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
            // Mirror the canvas opacity helper: TRANSPARENT (RGB 0,0,0) + alpha.
            painter.save_layer(None, &Paint::fill(flui_types::Color::rgba(0, 0, 0, 128)));
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0)),
                &Paint::fill(flui_types::Color::WHITE),
            );
            painter.restore_layer();
        });

        let (r, g, b) = (i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
        // White at group opacity 0.5 over black ≈ (128,128,128). The pre-fix
        // RGB-as-tint bug gives (0,0,0) — assert clearly above black.
        assert!(
            r > 100 && g > 100 && b > 100,
            "expected mid-gray ~128 (group opacity, white chroma); \
             a near-black result means the alpha-only paint's RGB was wrongly \
             used as a tint. pixel = {px:?}"
        );
        assert!(
            (r - 128).abs() <= 12 && (g - 128).abs() <= 12 && (b - 128).abs() <= 12,
            "R,G,B = {r},{g},{b}, expected ~128. pixel = {px:?}"
        );
    }

    /// P0 regression: decoded-image textures must NOT be sRGB when the surface
    /// is UNorm.
    ///
    /// Before the fix `texture_cache.rs` created image textures as
    /// `Rgba8UnormSrgb`. On sample the GPU applies the sRGB→linear EOTF
    /// (byte 0x80 → linear ≈0.216), the shader outputs that linear float,
    /// and the UNorm surface stores it as ≈0x37 — much too dark. After the fix
    /// `IMAGE_TEXTURE_FORMAT = Rgba8Unorm`: the byte is sampled verbatim as
    /// byte/255, the shader emits it unchanged, and the UNorm surface stores
    /// 0x80 → 0x80.
    ///
    /// The test creates an image texture using `IMAGE_TEXTURE_FORMAT` (the same
    /// constant `texture_cache` uses at runtime), fills every pixel with 0x80,
    /// draws it full-frame via the external-texture path, and asserts the center
    /// readback is ≈0x80 (±3).  When `IMAGE_TEXTURE_FORMAT` was `Rgba8UnormSrgb`
    /// the test would have read ≈0x37 (55) and failed.
    #[test]
    fn decoded_image_midtone_round_trips() {
        const SIZE: u32 = 64;
        let (device, queue) = test_device_and_queue();

        // Create a texture using the format that texture_cache uses for decoded
        // images.  All pixels = RGBA(0x80, 0x80, 0x80, 0xFF).
        let img_format = crate::wgpu::texture_cache::IMAGE_TEXTURE_FORMAT;
        let img_data: Vec<u8> = (0..SIZE * SIZE)
            .flat_map(|_| [0x80u8, 0x80, 0x80, 0xFF])
            .collect();
        let gpu_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("midtone round-trip image"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: img_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * SIZE),
                rows_per_image: Some(SIZE),
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );

        // Readback target: UNorm, matching the production surface format.
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("readback target"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: READBACK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            READBACK_FORMAT,
            (SIZE, SIZE),
        );
        let tex_id = flui_types::painting::TextureId::new(99);
        painter
            .external_texture_registry_mut()
            .register(tex_id, gpu_tex, SIZE, SIZE, false, false);
        painter.draw_texture(
            tex_id,
            Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
            None,
            flui_types::painting::FilterQuality::None,
            1.0,
        );

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            // Clear to black so the image pixels are the only contributor.
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("readback clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
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
        painter
            .render_to_view(&target_view, &mut encoder)
            .expect("painter.render must succeed");

        let bytes_per_pixel = 4u32;
        let unpadded = SIZE * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded.div_ceil(align) * align;
        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback buffer"),
            size: u64::from(padded_bytes_per_row) * u64::from(SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = readback_buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| {
            r.expect("buffer mapping must succeed");
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete");

        let data = slice.get_mapped_range();
        let center = SIZE / 2;
        let row = center as usize * padded_bytes_per_row as usize;
        let col = center as usize * bytes_per_pixel as usize;
        let off = row + col;
        let px = [data[off], data[off + 1], data[off + 2], data[off + 3]];
        drop(data);
        readback_buf.unmap();

        for (i, label) in ["R", "G", "B"].iter().enumerate() {
            let v = i32::from(px[i]);
            assert!(
                (v - 0x80).abs() <= 3,
                "channel {label} = {v} (0x{v:02X}), expected ~0x80 (128). \
                 A value ~0x37 (55) means the image texture was sampled as sRGB \
                 (GPU applied EOTF on read) — IMAGE_TEXTURE_FORMAT must be Rgba8Unorm. \
                 full pixel = {px:?}"
            );
        }
    }

    /// BUG 1 (fill rule hardcoded EvenOdd): a default-fill-type `Path` of two
    /// overlapping same-winding triangles must fill the overlap SOLID (non-zero
    /// winding, the FLUI/Flutter default), not punch a hole there (even-odd).
    ///
    /// The two triangles share the same winding, so the overlap region has a
    /// non-zero winding number (filled under NonZero) but an even crossing count
    /// (a hole under EvenOdd). Sampling the overlap center discriminates the two
    /// rules: before the fix `tessellate_fill` hardcoded
    /// `FillOptions::default()` (EvenOdd), so the overlap read back the clear
    /// color (transparent hole over black ≈ 0). After the fix the path's
    /// `fill_type()` (default NonZero) flows through, so the overlap reads the
    /// opaque fill color (RED).
    #[test]
    fn path_fill_honors_nonzero_default_fill_rule() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
            // Default fill type is NonZero. Two same-winding triangles whose
            // bodies overlap around the frame center (~32,24).
            let mut path = flui_types::painting::path::Path::new();
            path.move_to(Point::new(px(4.0), px(4.0)));
            path.line_to(Point::new(px(56.0), px(4.0)));
            path.line_to(Point::new(px(30.0), px(56.0)));
            path.close();
            path.move_to(Point::new(px(8.0), px(4.0)));
            path.line_to(Point::new(px(60.0), px(4.0)));
            path.line_to(Point::new(px(34.0), px(56.0)));
            path.close();
            painter.draw_path(&path, &Paint::fill(flui_types::Color::rgb(255, 0, 0)));
        });

        let r = i32::from(px_val[0]);
        assert!(
            r > 200,
            "overlap center R = {r}, expected ~255 (opaque RED). \
             A near-zero R means the overlap was punched out as an EvenOdd hole \
             instead of filled under the path's default NonZero rule. pixel = {px_val:?}"
        );
    }

    /// BUG 2 follow-up (stale-scale hazard): `draw_shadow` tessellates a path and
    /// must prime the tessellator's flatten scale from the current CTM, like every
    /// other tessellation site. Otherwise shadow curves facet at whatever scale a
    /// previous draw left behind.
    ///
    /// We seed a stale scale (1.0) under a scale(8) CTM, then `draw_shadow`. With
    /// the prime call the tessellator reports 8.0; without it the stale 1.0
    /// survives — the assertion (inside the draw closure, so it runs with the live
    /// painter) discriminates the two.
    #[test]
    fn draw_shadow_primes_tessellator_scale() {
        let (device, queue) = test_device_and_queue();
        let _ = render_and_read_center(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
            painter.scale(8.0, 8.0);
            // Simulate a prior draw that left the tessellator at scale 1.0.
            painter.set_tessellator_max_scale_for_test(1.0);

            let mut path = flui_types::painting::path::Path::new();
            path.move_to(Point::new(px(8.0), px(8.0)));
            path.line_to(Point::new(px(24.0), px(8.0)));
            path.line_to(Point::new(px(24.0), px(24.0)));
            path.line_to(Point::new(px(8.0), px(24.0)));
            path.close();
            // elevation > 0.1 so the shadow actually tessellates.
            painter.draw_shadow(&path, flui_types::Color::BLACK, 4.0);

            let s = painter.tessellator_max_scale_for_test();
            assert!(
                (s - 8.0).abs() < 1e-3,
                "draw_shadow must prime the tessellator to the CTM scale (8.0); \
                 got {s}. A value of ~1.0 means draw_shadow tessellated with a \
                 stale scale (faceted shadow curves on HiDPI)."
            );
        });
    }

    /// BUG 3 (atlas packed with zero gutter): two images packed adjacently in
    /// the shared atlas must not bleed into each other under the Linear sampler.
    ///
    /// RED (A) is allocated first so it occupies atlas column range `[0, 64)`;
    /// BLUE (B) is allocated next, immediately to A's right. A is then drawn
    /// magnified AND extended past the right of the frame (`dst.x ∈ [-64, 128]`,
    /// a 3x stretch) so that its `max_u` maps near screen column 128. Column
    /// x=127 checks for BLUE bleed at the atlas seam.
    ///
    /// With the fix: `upload_image` clears a 1px transparent gutter on the right
    /// side of A. The bilinear kernel blends the last RED texel with alpha-zero
    /// (not B's solid BLUE), leaving B~0. R may be attenuated but is never BLUE.
    ///
    /// Before the fix: no gutter clear — A's `max_u` coincided with B's first
    /// texel and bilinear sampling raised B well above 40.
    #[test]
    fn atlas_neighbors_do_not_bleed_under_linear_sampling() {
        use flui_types::painting::Image;

        const SIZE: u32 = 128;
        let (device, queue) = test_device_and_queue();

        let red = Image::solid_color(64, 64, flui_types::Color::rgb(255, 0, 0));
        let blue = Image::solid_color(64, 64, flui_types::Color::rgb(0, 0, 255));

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            // RED packs first → atlas columns [0, 64). It is stretched over
            // screen x ∈ [-64, 128] (width 192, 3x): the dst maps source u=0..1
            // across that span, so screen x=127 → u≈0.995 (near max_u) and is a
            // fully-RED interior pixel because the geometric right edge sits at
            // x=128, off the sampled column.
            painter.draw_image(
                &red,
                Rect::from_xywh(px(-64.0), px(0.0), px(192.0), px(128.0)),
                flui_painting::BlendMode::SrcOver,
            );
            // BLUE packs next → atlas columns immediately right of RED's gutter.
            // Its slot is what an un-guttered bilinear sample of RED's right edge
            // would bleed into. Draw it off-screen; bleed is a texture-space
            // phenomenon, not screen-space.
            painter.draw_image(
                &blue,
                Rect::from_xywh(px(120.0), px(120.0), px(8.0), px(8.0)),
                flui_painting::BlendMode::SrcOver,
            );
        });

        // Sample the near-max_u column (x=127) at mid-height.
        //
        // With a transparent gutter the bilinear kernel at max_u blends the
        // last RED texel with an alpha-zero gutter pixel, which dims the RED
        // channel but contributes *zero* BLUE. So the correct assertion for the
        // "no bleed" property is `b < 40` (BLUE does not reach the sample site)
        // and `r > b + 80` (RED dominates BLUE even when partially attenuated).
        //
        // Without the gutter clear, BLUE from the neighboring atlas entry bleeds
        // in and raises B above 100 — clearly distinguishable from the ~0 B of
        // the transparent-gutter case.
        let edge = pixel_at(&rgba, SIZE, 127, 64);
        let (r, g, b) = (i32::from(edge[0]), i32::from(edge[1]), i32::from(edge[2]));
        assert!(
            b < 40 && r > b + 80,
            "RED's near-max_u column = (R={r}, G={g}, B={b}). \
             Expected B~0 (no BLUE bleed) and R dominant. \
             A B≥40 value means the Linear sampler bled BLUE from the neighboring \
             atlas entry — the transparent gutter strip in upload_image is missing."
        );
    }

    /// BUG 3 sharpness: a 2×1 image drawn exactly 1:1 must sample each texel
    /// with its own color, not a blend with its neighbor.
    ///
    /// With the (wrong) half-texel UV inset, `min_u` for a 2-wide image in a
    /// 2048-wide atlas shifts right by `0.5/2048 ≈ 0.000244`, and `max_u`
    /// shifts left symmetrically.  The left screen pixel's UV maps to roughly
    /// `texel 0.25` (mix of 75% RED + 25% GREEN) rather than `texel 0.5` (pure
    /// RED); the right pixel maps to roughly `texel 1.75` (25% RED + 75% GREEN)
    /// rather than `texel 1.5` (pure GREEN).  The RED and GREEN channels would
    /// both read ~191 instead of 255/0.
    ///
    /// With exact texel-boundary UVs and a transparent gutter: left pixel maps to
    /// `texel 0.5` → pure RED; right pixel maps to `texel 1.5` → pure GREEN.
    #[test]
    fn atlas_image_is_sharp_at_one_to_one() {
        use flui_types::painting::Image;

        // 2-pixel wide, 1-pixel tall render target (drawn 1:1).
        const W: u32 = 2;
        const H: u32 = 1;
        let (device, queue) = test_device_and_queue();

        // Left texel = RED, right texel = GREEN.
        let pixels: Vec<u8> = vec![
            255, 0, 0, 255, // left pixel: RED
            0, 255, 0, 255, // right pixel: GREEN
        ];
        let img = Image::from_rgba8(W, H, pixels);

        let rgba = render_to_rgba(&device, &queue, W, wgpu::Color::BLACK, |painter| {
            painter.draw_image(
                &img,
                Rect::from_xywh(px(0.0), px(0.0), px(2.0), px(1.0)),
                flui_painting::BlendMode::SrcOver,
            );
        });

        let left = pixel_at(&rgba, W, 0, 0);
        let right = pixel_at(&rgba, W, 1, 0);
        let (lr, lg) = (i32::from(left[0]), i32::from(left[1]));
        let (rr, rg) = (i32::from(right[0]), i32::from(right[1]));
        assert!(
            lr > 200 && lg < 55,
            "left pixel = (R={lr}, G={lg}): expected RED (~255,~0). \
             A blended value means uv_coords has a half-texel inset that \
             shifts sampling away from the texel center."
        );
        assert!(
            rg > 200 && rr < 55,
            "right pixel = (R={rr}, G={rg}): expected GREEN (~0,~255). \
             A blended value means uv_coords has a half-texel inset that \
             shifts sampling away from the texel center."
        );
    }

    // ===== Phase A per-draw blend mode (fixed-function Porter-Duff) =====
    //
    // These tests exercise the TESSELLATED path: a filled `Path` and stroked
    // shapes always tessellate (never the instanced fast path), so they go
    // through `pipeline_key_from_paint` → the blend pipeline keyed by
    // `PipelineKey::blend_mode`. All values are premultiplied-alpha results on
    // the UNorm readback target.

    /// Helper: a filled-path rect covering the whole `size`×`size` frame. Forces
    /// the tessellated path regardless of the (axis-aligned) transform, so the
    /// per-draw blend pipeline is selected.
    fn full_frame_fill_path(size: f32) -> flui_types::painting::path::Path {
        flui_types::painting::path::Path::rectangle(Rect::from_xywh(
            px(0.0),
            px(0.0),
            px(size),
            px(size),
        ))
    }

    /// SrcOver pixel-identity (regression guard for the premultiply switch):
    /// a 50%-alpha RED filled PATH over opaque white must read back the same
    /// straight-SrcOver value the old `input.color` + `ALPHA_BLENDING` path
    /// produced. Premultiplied SrcOver is `src + dst*(1-a)`; with
    /// `src = (0.502,0,0,0.502)` over white this is `(1.0, 0.498, 0.498)` ≈
    /// `(255, 127, 127)` — identical to the old output. A divergence here means
    /// the premultiply switch changed visible SrcOver output.
    #[test]
    fn blend_srcover_filled_path_pixel_identity() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |painter| {
            painter.draw_path(
                &full_frame_fill_path(64.0),
                &Paint::fill(flui_types::Color::rgba(255, 0, 0, 128)),
            );
        });

        let (r, g, b) = (
            i32::from(px_val[0]),
            i32::from(px_val[1]),
            i32::from(px_val[2]),
        );
        assert!(
            (r - 255).abs() <= 3,
            "R = {r}, expected ~255 (premultiplied SrcOver red over white). pixel = {px_val:?}"
        );
        assert!(
            (g - 127).abs() <= 4 && (b - 127).abs() <= 4,
            "G,B = {g},{b}, expected ~127 (50% red over white). \
             A drift here means premultiplied SrcOver is no longer identical to \
             the old straight-alpha output. pixel = {px_val:?}"
        );
    }

    /// SrcOver pixel-identity for a STROKED shape (also always tessellated).
    /// A 50%-alpha RED stroke (width 16, centerline at x=8) over opaque white,
    /// sampled on the left stroke band, must read the same premultiplied SrcOver
    /// value `(255, 127, 127)`.
    #[test]
    fn blend_srcover_stroked_rect_pixel_identity() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let rgba = render_to_rgba(&device, &queue, 64, wgpu::Color::WHITE, |painter| {
            // Inset rect so its left edge centerline sits at x=8; a 16px stroke
            // fully covers the band around x=8.
            painter.rect(
                Rect::from_ltrb(px(8.0), px(8.0), px(56.0), px(56.0)),
                &Paint::stroke(flui_types::Color::rgba(255, 0, 0, 128), 16.0),
            );
        });

        // Sample the left vertical stroke band, away from the corner.
        let p = pixel_at(&rgba, 64, 8, 32);
        let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
        assert!(
            (r - 255).abs() <= 4 && (g - 127).abs() <= 6 && (b - 127).abs() <= 6,
            "stroke-band pixel = (R={r}, G={g}, B={b}), expected ~(255,127,127) \
             (premultiplied SrcOver 50% red over white). pixel = {p:?}"
        );
    }

    /// Clear: an OPAQUE shape drawn with `BlendMode::Clear` over a red background
    /// punches out to fully transparent. Clear factors are `src Zero, dst Zero`,
    /// so the covered texel becomes `(0,0,0,0)` regardless of source color.
    #[test]
    fn blend_clear_punches_out() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::RED, |painter| {
            painter.draw_path(
                &full_frame_fill_path(64.0),
                &Paint::fill(flui_types::Color::rgb(0, 255, 0)).with_blend_mode(BlendMode::Clear),
            );
        });

        assert_eq!(
            px_val,
            [0, 0, 0, 0],
            "Clear must punch the covered region to transparent (0,0,0,0); \
             got {px_val:?}. A red result means Clear fell back to SrcOver."
        );
    }

    /// Plus (Lighter): a RED shape drawn `Plus` over a green background sums the
    /// components → yellow. Plus factors are `src One, dst One`, so
    /// `result = src + dst = (1,1,0)` → `(255, 255, 0)`.
    #[test]
    fn blend_plus_sums_to_yellow() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::GREEN, |painter| {
            painter.draw_path(
                &full_frame_fill_path(64.0),
                &Paint::fill(flui_types::Color::rgb(255, 0, 0)).with_blend_mode(BlendMode::Plus),
            );
        });

        let (r, g, b) = (
            i32::from(px_val[0]),
            i32::from(px_val[1]),
            i32::from(px_val[2]),
        );
        assert!(
            r > 250 && g > 250 && b < 5,
            "Plus(red, green) must read back yellow ~(255,255,0); got {px_val:?}. \
             A non-additive result means Plus fell back to SrcOver."
        );
    }

    /// Modulate: an opaque 50%-gray shape drawn `Modulate` over RED multiplies
    /// the components → ~half red. Modulate color factor is `src Dst, dst Zero`,
    /// so `result.rgb = src.rgb * dst.rgb`. With premultiplied opaque gray
    /// `(0.502,0.502,0.502)` over red `(1,0,0)` → `(0.502, 0, 0)` ≈ `(128,0,0)`.
    #[test]
    fn blend_modulate_multiplies_to_half_red() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::RED, |painter| {
            painter.draw_path(
                &full_frame_fill_path(64.0),
                &Paint::fill(flui_types::Color::rgb(128, 128, 128))
                    .with_blend_mode(BlendMode::Modulate),
            );
        });

        let (r, g, b) = (
            i32::from(px_val[0]),
            i32::from(px_val[1]),
            i32::from(px_val[2]),
        );
        assert!(
            (r - 128).abs() <= 6,
            "R = {r}, expected ~128 (gray * red). pixel = {px_val:?}"
        );
        assert!(
            g < 5 && b < 5,
            "G,B = {g},{b}, expected ~0 (red has no green/blue to modulate). \
             pixel = {px_val:?}"
        );
    }

    /// DstOver: an opaque BLUE shape drawn `DstOver` over opaque RED leaves the
    /// destination unchanged where it is already opaque. DstOver factors are
    /// `src OneMinusDstAlpha, dst One`; with `dst.a = 1` the source contributes
    /// nothing, so the result stays RED.
    #[test]
    fn blend_dstover_keeps_opaque_destination() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::RED, |painter| {
            painter.draw_path(
                &full_frame_fill_path(64.0),
                &Paint::fill(flui_types::Color::rgb(0, 0, 255)).with_blend_mode(BlendMode::DstOver),
            );
        });

        let (r, g, b) = (
            i32::from(px_val[0]),
            i32::from(px_val[1]),
            i32::from(px_val[2]),
        );
        assert!(
            r > 250 && g < 5 && b < 5,
            "DstOver over opaque red must stay red ~(255,0,0); got {px_val:?}. \
             A blue result means the source wrongly painted over the opaque dest."
        );
    }

    /// Advanced fallback honesty: `BlendMode::Multiply` is an advanced (dst-read)
    /// mode NOT supported by the Phase A fixed-function path. It must fall back
    /// to SrcOver (warn-once), NOT render garbage or panic. We assert the
    /// Multiply draw produces the exact same pixel as an explicit SrcOver draw
    /// of the same shape — documenting the Phase-A fallback.
    #[test]
    fn blend_advanced_multiply_falls_back_to_srcover() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let color = flui_types::Color::rgba(255, 0, 0, 128);

        let multiply_px = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |p| {
            p.draw_path(
                &full_frame_fill_path(64.0),
                &Paint::fill(color).with_blend_mode(BlendMode::Multiply),
            );
        });
        let srcover_px = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |p| {
            p.draw_path(
                &full_frame_fill_path(64.0),
                &Paint::fill(color).with_blend_mode(BlendMode::SrcOver),
            );
        });

        assert_eq!(
            multiply_px, srcover_px,
            "advanced Multiply must fall back to SrcOver (Phase A): \
             Multiply pixel {multiply_px:?} must equal SrcOver pixel {srcover_px:?}. \
             A difference means Multiply was treated as a real (incorrect) \
             fixed-function blend instead of the honest SrcOver fallback."
        );
        // And the fallback must be the known SrcOver value, not transparent/garbage.
        let r = i32::from(srcover_px[0]);
        assert!(
            (r - 255).abs() <= 3,
            "fallback R = {r}, expected ~255 (SrcOver red over white). pixel = {srcover_px:?}"
        );
    }

    /// Phase-A gradient + non-SrcOver honesty: drawing a gradient (shader) fill
    /// with `BlendMode::Clear` must NOT panic and must render as SrcOver (the
    /// gradient pipeline ignores `blend_mode` in Phase A). The test documents
    /// the limit — a white background must remain visible (non-zero alpha)
    /// because Clear is silently downgraded to SrcOver for gradients.
    ///
    /// Phase B will add dst-sample blended gradient support.
    #[test]
    fn gradient_with_non_srcover_blend_mode_does_not_panic() {
        use flui_painting::Paint;
        use flui_types::painting::TileMode;

        let (device, queue) = test_device_and_queue();

        // A horizontal red→blue linear gradient with BlendMode::Clear.
        // Phase A: Clear is ignored; the gradient renders via SrcOver so the
        // white background is partially or fully covered — NOT erased to (0,0,0,0).
        let shader = flui_painting::Shader::linear_gradient(
            flui_types::Point::new(px(0.0), px(0.0)).into(),
            flui_types::Point::new(px(64.0), px(0.0)).into(),
            vec![
                flui_types::Color::rgb(255, 0, 0),
                flui_types::Color::rgb(0, 0, 255),
            ],
            None,
            TileMode::Clamp,
        );
        let paint = Paint::fill(flui_types::Color::WHITE)
            .with_shader(shader)
            .with_blend_mode(BlendMode::Clear);

        // Must not panic. The result is SrcOver (gradient), so alpha stays 255.
        // A working Clear would produce (0,0,0,0) — that must NOT happen here.
        let px_val = render_and_read_center(&device, &queue, 64, wgpu::Color::WHITE, |painter| {
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0)),
                &paint,
            );
        });

        // The alpha channel must be non-zero: if Clear had been honored the
        // output would be (0,0,0,0). SrcOver of any opaque gradient keeps a=255.
        assert!(
            px_val[3] > 0,
            "gradient + Clear must not erase the target to alpha=0 in Phase A \
             (gradient blend_mode is SrcOver, not Clear). got {px_val:?}"
        );
    }

    /// Draw-order correctness for non-SrcOver blend modes (P1 regression).
    ///
    /// `flush_segment` renders batches in FIXED order (instanced → tessellated),
    /// not recording order.  Without the segment-split fix, a non-SrcOver
    /// tessellated shape batched into the same segment as LATER instanced draws
    /// would execute AFTER those draws, erasing content that was laid down after it.
    ///
    /// Scenario (64×64 frame, cleared to black):
    ///   1. Draw opaque RED instanced rect over the entire frame (SrcOver) → S0.
    ///   2. Draw a Clear `draw_path` over the entire frame (non-SrcOver →
    ///      tessellated → segment-split fix seals S0 and opens S1).
    ///   3. Draw opaque GREEN instanced rect over the entire frame (SrcOver → S1).
    ///
    /// Correct draw order: RED, then Clear (transparent), then GREEN → center GREEN.
    ///
    /// Without the fix (all three in segment S0):
    ///   - `flush_segment` runs instanced FIRST (RED + GREEN: last writer wins →
    ///     GREEN), then tessellated Clear → transparent. Center = transparent.
    ///   - The GREEN assertion (G > 200) fails.
    ///
    /// With the fix (segment split after Clear):
    ///   - S0 flush: RED (instanced), Clear (tess) → transparent.
    ///   - S1 flush: GREEN (instanced) → GREEN on transparent → GREEN visible.
    ///   - Center reads GREEN ~(0,255,0). Assertion passes.
    ///
    /// RED-BEFORE (no fix): center alpha = 0, G = 0 (cleared by out-of-order Clear).
    /// GREEN-AFTER (fix):   center pixel ~(0,255,0,255).
    #[test]
    fn blend_clear_respects_draw_order() {
        use flui_painting::Paint;

        const SIZE: u32 = 64;
        let (device, queue) = test_device_and_queue();

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            let red = flui_types::Color::rgb(255, 0, 0);
            let green = flui_types::Color::rgb(0, 255, 0);

            // Step 1: fill frame RED via instanced path (SrcOver → S0 rect_batch).
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(red),
            );

            // Step 2: Clear entire frame via tessellated path (BlendMode::Clear).
            // The segment-split fix appends Clear to S0's tess_batches then seals
            // S0 → draw_order, opening fresh S1.
            // Without fix: Clear goes into S0 alongside the RED and GREEN instanced
            // draws, and flush_segment would run instanced FIRST (RED+GREEN both
            // rendered, last-writer GREEN wins), then tessellated Clear → erases
            // everything; center is transparent.
            painter.draw_path(
                &full_frame_fill_path(SIZE as f32),
                &Paint::fill(red).with_blend_mode(BlendMode::Clear),
            );

            // Step 3: fill frame GREEN via instanced path (SrcOver → S1 rect_batch).
            // With the fix: S1 flushes entirely AFTER S0 (which ended with Clear),
            // so GREEN is drawn on top of transparent → GREEN visible.
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(green),
            );
        });

        // Center must be GREEN: drawn AFTER the Clear.
        // Without fix: center is transparent (0,0,0,0) — Clear ran last, erased GREEN.
        // With fix:    center is ~(0,255,0,255) — Clear sealed S0, GREEN in S1 is intact.
        let center = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 2);
        assert!(
            center[1] > 200 && center[0] < 10 && center[2] < 10,
            "center pixel = {center:?}, expected GREEN ~(0,255,0). \
             A transparent or red result means the out-of-order Clear erased \
             the GREEN that was drawn AFTER it (segment-split fix missing)."
        );
        assert_eq!(
            center[3], 255,
            "center alpha = {}, expected 255 (GREEN fully covers the cleared frame). \
             alpha=0 means the Clear ran AFTER GREEN and erased it. pixel = {center:?}",
            center[3]
        );
    }

    // ===== T9a Characterisation readback safety-net =====
    //
    // Locks down the relocated batcher slow path (non-axis-aligned rect with a
    // non-SrcOver blend mode).  A regression in the moved branch would pass the
    // instanced-path tests but silently break the tessellated-path segment seal.

    /// T9a-1: rotated rect with `BlendMode::Clear` seals its segment so a later
    /// `SrcOver` instanced rect composites correctly.
    ///
    /// # What this covers
    ///
    /// After T9a extracted `DrawBatcher::rect`, the *slow path* inside that
    /// method — reached when the transform is NOT axis-aligned OR the blend mode
    /// is not `SrcOver` — calls `add_tessellated_with_key`, which in turn calls
    /// `finish_current_segment` for any non-`SrcOver` blend.  This is the moved
    /// code that had no GPU-readback coverage before this test.
    ///
    /// # Draw sequence
    ///
    /// 1. Fill frame RED via the fast instanced path (SrcOver, axis-aligned) → S0.
    /// 2. `save` + `rotate(45°)` so `is_axis_aligned()` returns `false`.
    ///    Draw an overlapping rect with `BlendMode::Clear` via the batcher slow
    ///    path.  `add_tessellated_with_key` appends it to S0's tess_batches then
    ///    seals S0 (non-SrcOver contract), opening S1.  `restore` returns to
    ///    identity.
    /// 3. Fill frame GREEN via the fast instanced path (SrcOver, axis-aligned) → S1.
    ///
    /// # Correct outcome
    ///
    /// - S0 flushes: RED instanced, then Clear tessellated → frame transparent at
    ///   the rotated quad region.
    /// - S1 flushes: GREEN instanced on top of (possibly transparent) background →
    ///   center pixel is GREEN.
    ///
    /// # Failure modes caught
    ///
    /// - **Slow-path seal missing** (seal removed from `add_tessellated_with_key`):
    ///   Clear and GREEN end up in the same segment; `flush_segment` runs instanced
    ///   first (RED+GREEN → GREEN wins), then Clear erases everything → center
    ///   transparent.  `center[1] > 200` fails.
    /// - **Slow path not reached** (rotation guard removed, fast path taken):
    ///   Clear is submitted as an instanced rect, bypasses `add_tessellated_with_key`,
    ///   no segment seal → same failure mode as above.
    #[test]
    fn batcher_rotated_clear_rect_seals_segment_before_srcover() {
        use flui_painting::Paint;
        use std::f32::consts::FRAC_PI_4;

        const SIZE: u32 = 64;
        let (device, queue) = test_device_and_queue();

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            let red = flui_types::Color::rgb(255, 0, 0);
            let green = flui_types::Color::rgb(0, 255, 0);

            // Step 1: fill the frame RED via the fast instanced path.
            // axis-aligned + SrcOver → S0 rect_batch.
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(red),
            );

            // Step 2: draw a large rotated rect with BlendMode::Clear.
            // The 45° rotation makes is_axis_aligned() = false AND blend_mode !=
            // SrcOver, so DrawBatcher::rect takes the tessellated slow path.
            // add_tessellated_with_key appends the tess batch then calls
            // finish_current_segment (non-SrcOver contract), sealing S0 and
            // opening S1.
            painter.save();
            // Rotate around the frame centre so the rotated quad covers centre.
            let half = SIZE as f32 / 2.0;
            painter.translate(flui_types::Offset::new(px(half), px(half)));
            painter.rotate(FRAC_PI_4);
            painter.translate(flui_types::Offset::new(px(-half), px(-half)));
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(red).with_blend_mode(BlendMode::Clear),
            );
            painter.restore();

            // Step 3: fill the frame GREEN via the fast instanced path (SrcOver).
            // After step 2 sealed S0, this goes into S1.
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(green),
            );
        });

        // The center pixel must be GREEN: S1 (GREEN fill) flushed after S0 (which
        // ended with Clear), so GREEN is drawn on top of whatever Clear left.
        // Failure = center is transparent or red (Clear ran after GREEN in the same
        // segment, erasing it), indicating the moved slow-path seal was broken.
        let center = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 2);
        assert!(
            center[1] > 200 && center[0] < 10 && center[2] < 10,
            "center pixel = {center:?}, expected GREEN ~(0,255,0,255). \
             A transparent or red result means the rotated-Clear rect did not seal \
             its segment before the subsequent SrcOver rect (T9a slow-path seal broken)."
        );
        assert_eq!(
            center[3], 255,
            "center alpha = {}, expected 255 (GREEN fully covers the frame). \
             alpha=0 means Clear executed after GREEN (segment seal missing). \
             pixel = {center:?}",
            center[3]
        );
    }

    // ===== T6 Characterisation readback safety-net =====
    //
    // These tests lock down the SDF-clip baking (clip_rrect / clip_rsuperellipse
    // corner cutouts) and the nested save+clip+restore scissor restoration that
    // had ZERO pixel coverage before T6. They are characterisation tests: they
    // pass on the current (correct) code and will FAIL if T7 (GpuStateStack
    // extraction) breaks clip/scissor behaviour.
    //
    // Each test discriminates: the assertion would fail if the clip were a plain
    // axis-aligned square (no SDF applied) or if save/restore leaked the scissor.

    /// T6-1: `clip_rrect` SDF baking removes corner pixels.
    ///
    /// A 100×100 target is cleared to BLACK. An 80×80 RRect with 20px uniform
    /// corner radius is set as the active clip. The entire 80×80 bounding box is
    /// then filled RED.
    ///
    /// The pixel at the TOP-LEFT CORNER of the bounding box (x=0, y=0 relative to
    /// the rrect) sits inside the axis-aligned bounding box but outside the
    /// rounded corner arc. The SDF shader discards it; a plain scissor-only clip
    /// would paint it RED.
    ///
    /// Interior pixel (50, 50) is well inside every corner arc — must be RED.
    /// Corner pixel (10, 10) is inside the bbox but outside the arc — must be BLACK.
    ///
    /// Without SDF: corner pixel = RED (clip is merely a square scissor).
    /// With SDF:    corner pixel = BLACK (fragment discarded by rrect SDF).
    #[test]
    fn clip_rrect_sdf_removes_corner_pixels() {
        use flui_painting::Paint;

        const SIZE: u32 = 100;
        // The clip rect: 10..90 in both axes (80×80), 20px uniform corner radius.
        // The corner at (10,10) to (30,30) is a quadrant governed by the arc.
        // The exact centre of the corner quarter-circle is at (30, 30) screen-space
        // (i.e. rrect.left + radius, rrect.top + radius).  The pixel at (11, 11) is
        // 1 pixel past the corner — outside the arc, inside the bbox.
        const RRECT_LEFT: f32 = 10.0;
        const RRECT_TOP: f32 = 10.0;
        const RRECT_RIGHT: f32 = 90.0;
        const RRECT_BOTTOM: f32 = 90.0;
        const RADIUS: f32 = 20.0;

        let (device, queue) = test_device_and_queue();

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            let rrect = flui_types::RRect::from_rect_circular(
                Rect::from_xywh(
                    px(RRECT_LEFT),
                    px(RRECT_TOP),
                    px(RRECT_RIGHT - RRECT_LEFT),
                    px(RRECT_BOTTOM - RRECT_TOP),
                ),
                px(RADIUS),
            );
            painter.clip_rrect(rrect);

            // Fill the entire canvas RED. Only pixels passing the rrect SDF will
            // actually be painted; the rest remain BLACK (clear colour).
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(flui_types::Color::rgb(255, 0, 0)),
            );
        });

        // Interior pixel at (50, 50) — deep inside the rrect, far from all corners.
        // Must be RED; if the clip discards everything this test would also fail.
        let interior = pixel_at(&rgba, SIZE, 50, 50);
        assert!(
            interior[0] > 200,
            "interior pixel (50,50) R={}, expected ~255 (RED fill inside rrect). \
             clip_rrect is discarding too much — possible SDF radius overclaim. \
             pixel={interior:?}",
            interior[0]
        );

        // Corner pixel: (11, 11) is 1px inside the axis-aligned bbox but inside
        // the corner arc's quadrant. At radius=20 the SDF for a point at distance
        // (~13 px) from the corner centre (30,30) is positive (outside the arc).
        //
        // Discriminator: a plain scissor-only implementation would paint this RED
        // (it is inside the 10..90 scissor).  The SDF shader must discard the
        // draw, leaving the opaque BLACK clear colour.
        //
        // The clear colour is wgpu::Color::BLACK = (0.0, 0.0, 0.0, 1.0), so the
        // pixel is [0, 0, 0, 255]. We check R < 30 (not alpha) to discriminate:
        //   - SDF applied correctly → R ≈ 0 (BLACK, not painted) ✓
        //   - SDF missing (scissor-only) → R ≈ 255 (RED fill bleeds into corner)
        let corner = pixel_at(&rgba, SIZE, 11, 11);
        assert!(
            corner[0] < 30,
            "corner pixel (11,11) R={}, expected ~0 (BLACK — outside rounded corner arc). \
             A non-zero R means clip_rrect is acting as a plain scissor (no SDF applied). \
             pixel={corner:?}",
            corner[0]
        );

        // Sanity: a pixel strictly outside the bounding box must be BLACK.
        let outside_bbox = pixel_at(&rgba, SIZE, 5, 5);
        assert!(
            outside_bbox[0] < 10,
            "pixel (5,5) is outside the rrect bbox, R={}, expected 0. \
             pixel={outside_bbox:?}",
            outside_bbox[0]
        );
    }

    /// T6-2: `clip_rsuperellipse` SDF baking removes corner pixels.
    ///
    /// Identical geometry to T6-1 but uses `clip_rsuperellipse` (iOS squircle)
    /// with the same 20px radii. The superellipse SDF is strictly tighter in the
    /// corner region than a standard rrect arc, so the corner pixel at (11,11)
    /// must also be discarded (the squircle corner extends further inward).
    ///
    /// Without SDF: corner = RED (scissor only).
    /// With SDF:    corner = BLACK (superellipse SDF discards the corner).
    #[test]
    fn clip_rsuperellipse_sdf_removes_corner_pixels() {
        use flui_painting::Paint;
        use flui_types::geometry::RSuperellipse;

        const SIZE: u32 = 100;
        const RRECT_LEFT: f32 = 10.0;
        const RRECT_TOP: f32 = 10.0;
        const RRECT_RIGHT: f32 = 90.0;
        const RRECT_BOTTOM: f32 = 90.0;
        const RADIUS: f32 = 20.0;

        let (device, queue) = test_device_and_queue();

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            let rse = RSuperellipse::from_ltrb_xy(
                px(RRECT_LEFT),
                px(RRECT_TOP),
                px(RRECT_RIGHT),
                px(RRECT_BOTTOM),
                px(RADIUS),
                px(RADIUS),
            );
            painter.clip_rsuperellipse(rse);

            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(flui_types::Color::rgb(0, 0, 255)),
            );
        });

        // Interior pixel must be BLUE (painted inside the superellipse).
        let interior = pixel_at(&rgba, SIZE, 50, 50);
        assert!(
            interior[2] > 200,
            "interior pixel (50,50) B={}, expected ~255 (BLUE fill inside rsuperellipse). \
             Superellipse SDF is discarding interior pixels. pixel={interior:?}",
            interior[2]
        );

        // Corner pixel (11,11) must remain BLACK — squircle corner discards it.
        // A scissor-only path would paint it BLUE (non-zero B channel).
        //
        // Clear colour is wgpu::Color::BLACK = [0, 0, 0, 255] so we check B < 30:
        //   - SDF applied → B ≈ 0 (BLACK, fill discarded) ✓
        //   - SDF missing → B ≈ 255 (BLUE bleeds into corner)
        let corner = pixel_at(&rgba, SIZE, 11, 11);
        assert!(
            corner[2] < 30,
            "corner pixel (11,11) B={}, expected ~0 (BLACK — outside squircle arc). \
             A non-zero B means clip_rsuperellipse is acting as a plain scissor. \
             pixel={corner:?}",
            corner[2]
        );
    }

    /// T6-3: nested `save + clip_rect + paint + restore` correctly removes the scissor.
    ///
    /// This test proves the save/restore scissor asymmetry is DESIGN (correct), not
    /// a bug. See the `save()` comment block for the invariant proof.
    ///
    /// Layout (100×100 target, cleared to BLACK):
    ///   - Paint the full canvas GREEN.
    ///   - save() → clip_rect to LEFT half (x 0..50) → paint RIGHT half RED.
    ///   - restore() → the scissor must be gone.
    ///   - Paint a narrow BLUE column at x=60..62 (right of the clip boundary).
    ///
    /// Assertions:
    ///   A. LEFT interior (x=25, y=50):  GREEN (painted before clip, not touched after).
    ///   B. RIGHT interior before BLUE (x=55, y=50): GREEN (RED was clipped away).
    ///   C. BLUE column (x=61, y=50): BLUE — proves restore removed the scissor so
    ///      the post-restore paint reaches the right half.
    ///
    /// Without correct restore: BLUE column = BLACK (scissor still active after restore).
    /// With correct restore:    BLUE column = BLUE.
    #[test]
    fn nested_save_clip_restore_removes_scissor() {
        use flui_painting::Paint;

        const SIZE: u32 = 100;
        let (device, queue) = test_device_and_queue();

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            let green = flui_types::Color::rgb(0, 255, 0);
            let red = flui_types::Color::rgb(255, 0, 0);
            let blue = flui_types::Color::rgb(0, 0, 255);

            // Step 1: paint the full canvas GREEN (baseline for both halves).
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &Paint::fill(green),
            );

            // Step 2: save, clip to left half, try to paint the right half RED.
            // The RED paint must be clipped (scissor blocks x≥50).
            painter.save();
            painter.clip_rect(Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(SIZE as f32)));
            painter.rect(
                Rect::from_xywh(px(50.0), px(0.0), px(50.0), px(SIZE as f32)),
                &Paint::fill(red),
            );
            painter.restore();

            // Step 3: after restore the scissor must be cleared. Paint a BLUE column
            // at x=60..62 which is in the right half (would be clipped if scissor leaked).
            painter.rect(
                Rect::from_xywh(px(60.0), px(0.0), px(2.0), px(SIZE as f32)),
                &Paint::fill(blue),
            );
        });

        // A. Left half (x=25, y=50): must be GREEN (painted in step 1, unaffected).
        let left = pixel_at(&rgba, SIZE, 25, 50);
        assert!(
            left[1] > 200 && left[0] < 10 && left[2] < 10,
            "left interior (25,50) expected GREEN, got {left:?}. \
             Left half should be the original GREEN fill."
        );

        // B. Right half between clip boundary and blue column (x=55, y=50):
        // must be GREEN. RED was clipped by the scissor, so GREEN underneath survives.
        let right_no_blue = pixel_at(&rgba, SIZE, 55, 50);
        assert!(
            right_no_blue[1] > 200 && right_no_blue[0] < 30 && right_no_blue[2] < 30,
            "right interior (55,50) expected GREEN (RED clipped away), got {right_no_blue:?}. \
             If RED appears the scissor did not clip during save+clip+restore."
        );

        // C. BLUE column (x=61, y=50): must be BLUE.
        // Discriminator: if restore() leaked the scissor, x=61 is still clipped and
        // stays GREEN; only a correct restore allows the post-restore blue paint through.
        let blue_col = pixel_at(&rgba, SIZE, 61, 50);
        assert!(
            blue_col[2] > 200 && blue_col[0] < 10 && blue_col[1] < 30,
            "blue column (61,50) expected BLUE, got {blue_col:?}. \
             A non-blue result means restore() left the scissor active (leaked scissor), \
             blocking the post-restore BLUE paint from reaching the right half."
        );
    }

    /// Verify that `DrawBatcher::draw_shadow` (T9b) keeps its `save`/`restore`
    /// calls balanced so the CTM is unchanged after the call returns.
    ///
    /// # Discriminating strategy
    ///
    /// Draw a shadow, then paint a small 4×4 RED square at a known origin
    /// (`x=10, y=10`).  Read back the pixel at the square's center.
    ///
    /// If `draw_shadow` leaks a `translate` (i.e., restores one fewer time
    /// than it saves), the CTM after the call carries a residual offset, and
    /// the red square would be painted at a shifted position — so the expected
    /// pixel reads as the background color instead of red.
    ///
    /// This is a real discriminating assertion, not a tautology: the test
    /// would fail (background pixel ≠ red) on a build where the batcher's
    /// `state.restore()` call is removed.
    #[cfg(feature = "enable-wgpu-tests")]
    #[test]
    fn draw_shadow_save_restore_is_balanced() {
        use flui_painting::Paint;
        use flui_types::Color;

        const SIZE: u32 = 64;
        let (device, queue) = test_device_and_queue();

        // Draw a shadow then a small red square at a known absolute origin.
        let rgba = render_to_rgba(
            &device,
            &queue,
            SIZE,
            // Black background.
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            |painter| {
                // Construct a simple closed triangle path.  The exact shape does not
                // matter for this test — we just need an elevation large enough to
                // produce at least one shadow layer (elevation=8 → blur_radius=8,
                // num_layers=4).
                let mut path = flui_types::painting::path::Path::new();
                path.move_to(Point::new(px(30.0), px(5.0)));
                path.line_to(Point::new(px(55.0), px(50.0)));
                path.line_to(Point::new(px(5.0), px(50.0)));
                path.close();

                // draw_shadow mutates state (save/translate/restore per layer).
                // After it returns the CTM must be back at the identity translation.
                painter.draw_shadow(&path, Color::rgba(0, 0, 0, 180), 8.0);

                // Paint a 4×4 red square at (10, 10) in absolute canvas space.
                // If CTM has leaked a translation this lands somewhere else and the
                // pixel at (12, 12) reads black (background), not red.
                painter.rect(
                    Rect::from_xywh(px(10.0), px(10.0), px(4.0), px(4.0)),
                    &Paint::fill(Color::rgba(255, 0, 0, 255)),
                );
            },
        );

        // The center of the red square — pixel (12, 12) — must be red.
        // A leaked translate shifts the square away: the pixel reads background (black).
        let center = pixel_at(&rgba, SIZE, 12, 12);
        assert!(
            center[0] > 200 && center[1] < 30 && center[2] < 30,
            "pixel (12,12) expected RED (square at origin 10,10), got {center:?}. \
             A non-red result means draw_shadow leaked a translate (save/restore imbalance), \
             shifting the post-shadow rect away from its intended origin."
        );
    }

    /// T9c characterisation: `rect` with a `Fill` + `LinearGradient` shader routes
    /// through `DrawBatcher::dispatch_shader_rect` → `DrawBatcher::gradient_rect`.
    ///
    /// # Discriminating strategy
    ///
    /// A 64×64 frame is cleared to TRANSPARENT (alpha=0).  A horizontal red→blue
    /// linear gradient is painted over the full width via `painter.rect(…, &paint)`
    /// where `paint` carries a `LinearGradient` shader.
    ///
    /// We sample:
    ///   - The LEFT column  (x=2,  y=32) — must be predominantly RED   (R > B).
    ///   - The RIGHT column (x=61, y=32) — must be predominantly BLUE  (B > R).
    ///   - Both pixels must be opaque (alpha=255) — the gradient rendered.
    ///
    /// A regression where `dispatch_shader_rect` is not called would fall through
    /// to the solid-fill path, producing a uniform color at both columns (same R
    /// and B channels), so the `R > B` and `B > R` assertions would both fail.
    #[cfg(feature = "enable-wgpu-tests")]
    #[test]
    fn linear_gradient_rect_dispatches_through_thin_shim() {
        use flui_painting::{Paint, Shader};
        use flui_types::{Color, painting::TileMode};

        const SIZE: u32 = 64;
        let (device, queue) = test_device_and_queue();

        // Horizontal red→blue gradient spanning the full frame width.
        let gradient_shader = Shader::linear_gradient(
            flui_types::Point::new(px(0.0), px(0.0)).into(),
            flui_types::Point::new(px(SIZE as f32), px(0.0)).into(),
            vec![Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)],
            None,
            TileMode::Clamp,
        );
        let gradient_paint = Paint::fill(Color::WHITE).with_shader(gradient_shader);

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::TRANSPARENT, |painter| {
            painter.rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                &gradient_paint,
            );
        });

        // Left column — must be predominantly RED.
        let left_pixel = pixel_at(&rgba, SIZE, 2, 32);
        // Right column — must be predominantly BLUE.
        let right_pixel = pixel_at(&rgba, SIZE, 61, 32);

        assert!(
            left_pixel[3] == 255,
            "left pixel alpha={}, expected 255 (gradient rendered opaque). \
             Alpha=0 means the Fill+shader path was skipped entirely. pixel={left_pixel:?}",
            left_pixel[3]
        );
        assert!(
            right_pixel[3] == 255,
            "right pixel alpha={}, expected 255 (gradient rendered opaque). pixel={right_pixel:?}",
            right_pixel[3]
        );
        assert!(
            left_pixel[0] > left_pixel[2],
            "left pixel R={} B={}: expected R > B (red end of gradient). \
             Equal R and B means the shader dispatch was skipped and solid fill ran instead. \
             pixel={left_pixel:?}",
            left_pixel[0],
            left_pixel[2]
        );
        assert!(
            right_pixel[2] > right_pixel[0],
            "right pixel R={} B={}: expected B > R (blue end of gradient). \
             Equal R and B means the shader dispatch was skipped and solid fill ran instead. \
             pixel={right_pixel:?}",
            right_pixel[0],
            right_pixel[2]
        );
    }

    /// T9d characterisation: `draw_path` cache-hit branch uses the *current*
    /// `paint.color`, not the color from the first (cache-miss) tessellation.
    ///
    /// # Discriminating strategy
    ///
    /// Both draws happen in the **same painter frame** so the second call hits
    /// the per-frame `path_cache` entry written by the first call:
    ///
    ///   1. Draw a filled triangle that covers the top-left quadrant in RED.
    ///   2. Draw the **identical path** in BLUE — the cache returns the
    ///      untransformed positions; the cache-hit branch must reconstruct
    ///      `Vertex`s with the *current* blue paint color before submitting.
    ///
    /// Sampling the center of the second triangle must yield BLUE (not RED).
    /// If the cache-hit branch silently reuses the first tessellation's
    /// `Vertex::color` bytes, the pixel stays red and the assertion fails.
    ///
    /// The triangle is translated for the second draw so it does not overlap
    /// with the first, making the test pixel unambiguous.
    #[cfg(feature = "enable-wgpu-tests")]
    #[test]
    fn draw_path_cache_hit_uses_current_paint_color() {
        use flui_types::painting::path::Path;

        const SIZE: u32 = 64;
        let (device, queue) = test_device_and_queue();

        // A filled right-triangle occupying the top-left 32×32 area.
        let triangle_path = {
            let mut p = Path::new();
            p.move_to(flui_types::Point::new(px(0.0), px(0.0)));
            p.line_to(flui_types::Point::new(px(32.0), px(0.0)));
            p.line_to(flui_types::Point::new(px(0.0), px(32.0)));
            p.close();
            p
        };

        let red_paint = flui_painting::Paint::fill(flui_types::Color::rgb(255, 0, 0));
        let blue_paint = flui_painting::Paint::fill(flui_types::Color::rgb(0, 0, 255));

        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::TRANSPARENT, |painter| {
            // First draw: cache MISS — tessellates and caches; renders at origin.
            painter.draw_path(&triangle_path, &red_paint);

            // Translate right so the second triangle doesn't overlap the first.
            painter.translate(flui_types::Offset::new(px(32.0), px(0.0)));

            // Second draw: cache HIT — must use blue_paint.color, not cached red.
            painter.draw_path(&triangle_path, &blue_paint);
        });

        // Sample a pixel well inside the second (blue) triangle's area.
        // After the translate(32, 0), the second triangle spans x=[32..64], y=[0..32].
        // x=40, y=8 is safely inside the filled region.
        let second_triangle_pixel = pixel_at(&rgba, SIZE, 40, 8);

        assert_eq!(
            second_triangle_pixel[3], 255,
            "second triangle pixel alpha={}, expected 255 (path rendered opaque). \
             Alpha=0 means draw_path did not submit geometry. pixel={second_triangle_pixel:?}",
            second_triangle_pixel[3]
        );
        assert!(
            second_triangle_pixel[2] > 200,
            "second triangle pixel B={}: expected B > 200 (blue fill). \
             Low blue means the cache-hit branch reused the first draw's red color. \
             pixel={second_triangle_pixel:?}",
            second_triangle_pixel[2]
        );
        assert!(
            second_triangle_pixel[0] < 10,
            "second triangle pixel R={}: expected R < 10 (no red leakage from cache). \
             High red means the cache-hit branch did not apply current paint.color. \
             pixel={second_triangle_pixel:?}",
            second_triangle_pixel[0]
        );
    }

    /// `draw_image_filtered` with `ColorFilter::Mode` must tint an **opaque**
    /// image — the tint has to composite *over* the image, not under it.
    ///
    /// This is the regression guard for the pre-T9 bug: the old `Mode` branch
    /// drew the image into `cached_images` and a half-alpha tint rect into
    /// `rect_batch`, but `flush_segment` flushes `rect_batch` *before*
    /// `cached_images`, so an opaque image fully occluded the tint and the color
    /// filter had no visible effect. The fix bakes `blend_mode(src = color,
    /// dst = pixel)` into the image pixels (per `ui.ColorFilter.mode`), so the
    /// tint is in the same flush bucket as the image.
    ///
    /// ## Blend math (opaque GREEN image, BLACK background)
    ///
    /// Filter = `mode(rgba(255, 0, 0, 128), SrcOver)` over `rgba(0, 200, 0, 255)`:
    /// `srcOver(src = red·0.5, dst = green)` ≈ `rgba(128, 100, 0, 255)`. Drawn
    /// opaque on black it reads back ≈ `(128, 100, 0)`.
    ///
    /// | Scenario                                   | R    | G    |
    /// |--------------------------------------------|------|------|
    /// | Fixed (tint baked over image)              | ~128 | ~100 |
    /// | Old bug (tint occluded under opaque image) | ~0   | ~200 |
    ///
    /// So `R` is raised well above 0 (tint applied) while `G` stays mid-range
    /// (the image content survives). The old code fails the `R` assertion.
    #[test]
    fn draw_image_filtered_mode_tints_opaque_image() {
        use flui_painting::display_list::ColorFilter;
        use flui_types::{painting::Image, styling::Color};

        const SIZE: u32 = 16;
        let (device, queue) = test_device_and_queue();

        // Opaque GREEN source image — an opaque image is exactly what the old
        // overlay path failed to tint (the tint rect was drawn underneath it).
        let green_pixels: Vec<u8> = (0..SIZE * SIZE).flat_map(|_| [0u8, 200, 0, 255]).collect();
        let green_image = Image::from_rgba8(SIZE, SIZE, green_pixels);

        // Half-alpha RED tint via SrcOver — mixes with the image so both the
        // tint (raised R) and the surviving image content (mid G) are visible.
        let red_filter = ColorFilter::mode(
            Color::rgba(255, 0, 0, 128),
            flui_painting::BlendMode::SrcOver,
        );

        let px_val = render_and_read_center(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            painter.draw_image_filtered(
                &green_image,
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                red_filter,
                flui_painting::BlendMode::SrcOver,
            );
        });

        let (r, g, b) = (
            i32::from(px_val[0]),
            i32::from(px_val[1]),
            i32::from(px_val[2]),
        );

        // R raised to ~128: the tint composited over the image. R ≈ 0 means the
        // tint was occluded under the opaque image (the original bug).
        assert!(
            (90..=165).contains(&r),
            "R={r}: expected R in [90, 165] (half-alpha RED over GREEN ≈ 128). \
             R ≈ 0 means the tint was drawn under the opaque image and occluded \
             (the pre-fix bug). pixel={px_val:?}"
        );

        // G mid-range ~100: the underlying image content survives the tint.
        assert!(
            (55..=135).contains(&g),
            "G={g}: expected G in [55, 135] (GREEN image showing through the \
             half-alpha tint). G ≈ 200 means no tint was applied; G ≈ 0 means the \
             image content was lost. pixel={px_val:?}"
        );

        // B near zero: neither the RED tint nor the GREEN image contributes blue.
        assert!(
            b <= 35,
            "B={b}: expected B ≈ 0 (no blue contributor). pixel={px_val:?}"
        );
    }

    /// `ColorFilter::Mode` must honor its `blend_mode`, not just composite a
    /// fixed SrcOver tint. The old branch ignored `blend_mode` entirely.
    ///
    /// `mode(RED, Modulate)` over an opaque WHITE image multiplies per channel:
    /// `red · white = red`. So a white image becomes pure red — green and blue
    /// are driven to zero. The old code (which ignored the mode and drew an
    /// occluded half-alpha overlay) leaves the white image untouched, so its
    /// `G ≈ 255` fails the green assertion below.
    #[test]
    fn draw_image_filtered_mode_honors_blend_mode() {
        use flui_painting::display_list::ColorFilter;
        use flui_types::{painting::Image, styling::Color};

        const SIZE: u32 = 16;
        let (device, queue) = test_device_and_queue();

        // Opaque WHITE source image.
        let white_pixels: Vec<u8> = (0..SIZE * SIZE)
            .flat_map(|_| [255u8, 255, 255, 255])
            .collect();
        let white_image = Image::from_rgba8(SIZE, SIZE, white_pixels);

        let modulate_red = ColorFilter::mode(
            Color::rgba(255, 0, 0, 255),
            flui_painting::BlendMode::Modulate,
        );

        let px_val = render_and_read_center(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            painter.draw_image_filtered(
                &white_image,
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                modulate_red,
                flui_painting::BlendMode::SrcOver,
            );
        });

        let (r, g, b) = (
            i32::from(px_val[0]),
            i32::from(px_val[1]),
            i32::from(px_val[2]),
        );

        // Modulate(RED, WHITE) = RED.
        assert!(
            r >= 200,
            "R={r}: expected R ≈ 255 (red·white = red). pixel={px_val:?}"
        );
        assert!(
            g <= 40,
            "G={g}: expected G ≈ 0 (modulate kills the white image's green). \
             G ≈ 255 means blend_mode was ignored and the white image was left \
             untinted. pixel={px_val:?}"
        );
        assert!(
            b <= 40,
            "B={b}: expected B ≈ 0 (modulate kills the white image's blue). \
             pixel={px_val:?}"
        );
    }

    /// `ColorFilter::Matrix` recolors the image on the CPU then routes through
    /// `draw_image`. A red↔blue channel-swap matrix turns an opaque RED image
    /// blue — covering the CPU-recolor delegation path the `Mode` branch now
    /// shares.
    #[test]
    fn draw_image_filtered_matrix_swaps_channels() {
        use flui_painting::display_list::ColorFilter;
        use flui_types::painting::Image;

        const SIZE: u32 = 16;
        let (device, queue) = test_device_and_queue();

        // Opaque RED source image.
        let red_pixels: Vec<u8> = (0..SIZE * SIZE).flat_map(|_| [255u8, 0, 0, 255]).collect();
        let red_image = Image::from_rgba8(SIZE, SIZE, red_pixels);

        // 5×4 row-major matrix swapping R and B (R'=B, G'=G, B'=R, A'=A).
        let swap_rb = ColorFilter::matrix([
            0.0, 0.0, 1.0, 0.0, 0.0, // R' = B
            0.0, 1.0, 0.0, 0.0, 0.0, // G' = G
            1.0, 0.0, 0.0, 0.0, 0.0, // B' = R
            0.0, 0.0, 0.0, 1.0, 0.0, // A' = A
        ]);

        let px_val = render_and_read_center(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            painter.draw_image_filtered(
                &red_image,
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
                swap_rb,
                flui_painting::BlendMode::SrcOver,
            );
        });

        let (r, g, b) = (
            i32::from(px_val[0]),
            i32::from(px_val[1]),
            i32::from(px_val[2]),
        );

        assert!(
            b >= 200,
            "B={b}: expected B ≈ 255 (R swapped into B). pixel={px_val:?}"
        );
        assert!(
            r <= 40,
            "R={r}: expected R ≈ 0 (B=0 swapped into R). pixel={px_val:?}"
        );
        assert!(
            g <= 40,
            "G={g}: expected G ≈ 0 (unchanged). pixel={px_val:?}"
        );
    }

    /// Two filtered draws of the **same source image** with **different** color
    /// filters in one frame must not alias in the texture cache.
    ///
    /// Each filter produces a short-lived temporary `Image`; filtered draws key
    /// the cache on a hash of the produced bytes, not the temporary's pointer.
    /// If they keyed on the pointer (as a plain `draw_image` does), the second
    /// temporary — frequently reallocated at the just-freed address of the first
    /// — would collide on key and the cache would return the first filter's
    /// texture for the second draw (it hits on key alone, never re-comparing
    /// bytes). Here a white source is modulated RED in the top half and BLUE in
    /// the bottom half; a collision would paint the bottom half red.
    #[test]
    fn draw_image_filtered_distinct_filters_do_not_alias() {
        use flui_painting::display_list::ColorFilter;
        use flui_types::{painting::Image, styling::Color};

        const SIZE: u32 = 16;
        let (device, queue) = test_device_and_queue();

        let white_pixels: Vec<u8> = (0..SIZE * SIZE)
            .flat_map(|_| [255u8, 255, 255, 255])
            .collect();
        let white_image = Image::from_rgba8(SIZE, SIZE, white_pixels);

        let modulate_red = ColorFilter::mode(
            Color::rgba(255, 0, 0, 255),
            flui_painting::BlendMode::Modulate,
        );
        let modulate_blue = ColorFilter::mode(
            Color::rgba(0, 0, 255, 255),
            flui_painting::BlendMode::Modulate,
        );

        let half = SIZE as f32 / 2.0;
        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            // Two separate filtered draws → two short-lived temporaries, the
            // second likely reusing the first's freed allocation address.
            painter.draw_image_filtered(
                &white_image,
                Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(half)),
                modulate_red,
                flui_painting::BlendMode::SrcOver,
            );
            painter.draw_image_filtered(
                &white_image,
                Rect::from_xywh(px(0.0), px(half), px(SIZE as f32), px(half)),
                modulate_blue,
                flui_painting::BlendMode::SrcOver,
            );
        });

        let top = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 4);
        let bottom = pixel_at(&rgba, SIZE, SIZE / 2, SIZE * 3 / 4);

        // Top half: modulate RED → red.
        assert!(
            top[0] >= 200 && top[2] <= 40,
            "top={top:?}: expected red (R≈255, B≈0) from modulate-RED"
        );
        // Bottom half: modulate BLUE → blue. A cache collision with the first
        // (red) temporary would paint this red instead.
        assert!(
            bottom[2] >= 200 && bottom[0] <= 40,
            "bottom={bottom:?}: expected blue (B≈255, R≈0) from modulate-BLUE. \
             Red here means the second filtered draw aliased the first in the \
             texture cache (pointer-identity key on a freed temporary)."
        );
    }

    /// T10a: external-texture resolution happens at replay time, not record time.
    ///
    /// Register a solid-RED texture under ID 77, record `draw_texture`, then
    /// call `update()` on the same ID replacing it with a solid-GREEN texture —
    /// all BEFORE `render()`.  Assert the readback shows GREEN, not RED.
    ///
    /// This test FAILS before T10a (record-time resolution → RED survives the
    /// update) and PASSES after (replay-time resolution → GREEN wins).
    ///
    /// Flutter reference: `Texture` widget and the engine's `ExternalTextureRegistry`
    /// feed the platform's most-recently-uploaded frame to the rasterizer at
    /// present time, not at the `drawImage` command time — any frame produced
    /// between record and rasterize is presented (latest-frame semantics).
    /// This test encodes that contract for the FLUI IR.
    #[test]
    fn external_texture_resolves_at_replay_not_record_time() {
        const SIZE: u32 = 16;
        let (device, queue) = test_device_and_queue();

        // Helper: create a solid-color 1-channel RGBA Unorm texture.
        let make_solid_texture =
            |device: &wgpu::Device, queue: &wgpu::Queue, r: u8, g: u8, b: u8| -> wgpu::Texture {
                let data: Vec<u8> = (0..SIZE * SIZE).flat_map(|_| [r, g, b, 0xFFu8]).collect();
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("solid color test texture"),
                    size: wgpu::Extent3d {
                        width: SIZE,
                        height: SIZE,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * SIZE),
                        rows_per_image: Some(SIZE),
                    },
                    wgpu::Extent3d {
                        width: SIZE,
                        height: SIZE,
                        depth_or_array_layers: 1,
                    },
                );
                tex
            };

        let tex_id = flui_types::painting::TextureId::new(77);

        // Build a painter with a full-size UNorm render target.
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("replay-timing readback target"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: READBACK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            READBACK_FORMAT,
            (SIZE, SIZE),
        );

        // Step 1: register a solid-RED texture.
        painter.external_texture_registry_mut().register(
            tex_id,
            make_solid_texture(&device, &queue, 0xFF, 0x00, 0x00),
            SIZE,
            SIZE,
            true,  // dynamic
            false, // nearest sampler
        );

        // Step 2: record draw_texture.  Under record-time resolution this would
        // capture RED's TextureView into the IR.  Under replay-time resolution
        // only the TextureId is stored.
        painter.draw_texture(
            tex_id,
            Rect::from_xywh(px(0.0), px(0.0), px(SIZE as f32), px(SIZE as f32)),
            None,
            flui_types::painting::FilterQuality::None,
            1.0,
        );

        // Step 3: update the same ID to a solid-GREEN texture BEFORE render().
        // Under record-time resolution this update would be invisible (the old
        // RED view was already cloned into the IR).  Under replay-time resolution
        // the registry lookup at flush time picks up GREEN.
        let updated = painter.external_texture_registry_mut().update(
            tex_id,
            make_solid_texture(&device, &queue, 0x00, 0xFF, 0x00),
        );
        assert!(updated, "update must return true when the ID is registered");

        // Step 4: render.
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("replay-timing clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
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
        painter
            .render_to_view(&target_view, &mut encoder)
            .expect("painter.render must succeed");

        // Readback.
        let bytes_per_pixel = 4u32;
        let unpadded = SIZE * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded.div_ceil(align) * align;
        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("replay-timing readback buffer"),
            size: u64::from(padded_bytes_per_row) * u64::from(SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = readback_buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| {
            r.expect("buffer mapping must succeed");
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete the readback copy");

        let data = slice.get_mapped_range();
        // Sample center pixel.
        let center = (SIZE / 2) as usize;
        let stride = padded_bytes_per_row as usize;
        let off = center * stride + center * 4;
        let pixel = [data[off], data[off + 1], data[off + 2], data[off + 3]];
        drop(data);
        readback_buf.unmap();

        let (r, g, b) = (
            i32::from(pixel[0]),
            i32::from(pixel[1]),
            i32::from(pixel[2]),
        );
        // Must be GREEN (updated texture), NOT RED (originally recorded texture).
        // Failure here means resolution happened at record time (T10a regressed).
        assert!(
            g > 200 && r < 20,
            "Center pixel = {pixel:?}: expected GREEN (G>200, R<20) to prove \
             replay-time resolution (T10a). \
             RED (R>200, G<20) means the TextureView was captured at record time \
             and the update() was invisible — regression in T10a record/replay seam."
        );
        assert!(
            b < 20,
            "B = {b}, expected near-zero (solid GREEN texture, no blue). pixel = {pixel:?}"
        );
    }

    /// T10a edge: unregistered external texture at replay time is warn-skipped, not rendered.
    ///
    /// Sequence:
    /// 1. Register a solid-GREEN texture under ID 88.
    /// 2. Record `draw_texture` filling the top-left quadrant (GREEN expected there).
    /// 3. Record a solid-RED `rect` in the bottom-right quadrant as a "frame is alive" marker.
    /// 4. Unregister ID 88 via `unregister()` — before `render()`.
    /// 5. `render()`.
    ///
    /// Assert (a): top-left quadrant center is NOT GREEN — the missing texture was
    /// skipped, leaving the black clear color.
    /// Assert (b): bottom-right quadrant center IS RED — the skip did not abort the
    /// frame; subsequent draws still executed.
    ///
    /// This locks the Flutter-aligned "removed external texture is not composited"
    /// contract: unregistered platform textures must not leave stale pixels, and
    /// a missing texture must not prevent the rest of the frame from rendering.
    #[test]
    fn external_texture_unregistered_at_replay_is_skipped() {
        use flui_painting::Paint;

        const SIZE: u32 = 32;
        let half = SIZE / 2;

        let (device, queue) = test_device_and_queue();
        let tex_id = flui_types::painting::TextureId::new(88);

        // Build a solid-GREEN texture (SIZE×SIZE).
        let green_data: Vec<u8> = (0..SIZE * SIZE)
            .flat_map(|_| [0x00u8, 0xFFu8, 0x00u8, 0xFFu8])
            .collect();
        let green_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("unregistered-test green texture"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &green_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &green_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * SIZE),
                rows_per_image: Some(SIZE),
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );

        // Use render_to_rgba so the draw closure gets &mut WgpuPainter.
        // We register, record, unregister, then draw the RED marker — all inside
        // the closure, before render() is called by render_to_rgba.
        let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
            // Step 1: register GREEN under ID 88.
            painter.external_texture_registry_mut().register(
                tex_id, green_tex, SIZE, SIZE, false, // static
                true,  // linear filter
            );

            // Step 2: record draw_texture in the top-left quadrant.
            painter.draw_texture(
                tex_id,
                Rect::from_xywh(px(0.0), px(0.0), px(half as f32), px(half as f32)),
                None,
                flui_types::painting::FilterQuality::None,
                1.0,
            );

            // Step 3: unregister ID 88 BEFORE render() — the recorded draw must be skipped.
            let removed = painter.external_texture_registry_mut().unregister(tex_id);
            assert!(removed, "unregister must return true for a registered id");

            // Step 4: record a solid-RED rect in the bottom-right quadrant as a
            // "frame is alive" marker.  This must survive the external-texture skip.
            painter.rect(
                Rect::from_xywh(
                    px(half as f32),
                    px(half as f32),
                    px(half as f32),
                    px(half as f32),
                ),
                &Paint::fill(flui_types::Color::rgba(0xFF, 0x00, 0x00, 0xFF)),
            );
        });

        // Assert (a): top-left quadrant center was NOT rendered (GREEN texture skipped →
        // background black remains).
        let tl = pixel_at(&rgba, SIZE, half / 2, half / 2);
        let (tl_r, tl_g) = (i32::from(tl[0]), i32::from(tl[1]));
        assert!(
            tl_g < 20 && tl_r < 20,
            "Top-left center pixel = {tl:?}: expected near-BLACK (unregistered external \
             texture must be skipped, not composited). High G={tl_g} means the GREEN texture \
             was rendered despite being unregistered before render() — T10a skip-on-missing contract broken."
        );

        // Assert (b): bottom-right quadrant center IS RED — the skip did not abort the frame.
        let br = pixel_at(&rgba, SIZE, half + half / 2, half + half / 2);
        let (br_r, br_g) = (i32::from(br[0]), i32::from(br[1]));
        assert!(
            br_r > 200 && br_g < 20,
            "Bottom-right center pixel = {br:?}: expected RED (marker rect must render \
             even when a preceding external-texture draw was skipped). \
             R={br_r}, G={br_g}. If R<200, the frame was aborted instead of skip+continue."
        );
    }

    /// Regression: `flush_opacity_layer` must silently no-op when viewport is zero-sized.
    ///
    /// The UV composite inside `flush_opacity_layer` divides by `vp_w` and `vp_h` to
    /// compute texture coordinates.  Before the guard was restored, submitting an
    /// `OpacityLayer` at viewport (0, 0) would push `inf`/`NaN` into the texture
    /// instance buffer.  The guard restores the original behavior: zero GPU work,
    /// zero mutations.
    ///
    /// This test verifies that submitting a scene containing an `OpacityLayer` with a
    /// zero-size viewport neither panics nor produces a GPU error.
    #[test]
    fn opacity_layer_zero_viewport_is_noop() {
        use flui_painting::Paint;

        let (device, queue) = test_device_and_queue();
        let format = wgpu::TextureFormat::Rgba8Unorm;

        // Construct a painter with viewport (0, 0).
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (0, 0),
        );

        // Draw inside a save_layer so a PendingOpacityLayer is enqueued.
        // Use a semi-transparent paint so opacity < 1 (group-opacity layer).
        painter.save_layer(None, &Paint::fill(flui_types::Color::rgba(255, 0, 0, 128)));
        painter.rect(
            flui_types::Rect::from_xywh(px(0.0), px(0.0), px(1.0), px(1.0)),
            &Paint::fill(flui_types::Color::RED),
        );
        painter.restore();

        // A 1×1 texture to serve as the render target (smallest valid view).
        let target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("zero-vp test target"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zero-vp test encoder"),
        });

        // Must not panic. The zero-viewport guard fires before any GPU commands
        // are recorded for the OpacityLayer, so the encoder submission is also safe.
        painter
            .render_to_view(&target_view, &mut encoder)
            .expect("render on zero viewport must succeed without panic");

        queue.submit(std::iter::once(encoder.finish()));
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete after zero-viewport render");
    }
}
