//! Monotonic per-owner work counters for the four pipeline phases.
//!
//! A frame is not one call on [`PipelineOwner`](super::PipelineOwner): the
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
    /// Render nodes whose layout actually ran: past the clean-child
    /// short-circuit that serves a cached geometry. Intrinsic queries do not
    /// count.
    pub nodes_laid_out: u64,
    /// Render nodes whose `paint_raw` ran. A repaint boundary grafted from
    /// its retained layers is not a paint.
    pub nodes_painted: u64,
    /// Layers the paint composer created fresh this pass: sealed pictures and
    /// pushed effect or boundary layers.
    pub layers_produced: u64,
    /// Layers cloned from a clean repaint boundary's retained output instead
    /// of being painted again.
    pub layers_reused: u64,
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
            nodes_laid_out: self.nodes_laid_out.saturating_sub(earlier.nodes_laid_out),
            nodes_painted: self.nodes_painted.saturating_sub(earlier.nodes_painted),
            layers_produced: self.layers_produced.saturating_sub(earlier.layers_produced),
            layers_reused: self.layers_reused.saturating_sub(earlier.layers_reused),
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
        self.nodes_laid_out = self.nodes_laid_out.saturating_add(rhs.nodes_laid_out);
        self.nodes_painted = self.nodes_painted.saturating_add(rhs.nodes_painted);
        self.layers_produced = self.layers_produced.saturating_add(rhs.layers_produced);
        self.layers_reused = self.layers_reused.saturating_add(rhs.layers_reused);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(base: u64) -> PipelineCounters {
        PipelineCounters {
            layout_passes: base,
            layout_roots: base + 1,
            nodes_laid_out: base + 2,
            nodes_painted: base + 3,
            layers_produced: base + 4,
            layers_reused: base + 5,
            semantics_nodes_updated: base + 6,
            frames_produced: base + 7,
        }
    }

    fn all(value: u64) -> PipelineCounters {
        PipelineCounters {
            layout_passes: value,
            layout_roots: value,
            nodes_laid_out: value,
            nodes_painted: value,
            layers_produced: value,
            layers_reused: value,
            semantics_nodes_updated: value,
            frames_produced: value,
        }
    }

    #[test]
    fn since_subtracts_every_field_and_saturates() {
        let later = sample(10);
        let earlier = sample(3);
        assert_eq!(later.since(earlier), all(7));
        assert_eq!(earlier.since(later), PipelineCounters::default());
    }

    #[test]
    fn add_sums_every_field() {
        assert_eq!(sample(3) + all(1), sample(4));
        let mut total = sample(0);
        total += sample(0);
        assert_eq!(total.frames_produced, 14);
        assert_eq!(total.layout_passes, 0);
    }
}
