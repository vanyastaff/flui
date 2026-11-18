//! A chunk's IR kept past its frame, so an unchanged chunk drawn again under
//! the same device transform and ambient state replays it instead of
//! recording its commands again (ADR-0304 §9).
//!
//! A `ChunkRecording` is the slice of the segment one chunk appended: every
//! arena's new elements, with each range, offset and index made relative to
//! where the chunk began, and runs that carry no clip chain. Splicing it
//! (`DrawSegment::splice`) rebases them onto the segment as it stands,
//! attaches the clip chain, the SDF clip and the scissor the chunk is entered
//! under, and re-runs each arena's merge rule on the first element only, as
//! recording the chunk there would have. Only a chunk that leaves the
//! segment's draw order and layer depth alone, opens no clip of its own and
//! draws no external texture is kept: every element it appended then sits
//! under the clip it was entered with, which the key holds or the splice
//! writes.
//!
//! What the IR bakes from outside the chunk is in its key: the device
//! transform (`RasterKey`), the scissor and opacity it was entered under and
//! the size of the target (`AmbientKey`) and the painter's resources
//! (`ResourceEpoch`): a glyph texel or an image's atlas rectangle is valid
//! only while no slot the recording names was freed since.

use std::num::NonZeroU64;

use flui_foundation::IdMint;
use flui_foundation::geometry::{Matrix4, canonical_bits, canonical_bits_f64};
use flui_layer::{Bounds, ContentVersion};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::command_ir::{
    CachedImageDraw, DrawRun, DrawSegment, GradientRun, RecordedRun, ScissorRect, ScissorRegion,
    TessellatedBatch,
};
use crate::effects::GradientStop;
use crate::instancing::{
    ArcInstance, CircleInstance, ClippableInstance, GlyphInstance, LinearGradientInstance,
    RadialGradientInstance, RectInstance, ShadowInstance, SweepGradientInstance,
};
use crate::state_stack::ResolvedClip;
use crate::vertex::Vertex;

/// The bytes a painter's recording cache holds at a frame's end, at most.
pub(crate) const BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// Whether a frame may replay recordings from the cache. A publication whose
/// damage is `Full(DamageOff)` (`FLUI_DAMAGE=off`) records every chunk: the
/// switch is the fallback for stale pixels, the cache's among them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheReads {
    On,
    Off,
}

impl CacheReads {
    /// The reads a frame of `damage` is drawn with: off for
    /// `Full(DamageOff)`.
    pub(crate) fn for_damage(damage: &flui_layer::DamageRegion) -> Self {
        match damage {
            flui_layer::DamageRegion::Full(reason)
                if *reason == flui_layer::FullReason::DamageOff =>
            {
                Self::Off
            }
            _ => Self::On,
        }
    }
}

/// The painter's resources a recording's texels name: it advances whenever
/// a glyph slot is freed, the image cache evicts or resets its atlas, the
/// font source changes or the painter reclaims its caches, and a recording
/// keyed by an older one never replays. The target format is not among them:
/// a kept recording names no pipeline. Minted from an [`IdMint`], so it never
/// repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ResourceEpoch(NonZeroU64);

/// The device transform a chunk is recorded under, as canonical bits
/// (ADR-0098 §4): the linear part and the whole translation, so a chunk
/// moved by any amount records again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RasterKey {
    linear: [u64; 4],
    translation: [u64; 2],
}

/// A translation at or past this many device pixels is not cached: below
/// it, a coordinate's fractional part is exact in `f32`.
const TRANSLATION_LIMIT: f64 = 8_388_608.0;

impl RasterKey {
    /// The key of a chunk entered under `ctm`; `None` for a transform that
    /// is not a finite 2D affine one, or translates past the f32 lattice.
    pub(crate) fn of(ctm: &Matrix4) -> Option<Self> {
        let m = &ctm.m;
        let planar = [m[2], m[3], m[6], m[7], m[8], m[9], m[11], m[14]]
            .iter()
            .all(|&value| value == 0.0)
            && m[10] == 1.0
            && m[15] == 1.0;
        let finite = [m[0], m[1], m[4], m[5], m[12], m[13]]
            .iter()
            .all(|value| value.is_finite());
        if !planar
            || !finite
            || m[12].abs() >= TRANSLATION_LIMIT
            || m[13].abs() >= TRANSLATION_LIMIT
        {
            return None;
        }
        Some(Self {
            linear: [m[0], m[1], m[4], m[5]].map(canonical_bits_f64),
            translation: [m[12], m[13]].map(canonical_bits_f64),
        })
    }
}

/// The scissor a chunk is entered under, as far as its IR depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AmbientScissor {
    /// None, or one holding every pixel the chunk's bounds touch: no glyph
    /// was culled, so the splice writes the scissor it finds.
    Covers,
    /// One that cuts the chunk, in the chunk's pixel frame (less the whole
    /// pixels of its translation).
    Exact { x: i64, y: i64, w: u32, h: u32 },
}

/// What a chunk's IR bakes from outside it, beyond its transform: the scissor
/// and the opacity bits it is entered under, and the size of the target, which
/// a fill of the whole target (`DrawOp::Color`, `DrawOp::Paint`) records. The
/// SDF clip and the clip chain are attached at the splice, so they are not in
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AmbientKey {
    scissor: AmbientScissor,
    opacity: u32,
    viewport: (u32, u32),
}

impl AmbientKey {
    /// The ambient of a chunk with `bounds`, entered under `ctm` (already
    /// admitted by [`RasterKey::of`]) and `scissor` at `opacity`, on a target
    /// of `viewport` pixels.
    pub(crate) fn of(
        scissor: ScissorRect,
        ctm: &Matrix4,
        bounds: Bounds,
        opacity: f32,
        viewport: (u32, u32),
    ) -> Option<Self> {
        let scissor = match scissor {
            None => AmbientScissor::Covers,
            Some(rect) if covers(rect, ctm, bounds) => AmbientScissor::Covers,
            Some((x, y, w, h)) => {
                // Below `TRANSLATION_LIMIT`, so the floors fit.
                let origin_x = ctm.m[12].floor() as i64;
                let origin_y = ctm.m[13].floor() as i64;
                AmbientScissor::Exact {
                    x: x.checked_sub(origin_x)?,
                    y: y.checked_sub(origin_y)?,
                    w,
                    h,
                }
            }
        };
        Some(Self {
            scissor,
            opacity: canonical_bits(opacity),
            viewport,
        })
    }
}

