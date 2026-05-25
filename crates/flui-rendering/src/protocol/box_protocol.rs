//! Box protocol for 2D cartesian layout.
//!
//! This module provides the BoxProtocol and its capability implementations:
//! - [`BoxProtocol`]: Main protocol type
//! - [`BoxLayout`]: Layout capability (BoxConstraints → Size)
//! - [`BoxHitTest`]: Hit test capability (Offset → BoxHitTestResult)

use flui_foundation::RenderId;
use flui_tree::Arity;
use flui_types::{
    Size,
    geometry::{Matrix4, Offset, Point, Rect},
};

use crate::{
    constraints::{BoxConstraints, Constraints},
    parent_data::{BoxParentData, ParentData},
    protocol::{
        capabilities::{HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi},
        protocol::{BidirectionalProtocol, Protocol, ProtocolCompatible, sealed},
    },
};

// ============================================================================
// CHILD STATE
// ============================================================================
//
// Per-child layout-time bookkeeping owned by `BoxLayoutCtx`. Previously
// lived in `crates/flui-rendering/src/children_access.rs` alongside a
// 500-LOC closure-based iterator (`ChildrenAccess`) and the
// `ChildHandle` wrapper in `child_handle.rs` -- both fought the borrow
// checker for users that never appeared, so Mythos Step 5b deleted them
// outright. `ChildState<P>` itself stays because it IS the data shape
// `BoxLayoutContextApi::layout_child` / `position_child` /
// `child_geometry` / `child_parent_data` need.

/// Per-child layout-time state held by [`BoxLayoutCtx`].
///
/// Created by the pipeline before invoking a parent's `perform_layout`,
/// mutated through `BoxLayoutContextApi::layout_child` /
/// `position_child`, and read during the subsequent paint phase.
#[derive(Debug)]
pub struct ChildState<P: ParentData + Default> {
    /// Render ID of this child.
    pub id: RenderId,
    /// Computed size after layout.
    pub size: Size,
    /// Position offset set by parent.
    pub offset: Offset,
    /// Parent data for this child.
    pub parent_data: P,
}

impl<P: ParentData + Default> ChildState<P> {
    /// Creates a new child state with default values.
    pub fn new(id: RenderId) -> Self {
        Self {
            id,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data: P::default(),
        }
    }

    /// Creates a new child state with specific parent data.
    pub fn with_parent_data(id: RenderId, parent_data: P) -> Self {
        Self {
            id,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data,
        }
    }
}

// ============================================================================
// BOX PROTOCOL
// ============================================================================

/// Box protocol using 2D constraints and sizes.
///
/// This is the most common protocol for 2D layout with width/height
/// constraints. Used by most widgets: containers, buttons, text, images, etc.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxProtocol;

impl sealed::Sealed for BoxProtocol {}

impl Protocol for BoxProtocol {
    type Layout = BoxLayout;
    type HitTest = BoxHitTest;
    type DefaultParentData = BoxParentData;

    // PORT-CHECK-OK-DYN: protocol-layout-erasure (D-block PR-A1b U19, memo D5)
    type LayoutCtxErased<'ctx> = dyn BoxLayoutCtxErased + 'ctx;

    fn name() -> &'static str {
        "box"
    }

    /// D-block PR-A1 U17 — override the default no-op with the actual
    /// Flutter-parity `compute_relayout_boundary` call.
    ///
    /// `parent_uses_size = true` (conservative default per Copilot P1 review
    /// on PR #139): with the Flutter formula
    /// `is_boundary = !parent_uses_size || sized_by_parent || constraints.is_tight() || !has_parent`,
    /// passing `parent_uses_size = false` would make `!false = true` ⇒ EVERY
    /// non-root node defaults to a relayout boundary, immediately blocking
    /// [`PipelineOwner::mark_needs_layout`](crate::pipeline::PipelineOwner::mark_needs_layout)
    /// propagation at the leaf and breaking parents-depend-on-child-size
    /// flows. The conservative `true` default makes boundary-ness depend on
    /// the remaining three signals: tight constraints (always a boundary in
    /// Flutter — parent is forcing a single valid size), root (no parent),
    /// or `sized_by_parent` (constraints alone determine size). Non-tight
    /// non-root non-sized-by-parent nodes correctly default to non-boundary,
    /// preserving propagation.
    ///
    /// `sized_by_parent = false`: full Flutter parity for both parameters
    /// requires per-render-object trait methods that report their layout
    /// dependency shape; deferred to Core.2 alongside the intrinsic-
    /// dimension protocol.
    fn bootstrap_relayout_boundary(state: &crate::storage::RenderState<Self>, has_parent: bool) {
        state.compute_relayout_boundary(true, false, has_parent);
    }

    /// D-block PR-A1b U19 — wraps the given `BoxConstraints` in a typed
    /// `BoxLayoutCtx::<Leaf, BoxParentData>::new(constraints)` (no
    /// children, no callback) and hands an erased `&mut dyn
    /// BoxLayoutCtxErased` view to `f`.
    ///
    /// `Leaf` arity is used for the typed wrapper because this entry
    /// point does not expose children — calls to `layout_child` /
    /// `position_child` through the erased view will hit the
    /// `BoxLayoutCtxErased` blanket on `BoxLayoutCtx`, which forwards to
    /// `LayoutContextApi` whose `Leaf`-arity body returns `Size::ZERO` /
    /// no-op (the existing semantics for a no-children context).
    ///
    /// The pipeline's `layout_dirty_root` (U20) constructs its own typed
    /// context with children via disjoint borrows and bypasses this
    /// helper.
    fn with_leaf_erased_ctx<R>(
        constraints: BoxConstraints,
        f: impl FnOnce(&mut Self::LayoutCtxErased<'_>) -> R,
    ) -> R {
        let mut typed = BoxLayoutCtx::<flui_tree::Leaf, BoxParentData>::new(constraints);
        // PORT-CHECK-OK-DYN: protocol-layout-erasure (D-block PR-A1b U19, memo D5)
        let erased: &mut dyn BoxLayoutCtxErased = &mut typed;
        f(erased)
    }
}

