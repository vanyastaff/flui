//! A chunk's IR kept past its frame, so an unchanged chunk drawn again under
//! the same device transform replays it instead of recording its commands
//! again (ADR-0304 §9).
//!
//! A `ChunkRecording` is the slice of the segment one chunk appended: every
//! arena's new elements, with each range, offset and index made relative to
//! where the chunk began, runs that carry no clip chain, and every glyph the
//! chunk placed, none culled by the scissor it was captured under. Splicing
//! it (`DrawSegment::splice`) rebases them onto the segment as it stands,
//! applies the state the chunk is entered under (ADR-0306 §2): each region
//! takes the entry scissor, the entry SDF clip fills the instances, the runs
//! take the entry clip chain, and the glyphs wholly outside the entry
//! scissor are left out, as recording the chunk there would leave them out.
//! It re-runs each arena's merge rule on its first element, and on the run
//! and tessellated batch after one the cull emptied. Only a chunk that
//! leaves the segment's draw order and layer depth alone, opens no clip of
//! its own and draws no external texture is kept: every element it appended
//! then sits under the entry's scissor and clip alone.
//!
//! What the IR bakes from outside the chunk is in its key: the device
//! transform (`RasterKey`), the opacity it was entered under and the size of
//! the target (`AmbientKey`) and the painter's resources (`ResourceEpoch`): a
//! glyph texel or an image's atlas rectangle is valid only while no slot the
//! recording names was freed since. The scissor and clip are not, so one
//! recording serves a chunk scrolled under a fixed clip and a chunk drawn
//! under two clips.

pub(crate) mod admission;

pub(crate) use admission::Admission;

use std::num::NonZeroU64;

use flui_foundation::IdMint;
use flui_foundation::geometry::{Matrix4, canonical_bits, canonical_bits_f64};
use flui_layer::{ContentVersion, Lineage};
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
use crate::pixel_shift::{GridExtent, PixelOrigin, PixelShift, Shift, split_translation};
use crate::recency::{Place, RecencyList};
use crate::state_stack::ResolvedClip;
use crate::tallied_map::TalliedMap;
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

/// The device transform a chunk is recorded under, as far as its IR depends
/// on it, in canonical bits (ADR-0098 §4): the linear part and the fraction
/// of the translation. The whole pixels of the translation are the chunk's
/// [`PixelOrigin`], which a replay shifts its IR by. A chunk moved by whole
/// device pixels keeps its key when the fraction keeps its bits, which it
/// does for a translation on a dyadic grid (whole pixels, or a quarter
/// pixel); a non-dyadic fraction can round to other bits once the
/// translation crosses a power of two, and that move records again, as one
/// by a fraction of a pixel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RasterKey {
    linear: [u64; 4],
    fraction: [u64; 2],
}

/// A translation at or past this many device pixels is not cached: below
/// it, a coordinate's fractional part is exact in `f32`.
const TRANSLATION_LIMIT: f64 = 8_388_608.0;

impl RasterKey {
    /// The key of a chunk entered under `ctm` and the origin it is drawn at;
    /// `None` for a transform that is not a finite 2D affine one, or
    /// translates past the f32 lattice.
    pub(crate) fn of(ctm: &Matrix4) -> Option<(Self, PixelOrigin)> {
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
        let key = Self {
            linear: [m[0], m[1], m[4], m[5]].map(canonical_bits_f64),
            fraction: [m[12], m[13]].map(|t| canonical_bits_f64(split_translation(t).1)),
        };
        Some((key, PixelOrigin::of_bounded(m[12], m[13])))
    }
}

/// What a chunk's IR bakes from outside it, beyond its transform: the opacity
/// bits it is entered under and the size of the target, which a fill of the
/// whole target (`DrawOp::Color`, `DrawOp::Paint`) records. The scissor, the
/// SDF clip and the clip chain it is entered under are not in it: a chunk is
/// recorded in its own pixel frame, culled by none of them, and the splice
/// applies the entry's (ADR-0306 §2), as Flutter's raster cache keys an entry
/// on its matrix and picture and the parent clip layer applies when the entry
/// is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AmbientKey {
    opacity: u32,
    viewport: (u32, u32),
}

