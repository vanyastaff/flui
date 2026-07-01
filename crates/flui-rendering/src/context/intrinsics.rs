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
use crate::parent_data::{FlexParentData, ParentData};
use crate::storage::IntrinsicDimension;
use crate::traits::TextBaseline;

// ============================================================================
// DryBaselineChildRequest / DryBaselineChildResponse
// ============================================================================

/// Child probe kinds issued during a dry-baseline computation.
///
/// `#[non_exhaustive]`: this protocol enum is demonstrably growing; future
/// child-query kinds are additive variants rather than a breaking change.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum DryBaselineChildRequest {
    /// Dry baseline under `constraints`.
    Baseline(BoxConstraints, TextBaseline),
    /// Dry layout size under `constraints`.
    DryLayout(BoxConstraints),
    /// Intrinsic dimension value: `(dimension, extent)`.
    Intrinsic(IntrinsicDimension, f32),
}

/// Answers to [`DryBaselineChildRequest`].
///
/// `#[non_exhaustive]`: mirrors [`DryBaselineChildRequest`] — grows together.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum DryBaselineChildResponse {
    /// Child dry-baseline result.
    Baseline(Option<f32>),
    /// Child dry-layout size.
    DryLayout(Size),
    /// Child intrinsic value for a given dimension + extent.
    Intrinsic(f32),
}

// ============================================================================
// DryLayoutChildRequest / DryLayoutChildResponse  (new for ADR-0011)
// ============================================================================

/// Child probe kinds issued during a dry-layout computation.
///
/// A single enum so `BoxDryLayoutCtx` can hold one `&mut`-capturing callback
/// instead of two (which would require two simultaneous `&mut` borrows of the
/// slot map).  ISP: only the sub-query kinds a dry-layout pass legitimately
/// issues are present here.
///
/// `#[non_exhaustive]`: future child-query kinds are additive variants.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum DryLayoutChildRequest {
    /// Dry layout size under `constraints`.
    DryLayout(BoxConstraints),
    /// Intrinsic dimension value: `(dimension, extent)`.
    Intrinsic(IntrinsicDimension, f32),
}

/// Answers to [`DryLayoutChildRequest`].
///
/// `#[non_exhaustive]`: mirrors [`DryLayoutChildRequest`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum DryLayoutChildResponse {
    /// Child dry-layout size.
    DryLayout(Size),
    /// Child intrinsic value for a given dimension + extent.
    Intrinsic(f32),
}

// ============================================================================
// BoxDryBaselineCtx
// ============================================================================

/// Child-query channel for one dry-baseline computation.
///
/// Handed to [`crate::traits::RenderBox::compute_dry_baseline`]; baseline and dry-layout
/// child probes share one driver callback so the slot map is borrowed once.
pub struct BoxDryBaselineCtx<'a> {
    child_count: usize,
    /// Erased per-child parent data; indexed by child position.
    ///
    /// Populated by the driver from each child's `RenderNode::parent_data`
    /// (plus harness seeds when `test`/`testing` is active). Container
    /// objects downcast entries via [`Self::child_parent_data_as`].
    child_parent_data: &'a [Option<&'a dyn ParentData>],
    query: &'a mut (
                dyn FnMut(usize, DryBaselineChildRequest) -> DryBaselineChildResponse + Send + Sync
            ),
}

impl<'a> BoxDryBaselineCtx<'a> {
    /// Wraps the driver's child dry-baseline callback.
    pub(crate) fn new(
        child_count: usize,
        child_parent_data: &'a [Option<&'a dyn ParentData>],
        query: &'a mut (
                    dyn FnMut(usize, DryBaselineChildRequest) -> DryBaselineChildResponse
                        + Send
                        + Sync
                ),
    ) -> Self {
        Self {
            child_count,
            child_parent_data,
            query,
        }
    }

    /// Number of tree children.
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// Type-erased parent data the parent stored on child `index`, or `None`
    /// if no parent data has been set for that child.
    ///
    /// Use [`Self::child_parent_data_as`] to downcast to the concrete type.
    pub fn child_parent_data(&self, index: usize) -> Option<&'a dyn ParentData> {
        self.child_parent_data.get(index).copied().flatten()
    }

    /// Parent data for child `index`, downcast to the concrete type `T`.
    ///
    /// Returns `None` if the child has no parent data or if it is not of type `T`.
    /// Container objects call this with their own `Self::ParentData` associated type,
    /// which is the type they installed — mismatches surface as `None` rather than UB.
    pub fn child_parent_data_as<T: ParentData>(&self, index: usize) -> Option<&'a T> {
        self.child_parent_data(index)?.downcast_ref::<T>()
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
            DryBaselineChildResponse::DryLayout(_) | DryBaselineChildResponse::Intrinsic(_) => None,
        }
    }

    /// The size the child would take under `constraints`, without laying it out.
    pub fn child_dry_layout(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        match (self.query)(index, DryBaselineChildRequest::DryLayout(constraints)) {
            DryBaselineChildResponse::DryLayout(size) => size,
            DryBaselineChildResponse::Baseline(_) | DryBaselineChildResponse::Intrinsic(_) => {
                Size::ZERO
            }
        }
    }

    /// The child's intrinsic value for an arbitrary dimension and extent.
    ///
    /// Routes through the same memoized take-out `intrinsic_query` the real-layout
    /// path uses — dry baseline passes share the per-node intrinsic cache.
    pub fn child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        match (self.query)(index, DryBaselineChildRequest::Intrinsic(dimension, extent)) {
            DryBaselineChildResponse::Intrinsic(v) => v,
            DryBaselineChildResponse::Baseline(_) | DryBaselineChildResponse::DryLayout(_) => 0.0,
        }
    }

    /// The child's maximum intrinsic width for the given height.
    pub fn child_max_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxWidth, height)
    }

    /// The child's minimum intrinsic width for the given height.
    pub fn child_min_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinWidth, height)
    }

    /// The child's maximum intrinsic height for the given width.
    pub fn child_max_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxHeight, width)
    }

    /// The child's minimum intrinsic height for the given width.
    pub fn child_min_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinHeight, width)
    }
}

