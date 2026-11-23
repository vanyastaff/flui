//! The frame protocol a window slot and the headless retained
//! capture share (ADR-0087 §4): the damage owed, the retained target that
//! holds the last frame, and the sequence that turns a [`crate::frame::damage::FramePlan`] into
//! GPU work — pick the target, clear it unless the frame is partial, record
//! the content, blit a retained frame to the surface, commit.
//!
//! Only the steps that genuinely differ between a swapchain and a readback
//! texture ([`crate::frame::protocol::FrameSteps`]) live with each caller; everything a readback test
//! pins about partial frames runs through this one implementation.

use crate::device::domain::DeviceDomain;
#[cfg(test)]
use flui_foundation::geometry::Rect;
use std::sync::Arc;

use crate::error::EngineResult;
use crate::frame::damage::{DamageTracker, FramePlan, plan_frame};
use crate::frame::retained_target::RetainedTarget;

/// The raster side's work for one frame of the frame protocol, which the
/// engine's gate tests read (ADR-0304 §9) and the frame's trace event
/// carries; an offscreen slot's frame (`GpuContext`) keeps no count. Fixed
/// fields, so counting allocates nothing; each count adds saturating. A partial
/// frame of several passes sums them: a node or chunk two rects reach counts
/// once per pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RasterCounters {
    /// The scene nodes the walk entered: a version's boundaries it reached
    /// and did not cull (its effect entries are not nodes; a slot under an
    /// effect the walk skipped is never reached), or every layer of a tree it
    /// reached, a layer that then draws nothing included.
    pub(crate) nodes_entered: u64,
    /// The reaches the walk's cull tested
    /// ([`flui_layer::SubtreeCull::reaches_tested`]): one per attached slot
    /// asked about, and one per block of chunks and per chunk a run of
    /// chunks was tested by, the chunk a run stopped at counted once; `0`
    /// for a full frame and for a tree, which are walked whole.
    pub(crate) cull_tests: u64,
    /// The chunks of a version whose commands the frame recorded into the
    /// painter's IR; `0` for a tree, whose pictures are layers, not chunks.
    pub(crate) chunks_recorded: u64,
    /// The passes a partial frame redrew its region in, one per rect of the
    /// region; `0` for a full frame.
    pub(crate) region_passes: u64,
    /// The chunks whose kept recording the frame replayed instead of
    /// recording them (ADR-0304 §9); a chunk counts here or in
    /// [`Self::chunks_recorded`], never both.
    pub(crate) recording_cache_hits: u64,
    /// The recordings that left the painter's cache over the frame: evicted
    /// over its budget, replaced, or dropped with a resource epoch.
    pub(crate) recording_cache_evictions: u64,
    /// The bytes the cache held once the frame's recording settled: a level,
    /// not a count, so adding two counts keeps the larger.
    pub(crate) recording_cache_bytes: u64,
}

impl std::ops::AddAssign for RasterCounters {
    fn add_assign(&mut self, rhs: Self) {
        self.nodes_entered = self.nodes_entered.saturating_add(rhs.nodes_entered);
        self.cull_tests = self.cull_tests.saturating_add(rhs.cull_tests);
        self.chunks_recorded = self.chunks_recorded.saturating_add(rhs.chunks_recorded);
        self.region_passes = self.region_passes.saturating_add(rhs.region_passes);
        self.recording_cache_hits = self
            .recording_cache_hits
            .saturating_add(rhs.recording_cache_hits);
        self.recording_cache_evictions = self
            .recording_cache_evictions
            .saturating_add(rhs.recording_cache_evictions);
        self.recording_cache_bytes = self.recording_cache_bytes.max(rhs.recording_cache_bytes);
    }
}

/// The colour a frame starts from: the full clear pass and the clear a
/// partial frame draws inside its damage both paint it, so the background a
/// partial frame repaints is the one a full frame shows.
pub(crate) const BACKGROUND: flui_painting::styling::Color = flui_painting::styling::Color::WHITE;

