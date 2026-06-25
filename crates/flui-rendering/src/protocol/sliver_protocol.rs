//! Sliver protocol for scrollable viewport layout.
//!
//! This module provides the SliverProtocol and its capability implementations:
//! - [`SliverProtocol`]: Main protocol type for scrollable content
//! - [`SliverLayout`]: Layout capability (SliverConstraints → SliverGeometry)
//! - [`SliverHitTest`]: Hit test capability (MainAxisPosition →
//!   SliverHitTestResult)

use flui_foundation::RenderId;
use flui_tree::Arity;
use flui_types::{
    Size,
    geometry::{Matrix4, Offset, Rect},
};

use crate::{
    constraints::{BoxConstraints, Constraints, SliverConstraints, SliverGeometry},
    parent_data::{ParentData, SliverMultiBoxAdaptorParentData, SliverParentData},
    protocol::{
        box_protocol::BoxProtocol,
        capabilities::{
            ChildLayout, HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi,
        },
        protocol::{Protocol, sealed},
    },
    storage::IntrinsicDimension,
    traits::RenderObject,
};

/// A handle to a Box child materialized by the re-entrant build contract
/// ([`SliverLayoutCtxErased::build_and_layout_box_child`]): the child's tree
/// identity plus the [`Size`] it laid out to.
///
/// The consumer projects `size` onto the scroll axis to feed
/// `Virtualizer::set_measured`, and keeps `id` so it can position, re-measure, or
/// dispose the child on a later pass. Returning identity here (rather than bare
/// geometry) is what lets a future true-mid-pass backend and dispose-on-scroll-off
/// slot in without a breaking change to the contract.
///
/// `#[non_exhaustive]`: the handle may grow (e.g. a baseline or cross-axis offset
/// a grid needs) without a breaking change — same forward-compat intent as
/// [`ChildLayout`]. Built only inside this crate (by the build backend); external
/// consumers read its public fields.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BoxChildRef {
    /// Tree identity of the materialized child.
    pub id: RenderId,
    /// The size the child laid out to under the supplied `BoxConstraints`.
    pub size: Size,
}

impl BoxChildRef {
    /// Builds a handle for a child laid out at `size`. Centralizes construction
    /// of this `#[non_exhaustive]` type so a future field addition touches one
    /// site, not every backend.
    #[must_use]
    pub fn new(id: RenderId, size: Size) -> Self {
        Self { id, size }
    }
}

/// A child the re-entrant build contract asked to materialize during layout, but
/// which the layout walk cannot insert synchronously (its tree borrows are
/// frozen mid-pass). The pipeline drains these after the walk into the deferred-
/// mutation queue — the v1 next-frame backend behind
/// [`SliverLayoutCtxErased::build_and_layout_box_child`].
pub(crate) struct PendingBuild {
    /// Sliver parent the child is built under.
    pub parent: RenderId,
    /// Index within the parent at which to insert the child.
    pub index: usize,
    /// Logical item index to stamp into the child's parent-data after insertion.
    /// This is the key that maps back to the virtualizer's item space and is
    /// distinct from `index` (the dense child-slot position).
    pub logical_index: usize,
    /// Pre-built parent-data to install on the fresh `RenderNode` immediately
    /// after insertion.  Fresh nodes start with `parent_data = None`; the
    /// build backend seeds this field so `apply_deferred_mutation` can set the
    /// logical index even though no parent-data box exists yet.
    ///
    /// `None` for legacy non-lazy inserts that use the stamp-if-present path.
    pub initial_parent_data: Option<Box<dyn ParentData>>,
    /// The freshly-built (not-yet-inserted) child render object.
    pub object: Box<dyn RenderObject<BoxProtocol>>,
}

// ============================================================================
// SLIVER PROTOCOL
// ============================================================================

/// Sliver protocol for scrollable viewport children.
///
/// Slivers are laid out along a single scrolling axis with viewport
/// constraints. Used by scrollable widgets: ListView, GridView,
/// CustomScrollView, etc.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverProtocol;

impl sealed::Sealed for SliverProtocol {}

impl Protocol for SliverProtocol {
    type Layout = SliverLayout;
    type HitTest = SliverHitTest;
    type DefaultParentData = SliverParentData;

    // PORT-CHECK-OK-DYN: protocol-layout-erasure (D-block PR-A1b U19, memo D5)
    type LayoutCtxErased<'ctx> = dyn SliverLayoutCtxErased + 'ctx;

    // No sliver layout cache yet: no sliver object exposes intrinsic
    // queries, so invalidation never needs the cache-driven escalation.
    type LayoutCache = ();

    fn name() -> &'static str {
        "sliver"
    }

    fn debug_assert_layout_output(constraints: &SliverConstraints, geometry: &SliverGeometry) {
        let _ = constraints;
        geometry.debug_assert_valid();
    }

    fn validate_layout_output(
        render_object: &'static str,
        constraints: &SliverConstraints,
        geometry: &SliverGeometry,
    ) -> crate::error::RenderResult<()> {
        let _ = constraints;
        if let Some(reason) = geometry.validation_error() {
            return Err(crate::error::RenderError::invalid_geometry(
                render_object,
                reason,
            ));
        }
        Ok(())
    }

    /// D-block PR-A1b U19 — Sliver counterpart to
    /// [`BoxProtocol::with_leaf_erased_ctx`](super::BoxProtocol::with_leaf_erased_ctx).
    /// Wraps the given `SliverConstraints` in a typed
    /// `SliverLayoutCtx::<Leaf, SliverParentData>::new(constraints)` and
    /// hands an erased `&mut dyn SliverLayoutCtxErased` view to `f`.
    fn with_leaf_erased_ctx<R>(
        constraints: SliverConstraints,
        f: impl FnOnce(&mut Self::LayoutCtxErased<'_>) -> R,
    ) -> R {
        let mut typed = SliverLayoutCtx::<flui_tree::Leaf, SliverParentData>::new(constraints);
        // PORT-CHECK-OK-DYN: protocol-layout-erasure (D-block PR-A1b U19, memo D5)
        let erased: &mut dyn SliverLayoutCtxErased = &mut typed;
        f(erased)
    }
}

// ============================================================================
// SLIVER LAYOUT CAPABILITY
// ============================================================================

/// Layout capability for sliver (scrollable) layout.
///
/// Uses `SliverConstraints` for input and `SliverGeometry` for output.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverLayout;

/// Cache key for SliverConstraints.
///
/// Uses integer representation of floats (bits) for reliable hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SliverConstraintsCacheKey {
    axis_direction: u8,
    growth_direction: u8,
    user_scroll_direction: u8,
    cross_axis_direction: u8,
    cross_axis_extent_bits: u32,
    viewport_main_axis_extent_bits: u32,
    scroll_offset_bits: u32,
    remaining_paint_extent_bits: u32,
    overlap_bits: u32,
    remaining_cache_extent_bits: u32,
    cache_origin_bits: u32,
    preceding_scroll_extent_bits: u32,
}