// ============================================================================
// BoxIntrinsicsCtx
// ============================================================================

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
    /// Erased per-child parent data; same semantics as [`BoxDryLayoutCtx::child_parent_data`].
    child_parent_data: &'a [Option<&'a dyn ParentData>],
    query: &'a mut (dyn FnMut(usize, IntrinsicDimension, f32) -> f32 + Send + Sync),
}

impl<'a> BoxIntrinsicsCtx<'a> {
    /// Wraps the driver's child-query callback.
    pub(crate) fn new(
        child_count: usize,
        child_parent_data: &'a [Option<&'a dyn ParentData>],
        query: &'a mut (dyn FnMut(usize, IntrinsicDimension, f32) -> f32 + Send + Sync),
    ) -> Self {
        Self {
            child_count,
            child_parent_data,
            query,
        }
    }

    /// Number of tree children.
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// Type-erased parent data the parent stored on child `index`, or `None`.
    ///
    /// Use [`Self::child_parent_data_as`] for a typed downcast.
    pub fn child_parent_data(&self, index: usize) -> Option<&'a dyn ParentData> {
        self.child_parent_data.get(index).copied().flatten()
    }

    /// Parent data for child `index`, downcast to the concrete type `T`.
    pub fn child_parent_data_as<T: ParentData>(&self, index: usize) -> Option<&'a T> {
        self.child_parent_data(index)?.downcast_ref::<T>()
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
    ///
    /// Convenience downcast to [`FlexParentData`]; replaces the former bespoke
    /// `flex` closure. Multi-axis intrinsic implementations call this instead of
    /// going through [`Self::child_parent_data_as`] directly.
    pub fn child_flex(&self, index: usize) -> i32 {
        self.child_parent_data_as::<FlexParentData>(index)
            .and_then(|pd| pd.flex)
            .unwrap_or(0)
            .max(0)
    }
}

// ============================================================================
// BoxDryLayoutCtx
// ============================================================================

/// Child-query channel for one dry-layout computation.
///
/// Handed to [`RenderBox::compute_dry_layout`]; `child_dry_layout`
/// answers what size the child WOULD take under the given constraints,
/// memoized per child and constraints by the pipeline. `child_intrinsic`
/// and the named convenience methods answer the child's intrinsic dimensions
/// through the same memoized take-out walk — identical to `BoxIntrinsicsCtx`
/// and `BoxLayoutContext::child_intrinsic` so a proxy can share one
/// `child_constraints` helper across all three compute paths.
///
/// The backing field is a dispatched `query` callback rather than two
/// separate callbacks so the slot-map `&mut` is borrowed once: two
/// simultaneous `&mut` borrows of the slot map are impossible, so the
/// two sub-query kinds are packed into one enum-dispatched call.
///
/// [`RenderBox::compute_dry_layout`]: crate::traits::RenderBox::compute_dry_layout
pub struct BoxDryLayoutCtx<'a> {
    child_count: usize,
    /// Erased per-child parent data populated by the driver from each child's
    /// `RenderNode::parent_data`. Indexed by child position; entries are `None`
    /// when the child has no parent data set.
    ///
    /// Downcasting: container objects call [`Self::child_parent_data_as`] with their
    /// own `<Self as RenderBox>::ParentData` — the type they install on children — so
    /// mismatches are impossible in correctly-constructed trees.
    child_parent_data: &'a [Option<&'a dyn ParentData>],
    query:
        &'a mut (dyn FnMut(usize, DryLayoutChildRequest) -> DryLayoutChildResponse + Send + Sync),
}