impl AmbientKey {
    /// The ambient of a chunk entered at `opacity` on a target of `viewport`
    /// pixels.
    pub(crate) fn of(opacity: f32, viewport: (u32, u32)) -> Self {
        Self {
            opacity: canonical_bits(opacity),
            viewport,
        }
    }
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
    /// The whole device pixels the chunk was recorded at, which every device
    /// position it holds is relative to.
    origin: PixelOrigin,
    /// The glyphs the recording placed, which a replay keeps placed.
    glyph_keys: Vec<flui_painting::glyphs::GlyphKey>,
    /// What the glyphs it holds on the device grid cover, which a splice
    /// tests against the entry scissor before it tests each glyph; `None`
    /// when it holds none.
    glyph_extent: Option<GridExtent>,
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

/// Each of `items` through `rebase`, after `first` when there is one, in a
/// vector of exactly their number: a recording holds no spare room, and an
/// arena the chunk left alone allocates nothing. `None` when one does not
/// rebase.
fn rebased<T, U>(
    first: Option<U>,
    items: &[T],
    rebase: impl Fn(&T) -> Option<U>,
) -> Option<Vec<U>> {
    let mut out = Vec::with_capacity(items.len() + usize::from(first.is_some()));
    out.extend(first);
    for item in items {
        out.push(rebase(item)?);
    }
    Some(out)
}

/// What the chunk's first draw did to the last element before the mark.
#[derive(Clone, Copy)]
enum Growth {
    /// Left it as it was, or there was none.
    None,
    /// Added `count` elements to it, the first at `start`.
    Grew { start: u32, count: u32 },
}

/// What the chunk did to `last`, the start and count of the last element
/// before the mark, which held `extent` there; `None` when it shrank or the
/// sum overflows.
fn growth(last: Option<(u32, u32)>, extent: Option<usize>) -> Option<Growth> {
    let (Some((start, count)), Some(extent)) = (last, extent) else {
        return Some(Growth::None);
    };
    let extent = u32::try_from(extent).ok()?;
    let grown = count.checked_sub(extent)?;
    if grown == 0 {
        return Some(Growth::None);
    }
    Some(Growth::Grew {
        start: start.checked_add(extent)?,
        count: grown,
    })
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
    let last = match tail.len.checked_sub(1) {
        Some(last) => Some(regions.get(last)?),
        None => None,
    };
    let grown = match growth(last.map(|last| (last.start, last.count)), tail.extent)? {
        Growth::Grew { start, count } => Some(ScissorRegion {
            scissor: last?.scissor,
            start: start.checked_sub(mark)?,
            count,
        }),
        Growth::None => None,
    };
    rebased(grown, fresh, |region| {
        Some(ScissorRegion {
            scissor: region.scissor,
            start: region.start.checked_sub(mark)?,
            count: region.count,
        })
    })
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
    let last = match tail.len.checked_sub(1) {
        Some(last) => Some(runs.get(last)?),
        None => None,
    };
    let grown = match growth(last.map(|last| (last.start, last.count)), tail.extent)? {
        Growth::Grew { start, count } => {
            let last = last?;
            Some(GradientRun {
                scissor: last.scissor,
                blend: last.blend,
                start: start.checked_sub(mark)?,
                count,
                mesh: None,
            })
        }
        Growth::None => None,
    };
    rebased(grown, fresh, |run| {
        let mesh = match &run.mesh {
            Some(mesh) => {
                Some(mesh.start.checked_sub(mesh_mark)?..mesh.end.checked_sub(mesh_mark)?)
            }
            None => None,
        };
        Some(GradientRun {
            scissor: run.scissor,
            blend: run.blend,
            start: run.start.checked_sub(mark)?,
            count: run.count,
            mesh,
        })
    })
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
    /// The whole device pixels the chunk is entered at.
    pub(crate) origin: PixelOrigin,
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

    /// What was appended since `mark`, recorded under `clip` at `origin` and
    /// placing `glyph_keys`; `None` when it cannot be replayed alone: an arena
    /// below its mark (the segment was replaced) or an external image (its
    /// allocation is leased at recording).
    pub(crate) fn capture_since(
        &self,
        mark: &SegmentMark,
        clip: ResolvedClip,
        origin: PixelOrigin,
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

        let linear_gradients = rebased(
            None,
            linear_gradient_batch
                .instances
                .get(mark.linear_gradients..)?,
            |&instance: &LinearGradientInstance| {
                let mut instance = instance;
                instance.stop_offset = instance.stop_offset.checked_sub(stops_mark)?;
                Some(instance)
            },
        )?;
        let radial_gradients = rebased(
            None,
            radial_gradient_batch
                .instances
                .get(mark.radial_gradients..)?,
            |&instance: &RadialGradientInstance| {
                let mut instance = instance;
                instance.stop_offset = instance.stop_offset.checked_sub(stops_mark)?;
                Some(instance)
            },
        )?;
        let sweep_gradients = rebased(
            None,
            sweep_gradient_batch.instances.get(mark.sweep_gradients..)?,
            |&instance: &SweepGradientInstance| {
                let mut instance = instance;
                instance.stop_offset = instance.stop_offset.checked_sub(stops_mark)?;
                Some(instance)
            },
        )?;
        let mesh_indices = rebased(
            None,
            gradient_mesh_indices.get(mark.mesh_indices..)?,
            |index: &u32| index.checked_sub(mesh_vertex_mark),
        )?;
        let new_indices = rebased(None, indices.get(mark.indices..)?, |index: &u32| {
            index.checked_sub(vertex_mark)
        })?;

        let fresh_batches = tess_batches.get(mark.tess_batches.len..)?;
        let last_batch = match mark.tess_batches.len.checked_sub(1) {
            Some(last) => Some(tess_batches.get(last)?),
            None => None,
        };
        let grown_batch = match growth(
            last_batch.map(|last| (last.index_start, last.index_count)),
            mark.tess_batches.extent,
        )? {
            Growth::Grew { start, count } => Some(TessellatedBatch {
                index_start: start.checked_sub(index_mark)?,
                index_count: count,
                ..last_batch?.clone()
            }),
            Growth::None => None,
        };
        let first_batch_grew = grown_batch.is_some();
        let batches = rebased(grown_batch, fresh_batches, |batch: &TessellatedBatch| {
            Some(TessellatedBatch {
                index_start: batch.index_start.checked_sub(index_mark)?,
                ..batch.clone()
            })
        })?;

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
        let grown_run = match (mark.runs.extent, mark.runs.len.checked_sub(1)) {
            (Some(end), Some(last)) => {
                let last = &runs.get(last)?.kind;
                let now = range(last);
                if now.end > end {
                    Some(with_range(last, relative(end..now.end, bases.of(last))?))
                } else {
                    None
                }
            }
            _ => None,
        };
        let captured_runs = rebased(grown_run, fresh_runs, |run: &RecordedRun| {
            Some(with_range(
                &run.kind,
                relative(range(&run.kind), bases.of(&run.kind))?,
            ))
        })?;

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
            origin,
            glyph_keys,
            glyph_extent: None,
            bytes: 0,
        };
        recording.glyph_extent = grid_extent(&recording.glyphs);
        shrink_to_fit(&mut recording);
        recording.bytes = recording_bytes(&recording);
        Some(recording)
    }