/// [`BACKGROUND`] as a render pass's clear value. The channels map straight
/// through, as the painter maps a fill's colour; `partial_equals_full_inside_damage`
/// compares the two backgrounds pixel for pixel.
pub(crate) fn background_clear_value() -> wgpu::Color {
    let [r, g, b, a] = BACKGROUND.to_rgba_f32_array().map(f64::from);
    wgpu::Color { r, g, b, a }
}

/// Records one frame's content into `painter`: resets the per-frame
/// state and walks the scene, once for a full frame and once per rect of
/// the region `partial` redraws. Each pass opens
/// with `damage::begin_partial` (its rect's scissor and the clear inside
/// it) and walks the scene culled by its own rect. Answers what its walk
/// and recording did.
///
/// A window slot's swapchain frame and the crate's headless retained
/// capture both record through this, so the partial-frame protocol a
/// readback test pins is the one the swapchain path runs. Submitting the
/// painter's batches stays with the caller. `reads` says whether the walk
/// may replay kept recordings; the cache settles under its budget once the
/// walk is done.
pub(crate) fn record_frame_content(
    painter: &mut crate::painter::WgpuPainter,
    scene: &flui_layer::Scene,
    partial: Option<&flui_layer::Region>,
    reads: crate::record::cache::CacheReads,
) -> EngineResult<RasterCounters> {
    let recorded = record_passes(painter, scene, partial, reads)?;
    painter.settle_recording_cache();
    Ok(recorded)
}

/// [`record_frame_content`]'s walks.
fn record_passes(
    painter: &mut crate::painter::WgpuPainter,
    scene: &flui_layer::Scene,
    partial: Option<&flui_layer::Region>,
    reads: crate::record::cache::CacheReads,
) -> EngineResult<RasterCounters> {
    use crate::layers::dispatcher::LayerDispatcher;

    let Some(region) = partial else {
        let mut backend = LayerDispatcher::new(painter);
        // Reset per-frame clip/transform/opacity/layer state so that
        // partial-damage scissors from frame N cannot leak into frame N+1.
        backend.painter_mut().begin_frame_in_scope();
        return crate::layers::walk::record_scene(scene.version(), None, reads, &mut backend);
    };
    let mut recorded = RasterCounters::default();
    for (pass, &rect) in (0_u32..).zip(region.rects()) {
        // A dispatcher per pass: its Drop calls flush_active_transform(),
        // which balances any deferred lazy-coalescing save left by
        // `with_transform`, so the reset below finds the stacks balanced
        // and the next pass starts from no scissor but its own.
        let mut backend = LayerDispatcher::new(painter);
        backend.painter_mut().begin_frame_in_scope();
        backend.painter_mut().begin_region_pass(pass);
        // The walk culls by the whole pixels the scissor admits: a
        // boundary that reaches only the edge pixel still draws its share
        // of that pixel, which the clear wiped.
        let redrawn = crate::frame::damage::begin_partial(backend.painter_mut(), rect);
        tracing::trace!(
            left = rect.left(),
            top = rect.top(),
            right = rect.right(),
            bottom = rect.bottom(),
            "Damage scissor applied"
        );
        // Depth-first traversal of the scene. Backdrop-filter and
        // advanced-blend passes read from `render_texture`, which always
        // has COPY_SRC here: the swapchain image when the surface offers
        // it, the retained target (created with it) otherwise. A
        // version's walk skips the boundaries and chunks this rect cannot
        // reach.
        recorded +=
            crate::layers::walk::record_scene(scene.version(), Some(redrawn), reads, &mut backend)?;

        recorded.region_passes = recorded.region_passes.saturating_add(1);
    }
    Ok(recorded)
}