/// Whether `scissor` holds every whole pixel `bounds` touches under `ctm`.
fn covers(scissor: (i64, i64, u32, u32), ctm: &Matrix4, bounds: Bounds) -> bool {
    let (left_edge, top_edge, width, height) = scissor;
    let Bounds::Finite(rect) = bounds else {
        return false;
    };
    let m = &ctm.m;
    let corners = [
        (rect.left(), rect.top()),
        (rect.right(), rect.top()),
        (rect.left(), rect.bottom()),
        (rect.right(), rect.bottom()),
    ]
    .map(|(px, py)| (m[0] * px + m[4] * py + m[12], m[1] * px + m[5] * py + m[13]));
    let left = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
    let top = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
    let right = corners
        .iter()
        .map(|c| c.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = corners
        .iter()
        .map(|c| c.1)
        .fold(f64::NEG_INFINITY, f64::max);
    if ![left, top, right, bottom].iter().all(|v| v.is_finite()) {
        return false;
    }
    // i64 to f64 is exact here: a scissor is clamped to ±2^30.
    let (scissor_left, scissor_top) = (left_edge as f64, top_edge as f64);
    scissor_left <= left.floor()
        && scissor_top <= top.floor()
        && scissor_left + f64::from(width) >= right.ceil()
        && scissor_top + f64::from(height) >= bottom.ceil()
}

/// Which recording of which chunk under which state (ADR-0304 §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RecordingKey {
    pub(crate) content: ContentVersion,
    pub(crate) raster: RasterKey,
    pub(crate) ambient: AmbientKey,
    pub(crate) epoch: ResourceEpoch,
}

/// An arena's length where a chunk began, and the extent of its last
/// element, which the chunk's first draw may have grown in place: a region's
/// or gradient run's instance count, a run's range end, a tessellated
/// batch's index count.
#[derive(Debug, Clone, Copy)]
struct Tail {
    len: usize,
    extent: Option<usize>,
}

/// Where a chunk's recording began in a segment.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SegmentMark {
    rects: usize,
    circles: usize,
    arcs: usize,
    shadows: usize,
    linear_gradients: usize,
    radial_gradients: usize,
    sweep_gradients: usize,
    glyphs: usize,
    stops: usize,
    mesh_vertices: usize,
    mesh_indices: usize,
    vertices: usize,
    indices: usize,
    tess_batches: Tail,
    glyph_scissors: Tail,
    rect_scissors: Tail,
    circle_scissors: Tail,
    arc_scissors: Tail,
    shadow_scissors: Tail,
    linear_gradient_runs: Tail,
    radial_gradient_runs: Tail,
    sweep_gradient_runs: Tail,
    cached_images: usize,
    external_images: usize,
    runs: Tail,
}

/// One chunk's slice of a segment, every range relative to where it began.
#[derive(Debug)]
pub(crate) struct ChunkRecording {
    rects: Vec<RectInstance>,
    circles: Vec<CircleInstance>,
    arcs: Vec<ArcInstance>,
    shadows: Vec<ShadowInstance>,
    linear_gradients: Vec<LinearGradientInstance>,
    radial_gradients: Vec<RadialGradientInstance>,
    sweep_gradients: Vec<SweepGradientInstance>,
    glyphs: Vec<GlyphInstance>,
    stops: Vec<GradientStop>,
    mesh_vertices: Vec<[f32; 2]>,
    mesh_indices: Vec<u32>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    tess_batches: Vec<TessellatedBatch>,
    /// The first batch grew the batch before the chunk instead of starting
    /// its own, so no run of the chunk names it.
    first_batch_grew: bool,
    glyph_scissors: Vec<ScissorRegion>,
    rect_scissors: Vec<ScissorRegion>,
    circle_scissors: Vec<ScissorRegion>,
    arc_scissors: Vec<ScissorRegion>,
    shadow_scissors: Vec<ScissorRegion>,
    linear_gradient_runs: Vec<GradientRun>,
    radial_gradient_runs: Vec<GradientRun>,
    sweep_gradient_runs: Vec<GradientRun>,
    cached_images: Vec<CachedImageDraw>,
    runs: Vec<DrawRun>,
    /// The SDF clip every instance and batch was recorded under.
    clip: ResolvedClip,
    /// The glyphs the recording placed, which a replay keeps placed.
    glyph_keys: Vec<flui_painting::glyphs::GlyphKey>,
    bytes: usize,
}

impl ChunkRecording {
    pub(crate) fn glyph_keys(&self) -> &[flui_painting::glyphs::GlyphKey] {
        &self.glyph_keys
    }
}

/// Where each arena a run's range indexes begins: the segment's lengths at a
/// mark or at a splice, which a range is made relative to or rebased by.
#[derive(Clone, Copy)]
struct ArenaBases {
    rects: usize,
    circles: usize,
    arcs: usize,
    shadows: usize,
    linear_gradients: usize,
    radial_gradients: usize,
    sweep_gradients: usize,
    tess_batches: usize,
    cached_images: usize,
    external_images: usize,
    glyphs: usize,
}

impl ArenaBases {
    /// The base of the arena `run` indexes.
    fn of(&self, run: &DrawRun) -> usize {
        match run {
            DrawRun::Rect(_) => self.rects,
            DrawRun::Circle(_) => self.circles,
            DrawRun::Arc(_) => self.arcs,
            DrawRun::Shadow(_) => self.shadows,
            DrawRun::LinearGradient(_) => self.linear_gradients,
            DrawRun::RadialGradient(_) => self.radial_gradients,
            DrawRun::SweepGradient(_) => self.sweep_gradients,
            DrawRun::Tess(_) => self.tess_batches,
            DrawRun::CachedImage(_) => self.cached_images,
            DrawRun::ExternalImage(_) => self.external_images,
            DrawRun::Glyph(_) => self.glyphs,
        }
    }
}

fn range(run: &DrawRun) -> std::ops::Range<usize> {
    match run {
        DrawRun::Rect(range)
        | DrawRun::Circle(range)
        | DrawRun::Arc(range)
        | DrawRun::Shadow(range)
        | DrawRun::LinearGradient(range)
        | DrawRun::RadialGradient(range)
        | DrawRun::SweepGradient(range)
        | DrawRun::Tess(range)
        | DrawRun::CachedImage(range)
        | DrawRun::ExternalImage(range)
        | DrawRun::Glyph(range) => range.clone(),
    }
}

fn with_range(run: &DrawRun, range: std::ops::Range<usize>) -> DrawRun {
    match run {
        DrawRun::Rect(_) => DrawRun::Rect(range),
        DrawRun::Circle(_) => DrawRun::Circle(range),
        DrawRun::Arc(_) => DrawRun::Arc(range),
        DrawRun::Shadow(_) => DrawRun::Shadow(range),
        DrawRun::LinearGradient(_) => DrawRun::LinearGradient(range),
        DrawRun::RadialGradient(_) => DrawRun::RadialGradient(range),
        DrawRun::SweepGradient(_) => DrawRun::SweepGradient(range),
        DrawRun::Tess(_) => DrawRun::Tess(range),
        DrawRun::CachedImage(_) => DrawRun::CachedImage(range),
        DrawRun::ExternalImage(_) => DrawRun::ExternalImage(range),
        DrawRun::Glyph(_) => DrawRun::Glyph(range),
    }
}

/// `range` moved down by `base`; `None` when it reaches below it.
fn relative(range: std::ops::Range<usize>, base: usize) -> Option<std::ops::Range<usize>> {
    Some(range.start.checked_sub(base)?..range.end.checked_sub(base)?)
}