impl BidirectionalProtocol for BoxProtocol {}

// Self-compatibility
impl ProtocolCompatible<BoxProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true
    }
}

// ============================================================================
// BOX LAYOUT CAPABILITY
// ============================================================================

/// Layout capability for box (2D) layout.
///
/// Uses `BoxConstraints` for input and `Size` for output.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxLayout;

/// Cache key for BoxConstraints.
///
/// Uses integer representation of floats (bits) for reliable hashing.
/// This handles -0.0/+0.0 and provides exact equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxConstraintsCacheKey {
    min_width_bits: u32,
    max_width_bits: u32,
    min_height_bits: u32,
    max_height_bits: u32,
}

impl BoxConstraintsCacheKey {
    /// Creates a cache key from constraints.
    ///
    /// Returns `None` if any value is NaN.
    pub fn from_constraints(c: &BoxConstraints) -> Option<Self> {
        // NaN check using is_nan()
        if c.min_width.is_nan()
            || c.max_width.is_nan()
            || c.min_height.is_nan()
            || c.max_height.is_nan()
        {
            return None;
        }

        Some(Self {
            min_width_bits: c.min_width.to_bits(),
            max_width_bits: c.max_width.to_bits(),
            min_height_bits: c.min_height.to_bits(),
            max_height_bits: c.max_height.to_bits(),
        })
    }
}

impl LayoutCapability for BoxLayout {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type CacheKey = BoxConstraintsCacheKey;
    type Context<'ctx, A: Arity, P: ParentData + Default>
        = BoxLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;

    fn default_geometry() -> Self::Geometry {
        Size::ZERO
    }

    fn validate_constraints(constraints: &Self::Constraints) -> bool {
        constraints.is_normalized()
    }

    fn cache_key(constraints: &Self::Constraints) -> Option<Self::CacheKey> {
        BoxConstraintsCacheKey::from_constraints(constraints)
    }

    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints.normalize()
    }
}

/// Box layout context implementation.
///
/// This context provides access to constraints and children during layout.
/// Callback type for synchronous child layout.
///
/// Called when parent's `layout_child()` is invoked. The callback receives
/// the child's `RenderId` and constraints, performs layout on the child via
/// the RenderTree, and returns the child's size.
pub type LayoutChildCallback<'a> =
    &'a (dyn Fn(flui_foundation::RenderId, BoxConstraints) -> Size + Send + Sync);

/// Per-child geometry storage owned by the typed wrapper when bridging
/// from an erased context.
///
/// **D-block PR-A1b U19 (companion memo D5):** when the `RenderBox`
/// blanket impl constructs a `BoxLayoutCtx::from_erased(...)` Proxy view
/// of an `&mut dyn BoxLayoutCtxErased`, the typed wrapper needs to honour
/// the [`LayoutContextApi::child_geometry`] contract
/// (`Option<&Size>` — borrow-returning). The erased trait can only hand
/// out owned `Size` (no reference lifetime to bind to). The Proxy
/// variant therefore caches child sizes in this dense `Vec<Option<Size>>`
/// on `layout_child` calls (which already produce a `Size`
/// synchronously), and `child_geometry` reads from the cache. This loses
/// the strict "pre-existing geometry from a sibling's prior call"
/// semantics that Direct mode provides, but matches the typical
/// user-widget flow
/// (`let s = ctx.layout_child(i, c); … ctx.child_geometry(i)`).
///
/// # Storage shape (PR #141 Copilot review feedback, comment 3293746260)
///
/// Indexed by dense child index (`0..child_count`) so a hash map is
/// strictly worse on every dimension: lookup is `O(log n)` ↔ `O(1)`
/// indexed, allocation pattern is many small Hash buckets ↔ one
/// contiguous Vec, and CPU prefetch favours the contiguous Vec on the
/// hot layout path. `Option<Size>` is `Copy` and 12 bytes (Size +
/// discriminant); `Vec::with_capacity(child_count)` from
/// `erased.child_count()` pre-sizes the cache at Proxy construction
/// (one allocation per `from_erased` call); subsequent `layout_child`
/// writes are an in-place assignment with no reallocation.
type ProxyChildSizeCache = Vec<Option<Size>>;