/// The parts of a frame that depend on where it is presented.
pub(crate) trait FrameSteps {
    /// Encodes the clear before any destination reads.
    fn clear(
        &mut self,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<()>;

    /// Records and encodes the frame's content into `(view, texture)`;
    /// `retained` says the target is the retained texture rather than the
    /// surface, `partial` how a partial frame redraws its region, `reads`
    /// whether it may replay kept recordings. Answers what the walk and the
    /// recording did.
    ///
    /// # Errors
    /// The content pass's failure, which is the frame's.
    fn content(
        &mut self,
        view: &wgpu::TextureView,
        texture: &wgpu::Texture,
        retained: bool,
        partial: Option<&flui_layer::Region>,
        reads: crate::record::cache::CacheReads,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<RasterCounters>;

    /// Copies the whole retained texture onto the surface's view.
    fn blit(
        &mut self,
        retained: &wgpu::Texture,
        surface: &wgpu::Texture,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<Arc<crate::device::domain::PreparedPermit>>;

    /// Transfers all prepared ownership with the complete ordered frame.
    fn submit(
        &mut self,
        encoder: wgpu::CommandEncoder,
        permits: Vec<Arc<crate::device::domain::PreparedPermit>>,
    ) -> EngineResult<()>;

    /// How many kept recordings left the painter's cache since it was last
    /// asked and the bytes it holds, read once the frame is submitted.
    fn recording_cache_counts(&mut self) -> (u64, u64);

    /// Drops the recordings the painter keeps for later frames, as a raster
    /// panic does (ADR-0304 §10): called while the frame unwinds.
    fn unwound(&mut self);

    /// Resets frame-local state on success, error and unwind.
    fn finish(&mut self);
}

struct FrameCleanup<'a, S: FrameSteps>(&'a mut S);

impl<S: FrameSteps> Drop for FrameCleanup<'_, S> {
    fn drop(&mut self) {
        // Running while the frame unwinds, before anything catches it.
        if std::thread::panicking() {
            self.0.unwound();
        }
        self.0.finish();
    }
}

/// Font blob ids are local to the image that shapes a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontSource {
    Ordinary,
    Plugin,
}

impl FontSource {
    pub(crate) fn rasterizer(self) -> flui_painting::glyphs::SwashRasterizer {
        match self {
            Self::Ordinary => flui_painting::glyphs::SwashRasterizer::new(),
            Self::Plugin => flui_painting::glyphs::SwashRasterizer::with_owned_fonts(),
        }
    }
}

/// The texture a frame ends in: the surface's view and texture, and the
/// working format and size its content renders in, which a retained frame's
/// candidate takes.
pub(crate) struct FrameTarget<'a> {
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) texture: &'a wgpu::Texture,
    pub(crate) size: (u32, u32),
    pub(crate) format: wgpu::TextureFormat,
}

/// A presentation's damage, its retained target and the one-frame promotion to a
/// full repaint.
#[derive(Debug)]
pub(crate) struct FrameProtocol {
    damage: DamageTracker,
    retained: RetainedTarget,
    /// When set, the next [`Self::plan`] promotes its damage to a full
    /// repaint, and clears the flag.
    ///
    /// Set when a partial frame found an advanced shape straddling its
    /// damage edge (such a shape composites over its whole device bounds
    /// with no scissor, so its out-of-damage slice blends over the previous
    /// frame for one frame), and after a frame rendered outside the damage
    /// protocol ([`Self::end_unmanaged`]).
    force_full_next_frame: bool,
    /// The font namespace this presentation's last frame drew with.
    font_source: FontSource,
    /// Whether damage applied since the last frame that ran was switched off
    /// (`Full(DamageOff)`): that frame records every chunk.
    damage_off: bool,
    /// What the last frame [`Self::run`] was handed did: zero after a skipped
    /// frame or one that failed.
    counters: RasterCounters,
    /// The commands of every chunk of the last version a region was planned
    /// over, summed once per version: what bounds the gain of redrawing a
    /// region under its box ([`crate::frame::region_cost::shape`]).
    version_commands: Option<(flui_layer::SceneVersionId, u64)>,
}

impl Default for FrameProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameProtocol {
    /// A protocol whose first frame renders in full.
    pub(crate) fn new() -> Self {
        Self {
            damage: DamageTracker::new(),
            retained: RetainedTarget::default(),
            force_full_next_frame: false,
            font_source: FontSource::Ordinary,
            damage_off: false,
            counters: RasterCounters::default(),
            version_commands: None,
        }
    }