/// A region tail's elements from the mark on, the growth of the element
/// before it first, each `start` relative to the instance arena's `mark`.
fn capture_regions(
    regions: &[ScissorRegion],
    tail: Tail,
    mark: usize,
) -> Option<Vec<ScissorRegion>> {
    let mark = u32::try_from(mark).ok()?;
    let fresh = regions.get(tail.len..)?;
    let mut out = Vec::with_capacity(fresh.len() + 1);
    if let (Some(count), Some(last)) = (tail.extent, tail.len.checked_sub(1)) {
        let last = regions.get(last)?;
        let count = u32::try_from(count).ok()?;
        let grown = last.count.checked_sub(count)?;
        if grown > 0 {
            out.push(ScissorRegion {
                scissor: last.scissor,
                start: last.start.checked_add(count)?.checked_sub(mark)?,
                count: grown,
            });
        }
    }
    for region in fresh {
        out.push(ScissorRegion {
            scissor: region.scissor,
            start: region.start.checked_sub(mark)?,
            count: region.count,
        });
    }
    Some(out)
}

/// [`capture_regions`] for gradient runs, whose mesh ranges are relative to
/// the mesh index arena's `mesh_mark` too.
fn capture_gradient_runs(
    runs: &[GradientRun],
    tail: Tail,
    mark: usize,
    mesh_mark: usize,
) -> Option<Vec<GradientRun>> {
    let mark = u32::try_from(mark).ok()?;
    let mesh_mark = u32::try_from(mesh_mark).ok()?;
    let fresh = runs.get(tail.len..)?;
    let mut out = Vec::with_capacity(fresh.len() + 1);
    if let (Some(count), Some(last)) = (tail.extent, tail.len.checked_sub(1)) {
        let last = runs.get(last)?;
        let count = u32::try_from(count).ok()?;
        let grown = last.count.checked_sub(count)?;
        if grown > 0 {
            out.push(GradientRun {
                scissor: last.scissor,
                blend: last.blend,
                start: last.start.checked_add(count)?.checked_sub(mark)?,
                count: grown,
                mesh: None,
            });
        }
    }
    for run in fresh {
        let mesh = match &run.mesh {
            Some(mesh) => {
                Some(mesh.start.checked_sub(mesh_mark)?..mesh.end.checked_sub(mesh_mark)?)
            }
            None => None,
        };
        out.push(GradientRun {
            scissor: run.scissor,
            blend: run.blend,
            start: run.start.checked_sub(mark)?,
            count: run.count,
            mesh,
        });
    }
    Some(out)
}

fn region_tail(regions: &[ScissorRegion]) -> Tail {
    Tail {
        len: regions.len(),
        extent: regions.last().map(|region| region.count as usize),
    }
}

fn gradient_tail(runs: &[GradientRun]) -> Tail {
    Tail {
        len: runs.len(),
        extent: runs.last().map(|run| run.count as usize),
    }
}

/// Whether two SDF clips are the same bits: the clip a recording carries
/// and the one it is replayed under.
fn same_clip(a: &ResolvedClip, b: &ResolvedClip) -> bool {
    a.kind == b.kind
        && a.rrect.map(f32::to_bits) == b.rrect.map(f32::to_bits)
        && a.device_to_local.map(f32::to_bits) == b.device_to_local.map(f32::to_bits)
}

/// The state a recording is spliced under: the scissor, SDF clip and clip
/// chain the chunk is entered with.
pub(crate) struct SpliceAmbient<'a> {
    pub(crate) scissor: ScissorRect,
    pub(crate) clip: ResolvedClip,
    pub(crate) chain: &'a crate::clip_chain::ClipChain,
}

impl DrawSegment {
    /// Where a chunk recorded from now on begins.
    pub(crate) fn mark(&self) -> SegmentMark {
        // Every arena, so a new one fails to compile until it is mirrored
        // here, in `capture_since` and in `splice`.
        let Self {
            budget: _,
            rect_batch,
            circle_batch,
            arc_batch,
            shadow_batch,
            linear_gradient_batch,
            radial_gradient_batch,
            sweep_gradient_batch,
            current_gradient_stops,
            gradient_mesh_vertices,
            gradient_mesh_indices,
            vertices,
            indices,
            tess_batches,
            glyph_batch,
            glyph_scissors,
            rect_scissors,
            circle_scissors,
            arc_scissors,
            shadow_scissors,
            linear_gradient_runs,
            radial_gradient_runs,
            sweep_gradient_runs,
            cached_images,
            external_images,
            attachment_to_root: _,
            runs,
            record_error: _,
        } = self;
        SegmentMark {
            rects: rect_batch.instances.len(),
            circles: circle_batch.instances.len(),
            arcs: arc_batch.instances.len(),
            shadows: shadow_batch.instances.len(),
            linear_gradients: linear_gradient_batch.instances.len(),
            radial_gradients: radial_gradient_batch.instances.len(),
            sweep_gradients: sweep_gradient_batch.instances.len(),
            glyphs: glyph_batch.instances.len(),
            stops: current_gradient_stops.len(),
            mesh_vertices: gradient_mesh_vertices.len(),
            mesh_indices: gradient_mesh_indices.len(),
            vertices: vertices.len(),
            indices: indices.len(),
            tess_batches: Tail {
                len: tess_batches.len(),
                extent: tess_batches.last().map(|batch| batch.index_count as usize),
            },
            glyph_scissors: region_tail(glyph_scissors),
            rect_scissors: region_tail(rect_scissors),
            circle_scissors: region_tail(circle_scissors),
            arc_scissors: region_tail(arc_scissors),
            shadow_scissors: region_tail(shadow_scissors),
            linear_gradient_runs: gradient_tail(linear_gradient_runs),
            radial_gradient_runs: gradient_tail(radial_gradient_runs),
            sweep_gradient_runs: gradient_tail(sweep_gradient_runs),
            cached_images: cached_images.len(),
            external_images: external_images.len(),
            runs: Tail {
                len: runs.len(),
                extent: runs.last().map(|run| range(&run.kind).end),
            },
        }
    }

