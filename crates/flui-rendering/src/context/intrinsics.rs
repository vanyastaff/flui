//! Typed contexts for intrinsic-dimension and dry-layout queries.
//!
//! Box objects compute intrinsics/dry layout as PURE functions of their
//! configuration plus the same queries on their children. Objects hold
//! no child pointers in flui, so the child channel is a driver callback
//! — the same shape as the paint and hit-test walks. The driver
//! memoizes every level in the per-node layout cache; an object never
//! sees the cache.

use flui_types::Size;

use crate::constraints::BoxConstraints;
use crate::storage::IntrinsicDimension;
use crate::traits::TextBaseline;

/// Child probe kinds issued during a dry-baseline computation.
#[derive(Debug, Clone, Copy)]
pub enum DryBaselineChildRequest {
    /// Dry baseline under `constraints`.
    Baseline(BoxConstraints, TextBaseline),
    /// Dry layout size under `constraints`.
    DryLayout(BoxConstraints),
}

/// Answers to [`DryBaselineChildRequest`].
#[derive(Debug, Clone, Copy)]
pub enum DryBaselineChildResponse {
    /// Child dry-baseline result.
    Baseline(Option<f32>),
    /// Child dry-layout size.
    DryLayout(Size),
}

/// Child-query channel for one dry-baseline computation.
///
/// Handed to [`RenderBox::compute_dry_baseline`]; baseline and dry-layout
/// child probes share one driver callback so the slot map is borrowed once.
pub struct BoxDryBaselineCtx<'a> {
    child_count: usize,
    query: &'a mut (
                dyn FnMut(usize, DryBaselineChildRequest) -> DryBaselineChildResponse + Send + Sync
            ),
}

impl<'a> BoxDryBaselineCtx<'a> {
    /// Wraps the driver's child dry-baseline callback.
    pub(crate) fn new(
        child_count: usize,
        query: &'a mut (
                    dyn FnMut(usize, DryBaselineChildRequest) -> DryBaselineChildResponse
                        + Send
                        + Sync
                ),
    ) -> Self {
        Self { child_count, query }
    }

    /// Number of tree children.
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// The dry baseline the child would report under `constraints`.
    pub fn child_dry_baseline(
        &mut self,
        index: usize,
        constraints: BoxConstraints,
        baseline: TextBaseline,
    ) -> Option<f32> {
        match (self.query)(
            index,
            DryBaselineChildRequest::Baseline(constraints, baseline),
        ) {
            DryBaselineChildResponse::Baseline(v) => v,
            DryBaselineChildResponse::DryLayout(_) => None,
        }
    }

    /// The size the child would take under `constraints`, without laying it out.
    pub fn child_dry_layout(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        match (self.query)(index, DryBaselineChildRequest::DryLayout(constraints)) {
            DryBaselineChildResponse::DryLayout(size) => size,
            DryBaselineChildResponse::Baseline(_) => Size::ZERO,
        }
    }
}

/// Driver callbacks for one intrinsic computation.
pub struct IntrinsicChildChannel<'a> {
    /// Memoized child intrinsic probes.
    pub query: &'a mut (dyn FnMut(usize, IntrinsicDimension, f32) -> f32 + Send + Sync),
    /// Flex factor for child `index` (`0` when inflexible or unknown).
    pub flex: &'a mut (dyn FnMut(usize) -> i32 + Send + Sync),
}

/// Child-query channel for one intrinsic computation.
///
/// Handed to [`RenderBox::compute_min_intrinsic_width`] and friends.
/// Each child query is answered by the pipeline's memoizing walk —
/// repeated probes of the same `(dimension, extent)` on a child cost
/// one computation.
///
/// [`RenderBox::compute_min_intrinsic_width`]: crate::traits::RenderBox::compute_min_intrinsic_width
pub struct BoxIntrinsicsCtx<'a> {
    child_count: usize,
    query: &'a mut (dyn FnMut(usize, IntrinsicDimension, f32) -> f32 + Send + Sync),
    flex: &'a mut (dyn FnMut(usize) -> i32 + Send + Sync),
}

impl<'a> BoxIntrinsicsCtx<'a> {
    /// Wraps the driver's child-query and flex-factor callbacks.
    pub(crate) fn new(
        child_count: usize,
        query: &'a mut (dyn FnMut(usize, IntrinsicDimension, f32) -> f32 + Send + Sync),
        flex: &'a mut (dyn FnMut(usize) -> i32 + Send + Sync),
    ) -> Self {
        Self {
            child_count,
            query,
            flex,
        }
    }

