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
    parent_data::{ParentData, SliverParentData},
    protocol::{
        capabilities::{
            ChildLayout, HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi,
        },
        protocol::{Protocol, sealed},
    },
    storage::IntrinsicDimension,
};

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

    // PORT-CHECK-OK-DYN: protocol-layout-erasure — sanctioned erased layout-context boundary
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
        if let Some(reason) = geometry.content_contract_violation() {
            // A content bug, not a pipeline hazard: commit and consume the
            // geometry the way a Flutter RELEASE build does (the matching
            // Flutter checks are debug-only asserts, `sliver.dart:881-894`).
            // Rejecting it instead leaves the previous committed geometry
            // in place on every retry — a silent, permanent viewport
            // freeze.
            tracing::warn!(
                render_object,
                reason,
                "sliver geometry violates its content contract; committed as-is"
            );
        }
        Ok(())
    }

    /// Sliver counterpart to
    /// [`BoxProtocol::with_leaf_erased_ctx`](super::BoxProtocol::with_leaf_erased_ctx).
    /// Wraps the given `SliverConstraints` in a typed
    /// `SliverLayoutCtx::<Leaf, SliverParentData>::new(constraints)` and
    /// hands an erased `&mut dyn SliverLayoutCtxErased` view to `f`.
    fn with_leaf_erased_ctx<R>(
        constraints: SliverConstraints,
        f: impl FnOnce(&mut Self::LayoutCtxErased<'_>) -> R,
    ) -> R {
        let mut typed = SliverLayoutCtx::<flui_tree::Leaf, SliverParentData>::new(constraints);
        // PORT-CHECK-OK-DYN: protocol-layout-erasure — sanctioned erased layout-context boundary
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
    /// Whether the parent laid this child out during this pass — see the box
    /// protocol's `ChildState::laid_out_this_pass`, which this mirrors.
    pub laid_out_this_pass: bool,
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
            laid_out_this_pass: false,
            parent_data: P::default(),
        }
    }
}

/// Callback type for synchronous sliver child layout.
pub type SliverChildLayoutCallback<'a> = &'a dyn Fn(RenderId, SliverConstraints) -> SliverGeometry;

/// Callback type for cross-protocol box child layout driven by a Sliver parent.
pub type BoxChildLayoutCallback<'a> = &'a dyn Fn(RenderId, BoxConstraints) -> Size;

/// Callback type for cross-protocol box child intrinsic queries driven by a
/// Sliver parent.
pub type BoxChildIntrinsicCallback<'a> = &'a dyn Fn(RenderId, IntrinsicDimension, f32) -> f32;

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

impl<A: Arity, P: ParentData + Default> std::fmt::Debug for SliverLayoutCtx<'_, A, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Storage holds live driver callbacks / an erased pipeline context;
        // report the storage mode and the (Copy) constraints only.
        let (mode, constraints) = match &self.storage {
            SliverLayoutCtxStorage::Direct { constraints, .. } => ("Direct", constraints),
            SliverLayoutCtxStorage::Proxy { constraints, .. } => ("Proxy", constraints),
        };
        f.debug_struct("SliverLayoutCtx")
            .field("storage", &mode)
            .field("constraints", constraints)
            .finish_non_exhaustive()
    }
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
                children.as_ref().map_or(0, |c| c.len())
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
                        child.laid_out_this_pass = true;
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
                    child.laid_out_this_pass = true;
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
// SLIVER LAYOUT CTX ERASED (Sliver counterpart)
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
/// layout / parent-data access through this trait. No longer `Send +
/// Sync`-bound, for the same reason as
/// [`BoxLayoutCtxErased`](super::box_protocol::BoxLayoutCtxErased).
pub trait SliverLayoutCtxErased {
    /// Sliver constraints from parent.
    fn constraints(&self) -> SliverConstraints;

    /// Number of children visible to this context.
    fn child_count(&self) -> usize;

    /// Whether a descendant's layout was degraded during this node's pass
    /// (see `DegradationProbe`); contexts without a probe answer `false`.
    fn descendant_layout_degraded(&self) -> bool {
        false
    }

    /// Performs synchronous layout on child at `index` with the given
    /// constraints; returns the child's computed [`SliverGeometry`].
    fn layout_child(&mut self, index: usize, constraints: SliverConstraints) -> SliverGeometry;