    /// What was appended since `mark`, recorded under `clip` and placing
    /// `glyph_keys`; `None` when it cannot be replayed alone: an arena below
    /// its mark (the segment was replaced) or an external image (its
    /// allocation is leased at recording).
    pub(crate) fn capture_since(
        &self,
        mark: &SegmentMark,
        clip: ResolvedClip,
        glyph_keys: Vec<flui_painting::glyphs::GlyphKey>,
    ) -> Option<ChunkRecording> {
        let Self {
            budget: _,
            rect_batch,
            circle_batch,
            arc_batch,
            shadow_batch,
            linear_gradient_batch,
            radial_gradient_batch,
            sweep_gradient_batch,
            current_gradient_stops,
            gradient_mesh_vertices,
            gradient_mesh_indices,
            vertices,
            indices,
            tess_batches,
            glyph_batch,
            glyph_scissors,
            rect_scissors,
            circle_scissors,
            arc_scissors,
            shadow_scissors,
            linear_gradient_runs,
            radial_gradient_runs,
            sweep_gradient_runs,
            cached_images,
            external_images,
            attachment_to_root: _,
            runs,
            record_error: _,
        } = self;
        if external_images.len() != mark.external_images {
            return None;
        }
        let stops_mark = u32::try_from(mark.stops).ok()?;
        let vertex_mark = u32::try_from(mark.vertices).ok()?;
        let mesh_vertex_mark = u32::try_from(mark.mesh_vertices).ok()?;
        let index_mark = u32::try_from(mark.indices).ok()?;

        let linear_gradients = linear_gradient_batch
            .instances
            .get(mark.linear_gradients..)?
            .iter()
            .map(|&instance| {
                let mut instance = instance;
                instance.stop_offset = instance.stop_offset.checked_sub(stops_mark)?;
                Some(instance)
            })
            .collect::<Option<Vec<LinearGradientInstance>>>()?;
        let radial_gradients = radial_gradient_batch
            .instances
            .get(mark.radial_gradients..)?
            .iter()
            .map(|&instance| {
                let mut instance = instance;
                instance.stop_offset = instance.stop_offset.checked_sub(stops_mark)?;
                Some(instance)
            })
            .collect::<Option<Vec<RadialGradientInstance>>>()?;
        let sweep_gradients = sweep_gradient_batch
            .instances
            .get(mark.sweep_gradients..)?
            .iter()
            .map(|&instance| {
                let mut instance = instance;
                instance.stop_offset = instance.stop_offset.checked_sub(stops_mark)?;
                Some(instance)
            })
            .collect::<Option<Vec<SweepGradientInstance>>>()?;
        let mesh_indices = gradient_mesh_indices
            .get(mark.mesh_indices..)?
            .iter()
            .map(|index| index.checked_sub(mesh_vertex_mark))
            .collect::<Option<Vec<u32>>>()?;
        let new_indices = indices
            .get(mark.indices..)?
            .iter()
            .map(|index| index.checked_sub(vertex_mark))
            .collect::<Option<Vec<u32>>>()?;

        let mut first_batch_grew = false;
        let fresh_batches = tess_batches.get(mark.tess_batches.len..)?;
        let mut batches = Vec::with_capacity(fresh_batches.len() + 1);
        if let (Some(count), Some(last)) = (
            mark.tess_batches.extent,
            mark.tess_batches.len.checked_sub(1),
        ) {
            let last = tess_batches.get(last)?;
            let count = u32::try_from(count).ok()?;
            let grown = last.index_count.checked_sub(count)?;
            if grown > 0 {
                first_batch_grew = true;
                batches.push(TessellatedBatch {
                    index_start: last
                        .index_start
                        .checked_add(count)?
                        .checked_sub(index_mark)?,
                    index_count: grown,
                    ..last.clone()
                });
            }
        }
        for batch in fresh_batches {
            batches.push(TessellatedBatch {
                index_start: batch.index_start.checked_sub(index_mark)?,
                ..batch.clone()
            });
        }

        // Ranges index the recording's own arenas. Its first tessellated
        // batch is the growth of the batch before the mark when that grew.
        let bases = ArenaBases {
            rects: mark.rects,
            circles: mark.circles,
            arcs: mark.arcs,
            shadows: mark.shadows,
            linear_gradients: mark.linear_gradients,
            radial_gradients: mark.radial_gradients,
            sweep_gradients: mark.sweep_gradients,
            tess_batches: mark.tess_batches.len - usize::from(first_batch_grew),
            cached_images: mark.cached_images,
            external_images: mark.external_images,
            glyphs: mark.glyphs,
        };
        let fresh_runs = runs.get(mark.runs.len..)?;
        let mut captured_runs = Vec::with_capacity(fresh_runs.len() + 1);
        if let (Some(end), Some(last)) = (mark.runs.extent, mark.runs.len.checked_sub(1)) {
            let last = &runs.get(last)?.kind;
            let now = range(last);
            if now.end > end {
                let base = bases.of(last);
                captured_runs.push(with_range(last, relative(end..now.end, base)?));
            }
        }
        for run in fresh_runs {
            let base = bases.of(&run.kind);
            captured_runs.push(with_range(&run.kind, relative(range(&run.kind), base)?));
        }

        let mut recording = ChunkRecording {
            rects: rect_batch.instances.get(mark.rects..)?.to_vec(),
            circles: circle_batch.instances.get(mark.circles..)?.to_vec(),
            arcs: arc_batch.instances.get(mark.arcs..)?.to_vec(),
            shadows: shadow_batch.instances.get(mark.shadows..)?.to_vec(),
            linear_gradients,
            radial_gradients,
            sweep_gradients,
            glyphs: glyph_batch.instances.get(mark.glyphs..)?.to_vec(),
            stops: current_gradient_stops.get(mark.stops..)?.to_vec(),
            mesh_vertices: gradient_mesh_vertices.get(mark.mesh_vertices..)?.to_vec(),
            mesh_indices,
            vertices: vertices.get(mark.vertices..)?.to_vec(),
            indices: new_indices,
            tess_batches: batches,
            first_batch_grew,
            glyph_scissors: capture_regions(glyph_scissors, mark.glyph_scissors, mark.glyphs)?,
            rect_scissors: capture_regions(rect_scissors, mark.rect_scissors, mark.rects)?,
            circle_scissors: capture_regions(circle_scissors, mark.circle_scissors, mark.circles)?,
            arc_scissors: capture_regions(arc_scissors, mark.arc_scissors, mark.arcs)?,
            shadow_scissors: capture_regions(shadow_scissors, mark.shadow_scissors, mark.shadows)?,
            linear_gradient_runs: capture_gradient_runs(
                linear_gradient_runs,
                mark.linear_gradient_runs,
                mark.linear_gradients,
                mark.mesh_indices,
            )?,
            radial_gradient_runs: capture_gradient_runs(
                radial_gradient_runs,
                mark.radial_gradient_runs,
                mark.radial_gradients,
                mark.mesh_indices,
            )?,
            sweep_gradient_runs: capture_gradient_runs(
                sweep_gradient_runs,
                mark.sweep_gradient_runs,
                mark.sweep_gradients,
                mark.mesh_indices,
            )?,
            cached_images: cached_images.get(mark.cached_images..)?.to_vec(),
            runs: captured_runs,
            clip,
            glyph_keys,
            bytes: 0,
        };
        shrink_to_fit(&mut recording);
        recording.bytes = recording_bytes(&recording);
        Some(recording)
    }