    /// Number of tree children.
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// The child's intrinsic value for an arbitrary dimension.
    pub fn child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        (self.query)(index, dimension, extent)
    }

    /// The child's minimum intrinsic width for the given height.
    pub fn child_min_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinWidth, height)
    }

    /// The child's maximum intrinsic width for the given height.
    pub fn child_max_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxWidth, height)
    }

    /// The child's minimum intrinsic height for the given width.
    pub fn child_min_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinHeight, width)
    }

    /// The child's maximum intrinsic height for the given width.
    pub fn child_max_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxHeight, width)
    }

    /// Flex factor for child `index` (`0` when inflexible or unknown).
    pub fn child_flex(&mut self, index: usize) -> i32 {
        (self.flex)(index).max(0)
    }
}

/// Child-query channel for one dry-layout computation.
///
/// Handed to [`RenderBox::compute_dry_layout`]; `child_dry_layout`
/// answers what size the child WOULD take under the given constraints,
/// memoized per child and constraints by the pipeline.
///
/// [`RenderBox::compute_dry_layout`]: crate::traits::RenderBox::compute_dry_layout
pub struct BoxDryLayoutCtx<'a> {
    child_count: usize,
    dry: &'a mut (dyn FnMut(usize, BoxConstraints) -> Size + Send + Sync),
}

impl<'a> BoxDryLayoutCtx<'a> {
    /// Wraps the driver's child dry-layout callback.
    pub(crate) fn new(
        child_count: usize,
        dry: &'a mut (dyn FnMut(usize, BoxConstraints) -> Size + Send + Sync),
    ) -> Self {
        Self { child_count, dry }
    }

    /// Number of tree children.
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// The size the child would take under `constraints`, without
    /// laying it out.
    pub fn child_dry_layout(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        (self.dry)(index, constraints)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A leaf context for unit-testing `compute_*` implementations of
    /// childless objects: any child query is a contract violation and
    /// panics with the probe's coordinates.
    pub(crate) fn leaf_intrinsics<R>(f: impl FnOnce(&mut BoxIntrinsicsCtx<'_>) -> R) -> R {
        let mut deny_query = |index: usize, dim: IntrinsicDimension, extent: f32| -> f32 {
            panic!(
                "leaf object queried child {index} ({dim:?} @ {extent}) — \
                 a childless compute_* must not consult children"
            )
        };
        let mut deny_flex = |index: usize| -> i32 {
            panic!(
                "leaf object queried flex for child {index} — \
                 a childless compute_* must not consult children"
            )
        };
        let channel = IntrinsicChildChannel {
            query: &mut deny_query,
            flex: &mut deny_flex,
        };
        f(&mut BoxIntrinsicsCtx::new(0, channel.query, channel.flex))
    }

    /// Leaf context for `compute_dry_layout` tests; mirrors
    /// [`leaf_intrinsics`].
    pub(crate) fn leaf_dry_layout<R>(f: impl FnOnce(&mut BoxDryLayoutCtx<'_>) -> R) -> R {
        let mut deny = |index: usize, constraints: BoxConstraints| -> Size {
            panic!(
                "leaf object dry-laid-out child {index} ({constraints:?}) — \
                 a childless compute_dry_layout must not consult children"
            )
        };
        f(&mut BoxDryLayoutCtx::new(0, &mut deny))
    }

    /// Leaf context for `compute_dry_baseline` tests.
    pub(crate) fn leaf_dry_baseline<R>(f: impl FnOnce(&mut BoxDryBaselineCtx<'_>) -> R) -> R {
        let mut deny = |index: usize,
                        request: DryBaselineChildRequest|
         -> DryBaselineChildResponse {
            match request {
                DryBaselineChildRequest::Baseline(constraints, baseline) => panic!(
                    "leaf object dry-baselined child {index} ({constraints:?}, {baseline:?}) — \
                 a childless compute_dry_baseline must not consult children"
                ),
                DryBaselineChildRequest::DryLayout(constraints) => panic!(
                    "leaf object dry-laid out child {index} ({constraints:?}) during dry baseline — \
                 a childless compute_dry_baseline must not consult children"
                ),
            }
        };
        f(&mut BoxDryBaselineCtx::new(0, &mut deny))
    }
}