impl SliverConstraintsCacheKey {
    /// Creates a cache key from constraints.
    ///
    /// Returns `None` if any float value is NaN.
    pub fn from_constraints(c: &SliverConstraints) -> Option<Self> {
        // NaN check helper
        let is_nan = |v: f32| v.is_nan();

        if is_nan(c.cross_axis_extent)
            || is_nan(c.viewport_main_axis_extent)
            || is_nan(c.scroll_offset)
            || is_nan(c.remaining_paint_extent)
            || is_nan(c.overlap)
            || is_nan(c.remaining_cache_extent)
            || is_nan(c.cache_origin)
            || is_nan(c.preceding_scroll_extent)
        {
            return None;
        }

        Some(Self {
            axis_direction: c.axis_direction as u8,
            growth_direction: c.growth_direction as u8,
            user_scroll_direction: c.user_scroll_direction as u8,
            cross_axis_direction: c.cross_axis_direction as u8,
            cross_axis_extent_bits: c.cross_axis_extent.to_bits(),
            viewport_main_axis_extent_bits: c.viewport_main_axis_extent.to_bits(),
            scroll_offset_bits: c.scroll_offset.to_bits(),
            remaining_paint_extent_bits: c.remaining_paint_extent.to_bits(),
            overlap_bits: c.overlap.to_bits(),
            remaining_cache_extent_bits: c.remaining_cache_extent.to_bits(),
            cache_origin_bits: c.cache_origin.to_bits(),
            preceding_scroll_extent_bits: c.preceding_scroll_extent.to_bits(),
        })
    }
}

// ============================================================================
// CHILD STATE
// ============================================================================

/// Per-child layout-time state held by [`SliverLayoutCtx`].
#[derive(Debug)]
pub struct SliverChildState<P: ParentData + Default> {
    /// Render ID of this child.
    pub id: RenderId,
    /// Computed sliver geometry after layout.
    pub geometry: SliverGeometry,
    /// Position offset set by parent.
    pub offset: Offset,
    /// Parent data for this child.
    pub parent_data: P,
}

impl<P: ParentData + Default> SliverChildState<P> {
    /// Creates a new child state with default values.
    pub fn new(id: RenderId) -> Self {
        Self {
            id,
            geometry: SliverGeometry::ZERO,
            offset: Offset::ZERO,
            parent_data: P::default(),
        }
    }
}

/// Callback type for synchronous sliver child layout.
pub type SliverChildLayoutCallback<'a> =
    &'a (dyn Fn(RenderId, SliverConstraints) -> SliverGeometry + Send + Sync);

/// Callback type for cross-protocol box child layout driven by a Sliver parent.
pub type BoxChildLayoutCallback<'a> = &'a (dyn Fn(RenderId, BoxConstraints) -> Size + Send + Sync);

/// Callback type for cross-protocol box child intrinsic queries driven by a
/// Sliver parent.
pub type BoxChildIntrinsicCallback<'a> =
    &'a (dyn Fn(RenderId, IntrinsicDimension, f32) -> f32 + Send + Sync);

/// Dense per-child geometry cache used by Proxy storage.
type ProxySliverChildGeometryCache = Vec<Option<SliverGeometry>>;

impl LayoutCapability for SliverLayout {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type CacheKey = SliverConstraintsCacheKey;
    type Context<'ctx, A: Arity, P: ParentData + Default>
        = SliverLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;

    fn default_geometry() -> Self::Geometry {
        SliverGeometry::ZERO
    }

    fn validate_constraints(constraints: &Self::Constraints) -> bool {
        constraints.is_normalized()
    }

    fn cache_key(constraints: &Self::Constraints) -> Option<Self::CacheKey> {
        SliverConstraintsCacheKey::from_constraints(constraints)
    }

    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints.round_for_cache()
    }
}

/// Sliver layout context implementation.
///
/// Carries two storage modes mirroring [`super::box_protocol::BoxLayoutCtx`]:
///
/// 1. `Direct` (constructor [`SliverLayoutCtx::new`]): owns constraints and a
///    local geometry slot. This is the production path created by
///    [`SliverProtocol::with_leaf_erased_ctx`] and the pipeline.
/// 2. `Proxy` (constructor `SliverLayoutCtx::from_erased`): wraps
///    `&mut dyn SliverLayoutCtxErased` so the
///    `RenderObject<SliverProtocol>` blanket impl can reconstruct a typed
///    `SliverLayoutCtx<T::Arity, T::ParentData>` from the erased GAT
///    boundary and call `RenderSliver::perform_layout`. Completion writes
///    through to the underlying context so both the local cache and the
///    pipeline-side Direct ctx stay consistent.
pub struct SliverLayoutCtx<'ctx, A: Arity, P: ParentData + Default> {
    storage: SliverLayoutCtxStorage<'ctx, P>,
    _phantom: std::marker::PhantomData<(A, P)>,
}

/// Internal storage variants for [`SliverLayoutCtx`].
enum SliverLayoutCtxStorage<'ctx, P: ParentData + Default> {
    /// Production / pipeline path: owns constraints, geometry slot, and
    /// optional child layout access.
    Direct {
        constraints: SliverConstraints,
        children: Option<&'ctx mut Vec<SliverChildState<P>>>,
        child_ids: Option<&'ctx [RenderId]>,
        layout_child_callback: Option<SliverChildLayoutCallback<'ctx>>,
        layout_box_child_callback: Option<BoxChildLayoutCallback<'ctx>>,
        box_child_intrinsic_callback: Option<BoxChildIntrinsicCallback<'ctx>>,
    },
    /// Bridge path: wraps the erased context from the pipeline boundary.
    ///
    /// Constraints are eagerly cached (`SliverConstraints` is `Copy`) so
    /// [`LayoutContextApi::constraints`] can return `&SliverConstraints`
    /// against a stable storage slot rather than an ephemeral owned value.
    ///
    /// Completion writes through to the erased ctx in addition to filling
    /// the local cache, keeping both views consistent.
    // PORT-CHECK-OK-DYN: protocol-layout-erasure (Core.2 W3.1 sliver leaf bridge)
    Proxy {
        constraints: SliverConstraints,
        child_geometries: ProxySliverChildGeometryCache,
        erased: &'ctx mut dyn SliverLayoutCtxErased,
    },
}

