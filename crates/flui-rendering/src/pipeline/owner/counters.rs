//! Monotonic per-owner work counters for the four pipeline phases.
//!
//! A frame is not one call on [`PipelineOwner`]: the
//! layout↔build fixpoint runs `run_layout` several times before the final
//! `run_frame`, and the owner has no frame-begin hook. So the counters are
//! totals since the owner was constructed, and a frame driver takes the
//! difference across its frame with [`PipelineCounters::since`] — the shape
//! [`PipelineOwner::layout_roots_total`](super::PipelineOwner::layout_roots_total)
//! already has.
//!
//! Every field is a plain integer on the owner (or a `Cell` on the
//! single-threaded layout arena), bumped on the frame path with no atomics and
//! no locks.

use std::ops::{Add, AddAssign};

use crate::pipeline::phase::PipelinePhase;

use super::PipelineOwner;

/// Work the pipeline did, counted per phase. See the module docs.
///
/// Returned by [`PipelineOwner::counters`](super::PipelineOwner::counters) as
/// totals since construction; subtract two readings with [`Self::since`] to
/// get one frame's work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PipelineCounters {
    /// Non-empty layout batches drained by `run_layout`. A layout pass that
    /// found no dirty entry adds nothing.
    pub layout_passes: u64,
    /// Dirty layout entries drained, the figure
    /// [`PipelineOwner::layout_roots_total`](super::PipelineOwner::layout_roots_total)
    /// reports.
    pub layout_roots: u64,
    /// Render nodes the layout-invalidation walks marked on their way to a
    /// relayout boundary. A walk that reaches a path an earlier walk already
    /// settled stops there, so sibling marks share one ancestor walk.
    pub layout_mark_steps: u64,
    /// Render nodes whose layout actually ran: past the clean-child
    /// short-circuit that serves a cached geometry. Intrinsic queries do not
    /// count.
    pub nodes_laid_out: u64,
    /// Render nodes the layout walks acquired: every node a walk entered, including each one it
    /// served from a cache. Bounded by the nodes reached, not the size of the dirty subtree.
    pub nodes_acquired: u64,
    /// Render nodes whose `paint_raw` ran. A repaint boundary grafted from
    /// its retained layers is not a paint.
    pub nodes_painted: u64,
    /// Scene entries the paint composer created fresh this pass: sealed
    /// chunks, pushed effects, and the effects a composited-layer update
    /// rebuilds in a republished content.
    pub layers_produced: u64,
    /// Entries a pass carried over unchanged into a content it republishes:
    /// the chunks a repaint kept for the render objects that owed no paint
    /// (ADR-0304 §2), and the entries a composited-layer update shares. A
    /// clean boundary emits nothing and counts nothing.
    pub layers_reused: u64,
    /// Operations the patches paint buffered: publishes, placements and
    /// retirements (ADR-0304 §3).
    pub scene_ops: u64,
    /// Nodes carried by the accessibility updates the semantics owner
    /// delivered. A flush whose diff is empty adds nothing.
    pub semantics_nodes_updated: u64,
    /// `run_frame` calls that committed a layer tree.
    pub frames_produced: u64,
}

impl PipelineCounters {
    /// The work done between `earlier` and `self`, field by field.
    ///
    /// Saturating, so a reading taken from a different (for example,
    /// replaced) owner yields zeros rather than wrapping.
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            layout_passes: self.layout_passes.saturating_sub(earlier.layout_passes),
            layout_roots: self.layout_roots.saturating_sub(earlier.layout_roots),
            layout_mark_steps: self
                .layout_mark_steps
                .saturating_sub(earlier.layout_mark_steps),
            nodes_laid_out: self.nodes_laid_out.saturating_sub(earlier.nodes_laid_out),
            nodes_acquired: self.nodes_acquired.saturating_sub(earlier.nodes_acquired),
            nodes_painted: self.nodes_painted.saturating_sub(earlier.nodes_painted),
            layers_produced: self.layers_produced.saturating_sub(earlier.layers_produced),
            layers_reused: self.layers_reused.saturating_sub(earlier.layers_reused),
            scene_ops: self.scene_ops.saturating_sub(earlier.scene_ops),
            semantics_nodes_updated: self
                .semantics_nodes_updated
                .saturating_sub(earlier.semantics_nodes_updated),
            frames_produced: self.frames_produced.saturating_sub(earlier.frames_produced),
        }
    }
}

impl Add for PipelineCounters {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self {
        self += rhs;
        self
    }
}

impl AddAssign for PipelineCounters {
    fn add_assign(&mut self, rhs: Self) {
        self.layout_passes = self.layout_passes.saturating_add(rhs.layout_passes);
        self.layout_roots = self.layout_roots.saturating_add(rhs.layout_roots);
        self.layout_mark_steps = self.layout_mark_steps.saturating_add(rhs.layout_mark_steps);
        self.nodes_laid_out = self.nodes_laid_out.saturating_add(rhs.nodes_laid_out);
        self.nodes_acquired = self.nodes_acquired.saturating_add(rhs.nodes_acquired);
        self.nodes_painted = self.nodes_painted.saturating_add(rhs.nodes_painted);
        self.layers_produced = self.layers_produced.saturating_add(rhs.layers_produced);
        self.layers_reused = self.layers_reused.saturating_add(rhs.layers_reused);
        self.scene_ops = self.scene_ops.saturating_add(rhs.scene_ops);
        self.semantics_nodes_updated = self
            .semantics_nodes_updated
            .saturating_add(rhs.semantics_nodes_updated);
        self.frames_produced = self.frames_produced.saturating_add(rhs.frames_produced);
    }
}

/// What one paint pass's composer did, folded into [`PipelineCounters`] when
/// the pass commits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PaintCounts {
    pub(super) nodes_painted: u64,
    pub(super) layers_produced: u64,
    pub(super) layers_reused: u64,
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Dirty layout entries drained by every `run_layout` so far (monotonic).
    /// A frame's relayout-root count is the difference across the frame
    /// (ADR-0074 §8 telemetry). Entries that turned out already clean or
    /// layout-poisoned count too: they were scheduled, which is what the
    /// figure measures.
    #[inline]
    pub fn layout_roots_total(&self) -> u64 {
        self.scheduler.layout_drained_total()
    }

    /// Work totals for every phase since this owner was constructed
    /// (monotonic). A frame's work is the difference across the frame,
    /// [`PipelineCounters::since`](crate::pipeline::PipelineCounters::since):
    /// the owner has no frame-begin hook, because the layout↔build fixpoint
    /// runs `run_layout` several times before the frame's `run_frame`.
    #[inline]
    pub fn counters(&self) -> crate::pipeline::PipelineCounters {
        crate::pipeline::PipelineCounters {
            layout_roots: self.scheduler.layout_drained_total(),
            layout_mark_steps: self.scheduler.layout_mark_steps(),
            ..self.counters
        }
    }
}