    /// Appends `recording` as recording its chunk here, under `ambient`,
    /// would: every range rebased onto the arenas as they stand, the first
    /// element of each arena merged by that arena's own rule, the ambient
    /// scissor, SDF clip and clip chain attached. Each arena grows through
    /// the recording budget, as a recording's would; a refusal is recorded
    /// on it and fails the frame.
    pub(crate) fn splice(&mut self, recording: &ChunkRecording, ambient: &SpliceAmbient<'_>) {
        let Self {
            budget: _,
            rect_batch: _,
            circle_batch: _,
            arc_batch: _,
            shadow_batch: _,
            linear_gradient_batch: _,
            radial_gradient_batch: _,
            sweep_gradient_batch: _,
            current_gradient_stops: _,
            gradient_mesh_vertices: _,
            gradient_mesh_indices: _,
            vertices: _,
            indices: _,
            tess_batches: _,
            glyph_batch: _,
            glyph_scissors: _,
            rect_scissors: _,
            circle_scissors: _,
            arc_scissors: _,
            shadow_scissors: _,
            linear_gradient_runs: _,
            radial_gradient_runs: _,
            sweep_gradient_runs: _,
            cached_images: _,
            external_images: _,
            attachment_to_root: _,
            runs: _,
            record_error: _,
        } = &*self;
        let ChunkRecording {
            rects,
            circles,
            arcs,
            shadows,
            linear_gradients,
            radial_gradients,
            sweep_gradients,
            glyphs,
            stops,
            mesh_vertices,
            mesh_indices,
            vertices,
            indices,
            tess_batches,
            first_batch_grew,
            glyph_scissors,
            rect_scissors,
            circle_scissors,
            arc_scissors,
            shadow_scissors,
            linear_gradient_runs,
            radial_gradient_runs,
            sweep_gradient_runs,
            cached_images,
            runs,
            clip,
            glyph_keys: _,
            bytes: _,
        } = recording;
        if self.budget.error().is_some() {
            return;
        }
        // The u32 offsets an arena's elements hold, refused past u32 as a
        // recording refuses them.
        let mut offsets = [0_u32; 5];
        for (slot, (resource, base, added)) in offsets.iter_mut().zip([
            (
                "gradient stops",
                self.current_gradient_stops.len(),
                stops.len(),
            ),
            (
                "gradient mesh vertices",
                self.gradient_mesh_vertices.len(),
                mesh_vertices.len(),
            ),
            (
                "gradient mesh indices",
                self.gradient_mesh_indices.len(),
                mesh_indices.len(),
            ),
            ("tessellated vertices", self.vertices.len(), vertices.len()),
            ("tessellated indices", self.indices.len(), indices.len()),
        ]) {
            let total = base.saturating_add(added);
            // The base is at most the total, so a total that fits holds it.
            let (Ok(base), Ok(_)) = (u32::try_from(base), u32::try_from(total)) else {
                self.record_limit(resource, total, u32::MAX as usize);
                return;
            };
            *slot = base;
        }
        let [
            stop_base,
            mesh_vertex_base,
            mesh_index_base,
            vertex_base,
            index_base,
        ] = offsets;
        let reclip = !same_clip(clip, &ambient.clip);
        let entry = ambient.clip;
        let mut bases = ArenaBases {
            rects: self.rect_batch.instances.len(),
            circles: self.circle_batch.instances.len(),
            arcs: self.arc_batch.instances.len(),
            shadows: self.shadow_batch.instances.len(),
            linear_gradients: self.linear_gradient_batch.instances.len(),
            radial_gradients: self.radial_gradient_batch.instances.len(),
            sweep_gradients: self.sweep_gradient_batch.instances.len(),
            tess_batches: self.tess_batches.len(),
            cached_images: self.cached_images.len(),
            external_images: self.external_images.len(),
            glyphs: self.glyph_batch.instances.len(),
        };

        extend_clipped(&mut self.rect_batch.instances, rects, reclip, entry);
        extend_clipped(&mut self.circle_batch.instances, circles, reclip, entry);
        self.arc_batch.instances.extend_from_slice(arcs);
        self.shadow_batch.instances.extend_from_slice(shadows);
        self.linear_gradient_batch
            .instances
            .extend(linear_gradients.iter().map(|&instance| {
                let instance = instance.with_stop_offset(instance.stop_offset + stop_base);
                if reclip {
                    instance.with_clip(entry)
                } else {
                    instance
                }
            }));
        self.radial_gradient_batch
            .instances
            .extend(radial_gradients.iter().map(|&instance| {
                let instance = instance.with_stop_offset(instance.stop_offset + stop_base);
                if reclip {
                    instance.with_clip(entry)
                } else {
                    instance
                }
            }));
        self.sweep_gradient_batch
            .instances
            .extend(sweep_gradients.iter().map(|&instance| {
                let instance = instance.with_stop_offset(instance.stop_offset + stop_base);
                if reclip {
                    instance.with_clip(entry)
                } else {
                    instance
                }
            }));
        extend_clipped(&mut self.glyph_batch.instances, glyphs, reclip, entry);
        self.current_gradient_stops.extend_from_slice(stops);
        self.gradient_mesh_vertices.extend_from_slice(mesh_vertices);
        self.gradient_mesh_indices
            .extend(mesh_indices.iter().map(|&index| index + mesh_vertex_base));
        self.vertices.extend_from_slice(vertices);
        self.indices
            .extend(indices.iter().map(|&index| index + vertex_base));
        self.cached_images
            .extend(cached_images.iter().map(|draw| CachedImageDraw {
                scissor: ambient.scissor,
                instance: if reclip {
                    draw.instance.with_clip(entry)
                } else {
                    draw.instance
                },
                ..draw.clone()
            }));

        for (regions, captured) in [
            (&mut self.glyph_scissors, glyph_scissors),
            (&mut self.rect_scissors, rect_scissors),
            (&mut self.circle_scissors, circle_scissors),
            (&mut self.arc_scissors, arc_scissors),
            (&mut self.shadow_scissors, shadow_scissors),
        ] {
            for region in captured {
                Self::push_scissor_span(regions, ambient.scissor, region.count);
            }
        }
        for (gradient_runs, captured) in [
            (&mut self.linear_gradient_runs, linear_gradient_runs),
            (&mut self.radial_gradient_runs, radial_gradient_runs),
            (&mut self.sweep_gradient_runs, sweep_gradient_runs),
        ] {
            for run in captured {
                let mesh = run
                    .mesh
                    .as_ref()
                    .map(|mesh| mesh.start + mesh_index_base..mesh.end + mesh_index_base);
                Self::push_gradient_span(
                    gradient_runs,
                    ambient.scissor,
                    run.blend,
                    run.count,
                    mesh,
                );
            }
        }

        // Tessellated batches, then the runs. A first batch the chunk began
        // with (one that grew the batch before it, or the first of its first
        // run) may merge into the segment's last batch as it did, or did not,
        // where it was recorded; the batches after it are the chunk's own.
        let rebase_batch = |batch: &TessellatedBatch| TessellatedBatch {
            scissor: ambient.scissor,
            clip: entry,
            index_start: batch.index_start + index_base,
            ..batch.clone()
        };
        let mut captured_runs = runs.iter();
        let mut pending: Option<DrawRun> = None;
        let leads = *first_batch_grew
            || matches!(runs.first(), Some(DrawRun::Tess(range)) if range.start == 0);
        let mut later_batches = tess_batches.iter();
        if leads && let Some(first) = later_batches.next() {
            let before = self.tess_batches.len();
            self.push_tess_batch(rebase_batch(first), ambient.chain.clone());
            // A first batch the last one absorbed moves every later batch
            // down one.
            if self.tess_batches.len() == before {
                bases.tess_batches -= 1;
            }
            if !*first_batch_grew
                && let Some(DrawRun::Tess(range)) = captured_runs.next()
                && range.end > 1
            {
                pending = Some(DrawRun::Tess(1..range.end));
            }
        }
        self.tess_batches.extend(later_batches.map(rebase_batch));
        let rebase_run = |run: &DrawRun| {
            let base = bases.of(run);
            let range = range(run);
            with_range(run, range.start + base..range.end + base)
        };
        let first = pending
            .as_ref()
            .map(rebase_run)
            .or_else(|| captured_runs.next().map(rebase_run));
        if let Some(first) = first {
            self.record_run(first, ambient.chain.clone());
        }
        self.runs.extend(captured_runs.map(|run| RecordedRun {
            kind: rebase_run(run),
            clip: ambient.chain.clone(),
        }));
    }
}