    /// Appends `recording` as recording its chunk here, under `ambient`,
    /// would: every range rebased onto the arenas as they stand, every device
    /// position moved by the whole pixels between the origin it was recorded
    /// at and the one it is entered at, the first element of each arena
    /// merged by that arena's own rule, the ambient scissor, SDF clip and clip
    /// chain attached, and the glyphs outside the ambient scissor left out as
    /// recording culls them. Each arena grows through the recording budget, as a
    /// recording's would; a refusal is recorded on it and fails the frame.
    /// `scratch` is room the cull reuses from splice to splice.
    pub(crate) fn splice(
        &mut self,
        recording: &ChunkRecording,
        ambient: &SpliceAmbient<'_>,
        scratch: &mut Vec<usize>,
    ) {
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
            origin,
            glyph_keys: _,
            glyph_extent,
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
        let bases = ArenaBases {
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

        // Every device position moves from where the chunk was recorded to
        // where it is entered; scissors and clips are the entry's already.
        let shift = origin.shift_to(ambient.origin);
        extend_clipped(&mut self.rect_batch.instances, rects, shift, reclip, entry);
        extend_clipped(
            &mut self.circle_batch.instances,
            circles,
            shift,
            reclip,
            entry,
        );
        extend_moved(&mut self.arc_batch.instances, arcs, shift);
        extend_moved(&mut self.shadow_batch.instances, shadows, shift);
        self.linear_gradient_batch
            .instances
            .extend(linear_gradients.iter().map(|&instance| {
                let instance =
                    moved(instance, shift).with_stop_offset(instance.stop_offset + stop_base);
                if reclip {
                    instance.with_clip(entry)
                } else {
                    instance
                }
            }));
        self.radial_gradient_batch
            .instances
            .extend(radial_gradients.iter().map(|&instance| {
                let instance =
                    moved(instance, shift).with_stop_offset(instance.stop_offset + stop_base);
                if reclip {
                    instance.with_clip(entry)
                } else {
                    instance
                }
            }));
        self.sweep_gradient_batch
            .instances
            .extend(sweep_gradients.iter().map(|&instance| {
                let instance =
                    moved(instance, shift).with_stop_offset(instance.stop_offset + stop_base);
                if reclip {
                    instance.with_clip(entry)
                } else {
                    instance
                }
            }));
        // A recording holds every glyph its chunk placed; the ones wholly
        // outside the entry scissor are left out here, as recording the
        // chunk here would have left them out. A scissor that holds every
        // glyph on the grid leaves out none, and costs no pass over them.
        let kept = match (ambient.scissor, glyph_extent) {
            (Some(scissor), Some(extent))
                if !holds(scissor, moved(*extent, shift))
                    && kept_glyphs(glyphs, shift, scissor, scratch) =>
            {
                Some(scratch.as_slice())
            }
            _ => None,
        };
        match &kept {
            None => extend_clipped(
                &mut self.glyph_batch.instances,
                glyphs,
                shift,
                reclip,
                entry,
            ),
            Some(kept) => self.glyph_batch.instances.extend(
                glyphs
                    .iter()
                    .zip(kept.windows(2))
                    .filter(|(_, counts)| counts[1] > counts[0])
                    .map(|(&glyph, _)| {
                        let glyph = moved(glyph, shift);
                        if reclip {
                            glyph.with_clip(entry)
                        } else {
                            glyph
                        }
                    }),
            ),
        }
        self.current_gradient_stops.extend_from_slice(stops);
        // Unit-square coordinates, which the instance places.
        self.gradient_mesh_vertices.extend_from_slice(mesh_vertices);
        self.gradient_mesh_indices
            .extend(mesh_indices.iter().map(|&index| index + mesh_vertex_base));
        extend_moved(&mut self.vertices, vertices, shift);
        self.indices
            .extend(indices.iter().map(|&index| index + vertex_base));
        self.cached_images
            .extend(cached_images.iter().map(|draw| CachedImageDraw {
                scissor: ambient.scissor,
                instance: if reclip {
                    moved(draw.instance, shift).with_clip(entry)
                } else {
                    moved(draw.instance, shift)
                },
                ..draw.clone()
            }));

        // A chunk kept opens no scissor of its own, so each of its regions
        // is the entry scissor intersected with none: the entry scissor.
        for region in glyph_scissors {
            let count = match &kept {
                None => region.count,
                Some(kept) => {
                    let start = region.start as usize;
                    kept_count(kept, start..start + region.count as usize)
                }
            };
            if count > 0 {
                Self::push_scissor_span(&mut self.glyph_scissors, ambient.scissor, count);
            }
        }
        for (regions, captured) in [
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
        // where it was recorded. The batches after it are appended as the
        // runs that name them come up, so that the first batch of a run after
        // one the cull emptied meets the merge rule as it would recorded here.
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
        // How many of the recording's batches are appended, and how many of
        // those a batch before them absorbed, each moving every later batch
        // down one.
        let mut appended = 0;
        let mut absorbed = 0;
        if leads && let Some(first) = tess_batches.first() {
            if !self.push_tess_batch_apart(rebase_batch(first), ambient.chain.clone()) {
                absorbed += 1;
            }
            appended = 1;
            if !*first_batch_grew
                && let Some(DrawRun::Tess(range)) = captured_runs.next()
                && range.end > 1
            {
                pending = Some(DrawRun::Tess(1..range.end));
            }
        }
        let first = match pending.as_ref() {
            Some(pending) => Some(pending),
            None => captured_runs.next(),
        };
        // The first run, and a run after one the cull emptied, merge by the
        // recording rule, as recorded here they would; the runs between were
        // recorded apart and stay apart.
        let mut merges = true;
        let mut after_cull = false;
        for run in first.into_iter().chain(captured_runs) {
            let mut run = run.clone();
            if let DrawRun::Tess(range) = &mut run {
                let end = range.end.min(tess_batches.len());
                if after_cull && range.start == appended && appended < end {
                    if !self.push_tess_batch_apart(
                        rebase_batch(&tess_batches[appended]),
                        ambient.chain.clone(),
                    ) {
                        absorbed += 1;
                    }
                    appended += 1;
                    range.start += 1;
                    // The run its batch recorded is the one the rest of the
                    // range merges into.
                    merges = range.start < range.end;
                    after_cull = false;
                }
                if appended < end {
                    self.tess_batches
                        .extend(tess_batches[appended..end].iter().map(rebase_batch));
                    appended = end;
                }
            }
            match rebased_run(&run, bases, absorbed, kept) {
                None if matches!(run, DrawRun::Glyph(_)) => {
                    merges = true;
                    after_cull = true;
                }
                None => {}
                Some(kind) if merges => {
                    self.record_run(kind, ambient.chain.clone());
                    merges = false;
                    after_cull = false;
                }
                Some(kind) => {
                    after_cull = false;
                    self.runs.push(RecordedRun {
                        kind,
                        clip: ambient.chain.clone(),
                    });
                }
            }
        }
        if appended < tess_batches.len() {
            self.tess_batches
                .extend(tess_batches[appended..].iter().map(rebase_batch));
        }
    }
}

/// `run` rebased by `bases`, a tessellated range moved down by the
/// `absorbed` batches before it; a glyph range takes the glyphs `kept` keeps
/// of it, and one that keeps none leaves no run (`None`), as does an empty
/// range.
fn rebased_run(
    run: &DrawRun,
    bases: ArenaBases,
    absorbed: usize,
    kept: Option<&[usize]>,
) -> Option<DrawRun> {
    let range = match (kept, run) {
        (Some(kept), DrawRun::Glyph(range)) => kept[range.start]..kept[range.end],
        _ => range(run),
    };
    if range.is_empty() {
        return None;
    }
    // An absorbed batch merged into one at a lower index, so the batches
    // after it sit no lower than the base less the absorbed: no underflow.
    let base = bases.of(run);
    let (start, end) = match run {
        DrawRun::Tess(_) => (range.start + base - absorbed, range.end + base - absorbed),
        _ => (range.start + base, range.end + base),
    };
    Some(with_range(run, start..end))
}

impl DrawSegment {
    /// Pushes `batch` by the recording rule (`push_tess_batch`) and answers
    /// whether it stands apart: `false` when the segment's last batch
    /// absorbed it.
    fn push_tess_batch_apart(
        &mut self,
        batch: TessellatedBatch,
        chain: crate::clip_chain::ClipChain,
    ) -> bool {
        let before = self.tess_batches.len();
        self.push_tess_batch(batch, chain);
        self.tess_batches.len() != before
    }
}

/// Which of `glyphs`, each moved by `shift`, a recording here would have
/// kept under `scissor`, written into `kept`: for each index `i`, how many
/// glyphs before `i` are kept, so `kept[i + 1] > kept[i]` keeps glyph `i` and
/// a range `a..b` of the recording's glyphs is `kept[a]..kept[b]` of the ones
/// spliced. `false`, and `kept` left as it was, when the scissor leaves out
/// none.
fn kept_glyphs(
    glyphs: &[GlyphInstance],
    shift: Option<PixelShift>,
    scissor: (i64, i64, u32, u32),
    kept: &mut Vec<usize>,
) -> bool {
    let outside = |glyph: &GlyphInstance| outside_scissor(&moved(*glyph, shift), Some(scissor));
    let Some(first) = glyphs.iter().position(outside) else {
        return false;
    };
    kept.clear();
    kept.extend(0..=first);
    // Glyph `first` is left out.
    let mut count = first;
    kept.push(count);
    for glyph in &glyphs[first + 1..] {
        if !outside(glyph) {
            count += 1;
        }
        kept.push(count);
    }
    true
}

/// How many of the glyphs in `range` `kept` keeps.
fn kept_count(kept: &[usize], range: std::ops::Range<usize>) -> u32 {
    u32::try_from(kept[range.end] - kept[range.start])
        .expect("BUG: a region keeps at most the glyphs it counts in u32")
}

/// Whether `glyph` is placed on the device grid (an identity linear part
/// and no origin), the only placement the scissor culls.
fn on_grid(glyph: &GlyphInstance) -> bool {
    glyph.transform == [1.0, 0.0, 0.0, 1.0] && glyph.origin[0] == 0.0 && glyph.origin[1] == 0.0
}

/// Whether recording `glyph` under `scissor` would have left it out: the
/// cull of `batches/text.rs`, by the same rule. A glyph under an affine
/// placement is never culled, as in recording.
fn outside_scissor(glyph: &GlyphInstance, scissor: ScissorRect) -> bool {
    if !on_grid(glyph) {
        return false;
    }
    // A grid quad holds whole pixels, which `f32` holds exactly below 2^24,
    // where every quad a target can show lies, so the conversion gives back
    // the integers the recording culls with.
    let [x, y, w, h] = glyph.dst_rect.map(|value| value as i64);
    crate::batches::outside_scissor(scissor, x, y, w, h)
}

/// What the grid glyphs of `glyphs` cover; `None` when there are none.
fn grid_extent(glyphs: &[GlyphInstance]) -> Option<GridExtent> {
    let (left, top, right, bottom) = glyphs.iter().filter(|glyph| on_grid(glyph)).fold(
        None,
        |extent: Option<(f32, f32, f32, f32)>, glyph| {
            let [x, y, w, h] = glyph.dst_rect;
            Some(match extent {
                None => (x, y, x + w, y + h),
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x + w), bottom.max(y + h))
                }
            })
        },
    )?;
    Some(GridExtent([left, top, right - left, bottom - top]))
}