impl<'ctx, A: Arity, P: ParentData + Default> SliverLayoutCtx<'ctx, A, P> {
    /// Creates a new sliver layout context with given constraints. Direct storage.
    pub fn new(constraints: SliverConstraints) -> Self {
        Self {
            storage: SliverLayoutCtxStorage::Direct {
                constraints,
                children: None,
                child_ids: None,
                layout_child_callback: None,
                layout_box_child_callback: None,
                box_child_intrinsic_callback: None,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a new sliver layout context with children access.
    pub fn with_children(
        constraints: SliverConstraints,
        children: &'ctx mut Vec<SliverChildState<P>>,
    ) -> Self {
        Self {
            storage: SliverLayoutCtxStorage::Direct {
                constraints,
                children: Some(children),
                child_ids: None,
                layout_child_callback: None,
                layout_box_child_callback: None,
                box_child_intrinsic_callback: None,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a new sliver layout context with synchronous child layout.
    pub fn with_layout_callback(
        constraints: SliverConstraints,
        children: &'ctx mut Vec<SliverChildState<P>>,
        child_ids: &'ctx [RenderId],
        layout_child_callback: SliverChildLayoutCallback<'ctx>,
        layout_box_child_callback: Option<BoxChildLayoutCallback<'ctx>>,
        box_child_intrinsic_callback: Option<BoxChildIntrinsicCallback<'ctx>>,
    ) -> Self {
        Self {
            storage: SliverLayoutCtxStorage::Direct {
                constraints,
                children: Some(children),
                child_ids: Some(child_ids),
                layout_child_callback: Some(layout_child_callback),
                layout_box_child_callback,
                box_child_intrinsic_callback,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Constructs a Proxy-mode `SliverLayoutCtx` that delegates child
    /// operations to the given erased context.
    ///
    /// Used by the `RenderObject<SliverProtocol>` blanket impl in
    /// [`crate::traits::RenderSliver`] to hand a typed
    /// `&mut SliverLayoutCtx<T::Arity, T::ParentData>` to
    /// `RenderSliver::perform_layout`, given only
    /// `&mut dyn SliverLayoutCtxErased` at the trait boundary.
    ///
    /// Constraints are eagerly cached from `erased.constraints()` (cheap —
    /// `SliverConstraints` is `Copy`) so
    /// [`LayoutContextApi::constraints`] can return `&SliverConstraints`
    /// against a stable slot.
    ///
    /// **Visibility** — `pub(crate)`. The only sanctioned consumer is the
    /// `RenderObject<SliverProtocol>` blanket impl in
    /// [`crate::traits::RenderSliver`].
    // PORT-CHECK-OK-DYN: protocol-layout-erasure (Core.2 W3.1 sliver leaf bridge)
    pub(crate) fn from_erased(erased: &'ctx mut dyn SliverLayoutCtxErased) -> Self {
        let constraints = erased.constraints();
        debug_assert!(
            match erased.parent_data_type_id() {
                Some(id) => id == std::any::TypeId::of::<P>(),
                None => true,
            },
            "SliverLayoutCtx::from_erased: ParentData type mismatch — \
             underlying erased ctx reports TypeId={:?}, typed wrapper \
             requested {:?} ({})",
            erased.parent_data_type_id(),
            std::any::TypeId::of::<P>(),
            std::any::type_name::<P>(),
        );
        let child_count = erased.child_count();
        Self {
            storage: SliverLayoutCtxStorage::Proxy {
                constraints,
                child_geometries: vec![None; child_count],
                erased,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // SLIVER-SPECIFIC HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the scroll offset from constraints.
    pub fn scroll_offset(&self) -> f32 {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { constraints, .. }
            | SliverLayoutCtxStorage::Proxy { constraints, .. } => constraints.scroll_offset,
        }
    }

    /// Gets the remaining paint extent.
    pub fn remaining_paint_extent(&self) -> f32 {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { constraints, .. }
            | SliverLayoutCtxStorage::Proxy { constraints, .. } => {
                constraints.remaining_paint_extent
            }
        }
    }

    /// Gets the viewport main axis extent.
    pub fn viewport_main_axis_extent(&self) -> f32 {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { constraints, .. }
            | SliverLayoutCtxStorage::Proxy { constraints, .. } => {
                constraints.viewport_main_axis_extent
            }
        }
    }

    /// Gets the cross axis extent.
    pub fn cross_axis_extent(&self) -> f32 {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { constraints, .. }
            | SliverLayoutCtxStorage::Proxy { constraints, .. } => constraints.cross_axis_extent,
        }
    }

    /// Lays out a Box-protocol child of this Sliver parent.
    pub fn layout_box_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct {
                child_ids,
                layout_box_child_callback,
                ..
            } => {
                if let (Some(child_ids), Some(callback)) =
                    (*child_ids, layout_box_child_callback.as_ref())
                    && let Some(&child_id) = child_ids.get(index)
                {
                    return callback(child_id, constraints);
                }
                Size::ZERO
            }
            SliverLayoutCtxStorage::Proxy { erased, .. } => {
                erased.layout_box_child(index, constraints)
            }
        }
    }

    /// Queries one Box-protocol child's intrinsic dimension.
    pub fn box_child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct {
                child_ids,
                box_child_intrinsic_callback,
                ..
            } => {
                if let (Some(child_ids), Some(callback)) =
                    (*child_ids, box_child_intrinsic_callback.as_ref())
                    && let Some(&child_id) = child_ids.get(index)
                {
                    return callback(child_id, dimension, extent);
                }
                0.0
            }
            SliverLayoutCtxStorage::Proxy { erased, .. } => {
                erased.box_child_intrinsic(index, dimension, extent)
            }
        }
    }
}

impl<'ctx, A: Arity, P: ParentData + Default> LayoutContextApi<'ctx, SliverLayout, A, P>
    for SliverLayoutCtx<'ctx, A, P>
{
    fn constraints(&self) -> &SliverConstraints {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { constraints, .. }
            | SliverLayoutCtxStorage::Proxy { constraints, .. } => constraints,
        }
    }

    fn child_count(&self) -> usize {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { children, .. } => {
                children.as_ref().map(|c| c.len()).unwrap_or(0)
            }
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased.child_count(),
        }
    }

    fn layout_child(&mut self, index: usize, constraints: SliverConstraints) -> SliverGeometry {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct {
                children,
                child_ids,
                layout_child_callback,
                ..
            } => {
                if let (Some(child_ids), Some(callback)) =
                    (*child_ids, layout_child_callback.as_ref())
                    && let Some(&child_id) = child_ids.get(index)
                {
                    let geometry = callback(child_id, constraints);
                    if let Some(children) = children.as_mut()
                        && let Some(child) = children.get_mut(index)
                    {
                        child.geometry = geometry;
                    }
                    return geometry;
                }

                if let Some(children) = children.as_ref()
                    && let Some(child) = children.get(index)
                {
                    return child.geometry;
                }
                SliverGeometry::ZERO
            }
            SliverLayoutCtxStorage::Proxy {
                erased,
                child_geometries,
                ..
            } => {
                let geometry = erased.layout_child(index, constraints);
                if let Some(slot) = child_geometries.get_mut(index) {
                    *slot = Some(geometry);
                }
                geometry
            }
        }
    }

    fn position_child(&mut self, index: usize, offset: Offset) {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct { children, .. } => {
                if let Some(children) = children.as_mut()
                    && let Some(child) = children.get_mut(index)
                {
                    child.offset = offset;
                }
            }
            SliverLayoutCtxStorage::Proxy { erased, .. } => {
                erased.position_child(index, offset);
            }
        }
    }