/// Appends `instances`, each under `clip` when `reclip`.
fn extend_clipped<T: ClippableInstance + Copy>(
    arena: &mut crate::recording_budget::BudgetVec<T>,
    instances: &[T],
    reclip: bool,
    clip: ResolvedClip,
) {
    if reclip {
        arena.extend(instances.iter().map(|&instance| instance.with_clip(clip)));
    } else {
        arena.extend_from_slice(instances);
    }
}

/// What a recording holds, in bytes, for the cache's budget.
fn recording_bytes(recording: &ChunkRecording) -> usize {
    // By capacity, which `shrink_to_fit` brings to the length at capture.
    let ChunkRecording {
        rects,
        circles,
        arcs,
        shadows,
        linear_gradients,
        radial_gradients,
        sweep_gradients,
        glyphs,
        stops,
        mesh_vertices,
        mesh_indices,
        vertices,
        indices,
        tess_batches,
        first_batch_grew: _,
        glyph_scissors,
        rect_scissors,
        circle_scissors,
        arc_scissors,
        shadow_scissors,
        linear_gradient_runs,
        radial_gradient_runs,
        sweep_gradient_runs,
        cached_images,
        runs,
        clip: _,
        glyph_keys,
        bytes: _,
    } = recording;
    std::mem::size_of::<ChunkRecording>()
        + held(rects)
        + held(circles)
        + held(arcs)
        + held(shadows)
        + held(linear_gradients)
        + held(radial_gradients)
        + held(sweep_gradients)
        + held(glyphs)
        + held(stops)
        + held(mesh_vertices)
        + held(mesh_indices)
        + held(vertices)
        + held(indices)
        + held(tess_batches)
        + held(glyph_scissors)
        + held(rect_scissors)
        + held(circle_scissors)
        + held(arc_scissors)
        + held(shadow_scissors)
        + held(linear_gradient_runs)
        + held(radial_gradient_runs)
        + held(sweep_gradient_runs)
        + held(cached_images)
        + held(runs)
        + held(glyph_keys)
}

/// The bytes `values` holds on the heap.
fn held<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(std::mem::size_of::<T>())
}

/// Drops the spare capacity of every arena of `recording`.
fn shrink_to_fit(recording: &mut ChunkRecording) {
    let ChunkRecording {
        rects,
        circles,
        arcs,
        shadows,
        linear_gradients,
        radial_gradients,
        sweep_gradients,
        glyphs,
        stops,
        mesh_vertices,
        mesh_indices,
        vertices,
        indices,
        tess_batches,
        first_batch_grew: _,
        glyph_scissors,
        rect_scissors,
        circle_scissors,
        arc_scissors,
        shadow_scissors,
        linear_gradient_runs,
        radial_gradient_runs,
        sweep_gradient_runs,
        cached_images,
        runs,
        clip: _,
        glyph_keys,
        bytes: _,
    } = recording;
    rects.shrink_to_fit();
    circles.shrink_to_fit();
    arcs.shrink_to_fit();
    shadows.shrink_to_fit();
    linear_gradients.shrink_to_fit();
    radial_gradients.shrink_to_fit();
    sweep_gradients.shrink_to_fit();
    glyphs.shrink_to_fit();
    stops.shrink_to_fit();
    mesh_vertices.shrink_to_fit();
    mesh_indices.shrink_to_fit();
    vertices.shrink_to_fit();
    indices.shrink_to_fit();
    tess_batches.shrink_to_fit();
    glyph_scissors.shrink_to_fit();
    rect_scissors.shrink_to_fit();
    circle_scissors.shrink_to_fit();
    arc_scissors.shrink_to_fit();
    shadow_scissors.shrink_to_fit();
    linear_gradient_runs.shrink_to_fit();
    radial_gradient_runs.shrink_to_fit();
    sweep_gradient_runs.shrink_to_fit();
    cached_images.shrink_to_fit();
    runs.shrink_to_fit();
    glyph_keys.shrink_to_fit();
}

/// One kept recording and the frame it was last rastered in.
#[derive(Debug)]
struct Entry {
    key: RecordingKey,
    recording: ChunkRecording,
    last_rastered: u64,
}

/// A chunk keeps at most this many recordings (two transforms or ambients,
/// say a chunk drawn in two places), so one that moves every frame does not
/// fill the budget alone.
const PER_CONTENT: usize = 2;

/// The recordings a painter keeps, keyed by [`RecordingKey`], under
/// [`BUDGET_BYTES`] at each frame's end, the least recently rastered evicted
/// first.
#[derive(Debug)]
pub(crate) struct RecordingCache {
    entries: FxHashMap<ContentVersion, SmallVec<[Entry; PER_CONTENT]>>,
    bytes: usize,
    budget: usize,
    /// The frame now being drawn, counted from 0.
    frame: u64,
    mint: IdMint<NonZeroU64>,
    /// `None` once the mint is spent: nothing is kept after that.
    epoch: Option<ResourceEpoch>,
    /// Recordings dropped since [`Self::take_counts`] last reported.
    evictions: u64,
    /// Eviction's scratch, kept so a frame that evicts reuses its room.
    order: Vec<(u64, RecordingKey)>,
}

impl RecordingCache {
    pub(crate) fn new() -> Self {
        Self::with_budget(BUDGET_BYTES)
    }

    pub(crate) fn with_budget(budget: usize) -> Self {
        let mint = IdMint::new();
        let epoch = mint.next().ok().map(ResourceEpoch);
        Self {
            entries: FxHashMap::default(),
            bytes: 0,
            budget,
            frame: 0,
            mint,
            epoch,
            evictions: 0,
            order: Vec::new(),
        }
    }

    /// The epoch a recording made now is keyed by; `None` when nothing is
    /// kept.
    pub(crate) fn epoch(&self) -> Option<ResourceEpoch> {
        self.epoch
    }

    /// The painter's resources changed under every kept recording: they are
    /// dropped, and later ones are keyed by a new epoch.
    pub(crate) fn advance_epoch(&mut self) {
        let dropped = self
            .entries
            .values()
            .map(|entries| entries.len() as u64)
            .sum::<u64>();
        self.evictions = self.evictions.saturating_add(dropped);
        self.entries.clear();
        self.bytes = 0;
        self.epoch = self.mint.next().ok().map(ResourceEpoch);
    }

    /// The recording `key` names, stamped as rastered this frame.
    pub(crate) fn get(&mut self, key: &RecordingKey) -> Option<&ChunkRecording> {
        let frame = self.frame;
        let entry = self
            .entries
            .get_mut(&key.content)?
            .iter_mut()
            .find(|entry| entry.key == *key)?;
        entry.last_rastered = frame;
        Some(&entry.recording)
    }