/// Whether `scissor` holds every pixel of `extent`, so it culls no glyph
/// inside it.
fn holds((left, top, width, height): (i64, i64, u32, u32), extent: GridExtent) -> bool {
    // Whole pixels, exact in `f64`, as `outside_scissor` reads them.
    let [x, y, w, h] = extent.0.map(f64::from);
    let (left, top) = (left as f64, top as f64);
    x >= left && y >= top && x + w <= left + f64::from(width) && y + h <= top + f64::from(height)
}

/// `item` moved by `shift`, or as it is when there is none.
fn moved<T: Shift>(item: T, shift: Option<PixelShift>) -> T {
    match shift {
        Some(by) => item.shifted(by),
        None => item,
    }
}

/// Appends `items`, each moved by `shift`; a copy when there is none.
fn extend_moved<T: Shift>(
    arena: &mut crate::recording_budget::BudgetVec<T>,
    items: &[T],
    shift: Option<PixelShift>,
) {
    match shift {
        Some(by) => arena.extend(items.iter().map(|&item| item.shifted(by))),
        None => arena.extend_from_slice(items),
    }
}

/// Appends `instances`, each moved by `shift` and under `clip` when
/// `reclip`.
fn extend_clipped<T: ClippableInstance + Shift>(
    arena: &mut crate::recording_budget::BudgetVec<T>,
    instances: &[T],
    shift: Option<PixelShift>,
    reclip: bool,
    clip: ResolvedClip,
) {
    if reclip {
        arena.extend(
            instances
                .iter()
                .map(|&instance| moved(instance, shift).with_clip(clip)),
        );
    } else {
        extend_moved(arena, instances, shift);
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
        origin: _,
        glyph_keys,
        glyph_extent: _,
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
        origin: _,
        glyph_keys,
        glyph_extent: _,
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

/// The lineage of the scene version each target of a painter last drew.
///
/// A content version compares equal only within its lineage, and a target
/// that draws another lineage's version (a new scene owner, a renewed one)
/// or detaches never draws the old one's content again, so once no target
/// draws a lineage its recordings are dead: [`Self::draws`] names it, and
/// the painter drops them then instead of when the budget evicts them. A
/// target that draws a layer tree keeps the lineage it drew before.
#[derive(Debug)]
pub(crate) struct DrawnLineages<T> {
    drawn: FxHashMap<T, Lineage>,
}

impl<T: std::hash::Hash + Eq> DrawnLineages<T> {
    pub(crate) fn new() -> Self {
        Self {
            drawn: FxHashMap::default(),
        }
    }

    /// Notes that `target` draws `scene`, or nothing once it detaches
    /// (`None`), and answers the lineage it drew before when no target draws
    /// that lineage any more.
    pub(crate) fn draws(
        &mut self,
        target: T,
        scene: Option<flui_layer::SceneContent<'_>>,
    ) -> Option<Lineage> {
        let previous = match scene {
            None => self.drawn.remove(&target)?,
            Some(flui_layer::SceneContent::Tree(_)) => return None,
            Some(flui_layer::SceneContent::Version(version)) => {
                let lineage = version.id().lineage();
                let previous = self.drawn.insert(target, lineage)?;
                if previous == lineage {
                    return None;
                }
                previous
            }
        };
        // A handful of targets: one per window or offscreen slot.
        (!self.drawn.values().any(|&drawn| drawn == previous)).then_some(previous)
    }
}

/// One kept recording, the frame it was last rastered in, and where its
/// place sits in its lineage's list.
#[derive(Debug)]
struct Entry {
    key: RecordingKey,
    recording: ChunkRecording,
    last_rastered: u64,
    lineage_at: usize,
}

/// What a kept recording costs the budget beyond what it holds itself: the
/// rest of its boxed entry, its node in the recency list and one place in
/// each index. The indexes' slack is left out: a hash table's spare slots
/// and keys, the `SmallVec` a content version holds, the spare room of a
/// lineage's `Vec`, a few tens of bytes more per entry.
const ENTRY_OVERHEAD: usize = std::mem::size_of::<Entry>() - std::mem::size_of::<ChunkRecording>()
    + RecencyList::<Box<Entry>>::NODE_BYTES
    + 2 * std::mem::size_of::<Place>();

/// A chunk keeps at most this many recordings (two transforms or ambients,
/// say a chunk drawn in two places), so one that moves every frame does not
/// fill the budget alone.
const PER_CONTENT: usize = 2;

/// The recordings a painter keeps, keyed by [`RecordingKey`], under
/// [`BUDGET_BYTES`] at each frame's end, the least recently rastered evicted
/// first.
///
/// Each recording sits boxed in a [`RecencyList`] in the order it was last
/// rastered, so an eviction takes the oldest without looking at the others
/// and a free node of the list holds a few words. Two [`TalliedMap`]s find
/// its place: by content version, and in its lineage's list, so the
/// recordings of a lineage no target draws any more are dropped at the cost
/// of the ones dropped. Every step through the list and the maps counts what
/// it touches in a test build (`take_visits`). Once the cache is
/// empty it lets the room of all three go.
#[derive(Debug)]
pub(crate) struct RecordingCache {
    by_content: TalliedMap<ContentVersion, SmallVec<[Place; PER_CONTENT]>>,
    by_lineage: TalliedMap<Lineage, Vec<Place>>,
    recency: RecencyList<Box<Entry>>,
    /// What the kept recordings hold, each with [`ENTRY_OVERHEAD`].
    bytes: usize,
    budget: usize,
    /// The frame now being drawn, counted from 0.
    frame: u64,
    mint: IdMint<NonZeroU64>,
    /// `None` once the mint is spent: nothing is kept after that.
    epoch: Option<ResourceEpoch>,
    /// Recordings dropped since [`Self::take_counts`] last reported.
    evictions: u64,
    /// The keys drawn but not kept yet, which a recording is captured under
    /// only once they are drawn on enough frames, at most
    /// [`admission::CANDIDATES`] of them.
    candidates: admission::Candidates<RecordingKey>,
}

impl RecordingCache {
    pub(crate) fn new() -> Self {
        Self::with_budget(BUDGET_BYTES)
    }

    pub(crate) fn with_budget(budget: usize) -> Self {
        let mint = IdMint::new();
        let epoch = mint.next().ok().map(ResourceEpoch);
        Self {
            by_content: TalliedMap::new(),
            by_lineage: TalliedMap::new(),
            recency: RecencyList::new(),
            bytes: 0,
            budget,
            frame: 0,
            mint,
            epoch,
            evictions: 0,
            candidates: admission::Candidates::new(),
        }
    }

    /// The epoch a recording made now is keyed by; `None` when nothing is
    /// kept.
    pub(crate) fn epoch(&self) -> Option<ResourceEpoch> {
        self.epoch
    }

    /// The painter's resources changed under every kept recording: they are
    /// dropped, with the candidates keyed by the same epoch and the room
    /// that held them, and later ones are keyed by a new epoch.
    pub(crate) fn advance_epoch(&mut self) {
        let dropped = self.recency.len() as u64;
        self.evictions = self.evictions.saturating_add(dropped);
        self.release();
        self.candidates.release();
        self.epoch = self.mint.next().ok().map(ResourceEpoch);
    }

    /// Lets every recording go, and the room of the list and the maps.
    fn release(&mut self) {
        self.by_content.release();
        self.by_lineage.release();
        self.recency.release();
        self.bytes = 0;
    }

    /// The place of the recording `key` names.
    fn place(&mut self, key: &RecordingKey) -> Option<Place> {
        let Self {
            by_content,
            recency,
            ..
        } = self;
        by_content
            .get(&key.content)?
            .iter()
            .copied()
            .find(|&place| recency.get(place).key == *key)
    }

    /// The recording `key` names, stamped as rastered this frame.
    pub(crate) fn get(&mut self, key: &RecordingKey) -> Option<&ChunkRecording> {
        let place = self.place(key)?;
        let entry = self.recency.touch(place);
        entry.last_rastered = self.frame;
        Some(&entry.recording)
    }

    /// Takes the recording at `place` out of the list and its lineage's
    /// list, and counts it evicted; its content's places are the caller's.
    fn take_out(&mut self, place: Place) -> Box<Entry> {
        let entry = self.recency.remove(place);
        let lineage = entry.key.content.lineage();
        if let Some(places) = self.by_lineage.get_mut(&lineage) {
            places.swap_remove(entry.lineage_at);
            let moved = places.get(entry.lineage_at).copied();
            let emptied = places.is_empty();
            if let Some(moved) = moved {
                self.recency.get(moved).lineage_at = entry.lineage_at;
            }
            if emptied {
                self.by_lineage.remove(&lineage);
            }
        }
        self.bytes = self
            .bytes
            .saturating_sub(entry.recording.bytes + ENTRY_OVERHEAD);
        self.evictions = self.evictions.saturating_add(1);
        entry
    }

    /// Takes the recording at `place`, which `key` names, out of the cache.
    fn take(&mut self, key: &RecordingKey, place: Place) {
        if let Some(places) = self.by_content.get_mut(&key.content) {
            places.retain(|kept| *kept != place);
            if places.is_empty() {
                self.by_content.remove(&key.content);
            }
        }
        drop(self.take_out(place));
    }

    /// Notes a draw under `key`, which the cache does not hold, and answers
    /// whether to capture the recording it makes: only once `key` has been
    /// drawn on [`admission::DRAWS_TO_ADMIT`] frames, so a chunk
    /// drawn once is recorded and never captured (ADR-0304 §9).
    pub(crate) fn admission(&mut self, key: RecordingKey) -> Admission {
        self.candidates.draw(key, self.frame)
    }

    /// Drops the recording `key` names.
    pub(crate) fn remove(&mut self, key: &RecordingKey) {
        if let Some(place) = self.place(key) {
            self.take(key, place);
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
        let siblings: SmallVec<[Place; PER_CONTENT]> = self
            .by_content
            .get(&key.content)
            .cloned()
            .unwrap_or_default();
        // The recording it replaces, or else the sibling it pushes out.
        let replaced = siblings
            .iter()
            .copied()
            .find(|&place| self.recency.get(place).key == key)
            .or_else(|| {
                (siblings.len() >= PER_CONTENT)
                    .then(|| {
                        siblings
                            .iter()
                            .copied()
                            .min_by_key(|&place| self.recency.get(place).last_rastered)
                    })
                    .flatten()
            });
        if let Some(replaced) = replaced {
            self.take(&key, replaced);
        }
        self.bytes = self.bytes.saturating_add(recording.bytes + ENTRY_OVERHEAD);
        let lineage = key.content.lineage();
        let lineage_at = self.by_lineage.get(&lineage).map_or(0, Vec::len);
        let place = self.recency.push_newest(Box::new(Entry {
            key,
            recording,
            last_rastered: self.frame,
            lineage_at,
        }));
        self.by_lineage.get_or_default(lineage).push(place);
        self.by_content.get_or_default(key.content).push(place);
    }

    /// Drops every recording of a content version of `lineage`, at the cost
    /// of the ones dropped: no target draws its scenes any more, and no other
    /// lineage's content compares equal to one of them.
    pub(crate) fn drop_lineage(&mut self, lineage: Lineage) {
        let Some(places) = self.by_lineage.remove(&lineage) else {
            return;
        };
        for place in places {
            let entry = self.recency.remove(place);
            if let Some(siblings) = self.by_content.get_mut(&entry.key.content) {
                siblings.retain(|kept| *kept != place);
                if siblings.is_empty() {
                    self.by_content.remove(&entry.key.content);
                }
            }
            self.bytes = self
                .bytes
                .saturating_sub(entry.recording.bytes + ENTRY_OVERHEAD);
            self.evictions = self.evictions.saturating_add(1);
        }
        if self.recency.len() == 0 {
            self.release();
        }
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

    /// Evicts over the budget, ages the candidates the frame did not draw,
    /// and starts the next frame.
    pub(crate) fn end_frame(&mut self) {
        self.evict_over_budget();
        self.candidates.end_frame(self.frame);
        self.frame = self.frame.saturating_add(1);
    }

    /// Takes the oldest recordings out until the bytes are within the
    /// budget.
    fn evict_over_budget(&mut self) {
        while self.bytes > self.budget {
            let Some(oldest) = self.recency.oldest() else {
                break;
            };
            let key = self.recency.get(oldest).key;
            self.take(&key, oldest);
        }
        if self.recency.len() == 0 {
            self.release();
        }
    }

    #[cfg(test)]
    pub(crate) fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
    }

    /// How many recordings the cache holds.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.recency.len()
    }

    /// The room of the recency list, of the content and lineage indexes and
    /// of the candidates' list and index, in nodes and entries.
    #[cfg(test)]
    fn room(&self) -> [usize; 5] {
        let [candidates, candidate_places] = self.candidates.room();
        [
            self.recency.room(),
            self.by_content.room(),
            self.by_lineage.room(),
            candidates,
            candidate_places,
        ]
    }

    /// The nodes and map entries every step since this was last asked
    /// touched, lookups, insertions, evictions and drops alike, which starts
    /// the count again.
    #[cfg(test)]
    pub(crate) fn take_visits(&mut self) -> u64 {
        self.recency.take_visits()
            + self.by_content.take_visits()
            + self.by_lineage.take_visits()
            + self.candidates.take_visits()
    }

    /// Both indexes hold every kept recording's place once, and each
    /// recording knows where it sits in its lineage's list: the walk a test
    /// makes to check them, which counts every entry it reaches.
    #[cfg(test)]
    pub(crate) fn check_indexes(&mut self) {
        let held = self.recency.len();
        let by_content: usize = self.by_content.values().map(SmallVec::len).sum();
        let lists: Vec<Vec<Place>> = self.by_lineage.values().cloned().collect();
        assert_eq!(by_content, held, "places by content");
        assert_eq!(
            lists.iter().map(Vec::len).sum::<usize>(),
            held,
            "places by lineage"
        );
        for list in lists {
            for (at, place) in list.into_iter().enumerate() {
                assert_eq!(self.recency.get(place).lineage_at, at);
            }
        }
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
    use flui_layer::{ContentVersion, SceneOwner};

    use super::{AmbientKey, ChunkRecording, PixelOrigin, RasterKey, RecordingCache, RecordingKey};
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

    /// A recording that draws nothing and charges the cache `bytes`, its
    /// entry's overhead among them.
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
            origin: PixelOrigin::of_bounded(0.0, 0.0),
            glyph_keys: Vec::new(),
            glyph_extent: None,
            bytes: bytes
                .checked_sub(super::ENTRY_OVERHEAD)
                .expect("BUG: a test recording charges at least an entry's overhead"),
        }
    }

    /// The key of `content` drawn at `x` under `cache`'s epoch.
    fn key(cache: &RecordingCache, content: ContentVersion, x: f64) -> RecordingKey {
        let ctm = translated(x, 0.0);
        RecordingKey {
            content,
            raster: RasterKey::of(&ctm).expect("a translation is kept").0,
            ambient: AmbientKey::of(1.0, (64, 64)),
            epoch: cache.epoch().expect("a fresh cache has an epoch"),
        }
    }

    /// A transform is kept when it is a finite planar affine one whose
    /// translation stays below 2^23 pixels.
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
    }

    /// The key holds the fraction of the translation and the origin its
    /// whole pixels, so a whole-pixel move keeps the key and moves the
    /// origin, and a fractional one changes the key.
    #[test]
    fn a_raster_key_holds_the_fraction_and_the_origin_the_whole_pixels() {
        let split = |x: f64, y: f64| RasterKey::of(&translated(x, y)).expect("in range");
        let mut scaled = translated(3.0, 4.0);
        scaled.m[0] = 2.0;
        let scaled_moved = {
            let mut ctm = translated(-6.0, 9.0);
            ctm.m[0] = 2.0;
            ctm
        };
        // (row, a transform's key and origin, another's, whether the keys
        // are the same)
        for (row, (key, _), (other, _), same) in [
            ("a whole move", split(1.0, 2.0), split(4.0, -3.0), true),
            (
                "a whole move of a fraction",
                split(-0.5, 2.25),
                split(2.5, 7.25),
                true,
            ),
            ("a signed zero", split(-0.0, 0.0), split(0.0, -0.0), true),
            (
                "a move across most of the range",
                split(-8_388_606.5, 0.75),
                split(8_388_606.5, 0.75),
                true,
            ),
            (
                "a fractional move",
                split(1.25, 2.0),
                split(1.5, 2.0),
                false,
            ),
            (
                "a whole move under a scale",
                RasterKey::of(&scaled).expect("in range"),
                RasterKey::of(&scaled_moved).expect("in range"),
                true,
            ),
            (
                "another scale",
                split(3.0, 4.0),
                RasterKey::of(&scaled).expect("in range"),
                false,
            ),
        ] {
            assert_eq!(key == other, same, "{row}");
        }
        let origin = |x, y| split(x, y).1.pixels();
        assert_eq!(origin(1.0, 2.0), (1, 2));
        assert_eq!(origin(-0.5, 2.25), (-1, 2));
        assert_eq!(origin(-0.0, 7.999), (0, 7));
        assert_eq!(origin(-8_388_606.5, 8_388_607.0), (-8_388_607, 8_388_607));
        // A translation just below 0 rounds `t - floor(t)` to 1.0; the key
        // holds a fraction below 1 there, apart from a whole translation's.
        for tiny in [
            -(2.0_f64.powi(-60)),
            -(2.0_f64.powi(-54)),
            -f64::MIN_POSITIVE,
        ] {
            let (key, origin) = split(tiny, 0.0);
            let fraction = f64::from_bits(key.fraction[0]);
            assert!((0.0..1.0).contains(&fraction), "{tiny}: {fraction}");
            assert_eq!(origin.pixels(), (-1, 0), "{tiny}");
            assert_ne!(key, split(0.0, 0.0).0, "{tiny}");
        }
    }

    /// The move between two origins is their difference in whole pixels,
    /// exact at the ends of the range, and none between equal ones; a rect
    /// moved by it lands where one recorded at the second origin does.
    #[test]
    fn a_move_between_origins_is_their_difference_in_whole_pixels() {
        use crate::instancing::RectInstance;
        use crate::pixel_shift::Shift;
        use flui_painting::styling::Color;
        let origin = |x, y| PixelOrigin::of_bounded(x, y);
        assert_eq!(origin(3.5, 4.0).shift_to(origin(3.25, 4.75)), None);
        let rect =
            |x: f64, y: f64| RectInstance::rect(Rect::from_xywh(x, y, 2.0, 2.0), Color::RED).bounds;
        for (from, to) in [
            ((0.5, 0.0), (3.5, -2.0)),
            ((-8_388_607.0, 8_388_607.0), (8_388_607.0, -8_388_607.0)),
        ] {
            let by = origin(from.0, from.1)
                .shift_to(origin(to.0, to.1))
                .expect("the origins differ");
            let moved = RectInstance::rect(
                Rect::from_xywh(from.0.floor(), from.1.floor(), 2.0, 2.0),
                Color::RED,
            )
            .shifted(by);
            assert_eq!(
                moved.bounds,
                rect(to.0.floor(), to.1.floor()),
                "{from:?} to {to:?}"
            );
        }
    }

    /// The ambient key holds the opacity bits and the target's size, and no
    /// scissor or clip: the splice applies the entry's (ADR-0306 §2).
    #[test]
    fn an_ambient_key_holds_the_opacity_and_the_target_size() {
        assert_eq!(AmbientKey::of(1.0, (64, 64)), AmbientKey::of(1.0, (64, 64)));
        assert_ne!(
            AmbientKey::of(1.0, (64, 64)),
            AmbientKey::of(1.0, (32, 64)),
            "size"
        );
        assert_ne!(
            AmbientKey::of(0.5, (64, 64)),
            AmbientKey::of(1.0, (64, 64)),
            "opacity"
        );
    }

    /// Over its budget the cache evicts the least recently rastered
    /// recordings first, and its counts report each eviction once with the
    /// bytes it holds.
    #[test]
    fn the_cache_evicts_the_least_recently_rastered_over_its_budget() {
        let mut cache = RecordingCache::with_budget(2_500);
        let [a, b, c] = versions(3)[..] else {
            unreachable!("BUG: three versions were minted")
        };
        let (a, b, c) = (
            key(&cache, a, 0.0),
            key(&cache, b, 0.0),
            key(&cache, c, 0.0),
        );
        cache.insert(a, recording(1_000));
        cache.insert(b, recording(1_000));
        cache.end_frame();
        assert!(cache.get(&a).is_some(), "a is rastered in the next frame");
        cache.insert(c, recording(1_000));
        assert_eq!(
            cache.take_counts(),
            (0, 3_000),
            "over budget until it settles"
        );
        cache.settle();
        assert!(cache.get(&b).is_none(), "b was rastered least recently");
        assert!(cache.get(&a).is_some() && cache.get(&c).is_some());
        assert_eq!(cache.take_counts(), (1, 2_000));
        assert_eq!(
            cache.take_counts(),
            (0, 2_000),
            "an eviction is reported once"
        );
    }

    /// A chunk keeps two recordings: a third gives up its least recently
    /// rastered one.
    #[test]
    fn a_chunk_keeps_two_recordings() {
        let mut cache = RecordingCache::new();
        let content = versions(1)[0];
        let [first, second, third] = [0.0, 0.25, 0.5].map(|x| key(&cache, content, x));
        cache.insert(first, recording(1_000));
        cache.insert(second, recording(1_000));
        cache.end_frame();
        assert!(cache.get(&first).is_some(), "first is rastered again");
        cache.insert(third, recording(1_000));
        assert!(
            cache.get(&second).is_none(),
            "second was rastered least recently"
        );
        assert!(cache.get(&first).is_some() && cache.get(&third).is_some());
        assert_eq!(cache.take_counts(), (1, 2_000));
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
        cache.insert(a, recording(1_000));
        cache.insert(b, recording(1_000));
        cache.advance_epoch();
        assert_eq!(cache.take_counts(), (2, 0));
        cache.insert(a, recording(1_000));
        assert!(cache.get(&a).is_none(), "a stale epoch's recording");
        let fresh = key(&cache, a.content, 0.0);
        cache.insert(fresh, recording(1_000));
        assert!(cache.get(&fresh).is_some());
    }

    /// The bytes each test recording charges the cache.
    const UNIT: usize = 1_000;

    /// Over its budget, a cache evicts its least recently rastered recording
    /// and touches the same few entries whatever it holds: with 1 000 and
    /// with 10 000 recordings, each rastered in a frame of its own, a frame
    /// that rasters the newest again and keeps one more evicts the oldest,
    /// and each such frame touches as many entries at both sizes.
    #[test]
    fn an_eviction_looks_only_at_the_recordings_it_evicts() {
        // Per size, the entries each frame touched.
        let mut work = Vec::new();
        for kept in [1_000, 10_000] {
            let mut cache = RecordingCache::with_budget(kept * UNIT);
            let keys: Vec<RecordingKey> = versions(kept + 3)
                .into_iter()
                .map(|content| key(&cache, content, 0.0))
                .collect();
            for &key in &keys[..kept] {
                cache.insert(key, recording(UNIT));
                cache.end_frame();
            }
            assert_eq!(cache.take_counts(), (0, (kept * UNIT) as u64), "at {kept}");
            let frames: Vec<u64> = keys[kept..]
                .iter()
                .enumerate()
                .map(|(frame, &fresh)| {
                    let _ = cache.take_visits();
                    assert!(cache.get(&keys[kept - 1]).is_some());
                    cache.insert(fresh, recording(UNIT));
                    cache.end_frame();
                    let visits = cache.take_visits();
                    assert_eq!(
                        (cache.take_counts(), cache.len()),
                        ((1, (kept * UNIT) as u64), kept),
                        "frame {frame} at {kept}"
                    );
                    assert!(
                        cache.get(&keys[frame]).is_none(),
                        "frame {frame} at {kept}: the least recently rastered left"
                    );
                    visits
                })
                .collect();
            assert!(cache.get(&keys[kept - 1]).is_some(), "rastered each frame");
            cache.check_indexes();
            work.push(frames);
        }
        assert_eq!(work[0], work[1], "the work is the same at both sizes");
        assert!(work[0].iter().all(|&visits| visits < 32), "{work:?}");
    }

    /// A lineage no target draws any more is named once: when the last
    /// target that drew it draws another or detaches, not while another
    /// target still draws it, and not for a layer tree. Dropping it takes
    /// its recordings and leaves another lineage's.
    #[test]
    fn a_lineage_no_target_draws_drops_its_recordings() {
        use flui_layer::{LayerTree, SceneContent};

        let (old, new) = (SceneOwner::new(), SceneOwner::new());
        let (old_version, new_version) = (old.current(), new.current());
        let lineage = old_version.id().lineage();
        let tree = LayerTree::default();
        let mut drawn = super::DrawnLineages::new();
        for (row, target, scene, dead) in [
            (
                "a first draw",
                1,
                Some(SceneContent::Version(old_version)),
                None,
            ),
            (
                "a second target",
                2,
                Some(SceneContent::Version(old_version)),
                None,
            ),
            ("a layer tree", 1, Some(SceneContent::Tree(&tree)), None),
            (
                "one target moves on",
                1,
                Some(SceneContent::Version(new_version)),
                None,
            ),
            ("a detach that leaves none", 2, None, Some(lineage)),
            ("a detach of no lineage", 2, None, None),
        ] {
            assert_eq!(drawn.draws(target, scene), dead, "{row}");
        }

        // The work of dropping three recordings, beside 10 and beside 1 000
        // of another lineage.
        let mut work = Vec::new();
        for others in [10, 1_000] {
            let mut cache = RecordingCache::new();
            let old_keys: Vec<RecordingKey> = versions(3)
                .into_iter()
                .map(|content| key(&cache, content, 0.0))
                .collect();
            let other_keys: Vec<RecordingKey> = versions(others)
                .into_iter()
                .map(|content| key(&cache, content, 0.0))
                .collect();
            for &key in other_keys.iter().chain(&old_keys) {
                cache.insert(key, recording(UNIT));
            }
            let _ = (cache.take_counts(), cache.take_visits());
            cache.drop_lineage(old_keys[0].content.lineage());
            work.push(cache.take_visits());
            assert_eq!(
                (cache.take_counts(), cache.len()),
                ((3, (others * UNIT) as u64), others)
            );
            cache.check_indexes();
            assert!(other_keys.iter().all(|key| cache.get(key).is_some()));
            assert!(old_keys.iter().all(|key| cache.get(key).is_none()));
            cache.drop_lineage(other_keys[0].content.lineage());
            assert_eq!((cache.take_counts(), cache.len()), ((others as u64, 0), 0));
        }
        assert_eq!(work[0], work[1], "a drop costs what it drops: {work:?}");
    }

    /// Whatever empties the cache, an epoch, a lineage drop or an eviction
    /// over the budget, lets go of the room of the recency list and of both
    /// indexes, so memory the budget no longer counts is not kept. An epoch
    /// also lets the candidates' room go, since every key they hold names
    /// the epoch that passed; the other two leave the candidates to age out.
    #[test]
    fn a_cache_emptied_lets_its_room_go() {
        type Empty = fn(&mut RecordingCache, flui_layer::Lineage);
        let rows: [(&str, Empty, bool); 3] = [
            ("an epoch", |cache, _| cache.advance_epoch(), true),
            (
                "a lineage drop",
                |cache, lineage| {
                    cache.drop_lineage(lineage);
                },
                false,
            ),
            (
                "an eviction",
                |cache, _| {
                    cache.set_budget(0);
                    cache.settle();
                },
                false,
            ),
        ];
        for (row, empty, candidates_go) in rows {
            let mut cache = RecordingCache::new();
            let keys: Vec<RecordingKey> = versions(100)
                .into_iter()
                .map(|content| key(&cache, content, 0.0))
                .collect();
            // Drawn once, as candidates, and kept under a sibling's key.
            for &key in &keys {
                let _ = cache.admission(key);
                cache.insert(key, recording(UNIT));
            }
            assert!(
                cache.room().iter().all(|&room| room > 0),
                "{row}: filled {:?}",
                cache.room()
            );
            empty(&mut cache, keys[0].content.lineage());
            let room = cache.room();
            assert_eq!((cache.len(), &room[..3]), (0, &[0; 3][..]), "{row}");
            assert_eq!(
                room[3..].iter().all(|&room| room == 0),
                candidates_go,
                "{row}: the candidates' room {room:?}"
            );
        }
    }
}