    fn child_geometry(&self, index: usize) -> Option<&SliverGeometry> {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { children, .. } => children
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|child| &child.geometry),
            SliverLayoutCtxStorage::Proxy {
                child_geometries, ..
            } => child_geometries.get(index).and_then(Option::as_ref),
        }
    }

    fn child_parent_data(&self, index: usize) -> Option<&P> {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { children, .. } => children
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|child| &child.parent_data),
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased
                .child_parent_data_dyn(index)
                .and_then(|d| d.downcast_ref::<P>()),
        }
    }

    fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut P> {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct { children, .. } => children
                .as_mut()
                .and_then(|c| c.get_mut(index))
                .map(|child| &mut child.parent_data),
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased
                .child_parent_data_dyn_or_insert(index, &|| {
                    Box::new(P::default()) as Box<dyn ParentData>
                })
                .and_then(|d| d.downcast_mut::<P>()),
        }
    }
}

// ============================================================================
// SLIVER LAYOUT CTX ERASED (D-block PR-A1b U19 / memo D5 — Sliver counterpart)
// ============================================================================

/// Sliver counterpart to
/// [`BoxLayoutCtxErased`](super::box_protocol::BoxLayoutCtxErased) — protocol-typed but
/// arity- and parent-data-erased view of a sliver layout context for use
/// at the `RenderObject<SliverProtocol>::perform_layout_raw` trait
/// boundary.
///
/// The trait surface mirrors the Box erased layout bridge: the pipeline
/// owns parent-data-erased child slots, while the blanket impl rebuilds a
/// typed `SliverLayoutCtx<T::Arity, T::ParentData>` and delegates child
/// layout / parent-data access through this trait.
pub trait SliverLayoutCtxErased: Send + Sync {
    /// Sliver constraints from parent.
    fn constraints(&self) -> SliverConstraints;

    /// Number of children visible to this context.
    fn child_count(&self) -> usize;

    /// Performs synchronous layout on child at `index` with the given
    /// constraints; returns the child's computed [`SliverGeometry`].
    fn layout_child(&mut self, index: usize, constraints: SliverConstraints) -> SliverGeometry;

    /// Performs synchronous Box layout on child at `index`.
    fn layout_box_child(&mut self, index: usize, constraints: BoxConstraints) -> Size;

    /// On-demand build + layout of a Box child at `index`, materializing it via
    /// `build` when the child does not yet exist. The re-entrant build contract
    /// (ADR-0003 Decision 2): the lazy sibling of [`Self::layout_box_child`], for
    /// children created during the parent's own layout (e.g. a lazy `SliverList`
    /// building only the visible-plus-cache band).
    ///
    /// `logical_index` is the item index in the data source (e.g. the position in
    /// the list). It is distinct from `index` (the dense child-slot in the current
    /// render children vec). The backend stamps `logical_index` into the fresh
    /// child's parent-data after insertion, so the virtualizer reconciliation can
    /// identify which item each child represents.
    ///
    /// Returns a [`ChildLayout<BoxChildRef>`]: `Ready(handle)` when the child is
    /// laid out in this pass (the handle carries the child's id + size, so it can
    /// be re-measured or disposed later — a future true-mid-pass backend),
    /// `Scheduled` when it was queued to be built before a later pass (the v1
    /// next-frame backend), `NoChild` when `build` declines (end of an
    /// unknown-length source), or `Unwired` when no backend is wired (the default).
    ///
    /// `build(index)` is invoked **at most once**, only when a child must be
    /// created, and may return `None` to signal "no item at this index". A backend
    /// that finds the child already present lays it out without calling `build`.
    /// Borrow-safe by construction: this never mutates the render tree directly —
    /// the layout walk's borrows are frozen mid-pass, so a scheduling backend
    /// records the request for the pipeline to apply once the walk releases them.
    fn build_and_layout_box_child(
        &mut self,
        index: usize,
        logical_index: usize,
        constraints: BoxConstraints,
        build: &mut dyn FnMut(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>>,
    ) -> ChildLayout<BoxChildRef> {
        let _ = (index, logical_index, constraints, build);
        ChildLayout::Unwired
    }

    /// Performs a synchronous Box intrinsic query on child at `index`.
    fn box_child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32;

    /// Records the paint offset for child at `index`.
    fn position_child(&mut self, index: usize, offset: Offset);

    /// Reads child `index`'s parent data as `&dyn ParentData`.
    fn child_parent_data_dyn(&self, index: usize) -> Option<&dyn ParentData>;

    /// Mutable counterpart to [`Self::child_parent_data_dyn`].
    fn child_parent_data_dyn_mut(&mut self, index: usize) -> Option<&mut dyn ParentData>;

    /// Mutable access to child `index`'s parent data, creating it when
    /// the erased storage has no slot yet.
    fn child_parent_data_dyn_or_insert(
        &mut self,
        index: usize,
        _create: &dyn Fn() -> Box<dyn ParentData>,
    ) -> Option<&mut dyn ParentData> {
        self.child_parent_data_dyn_mut(index)
    }

    /// `TypeId` of the underlying parent-data type when known.
    fn parent_data_type_id(&self) -> Option<std::any::TypeId> {
        None
    }

    /// Enqueues a deferred removal for the child with the given render id.
    ///
    /// The removal is applied after the current layout walk releases its
    /// borrows (same discipline as [`Self::build_and_layout_box_child`]).
    /// The default is a no-op so leaf / test / Direct contexts need not
    /// override it.
    fn dispose_box_child(&mut self, id: flui_foundation::RenderId) {
        let _ = id;
    }

    /// Returns the [`RenderId`] of the child at
    /// dense slot `index`, if it exists.
    ///
    /// Used by consumers that need to dispose off-band children by id
    /// (see [`Self::dispose_box_child`]). Returns `None` when the slot is
    /// out of range or the context carries no id table (default).
    fn child_id(&self, index: usize) -> Option<flui_foundation::RenderId> {
        let _ = index;
        None
    }
}

impl<A: Arity, P: ParentData + Default> SliverLayoutCtxErased for SliverLayoutCtx<'_, A, P> {
    #[inline]
    fn constraints(&self) -> SliverConstraints {
        // Both Direct and Proxy cache constraints as an owned `Copy` value.
        // Disambiguate via `LayoutContextApi` since both traits define `constraints`.
        *<Self as LayoutContextApi<'_, SliverLayout, A, P>>::constraints(self)
    }

    #[inline]
    fn child_count(&self) -> usize {
        <Self as LayoutContextApi<'_, SliverLayout, A, P>>::child_count(self)
    }

    #[inline]
    fn layout_child(&mut self, index: usize, constraints: SliverConstraints) -> SliverGeometry {
        <Self as LayoutContextApi<'_, SliverLayout, A, P>>::layout_child(self, index, constraints)
    }

