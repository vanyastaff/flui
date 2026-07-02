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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_types::{Offset, geometry::px};

    use super::*;
    use crate::parent_data::{BoxParentData, FlexFit, FlexParentData};

    // ------------------------------------------------------------------
    // BoxIntrinsicsCtx
    // ------------------------------------------------------------------

    #[test]
    fn box_intrinsics_ctx_dispatches_and_reports_child_count() {
        let mut query = |index: usize, dim: IntrinsicDimension, extent: f32| -> f32 {
            assert_eq!(index, 2);
            assert_eq!(dim, IntrinsicDimension::MaxHeight);
            assert_eq!(extent, 42.0);
            99.0
        };
        let mut ctx = BoxIntrinsicsCtx::new(3, &[], &mut query);

        assert_eq!(ctx.child_count(), 3);
        assert_eq!(
            ctx.child_intrinsic(2, IntrinsicDimension::MaxHeight, 42.0),
            99.0
        );
    }

    #[test]
    fn box_intrinsics_ctx_named_convenience_methods_pass_the_correct_dimension() {
        let mut query = |_index: usize, dim: IntrinsicDimension, extent: f32| -> f32 {
            // Encode which dimension fired into the return value so each
            // convenience wrapper's assertion can tell them apart.
            match dim {
                IntrinsicDimension::MinWidth => extent + 1.0,
                IntrinsicDimension::MaxWidth => extent + 2.0,
                IntrinsicDimension::MinHeight => extent + 3.0,
                IntrinsicDimension::MaxHeight => extent + 4.0,
            }
        };
        let mut ctx = BoxIntrinsicsCtx::new(1, &[], &mut query);

        assert_eq!(ctx.child_min_intrinsic_width(0, 10.0), 11.0);
        assert_eq!(ctx.child_max_intrinsic_width(0, 10.0), 12.0);
        assert_eq!(ctx.child_min_intrinsic_height(0, 10.0), 13.0);
        assert_eq!(ctx.child_max_intrinsic_height(0, 10.0), 14.0);
    }

    #[test]
    fn box_intrinsics_ctx_parent_data_accessors_downcast_and_report_out_of_range() {
        let flex_data = FlexParentData::flexible(3);
        let slots: [Option<&dyn ParentData>; 2] = [Some(&flex_data), None];
        let mut query = |_i: usize, _d: IntrinsicDimension, _e: f32| -> f32 { 0.0 };
        let ctx = BoxIntrinsicsCtx::new(2, &slots, &mut query);

        assert!(ctx.child_parent_data(0).is_some());
        assert!(
            ctx.child_parent_data(1).is_none(),
            "no parent data set for this slot"
        );
        assert!(
            ctx.child_parent_data(5).is_none(),
            "index beyond child_count"
        );

        assert_eq!(
            ctx.child_parent_data_as::<FlexParentData>(0).unwrap().flex,
            Some(3)
        );
        assert!(
            ctx.child_parent_data_as::<BoxParentData>(0).is_none(),
            "downcast to the wrong concrete type must fail cleanly, not panic"
        );
    }

    #[test]
    fn box_intrinsics_ctx_child_flex_defaults_to_zero_and_clamps_negative() {
        let flexible = FlexParentData::flexible(5);
        let negative = FlexParentData::new(Offset::ZERO, Some(-2), FlexFit::Loose);
        let inflexible = FlexParentData::inflexible();
        let wrong_type = BoxParentData::zero();
        let slots: [Option<&dyn ParentData>; 4] = [
            Some(&flexible),
            Some(&negative),
            Some(&inflexible),
            Some(&wrong_type),
        ];
        let mut query = |_i: usize, _d: IntrinsicDimension, _e: f32| -> f32 { 0.0 };
        let ctx = BoxIntrinsicsCtx::new(4, &slots, &mut query);

        assert_eq!(ctx.child_flex(0), 5);
        assert_eq!(ctx.child_flex(1), 0, "negative flex must clamp to zero");
        assert_eq!(ctx.child_flex(2), 0, "None flex (inflexible) reports zero");
        assert_eq!(
            ctx.child_flex(3),
            0,
            "wrong parent-data type reports zero, not a panic"
        );
        assert_eq!(ctx.child_flex(9), 0, "out-of-range index reports zero");
    }

    // ------------------------------------------------------------------
    // BoxDryLayoutCtx
    // ------------------------------------------------------------------

    #[test]
    fn box_dry_layout_ctx_dispatches_dry_layout_and_intrinsic_requests() {
        let expected_size = Size::new(px(30.0), px(40.0));
        let mut query = |_index: usize, request: DryLayoutChildRequest| -> DryLayoutChildResponse {
            match request {
                DryLayoutChildRequest::DryLayout(_) => {
                    DryLayoutChildResponse::DryLayout(expected_size)
                }
                DryLayoutChildRequest::Intrinsic(_, extent) => {
                    DryLayoutChildResponse::Intrinsic(extent * 2.0)
                }
            }
        };
        let mut ctx = BoxDryLayoutCtx::new(1, &[], &mut query);

        assert_eq!(ctx.child_count(), 1);
        assert_eq!(
            ctx.child_dry_layout(0, BoxConstraints::tight(Size::ZERO)),
            expected_size
        );
        assert_eq!(
            ctx.child_intrinsic(0, IntrinsicDimension::MinWidth, 21.0),
            42.0
        );
        assert_eq!(ctx.child_max_intrinsic_width(0, 5.0), 10.0);
        assert_eq!(ctx.child_min_intrinsic_width(0, 5.0), 10.0);
        assert_eq!(ctx.child_max_intrinsic_height(0, 5.0), 10.0);
        assert_eq!(ctx.child_min_intrinsic_height(0, 5.0), 10.0);
    }

    #[test]
    fn box_dry_layout_ctx_falls_back_to_a_safe_default_on_a_mismatched_response() {
        // A misbehaving driver that always answers with the WRONG response
        // variant must not panic or return garbage -- the ctx methods
        // defensively coerce to Size::ZERO / 0.0 rather than trusting the
        // driver's response shape.
        let mut always_wrong_kind =
            |_index: usize, request: DryLayoutChildRequest| -> DryLayoutChildResponse {
                match request {
                    DryLayoutChildRequest::DryLayout(_) => DryLayoutChildResponse::Intrinsic(1.0),
                    DryLayoutChildRequest::Intrinsic(..) => {
                        DryLayoutChildResponse::DryLayout(Size::new(px(1.0), px(1.0)))
                    }
                }
            };
        let mut ctx = BoxDryLayoutCtx::new(1, &[], &mut always_wrong_kind);

        assert_eq!(
            ctx.child_dry_layout(0, BoxConstraints::tight(Size::ZERO)),
            Size::ZERO
        );
        assert_eq!(
            ctx.child_intrinsic(0, IntrinsicDimension::MinWidth, 1.0),
            0.0
        );
    }

    #[test]
    fn box_dry_layout_ctx_parent_data_accessor_downcasts_by_index() {
        let flex_data = FlexParentData::flexible(1);
        let slots: [Option<&dyn ParentData>; 1] = [Some(&flex_data)];
        let mut query = |_i: usize, _r: DryLayoutChildRequest| -> DryLayoutChildResponse {
            DryLayoutChildResponse::DryLayout(Size::ZERO)
        };
        let ctx = BoxDryLayoutCtx::new(1, &slots, &mut query);

        assert_eq!(
            ctx.child_parent_data_as::<FlexParentData>(0).unwrap().flex,
            Some(1)
        );
        assert!(ctx.child_parent_data_as::<BoxParentData>(0).is_none());
    }

    // ------------------------------------------------------------------
    // BoxDryBaselineCtx
    // ------------------------------------------------------------------

    #[test]
    fn box_dry_baseline_ctx_dispatches_baseline_layout_and_intrinsic_requests() {
        let expected_size = Size::new(px(11.0), px(22.0));
        let mut query =
            |_index: usize, request: DryBaselineChildRequest| -> DryBaselineChildResponse {
                match request {
                    DryBaselineChildRequest::Baseline(_, _) => {
                        DryBaselineChildResponse::Baseline(Some(17.0))
                    }
                    DryBaselineChildRequest::DryLayout(_) => {
                        DryBaselineChildResponse::DryLayout(expected_size)
                    }
                    DryBaselineChildRequest::Intrinsic(_, extent) => {
                        DryBaselineChildResponse::Intrinsic(extent + 1.0)
                    }
                }
            };
        let mut ctx = BoxDryBaselineCtx::new(1, &[], &mut query);

        assert_eq!(ctx.child_count(), 1);
        assert_eq!(
            ctx.child_dry_baseline(
                0,
                BoxConstraints::tight(Size::ZERO),
                TextBaseline::Alphabetic
            ),
            Some(17.0)
        );
        assert_eq!(
            ctx.child_dry_layout(0, BoxConstraints::tight(Size::ZERO)),
            expected_size
        );
        assert_eq!(
            ctx.child_intrinsic(0, IntrinsicDimension::MinWidth, 9.0),
            10.0
        );
        assert_eq!(ctx.child_max_intrinsic_width(0, 9.0), 10.0);
        assert_eq!(ctx.child_min_intrinsic_width(0, 9.0), 10.0);
        assert_eq!(ctx.child_max_intrinsic_height(0, 9.0), 10.0);
        assert_eq!(ctx.child_min_intrinsic_height(0, 9.0), 10.0);
    }

    #[test]
    fn box_dry_baseline_ctx_falls_back_to_a_safe_default_on_a_mismatched_response() {
        let mut always_wrong_kind = |_index: usize,
                                     request: DryBaselineChildRequest|
         -> DryBaselineChildResponse {
            match request {
                DryBaselineChildRequest::Baseline(..) => {
                    DryBaselineChildResponse::DryLayout(Size::ZERO)
                }
                DryBaselineChildRequest::DryLayout(_) => DryBaselineChildResponse::Intrinsic(1.0),
                DryBaselineChildRequest::Intrinsic(..) => {
                    DryBaselineChildResponse::Baseline(Some(1.0))
                }
            }
        };
        let mut ctx = BoxDryBaselineCtx::new(1, &[], &mut always_wrong_kind);

        assert_eq!(
            ctx.child_dry_baseline(
                0,
                BoxConstraints::tight(Size::ZERO),
                TextBaseline::Alphabetic
            ),
            None
        );
        assert_eq!(
            ctx.child_dry_layout(0, BoxConstraints::tight(Size::ZERO)),
            Size::ZERO
        );
        assert_eq!(
            ctx.child_intrinsic(0, IntrinsicDimension::MinWidth, 1.0),
            0.0
        );
    }

    #[test]
    fn box_dry_baseline_ctx_parent_data_accessor_downcasts_by_index() {
        let flex_data = FlexParentData::flexible(4);
        let slots: [Option<&dyn ParentData>; 1] = [Some(&flex_data)];
        let mut query = |_i: usize, _r: DryBaselineChildRequest| -> DryBaselineChildResponse {
            DryBaselineChildResponse::Baseline(None)
        };
        let ctx = BoxDryBaselineCtx::new(1, &slots, &mut query);

        assert_eq!(
            ctx.child_parent_data_as::<FlexParentData>(0).unwrap().flex,
            Some(4)
        );
        assert!(ctx.child_parent_data_as::<BoxParentData>(0).is_none());
    }

    // ------------------------------------------------------------------
    // test_support leaf helpers -- the panic-on-child-query contract
    // ------------------------------------------------------------------

    #[test]
    fn leaf_intrinsics_reports_zero_children_and_never_queries_when_unused() {
        let result = test_support::leaf_intrinsics(|ctx| ctx.child_count());
        assert_eq!(result, 0);
    }

    #[test]
    #[should_panic(expected = "a childless compute_* must not consult children")]
    fn leaf_intrinsics_panics_if_a_child_is_queried() {
        test_support::leaf_intrinsics(|ctx| {
            ctx.child_intrinsic(0, IntrinsicDimension::MinWidth, 0.0)
        });
    }

    #[test]
    #[should_panic(expected = "a childless compute_dry_layout must not consult children")]
    fn leaf_dry_layout_panics_if_a_child_is_queried() {
        test_support::leaf_dry_layout(|ctx| {
            ctx.child_dry_layout(0, BoxConstraints::tight(Size::ZERO))
        });
    }

    #[test]
    #[should_panic(expected = "a childless compute_dry_baseline must not consult children")]
    fn leaf_dry_baseline_panics_if_a_child_is_queried() {
        test_support::leaf_dry_baseline(|ctx| {
            ctx.child_dry_baseline(
                0,
                BoxConstraints::tight(Size::ZERO),
                TextBaseline::Alphabetic,
            )
        });
    }
}
