//! The frame protocol the windowed renderer and the headless retained
//! capture share (ADR-0087 §4): the damage owed, the retained target that
//! holds the last frame, and the sequence that turns a [`crate::damage::FramePlan`] into
//! GPU work — pick the target, clear it unless the frame is partial, record
//! the content, blit a retained frame to the surface, commit.
//!
//! Only the steps that genuinely differ between a swapchain and a readback
//! texture ([`crate::frame_protocol::FrameSteps`]) live with each caller; everything a readback test
//! pins about partial frames runs through this one implementation.

use crate::device_domain::DeviceDomain;
use flui_foundation::geometry::Rect;
use std::sync::Arc;

use crate::damage::{DamageTracker, FramePlan, RegionPlan, RegionPlanner, plan_frame};
use crate::error::EngineResult;
use crate::retained_target::RetainedTarget;

/// What recording one frame's content found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[must_use = "a straddling shape owes the next frame a full repaint"]
pub(crate) struct RecordedContent {
    /// Whether an advanced shape straddled the rect of the pass that drew
    /// it: outside that rect the retained pixels under it may be stale, so
    /// the next frame renders in full.
    pub(crate) straddled: bool,
    /// The work the frame's walk and recording did.
    pub(crate) counters: RasterCounters,
}

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
    /// The slots and chunks the walk asked its cull about; `0` for a full
    /// frame and for a tree, which are walked whole.
    pub(crate) cull_tests: u64,
    /// The chunks of a version whose commands the frame recorded into the
    /// painter's IR; `0` for a tree, whose pictures are layers, not chunks.
    pub(crate) chunks_recorded: u64,
    /// The passes a partial frame redrew its region in, one per rect of its
    /// [`RegionPlan`]; `0` for a full frame.
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
    /// whether it may replay kept recordings. Answers
    /// what the recording found, an advanced shape straddling a pass's rect
    /// among it, in which case the next frame renders in full.
    ///
    /// # Errors
    /// The content pass's failure, which is the frame's.
    fn content(
        &mut self,
        view: &wgpu::TextureView,
        texture: &wgpu::Texture,
        retained: bool,
        partial: Option<&RegionPlan>,
        reads: crate::recording_cache::CacheReads,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<RecordedContent>;

    /// Copies the whole retained texture onto the surface's view.
    fn blit(
        &mut self,
        retained: &wgpu::Texture,
        surface: &wgpu::Texture,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<Arc<crate::device_domain::PreparedPermit>>;

    /// Transfers all prepared ownership with the complete ordered frame.
    fn submit(
        &mut self,
        encoder: wgpu::CommandEncoder,
        permits: Vec<Arc<crate::device_domain::PreparedPermit>>,
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

/// A renderer's damage, its retained target and the one-frame promotion to a
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
    font_source: FontSource,
    /// Whether damage applied since the last frame that ran was switched off
    /// (`Full(DamageOff)`): that frame records every chunk.
    damage_off: bool,
    /// What the last frame [`Self::run`] was handed did: zero after a skipped
    /// frame or one that failed.
    counters: RasterCounters,
    /// How a partial frame's region is planned: [`RegionPlan::of`], or rect
    /// by rect where a test asked for it (`plan_rect_by_rect`, test builds only).
    plan_region: RegionPlanner,
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
            plan_region: RegionPlan::of,
        }
    }

    /// Selects a font namespace before damage planning or frame recording.
    /// Reloaded images can reuse blob ids, so both faces and atlas entries
    /// must retire together. A transition also invalidates unchanged pixels.
    pub(crate) fn select_font_source(
        &mut self,
        painter: &mut crate::painter::WgpuPainter,
        source: FontSource,
        reset_fonts: bool,
    ) {
        if self.font_source != source || reset_fonts {
            painter.reset_scene_fonts(source);
            self.font_source = source;
            self.damage.mark_full_repaint();
        }
    }

    /// Constructs an empty painter with this scene source's font ownership.
    /// Recovery and surface-format changes replace GPU resources, not the
    /// source namespace selected by the last scene.
    pub(crate) fn new_painter(
        &self,
        domain: Arc<DeviceDomain>,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> crate::painter::WgpuPainter {
        crate::painter::WgpuPainter::with_domain_and_font_source(
            domain,
            format,
            size,
            self.font_source,
        )
    }

    /// Adds the whole pixels `rect` touches on a surface of `surface` to the
    /// damage owed; a rect that is not finite owes a full repaint.
    pub(crate) fn mark_dirty(&mut self, rect: Rect<f64>, surface: (u32, u32)) {
        self.damage.mark_dirty(rect, surface);
    }

    /// Owes a publication's `damage`: a region rect by rect, a full repaint
    /// for any [`FullReason`](flui_layer::FullReason). Damage switched off
    /// (`Full(DamageOff)`) also keeps the next frame that runs from replaying
    /// kept recordings.
    pub(crate) fn apply_damage(&mut self, damage: flui_layer::DamageRegion) {
        use flui_layer::DamageRegion;
        self.damage_off |= crate::recording_cache::CacheReads::for_damage(&damage)
            == crate::recording_cache::CacheReads::Off;
        match damage {
            DamageRegion::Region(region) => self.damage.mark_region(&region),
            DamageRegion::Unchanged => {}
            DamageRegion::Full(_) => self.damage.mark_full_repaint(),
        }
    }

    /// Owes a full repaint.
    pub(crate) fn mark_full_repaint(&mut self) {
        self.damage.mark_full_repaint();
    }

    /// Whether any damage is owed.
    #[must_use]
    pub(crate) fn has_damage(&self) -> bool {
        self.damage.has_damage()
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

    /// The plan for a frame of `scene` on a target that owns `painter` (a
    /// `Renderer`, or the capture that stands in for one): `lineage` notes
    /// the scene owner it draws, and the painter's recordings of the one it
    /// drew before are dropped; the damage is closed over the backdrop
    /// readers at `size`. A slot of the context's shared painter notes its
    /// lineage in the context instead, which knows every slot.
    pub(crate) fn plan_scene(
        &mut self,
        painter: &mut crate::painter::WgpuPainter,
        lineage: &mut crate::recording_cache::DrawnLineages<()>,
        scene: &flui_layer::Scene,
        size: (u32, u32),
        intermediate_required: bool,
    ) -> FramePlan {
        if let Some(dead) = lineage.draws((), Some(scene.content())) {
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
            self.plan_region,
        )
    }

    /// Reconstruct every backdrop input before it reads the candidate target.
    /// Retained pixels outside damage contain the previous frame's *final*
    /// composition, including later siblings, not the current ordered backdrop.
    ///
    /// The damage is closed as the frame will redraw it: the closure of a
    /// region holds each read footprint it joined inside one rect, which a
    /// pass per rect redraws whole, but a plan that redraws the bounding box
    /// also redraws the gap between the rects, where a reader the rects did
    /// not meet is cut by the box's scissor. So a region planned as its box
    /// is closed as the box itself, and the frame redraws exactly what was
    /// closed.
    pub(crate) fn include_backdrop_dependencies(
        &mut self,
        scene: &flui_layer::Scene,
        size: (u32, u32),
    ) {
        use flui_layer::{DamageRegion, SceneContent};

        let Some(region) = self.damage.region() else {
            return;
        };
        let closed = match scene.content() {
            // A retained version closes the damage over its own filters and
            // destination-reading chunks, reading only those (ADR-0304 §6):
            // each publication's damage was closed under its version, and the
            // damage owed here may join several.
            SceneContent::Version(version) => {
                self.close_as_planned(region, |damage| version.close_damage(damage, size))
            }
            SceneContent::Tree(tree) => {
                let closures = tree_backdrop_closures(tree);
                // Growing one read region can expose a second filter: the
                // fixed point is reached before the partial clear and any
                // recording, not next frame (ADR-0304 §6).
                self.close_as_planned(region, |damage| {
                    flui_layer::close_over(damage, &closures, size)
                })
            }
        };
        self.apply_closed(DamageRegion::Region(region), closed);
    }

    /// `region` closed by `close` over what its plan redraws: a plan that
    /// redraws the bounding box closes the box itself, which holds every
    /// pixel the rects would add (closing only grows damage), and a closure
    /// that adds a rect outside the box is planned and closed again. One rect
    /// closes to one rect (a footprint that meets it merges into it), so a
    /// box is a fixed point after one round; the bound only keeps the loop
    /// finite.
    fn close_as_planned(
        &self,
        region: flui_layer::Region,
        close: impl Fn(flui_layer::DamageRegion) -> flui_layer::DamageRegion,
    ) -> flui_layer::DamageRegion {
        use flui_layer::{DamageRegion, Region};

        let plan_region = self.plan_region;
        let mut planned = region;
        for _ in 0..Region::MAX {
            if let RegionPlan::Bounds(bounds) = plan_region(planned) {
                planned = Region::new(bounds);
            }
            let closed = close(DamageRegion::Region(planned));
            let DamageRegion::Region(current) = closed else {
                return closed;
            };
            match plan_region(current) {
                RegionPlan::Bounds(bounds) if current.rects() != [bounds] => planned = current,
                RegionPlan::Bounds(_) | RegionPlan::Rects(_) => return closed,
            }
        }
        DamageRegion::Full(flui_layer::FullReason::Unbounded)
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

    /// Plans every later partial frame rect by rect, the plan production
    /// does not switch on yet ([`RegionPlan::of`]).
    #[cfg(test)]
    pub(crate) fn plan_rect_by_rect(&mut self) {
        self.plan_region = RegionPlan::rect_by_rect;
    }

    /// Runs `plan` (not [`FramePlan::Skip`]) against `surface`, whose texture
    /// is `size` in `format`.
    ///
    /// - [`FramePlan::Direct`] renders into the surface; the retained target
    ///   does not see the frame and stops being valid.
    /// - A retained plan renders into a separate candidate. Failure or unwind
    ///   preserves the committed pixels and forces a full retry.
    /// - Only a partial frame skips the full clear: each pass of its
    ///   [`RegionPlan`] clears inside its own rect instead
    ///   (`damage::begin_partial`), keeping the retained pixels outside them.
    ///
    /// # Errors
    /// The content pass's failure.
    pub(crate) fn run(
        &mut self,
        plan: FramePlan,
        domain: &Arc<DeviceDomain>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        (surface_view, surface_texture): (&wgpu::TextureView, &wgpu::Texture),
        steps: &mut impl FrameSteps,
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
        let (retained, partial) = match plan {
            FramePlan::Skip => return Ok(()),
            FramePlan::Direct => {
                self.retained.invalidate();
                (None, None)
            }
            FramePlan::RetainedFull => (
                Some(
                    self.retained
                        .begin_encoded(domain, size, format, false, &mut encoder)?,
                ),
                None,
            ),
            FramePlan::RetainedPartial(region) => (
                Some(
                    self.retained
                        .begin_encoded(domain, size, format, true, &mut encoder)?,
                ),
                Some(region),
            ),
        };
        let (view, texture) = match retained.as_ref() {
            Some(candidate) => (candidate.view(), candidate.texture()),
            None => (surface_view, surface_texture),
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
                crate::recording_cache::CacheReads::Off
            } else {
                crate::recording_cache::CacheReads::On
            },
            &mut encoder,
        )?;
        let mut permits = Vec::new();
        if let Some(candidate) = retained.as_ref() {
            permits = candidate.submission_permits();
            permits.push(steps.0.blit(
                candidate.texture(),
                surface_texture,
                surface_view,
                &mut encoder,
            )?);
        }
        steps.0.submit(encoder, permits)?;
        if let Some(candidate) = retained {
            self.retained.commit(candidate);
        }
        self.force_full_next_frame = recorded.straddled;
        self.damage_off = false;
        self.counters = recorded.counters;
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

    /// The frame presented: the damage it covered is paid.
    pub(crate) fn presented(&mut self) {
        self.damage.reset();
    }

    /// Opens a frame that no damage producer accounted for (a scene rendered
    /// straight through `Renderer::render_scene` instead of an executor slot):
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

/// The read closure of every backdrop filter in `tree`, under its transform.
///
/// Clips and isolation are ignored: extra repaint is safe, a missing input
/// is not. Replay still admits its exact read footprint.
fn tree_backdrop_closures(tree: &flui_layer::LayerTree) -> flui_layer::Closures {
    use flui_foundation::geometry::Matrix4;
    use flui_layer::Layer;

    let mut stack = vec![(tree.root(), Matrix4::IDENTITY)];
    let mut closures = flui_layer::Closures::new();
    while let Some((id, parent)) = stack.pop() {
        let Some(node) = tree.get(id) else {
            continue;
        };
        let layer = node.layer();
        let transform = match layer {
            Layer::Transform(layer) => parent * *layer.transform(),
            Layer::Follower(_) => {
                let Some(follower) = flui_layer::resolve_follower_transform(tree, id) else {
                    continue;
                };
                parent * follower
            }
            _ => {
                let offset = layer.local_translation();
                parent * Matrix4::translation(offset.dx, offset.dy, 0.0)
            }
        };
        if let Layer::BackdropFilter(_) = layer {
            closures.push_effect(layer, &transform, Rect::ZERO);
        }
        stack.extend(node.children().iter().map(|&child| (child, transform)));
    }
    closures
}