    #[inline]
    fn layout_box_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        SliverLayoutCtx::layout_box_child(self, index, constraints)
    }

    #[inline]
    fn build_and_layout_box_child(
        &mut self,
        index: usize,
        logical_index: usize,
        constraints: BoxConstraints,
        build: &mut dyn FnMut(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>>,
    ) -> ChildLayout<BoxChildRef> {
        match &mut self.storage {
            // Direct storage carries no build backend yet — the production
            // next-frame scheduler lands with its consumer (the lazy
            // `SliverList`, U3), at which point a build callback joins the other
            // Direct-storage layout callbacks. Until then there is nothing to
            // materialize: honestly `Unwired` (a bug if a production consumer
            // ever sees it) rather than a silent no-op masquerading as end-of-data.
            SliverLayoutCtxStorage::Direct { .. } => ChildLayout::Unwired,
            // Proxy forwards to the pipeline-built context underneath, so a
            // backend wired there (U3) is reached through this wrapper unchanged.
            SliverLayoutCtxStorage::Proxy { erased, .. } => {
                erased.build_and_layout_box_child(index, logical_index, constraints, build)
            }
        }
    }

    #[inline]
    fn box_child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        SliverLayoutCtx::box_child_intrinsic(self, index, dimension, extent)
    }

    #[inline]
    fn position_child(&mut self, index: usize, offset: Offset) {
        <Self as LayoutContextApi<'_, SliverLayout, A, P>>::position_child(self, index, offset)
    }

    #[inline]
    fn child_parent_data_dyn(&self, index: usize) -> Option<&dyn ParentData> {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { children, .. } => children
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|child| &child.parent_data as &dyn ParentData),
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased.child_parent_data_dyn(index),
        }
    }

    #[inline]
    fn child_parent_data_dyn_mut(&mut self, index: usize) -> Option<&mut dyn ParentData> {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct { children, .. } => children
                .as_mut()
                .and_then(|c| c.get_mut(index))
                .map(|child| &mut child.parent_data as &mut dyn ParentData),
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased.child_parent_data_dyn_mut(index),
        }
    }

    #[inline]
    fn parent_data_type_id(&self) -> Option<std::any::TypeId> {
        match &self.storage {
            SliverLayoutCtxStorage::Direct {
                children: Some(_), ..
            } => Some(std::any::TypeId::of::<P>()),
            SliverLayoutCtxStorage::Direct { children: None, .. }
            | SliverLayoutCtxStorage::Proxy { .. } => None,
        }
    }

    #[inline]
    fn dispose_box_child(&mut self, id: flui_foundation::RenderId) {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct { .. } => {}
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased.dispose_box_child(id),
        }
    }

    #[inline]
    fn child_id(&self, index: usize) -> Option<flui_foundation::RenderId> {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { .. } => None,
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased.child_id(index),
        }
    }
}

// ============================================================================
// ERASED DRIVER LAYOUT CONTEXT
// ============================================================================

/// Per-child layout state with parent-data-erased storage for the sliver
/// production layout walk.
#[derive(Debug)]
pub struct ErasedSliverChildState {
    /// Render ID of this child.
    pub id: RenderId,
    /// Computed sliver geometry after layout.
    pub geometry: SliverGeometry,
    /// Position offset set by parent.
    pub offset: Offset,
    /// Parent data, created on demand by the typed bridge.
    pub parent_data: Option<Box<dyn ParentData>>,
}

impl ErasedSliverChildState {
    /// Creates an empty child slot.
    pub fn new(id: RenderId) -> Self {
        Self {
            id,
            geometry: SliverGeometry::ZERO,
            offset: Offset::ZERO,
            parent_data: None,
        }
    }
}

/// Driver-native, parent-data-erased implementation of
/// [`SliverLayoutCtxErased`] used by the sliver subtree walk.
pub struct ErasedSliverLayoutCtx<'ctx> {
    constraints: SliverConstraints,
    children: &'ctx mut Vec<ErasedSliverChildState>,
    child_ids: &'ctx [RenderId],
    layout_child_callback: SliverChildLayoutCallback<'ctx>,
    layout_box_child_callback: BoxChildLayoutCallback<'ctx>,
    box_child_intrinsic_callback: BoxChildIntrinsicCallback<'ctx>,
    /// Tree id of the sliver being laid out — the parent for on-demand child
    /// builds recorded in `pending_builds`.
    node_id: RenderId,
    /// Sink for the re-entrant build contract's v1 next-frame backend: a child
    /// the parent asked to materialize that the frozen mid-pass borrows cannot
    /// insert now. Shared with the dirty-root walk, which drains it into the
    /// deferred-mutation queue once the walk releases its borrows. Empty unless a
    /// lazy sliver requests a not-yet-built child.
    pending_builds: &'ctx parking_lot::Mutex<Vec<PendingBuild>>,
    /// Symmetric remove sink for the re-entrant build contract (U3c D2): `(parent,
    /// child)` pairs of children the consumer wants evicted from the tree.
    /// The `parent` here is always `self.node_id` (the sliver itself), not the
    /// walk root — the pipeline must call `defer_remove(parent, child)` so that
    /// `mark_needs_layout` targets the sliver and it reflows after its child list
    /// changes.  Drained after the walk releases its borrows, before
    /// pending_builds are applied (Remove → Insert ordering, D3). Same
    /// `Mutex`-for-Send discipline as `pending_builds`.
    pending_removes: &'ctx parking_lot::Mutex<Vec<(RenderId, RenderId)>>,
}

impl<'ctx> ErasedSliverLayoutCtx<'ctx> {
    /// Creates the walk-side context over pre-built child slots. `node_id` is the
    /// sliver being laid out, `pending_builds` is the walk-owned sink for
    /// on-demand child builds (see [`PendingBuild`]), and `pending_removes` is the
    /// symmetric sink for deferred child removals (U3c D2).
    ///
    /// `pub(crate)`: the only constructor caller is the pipeline's sliver layout
    /// walk; the `PendingBuild` sink type it takes is crate-internal.
    pub(crate) fn new(
        constraints: SliverConstraints,
        children: &'ctx mut Vec<ErasedSliverChildState>,
        child_ids: &'ctx [RenderId],
        layout_child_callback: SliverChildLayoutCallback<'ctx>,
        layout_box_child_callback: BoxChildLayoutCallback<'ctx>,
        box_child_intrinsic_callback: BoxChildIntrinsicCallback<'ctx>,
        node_id: RenderId,
        pending_builds: &'ctx parking_lot::Mutex<Vec<PendingBuild>>,
        pending_removes: &'ctx parking_lot::Mutex<Vec<(RenderId, RenderId)>>,
    ) -> Self {
        Self {
            constraints,
            children,
            child_ids,
            layout_child_callback,
            layout_box_child_callback,
            box_child_intrinsic_callback,
            node_id,
            pending_builds,
            pending_removes,
        }
    }
}