/// The children reference allows `position_child` to store offsets that
/// will be used during painting.
///
/// **D-block PR-A1b U19 (companion memo D5) — storage variants.** The
/// context carries two storage modes:
///
/// 1. `Direct` (default constructors `new`, `with_children`,
///    `with_layout_callback`): pipeline owns the children `Vec`, child
///    IDs, and synchronous layout callback. This is the production path
///    used by `RenderEntry::layout_leaf_only` (leaf shape) and U20's
///    `layout_dirty_root` (parent+children disjoint-borrow shape).
/// 2. `Proxy` (constructor `from_erased`): wraps `&mut dyn
///    BoxLayoutCtxErased` so the `RenderObject<BoxProtocol>` blanket
///    impl can reconstruct a typed
///    `BoxLayoutCtx<T::Arity, T::ParentData>` to hand to
///    `RenderBox::perform_layout`. Child operations delegate through
///    the erased trait; typed parent-data access downcasts via
///    [`ParentData`].
pub struct BoxLayoutCtx<'ctx, A: Arity, P: ParentData + Default> {
    storage: BoxLayoutCtxStorage<'ctx, P>,
    _phantom: std::marker::PhantomData<A>,
}

/// Internal storage variants. See [`BoxLayoutCtx`] doc.
enum BoxLayoutCtxStorage<'ctx, P: ParentData + Default> {
    /// Production / pipeline path: owns child state and an optional
    /// synchronous layout callback.
    Direct {
        constraints: BoxConstraints,
        geometry: Option<Size>,
        /// Reference to children states for position_child to update
        /// offsets.
        children: Option<&'ctx mut Vec<ChildState<P>>>,
        /// Child render IDs for tree lookup during layout_child.
        child_ids: Option<&'ctx [flui_foundation::RenderId]>,
        /// Callback to perform synchronous child layout through
        /// RenderTree.
        layout_child_callback: Option<LayoutChildCallback<'ctx>>,
    },
    /// Bridge path used by the `RenderObject<BoxProtocol>` blanket impl
    /// to reconstruct a typed view of an erased context.
    Proxy {
        /// Cached at construction from `erased.constraints()`. `BoxConstraints`
        /// is `Copy`, so the cache is byte-cheap; caching avoids the
        /// `LayoutContextApi::constraints(&self) -> &BoxConstraints`
        /// reference-lifetime mismatch with the erased
        /// `fn constraints(&self) -> BoxConstraints` (owned).
        constraints: BoxConstraints,
        geometry: Option<Size>,
        /// Lazy cache of child sizes returned from
        /// `erased.layout_child(idx, c)` — see [`ProxyChildSizeCache`].
        child_sizes: ProxyChildSizeCache,
        /// The underlying erased context (typically a pipeline-side
        /// `BoxLayoutCtx` in Direct mode).
        // PORT-CHECK-OK-DYN: protocol-layout-erasure (D-block PR-A1b U19, memo D5)
        erased: &'ctx mut dyn BoxLayoutCtxErased,
    },
}