    /// Selects a font namespace before damage planning or frame recording.
    /// The painter replaces its faces and atlas when it serves another
    /// namespace ([`WgpuPainter::select_font_source`]); a change of the
    /// namespace this presentation's last frame drew with, or a reset,
    /// also owes a full repaint, since the pixels left on screen came from a
    /// scene of another namespace or an earlier load.
    ///
    /// [`WgpuPainter::select_font_source`]: crate::painter::WgpuPainter::select_font_source
    pub(crate) fn select_font_source(
        &mut self,
        painter: &mut crate::painter::WgpuPainter,
        source: FontSource,
        reset_fonts: bool,
    ) {
        painter.select_font_source(source, reset_fonts);
        if self.font_source != source || reset_fonts {
            self.font_source = source;
            self.damage.mark_full_repaint();
        }
    }

    /// Adds the whole pixels `rect` touches on a surface of `surface` to the
    /// damage owed; a rect that is not finite owes a full repaint.
    #[cfg(test)]
    pub(crate) fn mark_dirty(&mut self, rect: Rect<f64>, surface: (u32, u32)) {
        self.damage.mark_dirty(rect, surface);
    }

    /// Owes a full repaint.
    #[cfg(test)]
    pub(crate) fn mark_full_repaint(&mut self) {
        self.damage.mark_full_repaint();
    }

    /// Owes a publication's `damage`: a region rect by rect, a full repaint
    /// for any [`FullReason`](flui_layer::FullReason). Damage switched off
    /// (`Full(DamageOff)`) also keeps the next frame that runs from replaying
    /// kept recordings.
    pub(crate) fn apply_damage(&mut self, damage: flui_layer::DamageRegion) {
        use flui_layer::DamageRegion;
        self.damage_off |= crate::record::cache::CacheReads::for_damage(&damage)
            == crate::record::cache::CacheReads::Off;
        match damage {
            DamageRegion::Region(region) => self.damage.mark_region(&region),
            DamageRegion::Unchanged => {}
            DamageRegion::Full(_) => self.damage.mark_full_repaint(),
        }
    }

    /// The surface was resized, reconfigured or rebuilt: every pixel is
    /// owed, and the retained target no longer matches what it shows.
    pub(crate) fn surface_changed(&mut self) {
        self.damage.mark_full_repaint();
        self.retained.invalidate();
    }

    /// Drops the retained target's texture (its device is gone, or the
    /// surface was released and its memory goes with it).
    pub(crate) fn release_target(&mut self) {
        self.retained.release();
    }

    /// The plan for a frame of `scene` on a target that owns `painter` (the
    /// headless retained capture): `lineage` notes the scene owner it draws,
    /// and the painter's recordings of the one it drew before are dropped;
    /// the damage is closed over the backdrop readers at `size`. A slot of
    /// the context's shared painter notes its lineage in the context
    /// instead, which knows every slot.
    #[cfg(test)]
    pub(crate) fn plan_scene(
        &mut self,
        painter: &mut crate::painter::WgpuPainter,
        lineage: &mut crate::record::cache::DrawnLineages<()>,
        scene: &flui_layer::Scene,
        size: (u32, u32),
        intermediate_required: bool,
    ) -> FramePlan {
        if let Some(dead) = lineage.draws((), Some(scene.version())) {
            painter.drop_recordings_of(dead);
        }
        self.include_backdrop_dependencies(scene, size);
        self.plan(intermediate_required)
    }

    /// The plan for the next frame, applying a pending promotion to full.
    pub(crate) fn plan(&mut self, intermediate_required: bool) -> FramePlan {
        // Every frame plans first, so a skipped or failed frame counts nothing;
        // a plan asked for anything else (a slot asking whether it owes a
        // frame) clears the count too.
        self.counters = RasterCounters::default();
        if std::mem::take(&mut self.force_full_next_frame) {
            self.damage.mark_full_repaint();
            tracing::trace!("promoting this frame to a full repaint");
        }
        plan_frame(
            &self.damage,
            self.retained.is_valid(),
            intermediate_required,
        )
    }

