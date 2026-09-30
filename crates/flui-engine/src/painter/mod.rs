//! GPU-accelerated 2D painter using wgpu + lyon + an engine-owned glyph atlas
//!
//! This is the unified painter implementation that combines:
//! - Shape rendering via vertex batching
//! - Text as a glyph batch of the segment, sampled from `GlyphAtlas`
//! - Path tessellation via lyon
//! - Transform stack for coordinate transformations
//!
//! Follows SOLID and KISS principles with clean separation of concerns.

use std::sync::Arc;

use crate::{
    command_ir::{DrawItem, DrawSegment, ImageFilterPass, PendingOffscreenTexture},
    glyph_atlas::{GlyphAtlas, TextAtlas},
    layer_compositor::LayerCompositor,
    pipeline_set::PipelineSet,
    replay::GpuReplay,
    resources::GpuResources,
    state_stack::GpuStateStack,
};
use flui_foundation::geometry::Rect;

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
    batcher: crate::batches::DrawBatcher,

    // ===== Text =====
    /// The rasterised-glyph cache paragraphs are recorded against and the
    /// glyph pipeline samples.
    glyph_atlas: TextAtlas,
    /// Shapes the performance overlay's labels (`draw_label`); built on the
    /// first one, so a painter that draws no overlay holds no fonts.
    labels: Option<flui_painting::TextContext>,

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

    /// Whether this painter has already reported that a path clip was
    /// approximated by its bounding box.
    ///
    /// `WgpuPainter::clip_path` cannot clip to the exact shape, and that gap is
    /// worth a release-level signal — but paint is a full-tree descent every
    /// frame, so an unconditional warn fires once per clip per frame and trains
    /// an operator to filter the channel it is trying to reach.
    ///
    /// The latch is per painter, which is once per run for the one that
    /// matters: `Renderer` builds its painter with the GPU stack and keeps it.
    /// The two other construction sites are per-use — `HeadlessRenderer`
    /// builds one per capture, and an offscreen pass builds its own — so those
    /// report once each rather than once ever. That is the right side to err
    /// on: a capture that silently approximated a clip is worth one line.
    path_clip_approximated: bool,
}