impl<'ctx, A: Arity, P: ParentData + Default> BoxLayoutCtx<'ctx, A, P> {
    /// Creates a new box layout context with given constraints (no children
    /// access). Direct storage.
    pub fn new(constraints: BoxConstraints) -> Self {
        Self {
            storage: BoxLayoutCtxStorage::Direct {
                constraints,
                geometry: None,
                children: None,
                child_ids: None,
                layout_child_callback: None,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a new box layout context with children access. Direct storage.
    pub fn with_children(
        constraints: BoxConstraints,
        children: &'ctx mut Vec<ChildState<P>>,
    ) -> Self {
        Self {
            storage: BoxLayoutCtxStorage::Direct {
                constraints,
                geometry: None,
                children: Some(children),
                child_ids: None,
                layout_child_callback: None,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a new box layout context with full access for synchronous child
    /// layout. Direct storage.
    ///
    /// This constructor enables proper Flutter-style layout where parent's
    /// `layout_child()` triggers synchronous child layout through the
    /// RenderTree.
    pub fn with_layout_callback(
        constraints: BoxConstraints,
        children: &'ctx mut Vec<ChildState<P>>,
        child_ids: &'ctx [flui_foundation::RenderId],
        layout_child_callback: LayoutChildCallback<'ctx>,
    ) -> Self {
        Self {
            storage: BoxLayoutCtxStorage::Direct {
                constraints,
                geometry: None,
                children: Some(children),
                child_ids: Some(child_ids),
                layout_child_callback: Some(layout_child_callback),
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// **D-block PR-A1b U19** — constructs a Proxy-mode `BoxLayoutCtx`
    /// that delegates child / completion operations to the given erased
    /// context. Used by the `RenderObject<BoxProtocol>` blanket impl in
    /// [`crate::traits::RenderBox`] to hand a typed
    /// `&mut BoxLayoutCtx<T::Arity, T::ParentData>` to
    /// `RenderBox::perform_layout`, given only `&mut dyn BoxLayoutCtxErased`
    /// at the trait boundary.
    ///
    /// Constraints are eagerly cached from `erased.constraints()` (cheap —
    /// `BoxConstraints` is `Copy`) so
    /// [`LayoutContextApi::constraints`] can return `&BoxConstraints`
    /// against a stable storage slot rather than an ephemeral owned
    /// value produced per call.
    ///
    /// **Visibility** — `pub(crate)`. The only sanctioned consumer is
    /// the `RenderObject<BoxProtocol>` blanket impl in
    /// [`crate::traits::RenderBox`] (in-crate). User render-object
    /// authors implement `RenderBox::perform_layout` directly and never
    /// see the erased context; restricting the ctor prevents downstream
    /// code from constructing Proxy contexts (a sharp tool that requires
    /// the parent_data-downcast invariants and Direct↔Proxy semantic
    /// awareness documented on [`BoxLayoutCtxErased`]).
    // PORT-CHECK-OK-DYN: protocol-layout-erasure (D-block PR-A1b U19, memo D5)
    pub(crate) fn from_erased(erased: &'ctx mut dyn BoxLayoutCtxErased) -> Self {
        let constraints = erased.constraints();
        // Review fix #2: assert at construction time that the typed
        // wrapper P matches the underlying Direct ctx's P. The Proxy
        // bridge later downcasts via `child_parent_data_dyn().and_then(
        // |d| d.downcast_ref::<P>())` — a mismatch silently returns
        // None and causes the user's perform_layout to see no flex
        // (for example), producing wrong-but-quiet layout. This
        // debug_assert catches the construction-site bug instead. None
        // = no static evidence available (Proxy chain / no-children
        // Direct) → assert is a no-op, the downcast still guards at
        // runtime.
        debug_assert!(
            match erased.parent_data_type_id() {
                Some(id) => id == std::any::TypeId::of::<P>(),
                None => true,
            },
            "BoxLayoutCtx::from_erased: ParentData type mismatch — \
             underlying erased ctx reports TypeId={:?}, typed wrapper \
             requested {:?} ({})",
            erased.parent_data_type_id(),
            std::any::TypeId::of::<P>(),
            std::any::type_name::<P>(),
        );
        // Pre-size the dense `Vec<Option<Size>>` cache to the erased
        // ctx's child_count — one allocation per Proxy construction, no
        // per-`layout_child` reallocation. PR #141 Copilot review fix:
        // swapped from `HashMap<usize, Size>` (sparse + hashing on hot
        // path) to indexed `Vec<Option<Size>>` (O(1) access, contiguous).
        let child_count = erased.child_count();
        Self {
            storage: BoxLayoutCtxStorage::Proxy {
                constraints,
                geometry: None,
                child_sizes: vec![None; child_count],
                erased,
            },
            _phantom: std::marker::PhantomData,
        }
    }

    /// Gets the current geometry if layout is complete.
    pub fn geometry(&self) -> Option<&Size> {
        match &self.storage {
            BoxLayoutCtxStorage::Direct { geometry, .. }
            | BoxLayoutCtxStorage::Proxy { geometry, .. } => geometry.as_ref(),
        }
    }
}

impl<'ctx, A: Arity, P: ParentData + Default> LayoutContextApi<'ctx, BoxLayout, A, P>
    for BoxLayoutCtx<'ctx, A, P>
{
    fn constraints(&self) -> &BoxConstraints {
        match &self.storage {
            BoxLayoutCtxStorage::Direct { constraints, .. }
            | BoxLayoutCtxStorage::Proxy { constraints, .. } => constraints,
        }
    }

    fn is_complete(&self) -> bool {
        match &self.storage {
            BoxLayoutCtxStorage::Direct { geometry, .. }
            | BoxLayoutCtxStorage::Proxy { geometry, .. } => geometry.is_some(),
        }
    }

    fn complete_layout(&mut self, geometry: Size) {
        match &mut self.storage {
            BoxLayoutCtxStorage::Direct { geometry: g, .. } => *g = Some(geometry),
            BoxLayoutCtxStorage::Proxy {
                geometry: g,
                erased,
                ..
            } => {
                *g = Some(geometry);
                // Mirror completion to the underlying erased ctx so any
                // pipeline-side reader of the original Direct ctx sees
                // the result. (The blanket-impl bridge returns the Size
                // directly as well — this keeps both paths consistent.)
                erased.complete_layout(geometry);
            }
        }
    }

    fn child_count(&self) -> usize {
        match &self.storage {
            BoxLayoutCtxStorage::Direct { children, .. } => {
                children.as_ref().map(|c| c.len()).unwrap_or(0)
            }
            BoxLayoutCtxStorage::Proxy { erased, .. } => erased.child_count(),
        }
    }

    fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        match &mut self.storage {
            BoxLayoutCtxStorage::Direct {
                children,
                child_ids,
                layout_child_callback,
                ..
            } => {
                // Try to use the layout callback for synchronous child layout
                if let (Some(child_ids), Some(callback)) =
                    (*child_ids, layout_child_callback.as_ref())
                    && let Some(&child_id) = child_ids.get(index)
                {
                    // Perform synchronous layout through RenderTree
                    let size = callback(child_id, constraints);

                    // Update cached size in children state
                    if let Some(children) = children.as_mut()
                        && let Some(child) = children.get_mut(index)
                    {
                        child.size = size;
                    }

                    return size;
                }

                // Fallback: return cached size if available
                if let Some(children) = children.as_ref()
                    && let Some(child) = children.get(index)
                {
                    return child.size;
                }
                Size::ZERO
            }
            BoxLayoutCtxStorage::Proxy {
                erased,
                child_sizes,
                ..
            } => {
                let size = erased.layout_child(index, constraints);
                // Indexed write — `child_sizes` is pre-sized to
                // `erased.child_count()` at `from_erased` time. An
                // out-of-bounds index (caller passed an `index >=
                // child_count`) silently no-ops the cache write so
                // `child_geometry(index)` returns `None` — matches
                // Direct's behaviour where an out-of-range
                // `children.get(index)` also returns None.
                if let Some(slot) = child_sizes.get_mut(index) {
                    *slot = Some(size);
                }
                size
            }
        }
    }

    fn position_child(&mut self, index: usize, offset: Offset) {
        match &mut self.storage {
            BoxLayoutCtxStorage::Direct { children, .. } => {
                if let Some(children) = children.as_mut()
                    && let Some(child) = children.get_mut(index)
                {
                    child.offset = offset;
                }
            }
            BoxLayoutCtxStorage::Proxy { erased, .. } => {
                erased.position_child(index, offset);
            }
        }
    }

    fn child_geometry(&self, index: usize) -> Option<&Size> {
        match &self.storage {
            BoxLayoutCtxStorage::Direct { children, .. } => children
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|child| &child.size),
            BoxLayoutCtxStorage::Proxy { child_sizes, .. } => {
                // Indexed access — out-of-range returns None (consistent
                // with Direct's `children.get(index).map(...)`); in-range
                // unfilled slot (Some(None)) also returns None.
                child_sizes.get(index).and_then(Option::as_ref)
            }
        }
    }

    fn child_parent_data(&self, index: usize) -> Option<&P> {
        match &self.storage {
            BoxLayoutCtxStorage::Direct { children, .. } => children
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|child| &child.parent_data),
            BoxLayoutCtxStorage::Proxy { erased, .. } => {
                // P: ParentData : DowncastSync : 'static, so downcast is sound.
                erased
                    .child_parent_data_dyn(index)
                    .and_then(|d| d.downcast_ref::<P>())
            }
        }
    }

    fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut P> {
        match &mut self.storage {
            BoxLayoutCtxStorage::Direct { children, .. } => children
                .as_mut()
                .and_then(|c| c.get_mut(index))
                .map(|child| &mut child.parent_data),
            BoxLayoutCtxStorage::Proxy { erased, .. } => erased
                .child_parent_data_dyn_mut(index)
                .and_then(|d| d.downcast_mut::<P>()),
        }
    }
}

// ============================================================================
// BOX LAYOUT CTX ERASED (D-block PR-A1b U19 / memo D5)
// ============================================================================

/// Protocol-typed but **arity- and parent-data-erased** view of a box layout
/// context, suitable for trait-object use at the
/// [`RenderObject<BoxProtocol>::perform_layout_raw`](crate::traits::RenderObject::perform_layout_raw)
/// boundary.
///
/// # Motivation (D-block PR-A1b U19 / companion memo D5)
///
/// Pre-U19, the blanket impl `impl<T: RenderBox> RenderObject<BoxProtocol> for T`
/// could not bridge to the user's typed `RenderBox::perform_layout(ctx:
/// &mut BoxLayoutCtx<Self::Arity, Self::ParentData>)` because the trait
/// surface only carried protocol-typed constraints (no children, no
/// layout-callback). As a consequence, the blanket `perform_layout_raw`
/// shipped as a no-op returning the cached `*self.size()` — D-1's AE1
/// concretely showed `Size::ZERO` for fresh boxes (companion memo §D5).
///
/// `BoxLayoutCtxErased` is the trait-object-friendly wrapper picked in
/// memo D5: the pipeline / [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only)
/// constructs a typed [`BoxLayoutCtx<'_, A, P>`], the trait blanket impl
/// below coerces it to `&mut dyn BoxLayoutCtxErased`, and the
/// `RenderObject<BoxProtocol>` blanket impl in
/// [`crate::traits::RenderBox`] reconstructs a typed
/// `BoxLayoutCtx<T::Arity, T::ParentData>` via a `Proxy` storage variant
/// that delegates all child / position / parent-data operations back
/// through this trait.
///
/// # Parent-data downcast
///
/// `child_parent_data_dyn` / `_mut` expose children's parent data through
/// `&dyn ParentData`. The blanket impl's `Proxy` view then `downcast_ref::<T::ParentData>()`s
/// to recover the typed payload required by user widget code
/// (`ctx.child_parent_data(i) -> Option<&FlexParentData>` and similar).
/// The downcast is total in practice because the typed BoxLayoutCtx that
/// produced the erased view was constructed with `Vec<ChildState<P>>`
/// matching the same P; a mismatch indicates a bug at the construction
/// site (pipeline / blanket-impl logic error, not user code).
///
/// # Sliver counterpart
///
/// [`SliverLayoutCtxErased`](super::sliver_protocol::SliverLayoutCtxErased) is the
/// analogous trait for sliver layout. The sliver bridge is stubbed for
/// D-block — see [`crate::traits::RenderSliver`].
///
/// # Thread-safety
///
/// `Send + Sync` is required so the trait object can live inside a
/// `LayoutContextApi`-implementing type whose own supertrait requires
/// `Send + Sync` (see [`LayoutContextApi`] — the `Proxy` storage of
/// [`BoxLayoutCtx`] carries `&mut dyn BoxLayoutCtxErased`).
pub trait BoxLayoutCtxErased: Send + Sync {
    /// Box constraints from parent. Cheap copy (`BoxConstraints` is `Copy`).
    fn constraints(&self) -> BoxConstraints;

    /// Number of children visible to this context.
    fn child_count(&self) -> usize;

    /// Performs synchronous layout on child at `index` with the given
    /// constraints; returns the child's computed `Size`.
    fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Size;

    /// Records the paint offset for child at `index`.
    fn position_child(&mut self, index: usize, offset: Offset);

    /// Records the layout result (parent's own size) on the context.
    ///
    /// The completed size is read back from the typed context via
    /// `BoxLayoutCtx::geometry()` (returning `Option<&Size>`) — see the
    /// `RenderObject<BoxProtocol>` blanket impl in
    /// [`crate::traits::RenderBox`] for the read site. The erased trait
    /// intentionally exposes only the write — owned-`Size`-by-reference
    /// has no stable storage to bind to through trait-object dispatch,
    /// and the only readers are the bridge (typed-side `.geometry()`)
    /// and Proxy `complete_layout` mirror.
    fn complete_layout(&mut self, size: Size);

    /// Reads child `index`'s parent data as `&dyn ParentData`. Returns
    /// `None` if `index` is out of bounds or the context wasn't
    /// constructed with children access.
    ///
    /// The blanket impl downcasts via `downcast_ref` (the
    /// `DowncastSync` method generated by the `impl_downcast!` macro
    /// on `ParentData`) to recover the typed payload required by user
    /// widget code.
    fn child_parent_data_dyn(&self, index: usize) -> Option<&dyn ParentData>;

    /// Mutable counterpart to [`Self::child_parent_data_dyn`].
    fn child_parent_data_dyn_mut(&mut self, index: usize) -> Option<&mut dyn ParentData>;

    /// `TypeId` of the underlying parent-data type held by this erased
    /// context, when known.
    ///
    /// Returns `Some(TypeId::of::<P>())` for the blanket impl on
    /// `BoxLayoutCtx<A, P>` when the context was constructed with
    /// children access (and therefore the `P` type is observable as
    /// type-of-the-stored-Vec). Returns `None` for children-less Direct
    /// contexts (P is still in the type parameter but no concrete
    /// payload exists) and for Proxy contexts (which delegate to the
    /// underlying erased ctx).
    ///
    /// **Use:** the in-crate `BoxLayoutCtx::from_erased` ctor consults
    /// this to `debug_assert!` that the typed wrapper it is about to
    /// construct matches the underlying P — a mismatch indicates a
    /// pipeline / blanket-impl construction bug (a Direct ctx built
    /// with `Vec<ChildState<FlexParentData>>` would only be bridged
    /// to a typed `BoxLayoutCtx<_, FlexParentData>`, never to a
    /// `BoxLayoutCtx<_, BoxParentData>`). The default return is `None`
    /// so the debug_assert is a no-op for Proxy / no-children paths,
    /// which is correct: those carry no static evidence to check.
    ///
    /// Default `None` keeps the assertion conservative — only triggers
    /// on bug-shapes we can actually detect.
    fn parent_data_type_id(&self) -> Option<std::any::TypeId> {
        None
    }
}

impl<A: Arity, P: ParentData + Default> BoxLayoutCtxErased for BoxLayoutCtx<'_, A, P> {
    #[inline]
    fn constraints(&self) -> BoxConstraints {
        // Owned by-value (Copy). Inner storage holds the canonical copy
        // (Direct.constraints or Proxy.constraints cache); read via the
        // LayoutContextApi accessor and deref-copy.
        *<Self as LayoutContextApi<'_, BoxLayout, A, P>>::constraints(self)
    }

    #[inline]
    fn child_count(&self) -> usize {
        <Self as LayoutContextApi<'_, BoxLayout, A, P>>::child_count(self)
    }

    #[inline]
    fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        <Self as LayoutContextApi<'_, BoxLayout, A, P>>::layout_child(self, index, constraints)
    }

    #[inline]
    fn position_child(&mut self, index: usize, offset: Offset) {
        <Self as LayoutContextApi<'_, BoxLayout, A, P>>::position_child(self, index, offset)
    }

    #[inline]
    fn complete_layout(&mut self, size: Size) {
        <Self as LayoutContextApi<'_, BoxLayout, A, P>>::complete_layout(self, size)
    }

    #[inline]
    fn child_parent_data_dyn(&self, index: usize) -> Option<&dyn ParentData> {
        // Storage-aware: Direct returns own children's parent_data;
        // Proxy delegates back through the underlying erased ctx.
        match &self.storage {
            BoxLayoutCtxStorage::Direct { children, .. } => children
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|child| &child.parent_data as &dyn ParentData),
            BoxLayoutCtxStorage::Proxy { erased, .. } => erased.child_parent_data_dyn(index),
        }
    }

    #[inline]
    fn child_parent_data_dyn_mut(&mut self, index: usize) -> Option<&mut dyn ParentData> {
        match &mut self.storage {
            BoxLayoutCtxStorage::Direct { children, .. } => children
                .as_mut()
                .and_then(|c| c.get_mut(index))
                .map(|child| &mut child.parent_data as &mut dyn ParentData),
            BoxLayoutCtxStorage::Proxy { erased, .. } => erased.child_parent_data_dyn_mut(index),
        }
    }

    #[inline]
    fn parent_data_type_id(&self) -> Option<std::any::TypeId> {
        // Only report a concrete P when the Direct ctx actually holds
        // a `Vec<ChildState<P>>` payload (children present). For
        // children-less Direct ctxs and Proxy ctxs, returning None
        // (default) is correct — no concrete-payload evidence to assert
        // against. Proxy could chain through to the underlying erased's
        // own `parent_data_type_id` but today the upstream is always a
        // Direct ctx with the same P (the from_erased site is the only
        // construction path), so the extra plumbing buys nothing.
        match &self.storage {
            BoxLayoutCtxStorage::Direct {
                children: Some(_), ..
            } => Some(std::any::TypeId::of::<P>()),
            BoxLayoutCtxStorage::Direct { children: None, .. }
            | BoxLayoutCtxStorage::Proxy { .. } => None,
        }
    }
}

// ============================================================================
// BOX HIT TEST CAPABILITY
// ============================================================================

/// Hit test capability for box (2D) layout.
///
/// Uses `Offset` for position and standard hit test result.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxHitTest;

impl HitTestCapability for BoxHitTest {
    type Position = Offset;
    type Result = BoxHitTestResult;
    type Entry = BoxHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData>
        = BoxHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

/// Hit test result for box protocol.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    /// Path of hit test entries from leaf to root.
    pub path: Vec<BoxHitTestEntry>,
}

impl BoxHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self { path: Vec::new() }
    }

    /// Adds an entry to the hit test path.
    pub fn add(&mut self, entry: BoxHitTestEntry) {
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

    /// Clears all hit entries.
    pub fn clear(&mut self) {
        self.path.clear();
    }
}

/// Individual hit test entry for box protocol.
#[derive(Debug, Clone)]
pub struct BoxHitTestEntry {
    /// Target identifier.
    pub target_id: u64,
    /// Transform from target to root coordinates.
    pub transform: Matrix4,
}

impl BoxHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(target_id: u64, transform: Matrix4) -> Self {
        Self {
            target_id,
            transform,
        }
    }

    /// Creates a hit test entry with identity transform.
    pub fn with_id(target_id: u64) -> Self {
        Self::new(target_id, Matrix4::IDENTITY)
    }
}

/// Box hit test context implementation.
///
/// # Transform accumulation
///
/// Cycle 4 wave 5 R-24: `current_transform()` previously folded the
/// entire `transform_stack: Vec<Matrix4>` via
/// `iter().fold(IDENTITY, |acc, t| acc * t)` -- O(N) matrix-multiply
/// chain on every hit-test entry. Hit testing is hot-path; a 30-deep
/// tree paid 30 mat-mults per entry.
///
/// The fix mirrors Flutter's `HitTestResult._localTransforms` cache:
/// alongside the explicit `transform_stack`, the ctx maintains
/// `composed_transform: Matrix4` updated incrementally on
/// `push_transform` (one mat-mult) and recomputed on `pop_transform`
/// (one full re-fold over the now-shorter stack). Per-call cost
/// drops from O(stack_depth) to O(1) for queries, and pops stay
/// O(stack_depth) but amortize across the matched push.
pub struct BoxHitTestCtx<'ctx, A: Arity, P: ParentData> {
    position: Offset,
    result: BoxHitTestResult,
    transform_stack: Vec<Matrix4>,
    /// Cached composition of `transform_stack` in push-order. Kept in
    /// sync with the stack via `push_transform` (multiply in) and
    /// `pop_transform` (full re-fold over the truncated stack).
    composed_transform: Matrix4,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> BoxHitTestCtx<'ctx, A, P> {
    /// Creates a new box hit test context.
    pub fn new(position: Offset) -> Self {
        Self {
            position,
            result: BoxHitTestResult::new(),
            transform_stack: Vec::new(),
            composed_transform: Matrix4::IDENTITY,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns the current accumulated transform.
    ///
    /// O(1) -- reads the cached composition. See type-level doc for
    /// the R-24 incremental-composition design.
    pub fn current_transform(&self) -> Matrix4 {
        self.composed_transform
    }

    /// Recomputes [`Self::composed_transform`] from `transform_stack`.
    /// Used by `pop_transform` because matrix inversion to "subtract"
    /// the popped factor is more expensive (and more numerically
    /// fraught) than a full re-fold over a typically-shallow stack.
    #[inline]
    fn recompute_composed(&mut self) {
        self.composed_transform = self
            .transform_stack
            .iter()
            .fold(Matrix4::IDENTITY, |acc, t| acc * *t);
    }

    /// Adds self as a hit target with the given ID.
    pub fn add_self(&mut self, target_id: u64) {
        let transform = self.current_transform();
        self.result.add(BoxHitTestEntry::new(target_id, transform));
    }
}

impl<'ctx, A: Arity, P: ParentData> HitTestContextApi<'ctx, BoxHitTest, A, P>
    for BoxHitTestCtx<'ctx, A, P>
{
    fn position(&self) -> &Offset {
        &self.position
    }

    fn result(&self) -> &BoxHitTestResult {
        &self.result
    }

    fn result_mut(&mut self) -> &mut BoxHitTestResult {
        &mut self.result
    }

    fn add_hit(&mut self, entry: BoxHitTestEntry) {
        self.result.add(entry);
    }

    fn is_hit(&self, bounds: Rect) -> bool {
        bounds.contains(Point::new(self.position.dx, self.position.dy))
    }

    fn hit_test_child(&mut self, _index: usize, _position: Offset) -> bool {
        false // Override in actual implementation
    }

    fn push_transform(&mut self, transform: Matrix4) {
        // R-24: keep the cached composition in sync. One mat-mult
        // per push amortizes O(stack_depth) hit-test queries down
        // to O(1).
        self.transform_stack.push(transform);
        self.composed_transform *= transform;
    }

    fn pop_transform(&mut self) {
        // R-24: a popped factor cannot be "un-multiplied" cheaply
        // (would require matrix inverse + multiply, ~5x cost of a
        // forward fold and numerically fragile). Full re-fold over
        // the now-shorter stack is the cleanest fix; hit-test stacks
        // measure ~20-40 deep in practice, well within
        // matrix-multiply burst budgets.
        self.transform_stack.pop();
        self.recompute_composed();
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
    fn test_box_protocol_name() {
        assert_eq!(BoxProtocol::name(), "box");
    }

    #[test]
    fn test_box_layout_default_geometry() {
        let size = BoxLayout::default_geometry();
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_box_hit_test_result() {
        let mut result = BoxHitTestResult::new();
        assert!(result.is_empty());

        result.add(BoxHitTestEntry::with_id(1));
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_box_hit_test_context() {
        let ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(50.0), px(50.0)));

        let bounds = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
        assert!(ctx.is_hit(bounds));

        let outside = Rect::from_ltrb(px(100.0), px(100.0), px(200.0), px(200.0));
        assert!(!ctx.is_hit(outside));
    }

    /// Cycle 4 wave 5 R-24: incremental transform composition must
    /// stay numerically identical to the prior O(N) fold path.
    /// Builds a 3-deep stack and asserts the cached
    /// `current_transform()` equals the explicit `fold(IDENTITY, |a, t| a * t)`.
    #[test]
    fn test_box_hit_test_context_incremental_transform_matches_fold() {
        let mut ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(0.0), px(0.0)));

        // Mat₁: translate (10, 0)
        let t1 = Matrix4::translation(10.0, 0.0, 0.0);
        // Mat₂: rotation 90° about Z
        let t2 = Matrix4::rotation_z(std::f32::consts::FRAC_PI_2);
        // Mat₃: scale 2x
        let t3 = Matrix4::scaling(2.0, 2.0, 1.0);

        ctx.push_transform(t1);
        ctx.push_transform(t2);
        ctx.push_transform(t3);

        let expected = Matrix4::IDENTITY * t1 * t2 * t3;
        let got = ctx.current_transform();
        // Bit-exact: cache and explicit fold do the same mat-mults
        // in the same order.
        assert_eq!(got, expected);
    }

    /// Pop must restore the prior composed state. Push A, push B,
    /// pop B → composed == A.
    #[test]
    fn test_box_hit_test_context_pop_restores_composition() {
        let mut ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(0.0), px(0.0)));