    /// Performs synchronous Box layout on child at `index`.
    fn layout_box_child(&mut self, index: usize, constraints: BoxConstraints) -> Size;

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

    /// Records a child-build request for `logical_index` under this sliver
    /// — the producer half of the request-strategy seam.
    ///
    /// The caller does **not** supply a pre-built render object — the
    /// element tree decides what to build and at which dense slot to insert
    /// it. The request is parked in the arena's `pending_child_requests`
    /// sink; after the walk releases its borrows the pipeline moves it into
    /// `PipelineOwner::pending_child_requests` for the binding layer, which
    /// services it between layout passes of the same frame's fixpoint.
    ///
    /// Default: `Unwired` — Direct / test / leaf contexts that carry no sink
    /// are honestly inert rather than silently discarding the request.
    fn request_child_build(&mut self, logical_index: usize) -> ChildLayout {
        let _ = logical_index;
        ChildLayout::Unwired
    }

    /// Emit the element-owned retain band `[first, last)` for this sliver
    /// — the removal half of the request-strategy seam, drained by the
    /// binding layer between layout passes of the frame's fixpoint.
    ///
    /// Only `ErasedSliverLayoutCtx` (the pipeline-wired context) records the
    /// band; `Direct` / test / leaf contexts are honestly inert — they carry
    /// no `pending_retain_bands` sink.  The default is a no-op.
    fn emit_retain_band(&mut self, _first: usize, _last: usize) {}
}

impl<A: Arity, P: ParentData + Default> SliverLayoutCtxErased for SliverLayoutCtx<'_, A, P> {
    #[inline]
    fn descendant_layout_degraded(&self) -> bool {
        match &self.storage {
            // A direct context runs no walk, so nothing can have degraded.
            SliverLayoutCtxStorage::Direct { .. } => false,
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased.descendant_layout_degraded(),
        }
    }

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
        <Self as LayoutContextApi<'_, SliverLayout, A, P>>::position_child(self, index, offset);
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
    fn request_child_build(&mut self, logical_index: usize) -> ChildLayout {
        match &mut self.storage {
            // Direct storage carries no request sink — honestly Unwired so the
            // caller knows no backend is wired.
            SliverLayoutCtxStorage::Direct { .. } => ChildLayout::Unwired,
            SliverLayoutCtxStorage::Proxy { erased, .. } => {
                erased.request_child_build(logical_index)
            }
        }
    }

    #[inline]
    fn emit_retain_band(&mut self, first: usize, last: usize) {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct { .. } => {} // honestly inert
            SliverLayoutCtxStorage::Proxy { erased, .. } => erased.emit_retain_band(first, last),
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
    /// Whether the parent laid this child out during this pass — see the box
    /// protocol's `ErasedChildState::laid_out_this_pass`, which this mirrors.
    pub laid_out_this_pass: bool,
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
            laid_out_this_pass: false,
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
    /// Tree id of the sliver being laid out — the parent the producer sinks
    /// below record their requests against.
    node_id: RenderId,
    /// Sink for child-build requests from request-strategy slivers — the
    /// producer half of the request-strategy seam: `(sliver_id,
    /// logical_index)` pairs recorded when an absent in-band child is
    /// encountered.  No render object is pre-built here — the element tree
    /// decides what to build.  Drained by the binding layer via
    /// `PipelineOwner::take_pending_child_requests` between layout passes of
    /// the frame's fixpoint.
    pending_child_requests: &'ctx parking_lot::Mutex<Vec<(RenderId, usize)>>,
    /// Sink for retain-band signals from element-owned slivers — the removal
    /// half of the request-strategy seam. `RenderSliverList::perform_layout`
    /// emits `(sliver_id, first, last)` after the band walk via
    /// `ctx.emit_retain_band(first, last)`; the dirty-root walk drains this
    /// into `PipelineOwner::pending_retain_bands` for the binding layer,
    /// which evicts everything outside the band on the element side. The
    /// render side never disposes a child itself, which is what avoids a
    /// double-remove ABA on the arena slot.
    pending_retain_bands: &'ctx parking_lot::Mutex<Vec<(RenderId, usize, usize)>>,
    /// See [`DegradationProbe`](super::box_protocol::DegradationProbe): how
    /// this context learns of a degraded descendant. `None` in contexts the
    /// production walk did not build.
    degradation: Option<super::box_protocol::DegradationProbe<'ctx>>,
}

