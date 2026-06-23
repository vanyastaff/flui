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

use super::{
    command_ir::{DrawItem, DrawSegment, ImageFilterPass, PendingOffscreenTexture, ScissorRect},
    layer_compositor::LayerCompositor,
    pipelines::PipelineSet,
    replay::GpuReplay,
    resources::GpuResources,
    state_stack::GpuStateStack,
    text::TextRenderer,
};
use flui_types::{Rect, geometry::Pixels};

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

    /// Returns `true` if any surface-reading draw item in the current `draw_order`
    /// has bounds that STRADDLE the given `damage` rect.
    ///
    /// The items covered are:
    ///
    /// - `DrawItem::AdvancedShape` — a single tessellated shape with an advanced
    ///   (dst-read) blend mode.
    /// - `DrawItem::SsaaPath` with `blend.is_advanced()` — an SSAA path routed
    ///   through `flush_advanced_layer` at replay time.
    /// - `DrawItem::OpacityLayer` with `blend.is_advanced()` — a `saveLayer` with
    ///   an explicit advanced blend mode; composited via `flush_advanced_layer` with
    ///   `LoadOp::Load` and no scissor, so its `bounds` rect is the full composite
    ///   footprint written to the surface.
    ///
    /// `DrawItem::Filter` and `DrawItem::OffscreenTexture` are intentionally
    /// excluded: they composite their offscreen via premultiplied SrcOver and never
    /// read the surface backdrop, so they carry no stale-pixel hazard outside the
    /// scissor.
    ///
    /// "Straddle" means the bounds intersect the damage rect AND are NOT fully
    /// contained by it — i.e., part of the item falls outside the scissored
    /// region.  Items fully inside or fully outside do not straddle.
    ///
    /// Called by `renderer.rs` after `render_layer_recursive` to decide whether
    /// to schedule a full repaint on the next frame (self-healing).  Not test-gated
    /// because it is a production helper; it is also covered by the dedicated
    /// detector tests in `shape_blend_tests.rs`.
    pub(crate) fn has_advanced_shape_straddling(
        &self,
        damage: flui_types::Rect<flui_types::geometry::Pixels>,
    ) -> bool {
        use super::command_ir::DrawItem;
        self.draw_order.iter().any(|item| match item {
            DrawItem::AdvancedShape(op) => {
                op.device_bounds.intersects(&damage) && !damage.contains_rect(&op.device_bounds)
            }
            DrawItem::SsaaPath(op) => {
                // SsaaPath is routed through `flush_advanced_layer` when the blend
                // is advanced (dst-read).  Only those paths are subject to the same
                // stale-pixel hazard; tile-safe porter-duff SSAA paths do not read
                // the backdrop so they cannot write stale pixels outside the scissor.
                op.blend.is_advanced()
                    && op.device_bounds.intersects(&damage)
                    && !damage.contains_rect(&op.device_bounds)
            }
            DrawItem::OpacityLayer(op) => {
                // An advanced-blend saveLayer composites onto the surface via
                // `flush_advanced_layer` with `LoadOp::Load` and no scissor, reading
                // the full `op.bounds` region of the backdrop.  Any straddle of the
                // damage rect means unscissored pixels outside the damage may be
                // written with a stale-backdrop blend result.
                //
                // `DrawItem::Filter` and `DrawItem::OffscreenTexture` are excluded:
                // they composite via premultiplied SrcOver from an offscreen texture
                // and do not read the surface backdrop.
                op.blend.is_advanced()
                    && op.bounds.intersects(&damage)
                    && !damage.contains_rect(&op.bounds)
            }
            _ => false,
        })
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

    /// Returns the number of resolved [`DrawItem::Filter`] entries in the current
    /// draw order.
    ///
    /// A filter entry is placed by [`Self::restore_layer`] after a corresponding
    /// [`Self::save_layer_with_image_filter`] when the image-filter layer had at
    /// least one draw item inside it (non-empty content). Callers that check for
    /// zero verify that the filter layer was either culled or was empty.
    ///
    /// Gated to test builds; must never be called from production code.
    #[cfg(all(test, feature = "enable-wgpu-tests"))]
    pub(crate) fn filter_op_count_for_test(&self) -> usize {
        self.draw_order
            .iter()
            .filter(|item| matches!(item, DrawItem::Filter(_)))
            .count()
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

// ===== Submodule declarations (C1 LOC-cap split) =====
// The inherent `impl WgpuPainter` blocks for the public drawing API,
// transform/clip state, save-layer/filter composition, and gradients were
// moved out of this file to restore the C1 <1500-LOC cap. They are descendant
// modules of `painter`, so they retain access to WgpuPainter's private fields.
mod draw;
mod gradient;
mod layer;
mod transform_clip;

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
mod tests;