    /// Reconstruct every backdrop input before it reads the candidate target.
    /// Retained pixels outside damage contain the previous frame's *final*
    /// composition, including later siblings, not the current ordered backdrop.
    ///
    /// The damage is closed as the frame redraws it. A region of a version
    /// whose bounding box is predicted cheaper than its rects
    /// ([`region_cost`](crate::frame::region_cost)) becomes that box, which then
    /// closes as itself: a box redraws the gap between the rects, where a
    /// reader no rect met is cut by the box's scissor. Otherwise the closure
    /// of the region holds each read footprint it joined inside one rect,
    /// which that rect's pass redraws whole, and a reader no rect meets is
    /// left as the last frame drew it. A closure that adds a rect outside the
    /// box is planned and closed again, at most [`Region::MAX`] rounds before
    /// the frame repaints in full.
    ///
    /// [`Region::MAX`]: flui_layer::Region::MAX
    pub(crate) fn include_backdrop_dependencies(
        &mut self,
        scene: &flui_layer::Scene,
        size: (u32, u32),
    ) {
        use flui_layer::{DamageRegion, Region};

        let version = scene.version();
        for _ in 0..Region::MAX {
            let Some(region) = self.damage.region() else {
                return;
            };
            let owed = DamageRegion::Region(region);
            // The version closes the damage over its own filters and
            // destination-reading chunks, reading only those (ADR-0304 §6):
            // each publication's damage was closed under its version, and the
            // damage owed here may join several.
            let closed = version.close_damage(owed, size);
            self.apply_closed(owed, closed);
            let Some(region) = self.damage.region() else {
                return;
            };
            if region.rects().len() == 1 {
                return;
            }
            let commands = self.version_commands(version);
            if crate::frame::region_cost::shape(version, &region, commands)
                == crate::frame::region_cost::RegionShape::Rects
            {
                return;
            }
            // Every rect overlaps the box, so the tracker folds them into it.
            self.damage.mark_rect(region.bounds());
        }
        self.damage.mark_full_repaint();
    }

    /// The commands of every chunk of `version`, summed once per version.
    fn version_commands(&mut self, version: &flui_layer::SceneVersion) -> u64 {
        match self.version_commands {
            Some((id, commands)) if id == version.id() => commands,
            _ => {
                let commands = crate::frame::region_cost::version_commands(version);
                self.version_commands = Some((version.id(), commands));
                commands
            }
        }
    }

    /// Marks what closing `region` added, rect by rect.
    fn apply_closed(&mut self, region: flui_layer::DamageRegion, closed: flui_layer::DamageRegion) {
        use flui_layer::DamageRegion;
        match closed {
            DamageRegion::Region(closed) if DamageRegion::Region(closed) != region => {
                self.damage.mark_region(&closed);
            }
            DamageRegion::Region(_) | DamageRegion::Unchanged => {}
            DamageRegion::Full(_) => self.damage.mark_full_repaint(),
        }
    }

