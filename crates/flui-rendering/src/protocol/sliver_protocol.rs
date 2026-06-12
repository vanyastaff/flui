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
        box_protocol::BoxProtocol,
        capabilities::{HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi},
        protocol::{Protocol, ProtocolCompatible, sealed},
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

// Self-compatibility
impl ProtocolCompatible<SliverProtocol> for SliverProtocol {
    fn is_compatible() -> bool {
        true
    }
}

// Box and Sliver can be adapted together
impl ProtocolCompatible<BoxProtocol> for SliverProtocol {
    fn is_compatible() -> bool {
        true
    }
}

impl ProtocolCompatible<SliverProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true
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
        constraints.normalize()
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
        geometry: Option<SliverGeometry>,
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
        geometry: Option<SliverGeometry>,
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
                geometry: None,
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
                geometry: None,
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
                geometry: None,
                children: Some(children),
                child_ids: Some(child_ids),
                layout_child_callback: Some(layout_child_callback),
                layout_box_child_callback,
                box_child_intrinsic_callback,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Constructs a Proxy-mode `SliverLayoutCtx` that delegates completion
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
    /// **Generic parameter invariant** — for the leaf scope (no child
    /// parent-data access) the `P` type parameter only appears in
    /// `PhantomData`; a mismatch between the typed `P` and the underlying
    /// Direct ctx's `P` is benign here. The same invariant holds for
    /// `BoxLayoutCtx::from_erased` in the leaf case (see that function's
    /// documentation).
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
                geometry: None,
                child_geometries: vec![None; child_count],
                erased,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Gets the current geometry if layout is complete.
    pub fn geometry(&self) -> Option<&SliverGeometry> {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { geometry, .. }
            | SliverLayoutCtxStorage::Proxy { geometry, .. } => geometry.as_ref(),
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

    fn is_complete(&self) -> bool {
        match &self.storage {
            SliverLayoutCtxStorage::Direct { geometry, .. }
            | SliverLayoutCtxStorage::Proxy { geometry, .. } => geometry.is_some(),
        }
    }

    fn complete_layout(&mut self, geometry: SliverGeometry) {
        match &mut self.storage {
            SliverLayoutCtxStorage::Direct { geometry: g, .. } => *g = Some(geometry),
            SliverLayoutCtxStorage::Proxy {
                geometry: g,
                erased,
                ..
            } => {
                *g = Some(geometry);
                // Mirror completion to the underlying erased ctx so any
                // pipeline-side reader of the original Direct ctx sees
                // the result. The blanket-impl bridge returns the geometry
                // directly as well — keeping both paths consistent.
                erased.complete_layout(geometry);
            }
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

    /// Performs a synchronous Box intrinsic query on child at `index`.
    fn box_child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32;

    /// Records the paint offset for child at `index`.
    fn position_child(&mut self, index: usize, offset: Offset);

    /// Records the layout result (parent's own geometry) on the context.
    ///
    /// Symmetric with [`super::box_protocol::BoxLayoutCtxErased::complete_layout`] — the
    /// typed-side reader is [`SliverLayoutCtx::geometry`] returning
    /// `Option<&SliverGeometry>`. The erased trait intentionally exposes
    /// only the write.
    fn complete_layout(&mut self, geometry: SliverGeometry);

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
    fn complete_layout(&mut self, geometry: SliverGeometry) {
        // Delegates through `LayoutContextApi::complete_layout` so the
        // Proxy write-through path is exercised consistently.
        <Self as LayoutContextApi<'_, SliverLayout, A, P>>::complete_layout(self, geometry);
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
    geometry: Option<SliverGeometry>,
    children: &'ctx mut Vec<ErasedSliverChildState>,
    child_ids: &'ctx [RenderId],
    layout_child_callback: SliverChildLayoutCallback<'ctx>,
    layout_box_child_callback: BoxChildLayoutCallback<'ctx>,
    box_child_intrinsic_callback: BoxChildIntrinsicCallback<'ctx>,
}

impl<'ctx> ErasedSliverLayoutCtx<'ctx> {
    /// Creates the walk-side context over pre-built child slots.
    pub fn new(
        constraints: SliverConstraints,
        children: &'ctx mut Vec<ErasedSliverChildState>,
        child_ids: &'ctx [RenderId],
        layout_child_callback: SliverChildLayoutCallback<'ctx>,
        layout_box_child_callback: BoxChildLayoutCallback<'ctx>,
        box_child_intrinsic_callback: BoxChildIntrinsicCallback<'ctx>,
    ) -> Self {
        Self {
            constraints,
            geometry: None,
            children,
            child_ids,
            layout_child_callback,
            layout_box_child_callback,
            box_child_intrinsic_callback,
        }
    }

    /// The parent's completed geometry, when `complete_layout` ran.
    pub fn geometry(&self) -> Option<&SliverGeometry> {
        self.geometry.as_ref()
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

    fn complete_layout(&mut self, geometry: SliverGeometry) {
        self.geometry = Some(geometry);
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

    /// Adds self as a hit target with the given ID.
    pub fn add_self(&mut self, target_id: u64) {
        self.result
            .add(SliverHitTestEntry::new(target_id, self.position.main_axis));
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
        // For slivers, check if main axis position is within bounds height
        self.position.main_axis >= 0.0 && self.position.main_axis <= bounds.height().get()
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
    fn test_protocol_compatibility() {
        use crate::protocol::protocol::ProtocolCompatible;

        // Test self-compatibility
        assert!(<SliverProtocol as ProtocolCompatible<SliverProtocol>>::is_compatible());
        assert!(<BoxProtocol as ProtocolCompatible<BoxProtocol>>::is_compatible());

        // Box and Sliver protocols are compatible via adapters
        assert!(<SliverProtocol as ProtocolCompatible<BoxProtocol>>::is_compatible());
        assert!(<BoxProtocol as ProtocolCompatible<SliverProtocol>>::is_compatible());
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
}