impl std::fmt::Debug for ErasedSliverLayoutCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Callbacks are live driver closures and the pending_* sinks are
        // shared mutexes (never locked in fmt); report ids + child state.
        f.debug_struct("ErasedSliverLayoutCtx")
            .field("constraints", &self.constraints)
            .field("children", &self.children)
            .field("child_ids", &self.child_ids)
            .field("node_id", &self.node_id)
            .finish_non_exhaustive()
    }
}

impl<'ctx> ErasedSliverLayoutCtx<'ctx> {
    /// Creates the walk-side context over pre-built child slots. `node_id` is
    /// the sliver being laid out; `pending_child_requests` and
    /// `pending_retain_bands` are the producer/removal sinks for the
    /// request-strategy seam.
    ///
    /// `pub(crate)`: the only constructor caller is the pipeline's sliver
    /// layout walk.
    pub(crate) fn new(
        constraints: SliverConstraints,
        children: &'ctx mut Vec<ErasedSliverChildState>,
        child_ids: &'ctx [RenderId],
        layout_child_callback: SliverChildLayoutCallback<'ctx>,
        layout_box_child_callback: BoxChildLayoutCallback<'ctx>,
        box_child_intrinsic_callback: BoxChildIntrinsicCallback<'ctx>,
        node_id: RenderId,
        pending_child_requests: &'ctx parking_lot::Mutex<Vec<(RenderId, usize)>>,
        pending_retain_bands: &'ctx parking_lot::Mutex<Vec<(RenderId, usize, usize)>>,
        degradation: Option<super::box_protocol::DegradationProbe<'ctx>>,
    ) -> Self {
        Self {
            constraints,
            children,
            child_ids,
            layout_child_callback,
            layout_box_child_callback,
            box_child_intrinsic_callback,
            node_id,
            pending_child_requests,
            pending_retain_bands,
            degradation,
        }
    }
}

impl SliverLayoutCtxErased for ErasedSliverLayoutCtx<'_> {
    fn descendant_layout_degraded(&self) -> bool {
        self.degradation.is_some_and(|probe| probe.degraded())
    }

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
            slot.laid_out_this_pass = true;
            slot.geometry = geometry;
        }
        geometry
    }

    fn layout_box_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        let Some(&child_id) = self.child_ids.get(index) else {
            return Size::ZERO;
        };
        // A box child of a sliver — `RenderSliverToBoxAdapter`'s child, a
        // persistent header's. It has no sliver geometry to record, but it was
        // still laid out this pass, and the paint gate cannot tell the two
        // reasons for an unmarked slot apart.
        if let Some(slot) = self.children.get_mut(index) {
            slot.laid_out_this_pass = true;
        }
        (self.layout_box_child_callback)(child_id, constraints)
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
            slot.laid_out_this_pass = true;
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

    fn request_child_build(&mut self, logical_index: usize) -> ChildLayout {
        // Record the request so the binding layer can service it between
        // layout passes of this frame's fixpoint.  Returns `Scheduled`.
        // `self.node_id` is the sliver, giving the element tree enough
        // context to locate the right child manager without leaking any
        // view-layer type into this crate (H3 seam discipline).
        self.pending_child_requests
            .lock()
            .push((self.node_id, logical_index));
        ChildLayout::Scheduled
    }

    fn emit_retain_band(&mut self, first: usize, last: usize) {
        self.pending_retain_bands
            .lock()
            .push((self.node_id, first, last));
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
///
/// The third parameter mirrors the box protocol's
/// [`box_protocol::HitTestChildCallback`](crate::protocol::box_protocol::HitTestChildCallback)
/// for signature uniformity with the shared `RenderObject::hit_test_raw`
/// trait method, but `SliverHitTestCtx`'s own transform stack is a no-op
/// (see its `push_transform`/`pop_transform` impl below — main-axis
/// position covers the sliver protocol's needs), so this is always
/// `None` in practice.
pub type SliverHitTestChildCallback<'a> =
    &'a mut dyn FnMut(usize, Option<MainAxisPosition>, Option<Matrix4>) -> bool;

/// Sliver hit test context implementation.
pub struct SliverHitTestCtx<'ctx, A: Arity, P: ParentData> {
    position: MainAxisPosition,
    result: SliverHitTestResult,
    child_callback: Option<SliverHitTestChildCallback<'ctx>>,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<A: Arity, P: ParentData> std::fmt::Debug for SliverHitTestCtx<'_, A, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `child_callback` is the driver's live hit-test recursion.
        f.debug_struct("SliverHitTestCtx")
            .field("position", &self.position)
            .field("result", &self.result)
            .finish_non_exhaustive()
    }
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
}