    /// Drops the recording `key` names.
    pub(crate) fn remove(&mut self, key: &RecordingKey) {
        let Some(entries) = self.entries.get_mut(&key.content) else {
            return;
        };
        if let Some(index) = entries.iter().position(|entry| entry.key == *key) {
            let entry = entries.remove(index);
            self.bytes = self.bytes.saturating_sub(entry.recording.bytes);
            self.evictions = self.evictions.saturating_add(1);
        }
        if entries.is_empty() {
            self.entries.remove(&key.content);
        }
    }

    /// Keeps `recording` under `key`, a recording that completed; one keyed
    /// by an epoch that has passed is dropped. A chunk with
    /// [`PER_CONTENT`] recordings already gives up its least recently
    /// rastered one.
    pub(crate) fn insert(&mut self, key: RecordingKey, recording: ChunkRecording) {
        if self.epoch != Some(key.epoch) {
            return;
        }
        self.remove(&key);
        let entries = self.entries.entry(key.content).or_default();
        if entries.len() >= PER_CONTENT
            && let Some(oldest) = entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_rastered)
                .map(|(index, _)| index)
        {
            let entry = entries.remove(oldest);
            self.bytes = self.bytes.saturating_sub(entry.recording.bytes);
            self.evictions = self.evictions.saturating_add(1);
        }
        self.bytes = self.bytes.saturating_add(recording.bytes);
        entries.push(Entry {
            key,
            recording,
            last_rastered: self.frame,
        });
    }

    /// Evicts the least recently rastered recordings until the cache is
    /// within its budget.
    pub(crate) fn settle(&mut self) {
        self.evict_over_budget();
    }

    /// How many recordings left the cache since this was last asked, which
    /// starts the count again, and the bytes it holds now. Asked once a frame
    /// is submitted, so a frame that fails leaves its evictions to the next.
    pub(crate) fn take_counts(&mut self) -> (u64, u64) {
        let evictions = std::mem::take(&mut self.evictions);
        (evictions, u64::try_from(self.bytes).unwrap_or(u64::MAX))
    }

    /// Evicts over the budget and starts the next frame.
    pub(crate) fn end_frame(&mut self) {
        self.evict_over_budget();
        self.frame = self.frame.saturating_add(1);
    }

    fn evict_over_budget(&mut self) {
        if self.bytes <= self.budget {
            return;
        }
        let mut order = std::mem::take(&mut self.order);
        order.clear();
        order.extend(
            self.entries
                .values()
                .flatten()
                .map(|entry| (entry.last_rastered, entry.key)),
        );
        order.sort_unstable_by_key(|&(last_rastered, _)| last_rastered);
        for (_, key) in &order {
            if self.bytes <= self.budget {
                break;
            }
            self.remove(key);
        }
        order.clear();
        self.order = order;
    }
}