// GPU rendering routinely converts between numeric types for pixel coordinates,
// color channels, buffer indices, and instance counts.
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

        // Glyph bitmaps come from swash over the faces each paragraph's
        // runs carry (ADR-0092 §5): the atlas owns its rasterizer and the
        // registry of faces it has drawn, and shares no font state.
        let glyph_atlas = GlyphAtlas::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            &pipelines.glyph_atlas_bind_group_layout,
            flui_painting::glyphs::SwashRasterizer::new(),
        );

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
            batcher: crate::batches::DrawBatcher::new(),
            glyph_atlas,
            labels: None,
            state: GpuStateStack::new(),
            compositor: LayerCompositor::new(),
            current_segment: DrawSegment::new(),
            draw_order: Vec::new(),
            path_clip_approximated: false,
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
    pub(crate) fn reset_frame_state(&mut self) {
        // Assert save/restore balance at the frame boundary BEFORE clearing.
        //
        // Not placed in `GpuStateStack::Drop` because the LayerDispatcher
        // implicit-single-save (a lazy `active_transform` save, balanced by
        // `LayerDispatcher`'s own `Drop`) must not false-positive-panic here, and a
        // Drop panic during unwind aborts the process.
        //
        // The assertion logic lives in `GpuStateStack::debug_assert_balanced`
        // so it can be exercised by unit tests without a GPU.
        self.state.debug_assert_balanced();
        self.compositor.debug_assert_balanced();

        self.state.reset();
        self.compositor.reset();

        tracing::trace!("WgpuPainter::reset_frame_state: per-frame state cleared");
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
    /// because it is a production helper.
    pub(crate) fn has_advanced_shape_straddling(
        &self,
        damage: flui_foundation::geometry::Rect<f64>,
    ) -> bool {
        use crate::command_ir::DrawItem;
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

    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn offscreen_results_for_test(&self) -> Vec<(Rect<f64>, u32, u32)> {
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

    // ===== Offscreen Compositing =====

    /// Queue an offscreen-rendered texture for compositing into the main render target.
    ///
    /// This finalizes the current draw segment and inserts the offscreen texture
    /// into the draw order. Content drawn before this call will render before
    /// the offscreen texture, and content drawn after will render after it,
    /// preserving correct Z-ordering.
    pub(crate) fn queue_offscreen_result(
        &mut self,
        texture: crate::texture_pool::PooledTexture,
        bounds: Rect<f64>,
        blend: flui_painting::paint::BlendMode,
    ) {
        // Finalize the current segment and start a new one
        self.finish_current_segment();
        self.draw_order
            .push(DrawItem::OffscreenTexture(PendingOffscreenTexture {
                texture,
                bounds,
                blend,
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
        target: crate::render_target::RenderTarget<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> crate::error::EngineResult<()> {
        // Advance batcher cache frame counters and evict stale entries.
        self.batcher.path_cache.advance_frame();

        // Log rendering stats before finalising (so counts reflect pre-drain state).
        let glyph_count = self.current_segment.glyph_batch.len();
        let rect_count = self.current_segment.rect_batch.len();
        let circle_count = self.current_segment.circle_batch.len();
        let buffer_stats = self.resources.buffer_pool_mut().stats();

        tracing::trace!(
            vertices = self.current_segment.vertices.len(),
            indices = self.current_segment.indices.len(),
            glyphs = glyph_count,
            rects = rect_count,
            circles = circle_count,
            segments = self.draw_order.len(),
            cache_hit_rate = format!("{:.0}%", buffer_stats.reuse_rate * 100.0),
            "Drawing commands"
        );

        // Finalise the current segment and move the draw order into a local
        // vec.  `mem::take` hands the record side's existing allocation
        // straight to the replay side instead of copying it into a freshly
        // allocated one, so the frame's items never get reallocated on the way
        // out; `self.draw_order` is left empty for the next record pass.
        self.finish_current_segment();
        let items: Vec<DrawItem> = std::mem::take(&mut self.draw_order);

        self.replay.submit(
            items,
            self.size,
            self.surface_format,
            &self.device,
            &self.queue,
            &mut self.pipelines,
            &mut self.resources,
            &self.glyph_atlas,
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
        self.render(crate::render_target::RenderTarget::view_only(view), encoder)
    }

    /// Run end-of-frame maintenance: close the glyph atlas' frame, evict over-budget
    /// textures, reclaim a full atlas that holds stale entries, then reset
    /// use-counters.
    ///
    /// Call EXACTLY ONCE per frame, after the final `WgpuPainter::render` flush.
    /// `render` must not do this itself — it runs once per pass (backdrop-filter
    /// flushes invoke it mid-frame), so per-call maintenance would reset
    /// use-counters between passes and drop textures still in use this frame.
    pub(crate) fn end_frame_maintenance(&mut self) {
        // Close the glyph atlas' frame: slots this frame did not touch become
        // reclaimable. Must run here (the once-per-frame seam), not per
        // `render` pass — advancing mid-frame would let an earlier
        // backdrop-filter pass's glyphs be evicted under a later pass.
        self.glyph_atlas.end_frame();

        // Rewind the filter/composite uniform pool for next-frame reuse. Same
        // once-per-frame seam: rewinding per `render` would alias a backdrop-
        // flush uniform with a final-render uniform in the same frame. See
        // `UniformPool::reset_frame`.
        self.resources.uniform_pool_mut().reset_frame();

        // Reclaim over-budget vertex/index buffers (LRU-first). Must run here,
        // after the final submit, where every pooled buffer is free — dropping a
        // `wgpu::Buffer` only schedules the GPU free once submissions finish, so
        // this never reclaims memory the in-flight frame still reads. See
        // `BufferPool::evict_over_budget`.
        self.resources
            .buffer_pool_mut()
            .evict_over_budget(crate::buffer_pool::DEFAULT_BUDGET_BYTES);

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
    #[must_use]
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

    // ===== External Texture Registry Access =====

    /// Get a reference to the external texture registry
    ///
    /// Use this to register external textures (video frames, camera preview,
    /// etc.) that can be rendered via `Canvas::draw_texture()`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use flui_painting::paint::TextureId;
    ///
    /// # fn wire(painter: &mut flui_engine::WgpuPainter, gpu_texture: wgpu::Texture) {
    /// let texture_id = TextureId::new(42);
    /// painter.external_texture_registry_mut().register(
    ///     texture_id,
    ///     gpu_texture,
    ///     1920,
    ///     1080,
    ///     true,
    ///     true,
    /// );
    /// # }
    /// ```
    pub fn external_texture_registry(
        &self,
    ) -> &crate::external_texture_registry::ExternalTextureRegistry {
        self.resources.external_texture_registry()
    }

    /// Get a mutable reference to the external texture registry
    ///
    /// Use this to register, update, or unregister external textures.
    pub fn external_texture_registry_mut(
        &mut self,
    ) -> &mut crate::external_texture_registry::ExternalTextureRegistry {
        self.resources.external_texture_registry_mut()
    }

    // ===== Helper Methods =====

    /// Maximum basis length of the current transform's 2D linear part.
    ///
    /// Mirrors Impeller's `Matrix::GetMaxBasisLengthXY`: the larger of the two
    /// column-vector lengths of the upper-left 2x2. The tessellator divides its
    /// device-space chord-error budget by this so curves are subdivided finely
    /// enough at the magnification they will be baked and drawn at — see
    /// [`Tessellator::set_max_scale`](crate::tessellator::Tessellator::set_max_scale).
    ///
    /// Also consulted by `LayerDispatcher::render_shader_mask` to size the shader-mask
    /// offscreen at device resolution: on a HiDPI frame the live device-pixel
    /// ratio rides in the painter CTM (the `RenderView` root pushes
    /// `scale(dpr)`), so the offscreen child/result textures must be allocated
    /// `bounds * dpr` to avoid rendering the masked layer at half resolution.
    pub(crate) fn current_max_scale(&self) -> f32 {
        self.state.max_scale()
    }

    /// The accumulated current transform (CTM) as a [`flui_foundation::geometry::Matrix4`].
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
    pub(crate) fn current_transform_matrix(&self) -> flui_foundation::geometry::Matrix4 {
        self.state.current_transform_matrix()
    }

    /// Seal the current segment and start a fresh one.
    ///
    /// Forwards to `DrawBatcher::finish_current_segment`.  Called explicitly
    /// from `queue_offscreen_result` when an offscreen texture must be
    /// interleaved at the correct Z-position, and from the flush path to
    /// finalize the last segment before GPU submission.
    fn finish_current_segment(&mut self) {
        crate::batches::DrawBatcher::finish_current_segment(
            &mut self.current_segment,
            &mut self.draw_order,
        );
    }
}

// ===== Submodule declarations (C1 LOC-cap split) =====
// The inherent `impl WgpuPainter` blocks for the public drawing API,
// transform/clip state, and save-layer/filter composition were
// moved out of this file to restore the C1 <1500-LOC cap. They are descendant
// modules of `painter`, so they retain access to WgpuPainter's private fields.
mod draw;
mod layer;
mod transform_clip;

// ─── Shared growth helper ─────────────────────────────────────────────────────

/// Compute the total grown-bounds expansion in pixels for a pass chain.
///
/// Each growing pass expands the filter halo by its radius; bounds-preserving
/// passes contribute 0.  Summing the per-pass contributions is the correct
/// conservative bound: each growing pass enlarges the halo of the result of all
/// prior passes, so radii compose additively (inner→outer bounds
/// chaining).
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
pub(super) fn cumulative_growth(passes: &[ImageFilterPass]) -> f32 {
    passes
        .iter()
        .map(|pass| match pass {
            ImageFilterPass::Blur { sigma_x, sigma_y } => {
                crate::effects::kernel_radius(sigma_x.max(*sigma_y)) as f32
            }
            ImageFilterPass::Morph { radius, .. } => radius.ceil(),
            // Both are bounds-PRESERVING: neither grows the filter extent.
            ImageFilterPass::ColorMatrix(_) | ImageFilterPass::Identity => 0.0,
        })
        .sum()
}

#[cfg(all(test, feature = "testing"))]
mod tests;
