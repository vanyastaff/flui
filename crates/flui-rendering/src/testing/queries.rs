//! Box intrinsics, dry layout, and dry baseline queries on harness runs.
//!
//! Wraps [`PipelineOwner::box_intrinsic_dimension`], [`PipelineOwner::box_dry_layout`],
//! and [`PipelineOwner::box_dry_baseline`] with panic-on-error ergonomics for tests.
//! Queries do not require a committed layout pass — mount the tree and call the
//! helpers on a [`LayoutRun`] or [`FrameRun`] directly.
//!
//! [`PipelineOwner::box_intrinsic_dimension`]: crate::pipeline::PipelineOwner::box_intrinsic_dimension
//! [`PipelineOwner::box_dry_layout`]: crate::pipeline::PipelineOwner::box_dry_layout
//! [`PipelineOwner::box_dry_baseline`]: crate::pipeline::PipelineOwner::box_dry_baseline
//! [`LayoutRun`]: super::harness::LayoutRun
//! [`FrameRun`]: super::harness::FrameRun

use crate::constraints::BoxConstraints;
use crate::pipeline::{PipelineOwner, PipelinePhase};
use crate::storage::IntrinsicDimension;
use crate::traits::TextBaseline;
use flui_foundation::RenderId;
use flui_types::Size;

use super::harness::{FrameRun, LayoutRun};

/// Box query helpers for harness runs that own a mutable [`PipelineOwner`].
///
/// Implemented for [`LayoutRun`] and [`FrameRun`]. Methods panic on stale ids,
/// protocol mismatches, or other query failures — the same contract as other
/// harness inspection helpers.
pub trait BoxQueryRun {
    /// Pipeline phase held by the run.
    type Phase: PipelinePhase + Sync;

    /// Mutable access to the backing owner (used by default method bodies).
    fn pipeline_mut(&mut self) -> &mut PipelineOwner<Self::Phase>;

    /// Flutter `computeMinIntrinsicWidth` / `computeMaxIntrinsicWidth` /
    /// `computeMinIntrinsicHeight` / `computeMaxIntrinsicHeight` dispatch.
    fn intrinsic_dimension(
        &mut self,
        id: RenderId,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        self.pipeline_mut()
            .box_intrinsic_dimension(id, dimension, extent)
            .unwrap_or_else(|e| panic!("intrinsic query failed for {id:?}: {e}"))
    }

    /// Minimum width the subtree would prefer at the given height extent.
    fn min_intrinsic_width(&mut self, id: RenderId, height: f32) -> f32 {
        self.intrinsic_dimension(id, IntrinsicDimension::MinWidth, height)
    }

    /// Maximum width the subtree would prefer at the given height extent.
    fn max_intrinsic_width(&mut self, id: RenderId, height: f32) -> f32 {
        self.intrinsic_dimension(id, IntrinsicDimension::MaxWidth, height)
    }

    /// Minimum height the subtree would prefer at the given width extent.
    fn min_intrinsic_height(&mut self, id: RenderId, width: f32) -> f32 {
        self.intrinsic_dimension(id, IntrinsicDimension::MinHeight, width)
    }

    /// Maximum height the subtree would prefer at the given width extent.
    fn max_intrinsic_height(&mut self, id: RenderId, width: f32) -> f32 {
        self.intrinsic_dimension(id, IntrinsicDimension::MaxHeight, width)
    }

    /// Size the subtree would take under `constraints` without mutating layout
    /// state — Flutter's `getDryLayout`.
    fn dry_layout(&mut self, id: RenderId, constraints: BoxConstraints) -> Size {
        self.pipeline_mut()
            .box_dry_layout(id, constraints)
            .unwrap_or_else(|e| panic!("dry layout query failed for {id:?}: {e}"))
    }

    /// Baseline distance from the top edge under `constraints` without laying
    /// out — Flutter's `getDryBaseline`. `None` means the subtree reports no
    /// baseline for that axis/constraints pair (a valid, cacheable answer).
    fn dry_baseline(
        &mut self,
        id: RenderId,
        constraints: BoxConstraints,
        baseline: TextBaseline,
    ) -> Option<f32> {
        self.pipeline_mut()
            .box_dry_baseline(id, constraints, baseline)
            .unwrap_or_else(|e| panic!("dry baseline query failed for {id:?}: {e}"))
    }
}

impl BoxQueryRun for LayoutRun {
    type Phase = crate::pipeline::Layout;

    fn pipeline_mut(&mut self) -> &mut PipelineOwner<Self::Phase> {
        self.owner_mut()
    }
}

impl BoxQueryRun for FrameRun {
    type Phase = crate::pipeline::Idle;

    fn pipeline_mut(&mut self) -> &mut PipelineOwner<Self::Phase> {
        self.owner_mut()
    }
}