impl SliverLayoutCtxErased for ErasedSliverLayoutCtx<'_> {
    fn constraints(&self) -> SliverConstraints {
        self.constraints
    }

    fn child_count(&self) -> usize {
        self.child_ids.len()
    }

    fn layout_child(&mut self, index: usize, constraints: SliverConstraints) -> SliverGeometry {
        let Some(&child_id) = self.child_ids.get(index) else {
            return SliverGeometry::ZERO;
        };
        let geometry = (self.layout_child_callback)(child_id, constraints);
        if let Some(slot) = self.children.get_mut(index) {
            slot.geometry = geometry;
        }
        geometry
    }

    fn layout_box_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        let Some(&child_id) = self.child_ids.get(index) else {
            return Size::ZERO;
        };
        (self.layout_box_child_callback)(child_id, constraints)
    }

    fn build_and_layout_box_child(
        &mut self,
        index: usize,
        logical_index: usize,
        constraints: BoxConstraints,
        build: &mut dyn FnMut(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>>,
    ) -> ChildLayout<BoxChildRef> {
        // Existing child: lay it out through the walk and return its identity +
        // size — this is the `Ready` (mid-pass) arm.
        if let Some(&child_id) = self.child_ids.get(index) {
            let size = (self.layout_box_child_callback)(child_id, constraints);
            return ChildLayout::Ready(BoxChildRef::new(child_id, size));
        }
        // Absent: the layout walk's tree borrows are frozen mid-pass, so a freshly
        // built child cannot be inserted synchronously. Materialize it and record
        // the request; the dirty-root walk drains `pending_builds` into the
        // deferred-mutation queue after it releases its borrows, so the child is
        // inserted and laid out on a later pass (the v1 next-frame backend). A
        // `None` from the builder means the data source has no item here.
        match build(index) {
            Some(object) => {
                // Pre-build the parent-data box so `apply_deferred_mutation`
                // can install it on the fresh `RenderNode` even though the node
                // starts with `parent_data = None`.  This is D1 of the lazy-
                // sliver re-entrant build contract: the logical index must be
                // readable by `perform_layout` on the very next pass.
                let initial_parent_data: Option<Box<dyn ParentData>> = Some(Box::new(
                    SliverMultiBoxAdaptorParentData::new(logical_index),
                ));
                self.pending_builds.lock().push(PendingBuild {
                    parent: self.node_id,
                    index,
                    logical_index,
                    initial_parent_data,
                    object,
                });
                ChildLayout::Scheduled
            }
            None => ChildLayout::NoChild,
        }
    }

    fn box_child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        let Some(&child_id) = self.child_ids.get(index) else {
            return 0.0;
        };
        (self.box_child_intrinsic_callback)(child_id, dimension, extent)
    }

    fn position_child(&mut self, index: usize, offset: Offset) {
        if let Some(slot) = self.children.get_mut(index) {
            slot.offset = offset;
        }
    }

    fn child_parent_data_dyn(&self, index: usize) -> Option<&dyn ParentData> {
        self.children
            .get(index)
            .and_then(|slot| slot.parent_data.as_deref())
    }

    fn child_parent_data_dyn_mut(&mut self, index: usize) -> Option<&mut dyn ParentData> {
        self.children
            .get_mut(index)
            .and_then(|slot| slot.parent_data.as_deref_mut())
    }

    fn child_parent_data_dyn_or_insert(
        &mut self,
        index: usize,
        create: &dyn Fn() -> Box<dyn ParentData>,
    ) -> Option<&mut dyn ParentData> {
        let slot = self.children.get_mut(index)?;
        Some(slot.parent_data.get_or_insert_with(create).as_mut())
    }

    fn parent_data_type_id(&self) -> Option<std::any::TypeId> {
        self.children
            .iter()
            .find_map(|slot| slot.parent_data.as_deref())
            .map(|pd| pd.as_any().type_id())
    }

    fn dispose_box_child(&mut self, id: RenderId) {
        // Tag with `self.node_id` (the sliver) as the parent so the drain in
        // `layout_dirty_root` calls `defer_remove(sliver, child)` rather than
        // `defer_remove(walk_root, child)`. Using the walk root would misdirect
        // `mark_needs_layout` to a distant ancestor, preventing the lazy sliver
        // from reflowing after its child list shrinks.
        self.pending_removes.lock().push((self.node_id, id));
    }

    fn child_id(&self, index: usize) -> Option<RenderId> {
        self.child_ids.get(index).copied()
    }
}

// ============================================================================
// SLIVER HIT TEST CAPABILITY
// ============================================================================

/// Hit test capability for sliver (scrollable) layout.
///
/// Uses main axis position for hit testing along scroll direction.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverHitTest;

impl HitTestCapability for SliverHitTest {
    type Position = MainAxisPosition;
    type Result = SliverHitTestResult;
    type Entry = SliverHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData>
        = SliverHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

/// Main axis position for sliver hit testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MainAxisPosition {
    /// Position along the main (scroll) axis.
    pub main_axis: f32,
    /// Position along the cross axis.
    pub cross_axis: f32,
}

impl MainAxisPosition {
    /// Creates a new main axis position.
    pub fn new(main_axis: f32, cross_axis: f32) -> Self {
        Self {
            main_axis,
            cross_axis,
        }
    }

    /// Creates from an offset assuming vertical scrolling.
    pub fn from_vertical_offset(offset: Offset) -> Self {
        Self::new(offset.dy.get(), offset.dx.get())
    }

    /// Creates from an offset assuming horizontal scrolling.
    pub fn from_horizontal_offset(offset: Offset) -> Self {
        Self::new(offset.dx.get(), offset.dy.get())
    }
}

/// Hit test result for sliver protocol.
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    /// Path of hit test entries.
    pub path: Vec<SliverHitTestEntry>,
}

impl SliverHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self { path: Vec::new() }
    }

    /// Adds an entry to the hit test path.
    pub fn add(&mut self, entry: SliverHitTestEntry) {
        self.path.push(entry);
    }

    /// Returns whether any targets were hit.
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// Returns the number of hit entries.
    pub fn len(&self) -> usize {
        self.path.len()
    }
}

/// Individual hit test entry for sliver protocol.
#[derive(Debug, Clone)]
pub struct SliverHitTestEntry {
    /// Target identifier.
    pub target_id: u64,
    /// Main axis position where hit occurred.
    pub main_axis_position: f32,
}

impl SliverHitTestEntry {
    /// Creates a new sliver hit test entry.
    pub fn new(target_id: u64, main_axis_position: f32) -> Self {
        Self {
            target_id,
            main_axis_position,
        }
    }
}

/// Driver-supplied child recursion for the sliver hit-test walk.
pub type SliverHitTestChildCallback<'a> =
    &'a mut (dyn FnMut(usize, Option<MainAxisPosition>) -> bool + Send + Sync);