impl<'a> BoxDryLayoutCtx<'a> {
    /// Wraps the driver's dispatched child-query callback.
    pub(crate) fn new(
        child_count: usize,
        child_parent_data: &'a [Option<&'a dyn ParentData>],
        query: &'a mut (
                    dyn FnMut(usize, DryLayoutChildRequest) -> DryLayoutChildResponse + Send + Sync
                ),
    ) -> Self {
        Self {
            child_count,
            child_parent_data,
            query,
        }
    }

    /// Number of tree children.
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.child_count
    }

    /// Type-erased parent data the parent stored on child `index`, or `None`.
    ///
    /// Use [`Self::child_parent_data_as`] for a typed downcast.
    pub fn child_parent_data(&self, index: usize) -> Option<&'a dyn ParentData> {
        self.child_parent_data.get(index).copied().flatten()
    }

    /// Parent data for child `index`, downcast to the concrete type `T`.
    ///
    /// Returns `None` if the child has no parent data or if it is not of type `T`.
    pub fn child_parent_data_as<T: ParentData>(&self, index: usize) -> Option<&'a T> {
        self.child_parent_data(index)?.downcast_ref::<T>()
    }

    /// The size the child would take under `constraints`, without
    /// laying it out.
    pub fn child_dry_layout(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        match (self.query)(index, DryLayoutChildRequest::DryLayout(constraints)) {
            DryLayoutChildResponse::DryLayout(size) => size,
            DryLayoutChildResponse::Intrinsic(_) => Size::ZERO,
        }
    }

    /// The child's intrinsic value for an arbitrary dimension and extent.
    ///
    /// Routes through the same memoized take-out `intrinsic_query` the real-layout
    /// path uses — dry layout passes share the per-node intrinsic cache, so a child
    /// whose intrinsic was queried during `perform_layout` costs nothing here.
    pub fn child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        match (self.query)(index, DryLayoutChildRequest::Intrinsic(dimension, extent)) {
            DryLayoutChildResponse::Intrinsic(v) => v,
            DryLayoutChildResponse::DryLayout(_) => 0.0,
        }
    }

    /// The child's maximum intrinsic width for the given height.
    pub fn child_max_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxWidth, height)
    }

    /// The child's minimum intrinsic width for the given height.
    pub fn child_min_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinWidth, height)
    }

    /// The child's maximum intrinsic height for the given width.
    pub fn child_max_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxHeight, width)
    }

    /// The child's minimum intrinsic height for the given width.
    pub fn child_min_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinHeight, width)
    }
}

// ============================================================================
// test_support
// ============================================================================

/// Test helpers for exercising `compute_*` intrinsic implementations.
///
/// Promoted from `cfg(test) pub(crate)` so `flui-objects` tests can use
/// these when the `testing` feature is enabled.
#[cfg(any(test, feature = "testing"))]
pub mod test_support {
    use super::*;

    /// A leaf context for unit-testing `compute_*` implementations of
    /// childless objects: any child query is a contract violation and
    /// panics with the probe's coordinates.
    pub fn leaf_intrinsics<R>(f: impl FnOnce(&mut BoxIntrinsicsCtx<'_>) -> R) -> R {
        let mut deny_query = |index: usize, dim: IntrinsicDimension, extent: f32| -> f32 {
            panic!(
                "leaf object queried child {index} ({dim:?} @ {extent}) — \
                 a childless compute_* must not consult children"
            )
        };
        f(&mut BoxIntrinsicsCtx::new(0, &[], &mut deny_query))
    }

    /// Leaf context for `compute_dry_layout` tests; panics on any child query
    /// (both `DryLayout` and `Intrinsic` kinds).
    pub fn leaf_dry_layout<R>(f: impl FnOnce(&mut BoxDryLayoutCtx<'_>) -> R) -> R {
        let mut deny = |index: usize, request: DryLayoutChildRequest| -> DryLayoutChildResponse {
            match request {
                DryLayoutChildRequest::DryLayout(constraints) => panic!(
                    "leaf object dry-laid-out child {index} ({constraints:?}) — \
                         a childless compute_dry_layout must not consult children"
                ),
                DryLayoutChildRequest::Intrinsic(dim, extent) => panic!(
                    "leaf object queried intrinsic of child {index} ({dim:?} @ {extent}) \
                         during dry layout — a childless compute_dry_layout must not consult children"
                ),
            }
        };
        f(&mut BoxDryLayoutCtx::new(0, &[], &mut deny))
    }

    /// Leaf context for `compute_dry_baseline` tests; panics on any child query.
    pub fn leaf_dry_baseline<R>(f: impl FnOnce(&mut BoxDryBaselineCtx<'_>) -> R) -> R {
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
                DryBaselineChildRequest::Intrinsic(dim, extent) => panic!(
                    "leaf object queried intrinsic of child {index} ({dim:?} @ {extent}) \
                     during dry baseline — a childless compute_dry_baseline must not consult children"
                ),
            }
        };
        f(&mut BoxDryBaselineCtx::new(0, &[], &mut deny))
    }
}