impl<'ctx, A: Arity, P: ParentData> HitTestContextApi<'ctx, SliverHitTest, A, P>
    for SliverHitTestCtx<'ctx, A, P>
{
    fn position(&self) -> &MainAxisPosition {
        &self.position
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
            Some(callback) => callback(index, Some(position), None),
            None => false,
        }
    }

    fn hit_test_child_at_layout_offset(&mut self, index: usize) -> bool {
        match self.child_callback.as_mut() {
            Some(callback) => callback(index, None, None),
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

    // ========================================================================
    // SliverLayoutCtx (Direct storage) — scalar accessors and child dispatch
    // ========================================================================

    #[test]
    fn direct_ctx_scalar_accessors_read_from_constraints() {
        let constraints = SliverConstraints {
            scroll_offset: 12.0,
            remaining_paint_extent: 340.0,
            viewport_main_axis_extent: 600.0,
            cross_axis_extent: 250.0,
            ..SliverConstraints::default()
        };
        let ctx = SliverLayoutCtx::<Leaf, SliverParentData>::new(constraints);

        assert_eq!(ctx.scroll_offset(), 12.0);
        assert_eq!(ctx.remaining_paint_extent(), 340.0);
        assert_eq!(ctx.viewport_main_axis_extent(), 600.0);
        assert_eq!(ctx.cross_axis_extent(), 250.0);
    }

    #[test]
    fn layout_box_child_and_box_child_intrinsic_return_zero_without_callbacks_wired() {
        let mut ctx = SliverLayoutCtx::<Leaf, SliverParentData>::new(SliverConstraints::default());

        assert_eq!(
            ctx.layout_box_child(0, BoxConstraints::tight(Size::new(px(10.0), px(10.0)))),
            Size::ZERO
        );
        assert_eq!(
            ctx.box_child_intrinsic(0, IntrinsicDimension::MinWidth, 100.0),
            0.0
        );
    }

    #[test]
    fn with_children_exposes_count_geometry_and_parent_data_by_index() {
        let mut children = vec![
            SliverChildState::<SliverParentData>::new(RenderId::new(1)),
            SliverChildState::<SliverParentData>::new(RenderId::new(2)),
        ];
        children[0].geometry = SliverGeometry {
            paint_extent: 40.0,
            ..SliverGeometry::ZERO
        };
        children[1].geometry = SliverGeometry {
            paint_extent: 80.0,
            ..SliverGeometry::ZERO
        };

        {
            let constraints = SliverConstraints::default();
            let mut ctx = SliverLayoutCtx::<Leaf, SliverParentData>::with_children(
                constraints,
                &mut children,
            );

            assert_eq!(LayoutContextApi::child_count(&ctx), 2);
            assert_eq!(ctx.child_geometry(0).unwrap().paint_extent, 40.0);
            assert_eq!(ctx.child_geometry(1).unwrap().paint_extent, 80.0);
            assert!(ctx.child_geometry(2).is_none());

            assert_eq!(ctx.child_parent_data(0), Some(&SliverParentData::default()));
            assert!(ctx.child_parent_data(5).is_none());
            assert!(ctx.child_parent_data_mut(5).is_none());

            ctx.child_parent_data_mut(0).unwrap().layout_offset = 99.0;
            LayoutContextApi::position_child(&mut ctx, 1, Offset::new(px(3.0), px(4.0)));
        }

        assert_eq!(children[0].parent_data.layout_offset, 99.0);
        assert_eq!(children[1].offset, Offset::new(px(3.0), px(4.0)));
    }

    #[test]
    fn layout_child_falls_back_to_cached_geometry_when_no_callback_is_wired() {
        let mut children = vec![SliverChildState::<SliverParentData>::new(RenderId::new(1))];
        children[0].geometry = SliverGeometry {
            paint_extent: 55.0,
            ..SliverGeometry::ZERO
        };

        let constraints = SliverConstraints::default();
        let mut ctx =
            SliverLayoutCtx::<Leaf, SliverParentData>::with_children(constraints, &mut children);

        // No `layout_child_callback` was wired (plain `with_children`), so the
        // Direct-mode dispatch must fall back to the already-cached geometry
        // rather than invoking a nonexistent callback.
        let geometry = LayoutContextApi::layout_child(&mut ctx, 0, constraints);
        assert_eq!(geometry.paint_extent, 55.0);
    }

    #[test]
    fn layout_child_returns_zero_geometry_with_no_children_and_no_callback() {
        let mut ctx = SliverLayoutCtx::<Leaf, SliverParentData>::new(SliverConstraints::default());
        assert_eq!(
            LayoutContextApi::layout_child(&mut ctx, 0, SliverConstraints::default()),
            SliverGeometry::ZERO
        );
    }

    #[test]
    fn with_layout_callback_dispatches_child_layout_box_child_and_intrinsic_by_child_id() {
        use parking_lot::Mutex;

        let child_a = RenderId::new(10);
        let child_b = RenderId::new(20);
        let child_ids = [child_a, child_b];
        let mut children = vec![
            SliverChildState::<SliverParentData>::new(child_a),
            SliverChildState::<SliverParentData>::new(child_b),
        ];

        let sliver_calls: Mutex<Vec<RenderId>> = Mutex::new(Vec::new());
        let layout_child_callback = |id: RenderId, _c: SliverConstraints| -> SliverGeometry {
            sliver_calls.lock().push(id);
            SliverGeometry {
                paint_extent: 123.0,
                ..SliverGeometry::ZERO
            }
        };

        let box_calls: Mutex<Vec<RenderId>> = Mutex::new(Vec::new());
        let layout_box_child_callback = |id: RenderId, _c: BoxConstraints| -> Size {
            box_calls.lock().push(id);
            Size::new(px(11.0), px(22.0))
        };

        let intrinsic_calls: Mutex<Vec<RenderId>> = Mutex::new(Vec::new());
        let box_child_intrinsic_callback = |id: RenderId, _d: IntrinsicDimension, _e: f32| -> f32 {
            intrinsic_calls.lock().push(id);
            77.0
        };

        let constraints = SliverConstraints::default();
        let mut ctx = SliverLayoutCtx::<Leaf, SliverParentData>::with_layout_callback(
            constraints,
            &mut children,
            &child_ids,
            &layout_child_callback,
            Some(&layout_box_child_callback),
            Some(&box_child_intrinsic_callback),
        );

        // `layout_child` on index 1 must resolve child_ids[1] == child_b, invoke
        // the callback with that id, and write the returned geometry through to
        // the cached child state at the same index.
        let geometry = LayoutContextApi::layout_child(&mut ctx, 1, constraints);
        assert_eq!(geometry.paint_extent, 123.0);
        assert_eq!(*sliver_calls.lock(), vec![child_b]);
        assert_eq!(ctx.child_geometry(1).unwrap().paint_extent, 123.0);

        let size = ctx.layout_box_child(0, BoxConstraints::tight(Size::new(px(11.0), px(22.0))));
        assert_eq!(size, Size::new(px(11.0), px(22.0)));
        assert_eq!(*box_calls.lock(), vec![child_a]);

        let extent = ctx.box_child_intrinsic(0, IntrinsicDimension::MinWidth, 50.0);
        assert_eq!(extent, 77.0);
        assert_eq!(*intrinsic_calls.lock(), vec![child_a]);

        // An index beyond `child_ids` must not dispatch to any callback.
        let out_of_range = ctx.layout_box_child(5, BoxConstraints::tight(Size::ZERO));
        assert_eq!(out_of_range, Size::ZERO);
        assert_eq!(
            box_calls.lock().len(),
            1,
            "no additional dispatch for an out-of-range index"
        );
    }
}