/// Sliver hit test context implementation.
pub struct SliverHitTestCtx<'ctx, A: Arity, P: ParentData> {
    position: MainAxisPosition,
    result: SliverHitTestResult,
    child_callback: Option<SliverHitTestChildCallback<'ctx>>,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> SliverHitTestCtx<'ctx, A, P> {
    /// Creates a new sliver hit test context.
    pub fn new(position: MainAxisPosition) -> Self {
        Self {
            position,
            result: SliverHitTestResult::new(),
            child_callback: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a context wired to the pipeline driver's child recursion.
    pub fn with_child_callback(
        position: MainAxisPosition,
        callback: SliverHitTestChildCallback<'ctx>,
    ) -> Self {
        Self {
            position,
            result: SliverHitTestResult::new(),
            child_callback: Some(callback),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Adds self as a hit target with the given render ID.
    pub fn add_self(&mut self, target_id: RenderId) {
        self.result.add(SliverHitTestEntry::new(
            target_id.as_u64(),
            self.position.main_axis,
        ));
    }
}

impl<'ctx, A: Arity, P: ParentData> HitTestContextApi<'ctx, SliverHitTest, A, P>
    for SliverHitTestCtx<'ctx, A, P>
{
    fn position(&self) -> &MainAxisPosition {
        &self.position
    }

    fn result(&self) -> &SliverHitTestResult {
        &self.result
    }

    fn result_mut(&mut self) -> &mut SliverHitTestResult {
        &mut self.result
    }

    fn add_hit(&mut self, entry: SliverHitTestEntry) {
        self.result.add(entry);
    }

    fn is_hit(&self, bounds: Rect) -> bool {
        // Sliver bounds are interpreted as cross-axis width by main-axis height.
        self.position.main_axis >= 0.0
            && self.position.main_axis < bounds.height().get()
            && self.position.cross_axis >= 0.0
            && self.position.cross_axis < bounds.width().get()
    }

    fn hit_test_child(&mut self, index: usize, position: MainAxisPosition) -> bool {
        match self.child_callback.as_mut() {
            Some(callback) => callback(index, Some(position)),
            None => false,
        }
    }

    fn hit_test_child_at_layout_offset(&mut self, index: usize) -> bool {
        match self.child_callback.as_mut() {
            Some(callback) => callback(index, None),
            None => false,
        }
    }

    fn push_transform(&mut self, _transform: Matrix4) {
        // Slivers typically use main axis offset instead of full transforms
    }

    fn pop_transform(&mut self) {
        // No-op for basic sliver hit test
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_tree::Leaf;
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_sliver_protocol_name() {
        assert_eq!(SliverProtocol::name(), "sliver");
    }

    #[test]
    fn test_sliver_layout_default_geometry() {
        let geometry = SliverLayout::default_geometry();
        assert_eq!(geometry, SliverGeometry::ZERO);
    }

    /// The re-entrant build contract is mid-pass-shaped: a consumer folds every
    /// `ChildLayout` state to a distinct response — real extent on `Ready`
    /// (mid-pass), estimate on `Scheduled` (v1 next-frame), stop on `NoChild`
    /// (end of data), and "bug" on `Unwired` (a wired consumer must never see it).
    /// Swapping the v1 next-frame backend for a future true-mid-pass backend only
    /// changes which arm fires, never this call site — the forward-compat property.
    #[test]
    fn child_layout_consumer_handles_all_states() {
        #[derive(Debug, PartialEq)]
        enum Step {
            Use(f32),
            Estimate(f32),
            Stop,
            Bug,
        }
        fn classify(outcome: ChildLayout<f32>, estimate: f32) -> Step {
            match outcome {
                ChildLayout::Ready(extent) => Step::Use(extent),
                ChildLayout::Scheduled => Step::Estimate(estimate),
                ChildLayout::NoChild => Step::Stop,
                ChildLayout::Unwired => Step::Bug,
            }
        }
        assert_eq!(classify(ChildLayout::Ready(42.0), 10.0), Step::Use(42.0));
        assert_eq!(classify(ChildLayout::Scheduled, 10.0), Step::Estimate(10.0));
        assert_eq!(classify(ChildLayout::<f32>::NoChild, 10.0), Step::Stop);
        assert_eq!(classify(ChildLayout::<f32>::Unwired, 10.0), Step::Bug);
    }

    /// A `Direct`-storage context has no build backend wired (the production
    /// next-frame scheduler lands with its consumer, the lazy `SliverList`), so
    /// the contract returns `Unwired` — distinct from `NoChild`/end-of-data — and
    /// must NOT invoke the builder, since there is nowhere to put what it creates.
    #[test]
    fn build_and_layout_box_child_unwired_without_backend_never_builds() {
        use flui_types::layout::AxisDirection;

        use crate::{constraints::GrowthDirection, view::ScrollDirection};

        let constraints = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            ScrollDirection::Idle,
            0.0,
            0.0,
            0.0,
            600.0,
            400.0,
            AxisDirection::LeftToRight,
            600.0,
            600.0,
            0.0,
        );
        let mut ctx = SliverLayoutCtx::<Leaf, SliverParentData>::new(constraints);

        // The builder panics if called: `Unwired` must be reached without building.
        let mut build = |_index: usize| -> Option<Box<dyn RenderObject<BoxProtocol>>> {
            panic!("build must not run when the context has no build backend wired")
        };
        let outcome = SliverLayoutCtxErased::build_and_layout_box_child(
            &mut ctx,
            0,
            0,
            BoxConstraints::tight(Size::new(px(100.0), px(20.0))),
            &mut build,
        );
        assert_eq!(outcome, ChildLayout::Unwired);
    }

    /// The production walk-side context (`ErasedSliverLayoutCtx`) is the real v1
    /// next-frame backend: an existing child lays out to `Ready(handle)`, an
    /// absent index materializes via the builder and parks the request in the
    /// shared sink as `Scheduled`, and a declining builder yields `NoChild`
    /// without parking anything.
    #[test]
    fn erased_sliver_ctx_backend_ready_scheduled_nochild() {
        use flui_foundation::RenderId;
        use flui_tree::Leaf;
        use flui_types::{Size, geometry::px};

        use crate::{context::BoxLayoutContext, parent_data::BoxParentData, traits::RenderBox};

        /// Minimal leaf stub — only needed to satisfy the `build_one` closure's
        /// `Box<dyn RenderObject<BoxProtocol>>` return type. The test checks the
        /// scheduling contract, not the object's own layout behavior.
        #[derive(Debug)]
        struct BoxStub;
        impl flui_foundation::Diagnosticable for BoxStub {}
        impl RenderBox for BoxStub {
            type Arity = Leaf;
            type ParentData = BoxParentData;
            fn perform_layout(
                &mut self,
                _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
            ) -> Size {
                Size::new(px(50.0), px(30.0))
            }
            fn paint(&self, _ctx: &mut crate::context::PaintCx<'_, Leaf>) {}
        }

        let constraints = SliverConstraints::new(
            flui_types::layout::AxisDirection::TopToBottom,
            crate::constraints::GrowthDirection::Forward,
            crate::view::ScrollDirection::Idle,
            0.0,
            0.0,
            0.0,
            600.0,
            400.0,
            flui_types::layout::AxisDirection::LeftToRight,
            600.0,
            600.0,
            0.0,
        );

        let existing = RenderId::new(1);
        let parent = RenderId::new(99);
        let mut children = vec![ErasedSliverChildState::new(existing)];
        let child_ids = [existing];
        let sink: parking_lot::Mutex<Vec<PendingBuild>> = parking_lot::Mutex::new(Vec::new());

        // Existing-child layout returns a fixed size; sliver/intrinsic callbacks
        // are unused by this test but required to build the context.
        let layout_box =
            |_id: RenderId, _c: BoxConstraints| -> Size { Size::new(px(50.0), px(30.0)) };
        let layout_sliver =
            |_id: RenderId, _c: SliverConstraints| -> SliverGeometry { SliverGeometry::ZERO };
        let intrinsic = |_id: RenderId, _d: IntrinsicDimension, _e: f32| -> f32 { 0.0 };

        let pending_removes: parking_lot::Mutex<Vec<(RenderId, RenderId)>> =
            parking_lot::Mutex::new(Vec::new());
        let mut ctx = ErasedSliverLayoutCtx::new(
            constraints,
            &mut children,
            &child_ids,
            &layout_sliver,
            &layout_box,
            &intrinsic,
            parent,
            &sink,
            &pending_removes,
        );

        // index 0 exists -> Ready(handle), builder untouched, nothing parked.
        let mut never = |_idx: usize| -> Option<Box<dyn RenderObject<BoxProtocol>>> {
            panic!("must not build")
        };
        let r0 = SliverLayoutCtxErased::build_and_layout_box_child(
            &mut ctx,
            0,
            0,
            BoxConstraints::tight(Size::new(px(50.0), px(30.0))),
            &mut never,
        );
        assert_eq!(
            r0,
            ChildLayout::Ready(BoxChildRef::new(existing, Size::new(px(50.0), px(30.0))))
        );
        assert!(
            sink.lock().is_empty(),
            "existing child must not park a build"
        );

        // index 1 absent, builder produces -> Scheduled + one parked request.
        let mut build_one =
            |_idx: usize| -> Option<Box<dyn RenderObject<BoxProtocol>>> { Some(Box::new(BoxStub)) };
        let r1 = SliverLayoutCtxErased::build_and_layout_box_child(
            &mut ctx,
            1,
            1,
            BoxConstraints::tight(Size::ZERO),
            &mut build_one,
        );
        assert_eq!(r1, ChildLayout::Scheduled);
        {
            let parked = sink.lock();
            assert_eq!(parked.len(), 1, "absent child must park exactly one build");
            assert_eq!(parked[0].parent, parent);
            assert_eq!(parked[0].index, 1);
        }

        // index 5 absent, builder declines -> NoChild, nothing newly parked.
        let mut decline = |_idx: usize| -> Option<Box<dyn RenderObject<BoxProtocol>>> { None };
        let r2 = SliverLayoutCtxErased::build_and_layout_box_child(
            &mut ctx,
            5,
            5,
            BoxConstraints::tight(Size::ZERO),
            &mut decline,
        );
        assert_eq!(r2, ChildLayout::NoChild);
        assert_eq!(sink.lock().len(), 1, "a declined build must not park");
    }

    #[test]
    fn test_main_axis_position() {
        let pos = MainAxisPosition::new(100.0, 50.0);
        assert_eq!(pos.main_axis, 100.0);
        assert_eq!(pos.cross_axis, 50.0);
    }

    #[test]
    fn test_sliver_hit_test_result() {
        let mut result = SliverHitTestResult::new();
        assert!(result.is_empty());

        result.add(SliverHitTestEntry::new(1, 100.0));
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn sliver_hit_test_context_checks_main_and_cross_axis_bounds() {
        let bounds = Rect::from_ltrb(px(0.0), px(0.0), px(30.0), px(50.0));

        let inside: SliverHitTestCtx<'_, Leaf, SliverParentData> =
            SliverHitTestCtx::new(MainAxisPosition::new(49.999, 29.999));
        assert!(inside.is_hit(bounds));

        let main_upper_edge: SliverHitTestCtx<'_, Leaf, SliverParentData> =
            SliverHitTestCtx::new(MainAxisPosition::new(50.0, 10.0));
        assert!(!main_upper_edge.is_hit(bounds));

        let cross_upper_edge: SliverHitTestCtx<'_, Leaf, SliverParentData> =
            SliverHitTestCtx::new(MainAxisPosition::new(10.0, 30.0));
        assert!(!cross_upper_edge.is_hit(bounds));

        let negative_cross: SliverHitTestCtx<'_, Leaf, SliverParentData> =
            SliverHitTestCtx::new(MainAxisPosition::new(10.0, -0.1));
        assert!(!negative_cross.is_hit(bounds));
    }

    #[test]
    fn sliver_constraints_cache_key_includes_all_direction_fields() {
        use flui_types::layout::AxisDirection;

        use crate::{constraints::GrowthDirection, view::ScrollDirection};

        let base = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            ScrollDirection::Idle,
            10.0,
            20.0,
            0.0,
            100.0,
            300.0,
            AxisDirection::LeftToRight,
            100.0,
            120.0,
            -20.0,
        );

        let mut changed_user_scroll = base;
        changed_user_scroll.user_scroll_direction = ScrollDirection::Forward;
        assert_ne!(
            SliverConstraintsCacheKey::from_constraints(&base),
            SliverConstraintsCacheKey::from_constraints(&changed_user_scroll),
            "user_scroll_direction participates in SliverConstraints::Hash and must also \
             participate in the layout cache key",
        );

        let mut changed_cross_axis = base;
        changed_cross_axis.cross_axis_direction = AxisDirection::RightToLeft;
        assert_ne!(
            SliverConstraintsCacheKey::from_constraints(&base),
            SliverConstraintsCacheKey::from_constraints(&changed_cross_axis),
            "cross_axis_direction participates in SliverConstraints::Hash and must also \
             participate in the layout cache key",
        );
    }

    /// Exercises `SliverHitTestCtx::add_self` end-to-end.
    ///
    /// Constructs a real `SliverHitTestCtx`, calls `add_self(id)`, then asserts
    /// the entry written into the result carries `target_id == id.as_u64()`.
    /// A regression in the body (wrong accessor or cast) would fail this test.
    #[test]
    fn add_self_writes_render_id_as_u64_into_sliver_hit_result() {
        let id = RenderId::new(3);
        let main_axis_pos = MainAxisPosition::new(42.0, 10.0);
        let mut ctx: SliverHitTestCtx<'_, Leaf, SliverParentData> =
            SliverHitTestCtx::new(main_axis_pos);

        ctx.add_self(id);

        let entries = &ctx.result().path;
        assert_eq!(entries.len(), 1, "exactly one entry after add_self");
        assert_eq!(
            entries[0].target_id,
            id.as_u64(),
            "stored target_id must equal id.as_u64()"
        );
        assert_eq!(
            entries[0].main_axis_position, 42.0,
            "main_axis_position must reflect the context position at call time"
        );
    }
}