#[cfg(test)]
impl DrawSegment {
    /// The segment's IR as bytes, for a test that compares two recordings:
    /// the instance arenas as they are uploaded, the other elements by value,
    /// clip chains by their links' values.
    pub(crate) fn ir_bytes(&self, out: &mut Vec<u8>) {
        let Self {
            budget: _,
            rect_batch,
            circle_batch,
            arc_batch,
            shadow_batch,
            linear_gradient_batch,
            radial_gradient_batch,
            sweep_gradient_batch,
            current_gradient_stops,
            gradient_mesh_vertices,
            gradient_mesh_indices,
            vertices,
            indices,
            tess_batches,
            glyph_batch,
            glyph_scissors,
            rect_scissors,
            circle_scissors,
            arc_scissors,
            shadow_scissors,
            linear_gradient_runs,
            radial_gradient_runs,
            sweep_gradient_runs,
            cached_images,
            external_images,
            attachment_to_root,
            runs,
            record_error,
        } = self;
        for bytes in [
            bytemuck::cast_slice::<RectInstance, u8>(&rect_batch.instances),
            bytemuck::cast_slice(&circle_batch.instances),
            bytemuck::cast_slice(&arc_batch.instances),
            bytemuck::cast_slice(&shadow_batch.instances),
            bytemuck::cast_slice(&linear_gradient_batch.instances),
            bytemuck::cast_slice(&radial_gradient_batch.instances),
            bytemuck::cast_slice(&sweep_gradient_batch.instances),
            bytemuck::cast_slice(&glyph_batch.instances),
            bytemuck::cast_slice(current_gradient_stops),
            bytemuck::cast_slice(gradient_mesh_vertices),
            bytemuck::cast_slice(gradient_mesh_indices),
            bytemuck::cast_slice(vertices),
            bytemuck::cast_slice(indices),
        ] {
            out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(bytes);
        }
        // By their elements: what a recording charged its budget follows
        // the arenas' growth, which is not the IR.
        let text = format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{}{attachment_to_root:?}{record_error:?}",
            &tess_batches[..],
            &glyph_scissors[..],
            &rect_scissors[..],
            &circle_scissors[..],
            &arc_scissors[..],
            &shadow_scissors[..],
            &linear_gradient_runs[..],
            &radial_gradient_runs[..],
            &sweep_gradient_runs[..],
            &cached_images[..],
            external_images.len(),
        );
        out.extend_from_slice(text.as_bytes());
        for run in runs {
            out.extend_from_slice(format!("{:?}{}", run.kind, run.clip.describe()).as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::{Matrix4, Rect};
    use flui_layer::{Bounds, ContentVersion, SceneOwner};

    use super::{
        AmbientKey, AmbientScissor, ChunkRecording, RasterKey, RecordingCache, RecordingKey,
    };
    use crate::state_stack::ResolvedClip;

    fn translated(x: f64, y: f64) -> Matrix4 {
        Matrix4::translation(x, y, 0.0)
    }

    fn versions(count: usize) -> Vec<ContentVersion> {
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        (0..count)
            .map(|_| writer.next_content_version().expect("a content version"))
            .collect()
    }

    /// A recording that draws nothing and charges `bytes`.
    fn recording(bytes: usize) -> ChunkRecording {
        ChunkRecording {
            rects: Vec::new(),
            circles: Vec::new(),
            arcs: Vec::new(),
            shadows: Vec::new(),
            linear_gradients: Vec::new(),
            radial_gradients: Vec::new(),
            sweep_gradients: Vec::new(),
            glyphs: Vec::new(),
            stops: Vec::new(),
            mesh_vertices: Vec::new(),
            mesh_indices: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            tess_batches: Vec::new(),
            first_batch_grew: false,
            glyph_scissors: Vec::new(),
            rect_scissors: Vec::new(),
            circle_scissors: Vec::new(),
            arc_scissors: Vec::new(),
            shadow_scissors: Vec::new(),
            linear_gradient_runs: Vec::new(),
            radial_gradient_runs: Vec::new(),
            sweep_gradient_runs: Vec::new(),
            cached_images: Vec::new(),
            runs: Vec::new(),
            clip: ResolvedClip::NONE,
            glyph_keys: Vec::new(),
            bytes,
        }
    }

    /// The key of `content` drawn at `x` under `cache`'s epoch.
    fn key(cache: &RecordingCache, content: ContentVersion, x: f64) -> RecordingKey {
        let ctm = translated(x, 0.0);
        RecordingKey {
            content,
            raster: RasterKey::of(&ctm).expect("a translation is kept"),
            ambient: AmbientKey::of(None, &ctm, Bounds::Unbounded, 1.0, (64, 64))
                .expect("no scissor is kept"),
            epoch: cache.epoch().expect("a fresh cache has an epoch"),
        }
    }

    /// A transform is kept when it is a finite planar affine one whose
    /// translation stays below 2^23 pixels; its whole translation is in the
    /// key.
    #[test]
    fn a_raster_key_holds_planar_affine_transforms_in_range() {
        let mut projective = Matrix4::IDENTITY;
        projective.m[3] = 0.001;
        let mut depth = Matrix4::IDENTITY;
        depth.m[14] = 1.0;
        let mut infinite = Matrix4::IDENTITY;
        infinite.m[0] = f64::INFINITY;
        let mut sheared = Matrix4::IDENTITY;
        sheared.m[4] = 0.5;
        for (row, ctm, kept) in [
            ("identity", Matrix4::IDENTITY, true),
            ("a fractional translation", translated(3.25, -7.5), true),
            ("a shear", sheared, true),
            ("a projective row", projective, false),
            ("a depth translation", depth, false),
            ("a non-finite scale", infinite, false),
            (
                "a translation at the limit",
                translated(8_388_608.0, 0.0),
                false,
            ),
            (
                "a translation below the limit",
                translated(8_388_607.0, 0.0),
                true,
            ),
        ] {
            assert_eq!(RasterKey::of(&ctm).is_some(), kept, "{row}");
        }
        assert_ne!(
            RasterKey::of(&translated(1.0, 0.0)),
            RasterKey::of(&translated(2.0, 0.0)),
            "a whole-pixel move is another key"
        );
    }

    /// The scissor is keyed as `Covers` when it holds every pixel the chunk
    /// touches, and otherwise exactly, relative to the whole pixels of the
    /// translation; opacity and the target's size are keyed as they are.
    #[test]
    fn an_ambient_key_holds_the_scissor_that_cuts_the_chunk() {
        let bounds = Bounds::Finite(Rect::from_xywh(0.0, 0.0, 10.0, 10.0));
        let at = |x: f64, scissor, viewport| {
            AmbientKey::of(scissor, &translated(x, 0.0), bounds, 1.0, viewport)
                .expect("an affine transform in range")
        };
        assert_eq!(at(5.0, None, (64, 64)).scissor, AmbientScissor::Covers);
        assert_eq!(
            at(5.0, Some((0, 0, 64, 64)), (64, 64)).scissor,
            AmbientScissor::Covers,
            "a scissor around the chunk"
        );
        assert_eq!(
            at(5.5, Some((0, 0, 15, 64)), (64, 64)).scissor,
            AmbientScissor::Exact {
                x: -5,
                y: 0,
                w: 15,
                h: 64
            },
            "the chunk's last pixel column, 15, is outside"
        );
        assert_eq!(
            at(5.5, Some((0, 0, 16, 64)), (64, 64)).scissor,
            AmbientScissor::Covers,
            "the scissor holds the pixel the chunk's edge shares"
        );
        assert_eq!(
            at(5.0, Some((2, 0, 4, 64)), (64, 64)),
            at(7.0, Some((4, 0, 4, 64)), (64, 64)),
            "a scissor moved with the chunk by whole pixels"
        );
        assert_ne!(
            AmbientKey::of(
                Some((0, 0, 64, 64)),
                &Matrix4::IDENTITY,
                Bounds::Unbounded,
                1.0,
                (64, 64)
            ),
            AmbientKey::of(None, &Matrix4::IDENTITY, Bounds::Unbounded, 1.0, (64, 64)),
            "an unbounded chunk under a scissor is cut by it"
        );
        assert_ne!(at(5.0, None, (64, 64)), at(5.0, None, (32, 64)), "size");
        assert_ne!(
            AmbientKey::of(None, &Matrix4::IDENTITY, bounds, 0.5, (64, 64)),
            AmbientKey::of(None, &Matrix4::IDENTITY, bounds, 1.0, (64, 64)),
            "opacity"
        );
    }

    /// Over its budget the cache evicts the least recently rastered
    /// recordings first, and its counts report each eviction once with the
    /// bytes it holds.
    #[test]
    fn the_cache_evicts_the_least_recently_rastered_over_its_budget() {
        let mut cache = RecordingCache::with_budget(250);
        let [a, b, c] = versions(3)[..] else {
            unreachable!("BUG: three versions were minted")
        };
        let (a, b, c) = (
            key(&cache, a, 0.0),
            key(&cache, b, 0.0),
            key(&cache, c, 0.0),
        );
        cache.insert(a, recording(100));
        cache.insert(b, recording(100));
        cache.end_frame();
        assert!(cache.get(&a).is_some(), "a is rastered in the next frame");
        cache.insert(c, recording(100));
        assert_eq!(
            cache.take_counts(),
            (0, 300),
            "over budget until it settles"
        );
        cache.settle();
        assert!(cache.get(&b).is_none(), "b was rastered least recently");
        assert!(cache.get(&a).is_some() && cache.get(&c).is_some());
        assert_eq!(cache.take_counts(), (1, 200));
        assert_eq!(
            cache.take_counts(),
            (0, 200),
            "an eviction is reported once"
        );
    }

    /// A chunk keeps two recordings: a third gives up its least recently
    /// rastered one.
    #[test]
    fn a_chunk_keeps_two_recordings() {
        let mut cache = RecordingCache::new();
        let content = versions(1)[0];
        let [first, second, third] = [0.0, 1.0, 2.0].map(|x| key(&cache, content, x));
        cache.insert(first, recording(10));
        cache.insert(second, recording(10));
        cache.end_frame();
        assert!(cache.get(&first).is_some(), "first is rastered again");
        cache.insert(third, recording(10));
        assert!(
            cache.get(&second).is_none(),
            "second was rastered least recently"
        );
        assert!(cache.get(&first).is_some() && cache.get(&third).is_some());
        assert_eq!(cache.take_counts(), (1, 20));
    }

    /// A new epoch drops every recording and counts each, and a recording
    /// keyed by the old epoch is not kept.
    #[test]
    fn a_new_epoch_drops_every_recording() {
        let mut cache = RecordingCache::new();
        let [a, b] = versions(2)[..] else {
            unreachable!("BUG: two versions were minted")
        };
        let (a, b) = (key(&cache, a, 0.0), key(&cache, b, 0.0));
        cache.insert(a, recording(10));
        cache.insert(b, recording(10));
        cache.advance_epoch();
        assert_eq!(cache.take_counts(), (2, 0));
        cache.insert(a, recording(10));
        assert!(cache.get(&a).is_none(), "a stale epoch's recording");
        let fresh = key(&cache, a.content, 0.0);
        cache.insert(fresh, recording(10));
        assert!(cache.get(&fresh).is_some());
    }
}