    /// Runs `plan` (not [`FramePlan::Skip`]) against `target`, timing the
    /// frame on the GPU under its epoch when `timer` is given (ADR-0388).
    ///
    /// - [`FramePlan::Direct`] renders into the surface; the retained target
    ///   does not see the frame and stops being valid.
    /// - A retained plan renders into a separate candidate. Failure or unwind
    ///   preserves the committed pixels and forces a full retry.
    /// - Only a partial frame skips the full clear: each pass of its
    ///   region clears inside its own rect instead
    ///   (`damage::begin_partial`), keeping the retained pixels outside them.
    ///
    /// # Errors
    /// The content pass's failure.
    pub(crate) fn run(
        &mut self,
        plan: FramePlan,
        domain: &Arc<DeviceDomain>,
        target: FrameTarget<'_>,
        steps: &mut impl FrameSteps,
        mut timer: Option<(
            &mut crate::device::timer::FrameGpuTimer,
            flui_foundation::FrameEpoch,
        )>,
    ) -> EngineResult<()> {
        // A failure or unwind leaves damage owed and forces a deliberate full
        // retry; the previous committed allocation itself remains untouched.
        if matches!(plan, FramePlan::Skip) {
            return Ok(());
        }
        self.force_full_next_frame = true;
        let _submission_scope = domain.begin_frame_scope()?;
        let steps = FrameCleanup(steps);
        let mut encoder = domain
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("FLUI Complete Frame Encoder"),
            });
        // The frame's first timestamp precedes its clear, so the time covers
        // every pass of the frame (ADR-0388).
        let timed = timer
            .as_mut()
            .and_then(|(timer, epoch)| timer.begin(&mut encoder, *epoch));
        let (retained, partial) = match plan {
            FramePlan::Skip => return Ok(()),
            FramePlan::Direct => {
                self.retained.invalidate();
                (None, None)
            }
            FramePlan::RetainedFull => (
                Some(self.retained.begin_encoded(
                    domain,
                    target.size,
                    target.format,
                    false,
                    &mut encoder,
                )?),
                None,
            ),
            FramePlan::RetainedPartial(region) => (
                Some(self.retained.begin_encoded(
                    domain,
                    target.size,
                    target.format,
                    true,
                    &mut encoder,
                )?),
                Some(region),
            ),
        };
        let (view, texture) = match retained.as_ref() {
            Some(candidate) => (candidate.view(), candidate.texture()),
            None => (target.view, target.texture),
        };
        if partial.is_none() {
            steps.0.clear(view, &mut encoder)?;
        }
        let recorded = steps.0.content(
            view,
            texture,
            retained.is_some(),
            partial.as_ref(),
            if self.damage_off {
                crate::record::cache::CacheReads::Off
            } else {
                crate::record::cache::CacheReads::On
            },
            &mut encoder,
        )?;
        let mut permits = Vec::new();
        if let Some(candidate) = retained.as_ref() {
            permits = candidate.submission_permits();
            permits.push(steps.0.blit(
                candidate.texture(),
                target.texture,
                target.view,
                &mut encoder,
            )?);
        }
        if let (Some((timer, _)), Some(timed)) = (timer.as_mut(), timed) {
            timer.end(&mut encoder, timed);
        }
        steps.0.submit(encoder, permits)?;
        if let (Some((timer, _)), Some(timed)) = (timer, timed) {
            timer.submitted(timed);
        }
        if let Some(candidate) = retained {
            self.retained.commit(candidate);
        }
        // The frame committed: the full retry armed when it began is withdrawn.
        self.force_full_next_frame = false;
        self.damage_off = false;
        self.counters = recorded;
        let (evictions, bytes) = steps.0.recording_cache_counts();
        self.counters.recording_cache_evictions = evictions;
        self.counters.recording_cache_bytes = bytes;
        tracing::trace!(
            nodes_entered = self.counters.nodes_entered,
            cull_tests = self.counters.cull_tests,
            chunks_recorded = self.counters.chunks_recorded,
            region_passes = self.counters.region_passes,
            recording_cache_hits = self.counters.recording_cache_hits,
            recording_cache_evictions = self.counters.recording_cache_evictions,
            recording_cache_bytes = self.counters.recording_cache_bytes,
            "frame submitted"
        );
        Ok(())
    }

    /// What the last frame run did; zero after a skipped frame or one that
    /// failed.
    #[cfg(test)]
    pub(crate) fn counters(&self) -> RasterCounters {
        self.counters
    }

    /// The region a partial frame would redraw now: `None` when the frame
    /// repaints in full or owes nothing.
    #[cfg(test)]
    pub(crate) fn owed_region(&self) -> Option<flui_layer::Region> {
        self.damage.region()
    }

    /// The frame presented: the damage it covered is paid.
    pub(crate) fn presented(&mut self) {
        self.damage.reset();
    }

    /// Opens a frame that no damage producer accounted for (a scene
    /// lent to a window slot outside its frames, `WindowSlot::render_lent`):
    /// it renders in full.
    pub(crate) fn begin_unmanaged(&mut self) {
        self.damage.mark_full_repaint();
    }

    /// Closes a frame opened by [`Self::begin_unmanaged`], whatever became of
    /// it. The frame showed a scene the producer never saw, so neither the
    /// retained target nor the producer's next diff describes the screen any
    /// more: the target is invalidated and the next frame renders in full,
    /// even one the producer found unchanged.
    pub(crate) fn end_unmanaged(&mut self) {
        self.retained.invalidate();
        self.force_full_next_frame = true;
    }

    /// The retained target's texture, when one is allocated.
    #[cfg(test)]
    pub(crate) fn retained_texture(&self) -> Option<&wgpu::Texture> {
        self.retained.texture()
    }
}