        let t1 = Matrix4::translation(5.0, 5.0, 0.0);
        let t2 = Matrix4::scaling(3.0, 3.0, 1.0);

        ctx.push_transform(t1);
        let after_t1 = ctx.current_transform();

        ctx.push_transform(t2);
        ctx.pop_transform();

        assert_eq!(ctx.current_transform(), after_t1);
    }

    /// Empty stack returns identity.
    #[test]
    fn test_box_hit_test_context_empty_stack_is_identity() {
        let ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(0.0), px(0.0)));
        assert_eq!(ctx.current_transform(), Matrix4::IDENTITY);
    }

    #[test]
    fn test_box_layout_context() {
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let mut ctx: BoxLayoutCtx<'_, Leaf, BoxParentData> = BoxLayoutCtx::new(constraints);

        // D-block PR-A1b U19: `BoxLayoutCtx` now implements both
        // `LayoutContextApi` (user-facing API, returns `&BoxConstraints`)
        // and `BoxLayoutCtxErased` (trait-object bridge, returns owned
        // `BoxConstraints` by Copy). UFCS disambiguates inside this
        // module where both traits are in scope; downstream user code
        // typically only imports `LayoutContextApi` so the bare-method
        // form keeps working.
        assert!(!ctx.is_complete());
        assert_eq!(
            <BoxLayoutCtx<'_, Leaf, BoxParentData> as LayoutContextApi<
                '_,
                BoxLayout,
                Leaf,
                BoxParentData,
            >>::constraints(&ctx)
            .max_width,
            px(100.0)
        );

        <BoxLayoutCtx<'_, Leaf, BoxParentData> as LayoutContextApi<
            '_,
            BoxLayout,
            Leaf,
            BoxParentData,
        >>::complete_layout(&mut ctx, Size::new(px(100.0), px(100.0)));
        assert!(ctx.is_complete());
    }

    #[test]
    fn test_box_constraints_cache_key_equality() {
        let c1 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c2 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c3 = BoxConstraints::tight(Size::new(px(200.0), px(100.0)));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();
        let key3 = BoxConstraintsCacheKey::from_constraints(&c3).unwrap();

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_box_constraints_cache_key_nan() {
        let c = BoxConstraints::new(px(f32::NAN), px(100.0), px(0.0), px(100.0));
        assert!(BoxConstraintsCacheKey::from_constraints(&c).is_none());
    }

    #[test]
    fn test_box_constraints_cache_key_negative_zero() {
        // -0.0 and +0.0 should produce different cache keys (bit-exact)
        let c1 = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0));
        let c2 = BoxConstraints::new(px(-0.0), px(100.0), px(0.0), px(100.0));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();

        // They have different bits, so different keys
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_box_constraints_cache_key_hash() {
        use std::collections::HashSet;

        let c1 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c2 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c3 = BoxConstraints::tight(Size::new(px(200.0), px(100.0)));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();
        let key3 = BoxConstraintsCacheKey::from_constraints(&c3).unwrap();

        let mut set = HashSet::new();
        set.insert(key1);

        // key2 is equal to key1, so set size should stay 1
        set.insert(key2);
        assert_eq!(set.len(), 1);

        // key3 is different, so set size should become 2
        set.insert(key3);
        assert_eq!(set.len(), 2);
    }
}
