//! `RenderObject<P>` trait - Protocol-aware base trait for render objects.
//!
//! This module defines the core `RenderObject<P>` trait that all concrete
//! render objects implement through their protocol-specific traits (RenderBox,
//! RenderSliver).
//!
//! # Architecture
//!
//! FLUI uses a three-tree architecture inspired by Flutter:
//!
//! ```text
//! View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
//! ```
//!
//! The render tree is built from protocol-aware render objects that use the
//! `RenderObject<P>` trait, where `P` is either `BoxProtocol` or
//! `SliverProtocol`.
//!
//! # Protocol System
//!
//! Instead of implementing `RenderObject<P>` directly, users implement:
//! - `RenderBox` for 2D box layout (most widgets)
//! - `RenderSliver` for scrollable content (lists, grids)
//!
//! These traits provide better APIs with typed contexts, Arity support, and
//! ParentData handling. Protocol adapters automatically bridge to the storage
//! layer.
//!
//! # Storage Integration
//!
//! Render objects are wrapped in `RenderEntry<P>` which adds:
//! - Tree structure via `NodeLinks` (parent, children, depth)
//! - Dirty state via `RenderState<P>` (needs_layout, needs_paint, etc)
//! - Thread-safe access via `RwLock`

use downcast_rs::{DowncastSync, impl_downcast};
use flui_foundation::Diagnosticable;

use crate::{
    hit_testing::{CursorIcon, MouseTrackerAnnotation},
    parent_data::ParentData,
    protocol::{Protocol, ProtocolConstraints, ProtocolGeometry, ProtocolPosition},
    semantics::SemanticsConfiguration,
};

/// Result of a raw hit-test bridge call.
///
/// Flutter's hit testing has two related but separate effects: a render object
/// may add itself to the hit-test path, and it may return `true` to stop
/// sibling traversal behind it. `HitTestBehavior::Translucent` relies on that
/// split: it contributes an entry but can still let lower siblings be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitTestOutcome {
    /// Whether the pipeline should append this render object to the global
    /// hit-test path.
    pub add_self: bool,

    /// Whether the parent should treat this subtree as having consumed the hit
    /// and stop testing siblings visually behind it.
    pub blocks_below: bool,
}

impl HitTestOutcome {
    /// A complete miss: no entry and no sibling blocking.
    #[must_use]
    pub const fn miss() -> Self {
        Self {
            add_self: false,
            blocks_below: false,
        }
    }

    /// The legacy/default behavior where adding self and blocking siblings are
    /// coupled to the same boolean.
    #[must_use]
    pub const fn from_hit(hit: bool) -> Self {
        Self {
            add_self: hit,
            blocks_below: hit,
        }
    }

    /// Add the current render object to the hit path without blocking lower
    /// siblings. This is the Flutter translucent side effect.
    #[must_use]
    pub const fn add_self_without_blocking() -> Self {
        Self {
            add_self: true,
            blocks_below: false,
        }
    }

    /// Builds an outcome from explicit entry/blocking bits.
    #[must_use]
    pub const fn new(add_self: bool, blocks_below: bool) -> Self {
        Self {
            add_self,
            blocks_below,
        }
    }
}

/// Base trait for all render objects in the render tree.
///
/// This trait defines the minimal interface required by the storage layer
/// to execute layout, painting, and hit testing. Users don't implement this
/// trait directly - instead, they implement protocol-specific traits like
/// `RenderBox` or `RenderSliver` which provide better APIs with typed
/// contexts and Arity/ParentData support.
///
/// # Type Parameters
///
/// - `P`: The layout protocol (BoxProtocol or SliverProtocol)
///
/// # Effect-layer and Lifecycle Methods
///
/// `RenderObject<P>` carries nine defaulted methods that are the former
/// capability-supertrait surface, now inlined directly on this trait so
/// concrete types need no boilerplate impl blocks:
///
/// - `paint_alpha`, `paint_layer_blend`, `skip_paint`, `paint_transform`,
///   `hit_test_transform` — paint-effect hooks (default `None`/`false`)
/// - `describe_semantics_configuration` — accessibility hook (default no-op)
/// - `reassemble` — hot-reload hook (default no-op; see note below)
/// - `attach`/`detach` — tree-lifecycle hook (default no-op; see
///   *Tree-lifecycle note* below)
///
/// All default to no-op / `None`. Override on `RenderBox` or `RenderSliver`
/// (the blanket impls forward every call from `RenderObject<P>` to the
/// protocol-specific trait). Direct `RenderObject<P>` impls (e.g.
/// `RenderViewAdapter`) get the defaults for free.
///
/// # Hot-reload note
///
/// `reassemble` is a documented FLUI divergence: Flutter's
/// `RenderObject.reassemble()` calls `markNeedsLayout` /
/// `markNeedsPaint` / `markNeedsCompositingBitsUpdate` /
/// `markNeedsSemanticsUpdate` / `visitChildren` — all pipeline-owner
/// operations that require traversing the render tree via
/// `PipelineOwner`. FLUI's object-level default is a no-op because
/// render objects do not hold a pipeline-owner handle; the real fix
/// (`PipelineOwner::reassemble_subtree`) is tracked as the hot-reload
/// epic and deferred deliberately.
///
/// # Tree-lifecycle note
///
/// `attach`/`detach` (ADR-0013) are the seam a render object that must mark
/// **itself** dirty out-of-band — an owned animation controller driving its
/// own layout, a delegate's repaint `Listenable` driving paint — subscribes
/// through: `attach` hands over a generational, least-privilege
/// [`RepaintHandle`](crate::pipeline::RepaintHandle) bound to this node;
/// `detach` is where the subscription is torn down. Neither is a hot path:
/// both fire only on structural insert/remove
/// ([`PipelineOwner::insert`](crate::pipeline::PipelineOwner::insert) and
/// its siblings,
/// [`PipelineOwner::remove_render_object`](crate::pipeline::PipelineOwner::remove_render_object)),
/// never mid-layout/paint/hit-test.
///
/// # Storage Integration
///
/// Render objects are wrapped in `RenderEntry<P>` which adds:
/// - Tree structure via `NodeLinks` (parent, children, depth)
/// - Dirty state via `RenderState<P>` (needs_layout, needs_paint, etc)
/// - Thread-safe access via `RwLock`
///
/// The storage layer calls these trait methods to drive the rendering pipeline.
pub trait RenderObject<P: Protocol>: Diagnosticable + DowncastSync + Send + Sync + 'static {
    // ========================================================================
    // Core Operations
    // ========================================================================

    /// Performs layout with a protocol-erased layout context.
    ///
    /// Called by `RenderEntry::layout_leaf_only()` (leaf path) and the
    /// pipeline's `layout_dirty_root` (U20, parent+children
    /// disjoint-borrow path). Returns either the computed geometry on
    /// success, or a typed [`RenderError`] on contract violation.
    ///
    /// **Users don't implement this directly.** Protocol traits like
    /// `RenderBox` provide blanket implementations that reconstruct a
    /// typed `BoxLayoutCtx<Self::Arity, Self::ParentData>` from the
    /// erased context (via the in-crate `BoxLayoutCtx::from_erased`
    /// ctor) and call the typed [`RenderBox::perform_layout`] method.
    ///
    /// # Signature evolution
    ///
    /// 1. **Pre-U19** — `fn perform_layout_raw(&mut self, constraints:
    ///    ProtocolConstraints<P>) -> ProtocolGeometry<P>`. Blanket impl
    ///    shipped as a no-op returning `*self.size()` because the trait
    ///    surface didn't carry children (companion memo D5).
    ///
    /// 2. **D-block PR-A1b U19 (PR #141)** — signature changed to
    ///    `fn perform_layout_raw(&mut self, ctx: &mut <P as Protocol>::LayoutCtxErased<'_>) -> ProtocolGeometry<P>`
    ///    so the blanket impl can construct a typed [`BoxLayoutCtx`]
    ///    with children access. Contract-violation signalling went
    ///    through `std::panic::panic_any(RenderError::ContractViolation)`
    ///    caught by `catch_unwind` in `RenderEntry::layout_leaf_only` —
    ///    `panic_any` was a niche escape hatch with hidden control flow.
    ///
    ///    The PR #141 review (finding #5) called this out as a
    ///    Constitution Principle 6 violation — using panic primitives
    ///    for an error condition the caller can structurally handle.
    ///
    /// 3. **Current shape (this PR, follow-up to #141 #5 Option A)** —
    ///    signature returns `RenderResult<ProtocolGeometry<P>>` so
    ///    contract violations propagate as typed `Err(RenderError::...)`
    ///    directly through `?`. `panic_any` removed; `catch_unwind` in
    ///    `RenderEntry::layout_leaf_only` retained only to wrap genuine
    ///    runtime panics from third-party user widget code into
    ///    [`RenderError::Poisoned`].
    ///
    /// The [`Protocol::LayoutCtxErased`] GAT resolves to the
    /// per-protocol trait-object form: `dyn BoxLayoutCtxErased` for
    /// `BoxProtocol`, `dyn SliverLayoutCtxErased` for `SliverProtocol`.
    ///
    /// [`BoxLayoutCtx`]: crate::protocol::BoxLayoutCtx
    /// [`RenderBox::perform_layout`]: crate::traits::RenderBox::perform_layout
    /// [`Protocol::LayoutCtxErased`]: crate::protocol::Protocol::LayoutCtxErased
    /// [`RenderError`]: crate::error::RenderError
    /// [`RenderError::Poisoned`]: crate::error::RenderError::Poisoned
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <P as Protocol>::LayoutCtxErased<'_>,
    ) -> crate::error::RenderResult<ProtocolGeometry<P>>;

    /// Records this render object's paint fragment.
    ///
    /// Called by the paint walk after layout. The recorder is
    /// pre-positioned at this node's origin in the current layer
    /// space; `child_count` is the number of tree children.
    ///
    /// **Users don't implement this directly.** Protocol traits provide
    /// blanket implementations that wrap the recorder in the typed,
    /// arity-gated [`PaintCx`](crate::context::PaintCx) and call the
    /// protocol-level `paint` (e.g.
    /// [`RenderBox::paint`](crate::traits::RenderBox::paint)). The
    /// recorded fragment is replayed into the layer tree by the
    /// pipeline owner — paint never touches the live tree (sans-IO).
    ///
    /// `size` is the node's laid-out paint size in local pixels — for
    /// the box protocol it is `RenderState::geometry` (the box's
    /// `Size`); for the sliver protocol it is the absolute paint size
    /// (`get_absolute_size(paint_extent)`). The pipeline resolves it
    /// from [`RenderState`](crate::storage::RenderState) — the **sole**
    /// owner of geometry — and hands it in, so paint code reads
    /// `ctx.size()` instead of a per-object `size` field (2B field
    /// dedup: render objects no longer cache their own geometry).
    fn paint_raw(
        &self,
        recorder: &mut crate::context::FragmentRecorder,
        child_count: usize,
        size: flui_types::Size,
    );

    /// Hit tests this render object with raw protocol types.
    ///
    /// Called by the hit-test walk. `position` is in this node's local
    /// space; `child_count` is the number of tree children; `hit_child`
    /// recurses into a child subtree — `Some(p)` at an exact position
    /// (the caller already transformed it), `None` at the child's
    /// laid-out position (`RenderState.offset`, resolved by the
    /// driver). Returns whether the position hits this node or any
    /// child. Hit entries are recorded by the driver, leaf-first.
    ///
    /// **Users don't implement this directly.** Protocol traits provide
    /// blanket implementations that create typed contexts and call the
    /// protocol-level `hit_test` (e.g. `RenderBox::hit_test`).
    ///
    /// `size` is the node's laid-out size in local pixels, resolved by
    /// the driver from [`RenderState`](crate::storage::RenderState)
    /// (geometry's sole owner). The box protocol uses it for the default
    /// bounds gate (`ctx.is_within_own_size()`); the sliver protocol
    /// ignores it (the driver owns the geometry/cross-axis gate). Render
    /// objects no longer cache their own size (2B field dedup).
    fn hit_test_raw(
        &self,
        position: ProtocolPosition<P>,
        child_count: usize,
        size: flui_types::Size,
        hit_child: &mut (dyn FnMut(usize, Option<ProtocolPosition<P>>) -> bool + Send + Sync),
    ) -> HitTestOutcome;

    // ========================================================================
    // Intrinsic / Dry Queries
    // ========================================================================

    /// Computes one intrinsic dimension with raw protocol types.
    ///
    /// Called by the pipeline's memoizing intrinsics walk
    /// (`PipelineOwner::box_intrinsic_dimension`); results are cached
    /// per node in `RenderState`'s layout cache, never here.
    /// `child_query` answers the same question for a tree child — the
    /// driver memoizes each level, so a child probed twice with the
    /// same extent computes once.
    ///
    /// **Users don't implement this directly.** Protocol traits provide
    /// blanket implementations that wrap `child_query` in a typed
    /// context and call the protocol-level `compute_*` methods (e.g.
    /// [`RenderBox::compute_min_intrinsic_width`](crate::traits::RenderBox::compute_min_intrinsic_width)).
    ///
    /// Default: `0.0` — Flutter's `RenderBox` default for every
    /// intrinsic dimension; protocols without intrinsic sizing (sliver)
    /// keep it.
    fn intrinsic_raw(
        &self,
        _dimension: crate::storage::IntrinsicDimension,
        _extent: f32,
        _child_count: usize,
        _child_parent_data: &[Option<&dyn ParentData>],
        _child_query: &mut (
                 dyn FnMut(usize, crate::storage::IntrinsicDimension, f32) -> f32 + Send + Sync
             ),
    ) -> f32 {
        0.0
    }

    /// Computes the dry-layout geometry for `constraints` — the
    /// geometry `perform_layout` WOULD produce, with no side effects.
    ///
    /// Same driver/memoization contract as
    /// [`intrinsic_raw`](Self::intrinsic_raw); `child_dry` answers the
    /// dry-layout question for a tree child.
    ///
    /// Default: the protocol's default geometry (Flutter's `RenderBox`
    /// debug-throws here; a wrong dry size is loud in layout tests
    /// without poisoning release builds).
    fn dry_layout_raw(
        &self,
        _constraints: ProtocolConstraints<P>,
        _child_count: usize,
        _child_parent_data: &[Option<&dyn ParentData>],
        _child_query: &mut (
                 dyn FnMut(
            usize,
            crate::context::DryLayoutChildRequest,
        ) -> crate::context::DryLayoutChildResponse
                     + Send
                     + Sync
             ),
    ) -> ProtocolGeometry<P> {
        P::default_geometry()
    }

    /// Computes the dry baseline for `constraints` — where the first
    /// baseline of the given kind WOULD sit after a layout with these
    /// constraints. `None` means "this box has no baseline".
    ///
    /// Container objects that derive their baseline from a child need a
    /// child-query channel like the other memoized walks; the driver
    /// memoizes every level in the per-node layout cache.
    fn dry_baseline_raw(
        &self,
        _constraints: ProtocolConstraints<P>,
        _baseline: crate::traits::TextBaseline,
        _child_count: usize,
        _child_parent_data: &[Option<&dyn ParentData>],
        _child_query: &mut (
                 dyn FnMut(
            usize,
            crate::context::DryBaselineChildRequest,
        ) -> crate::context::DryBaselineChildResponse
                     + Send
                     + Sync
             ),
    ) -> Option<f32> {
        None
    }

    /// Distance from the top of this box to its first baseline of `baseline`
    /// kind, after layout. Used by containers (`RenderBaseline`, flex
    /// baseline alignment) during `perform_layout`.
    ///
    /// Default: `None` (no baseline). Box objects override via
    /// [`RenderBox::compute_distance_to_actual_baseline`](crate::traits::RenderBox::compute_distance_to_actual_baseline).
    fn actual_baseline_raw(&self, _baseline: crate::traits::TextBaseline) -> Option<f32> {
        None
    }

    // ========================================================================
    // Optimization Boundaries
    // ========================================================================

    /// Returns whether this is a repaint boundary.
    ///
    /// Repaint boundaries create compositing layers for caching painted
    /// content. Use for widgets that change frequently (animations) or have
    /// expensive paint operations.
    ///
    /// Default: `false` (no caching)
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Returns whether this is a relayout boundary.
    ///
    /// Relayout boundaries prevent layout changes from propagating upward.
    /// Use for widgets with fixed sizes or `sized_by_parent = true`.
    ///
    /// Default: `false` (layout can propagate)
    fn is_relayout_boundary(&self) -> bool {
        false
    }

    /// Returns whether size depends only on constraints (not on children).
    ///
    /// When true, `perform_resize()` is called instead of `perform_layout()`
    /// when only constraints change, improving performance.
    ///
    /// Default: `false` (size depends on children)
    fn sized_by_parent(&self) -> bool {
        false
    }

    /// Returns whether this widget always needs compositing.
    ///
    /// Use for widgets that apply effects requiring their own layer
    /// (like clip or backdrop filters).
    ///
    /// Default: `false`
    fn always_needs_compositing(&self) -> bool {
        false
    }

    // ========================================================================
    // Geometry Access
    // ========================================================================
    //
    // 2B field dedup: geometry lives **only** on
    // `RenderState<P>` (geometry's sole owner). The former
    // `geometry()` / `set_geometry()` / `paint_bounds()` trait methods —
    // which forced every render object to cache its own size and risked
    // desync with the committed `RenderState` value — are gone. The
    // pipeline reads `entry.state().geometry()` directly; paint / hit_test
    // receive the resolved `size` as a method argument instead.

    // ========================================================================
    // Effect Layers
    // ========================================================================

    /// Returns the alpha value to apply to children.
    ///
    /// If `Some(alpha)`, the painting pipeline wraps children in an
    /// `OpacityLayer`. Used by `RenderOpacity` to implement opacity
    /// animations. Override on [`RenderBox`](crate::traits::RenderBox) or
    /// [`RenderSliver`](crate::traits::RenderSliver) — the blanket impls
    /// forward the call here. Default: `None` (no opacity effect).
    fn paint_alpha(&self) -> Option<u8> {
        None
    }

    /// Returns the blend mode for the opacity layer wrapping children.
    ///
    /// If `Some(mode)`, the pipeline passes the mode to
    /// `OpacityLayer::with_blend` so advanced blend modes (Multiply, Screen,
    /// etc.) are preserved through the layer-tree compositor path.
    /// Default: `None` (= `SrcOver`).
    fn paint_layer_blend(&self) -> Option<flui_types::painting::BlendMode> {
        None
    }

    /// Whether this render object should suppress all child painting.
    ///
    /// Returns `true` when the node is fully transparent and no children
    /// should be painted (e.g. `RenderOpacity` / `RenderSliverOpacity`
    /// at `alpha == 0` without the `always_needs_compositing` flag).
    /// Default: `false`.
    fn skip_paint(&self) -> bool {
        false
    }

    /// Returns the transform matrix to apply to children.
    ///
    /// If `Some(matrix)`, the painting pipeline wraps children in a
    /// `TransformLayer`. `size` is the node's laid-out size from
    /// [`RenderState`](crate::storage::RenderState) (2B field dedup).
    /// Default: `None` (no transform effect).
    fn paint_transform(&self, size: flui_types::Size) -> Option<flui_types::Matrix4> {
        let _ = size;
        None
    }

    /// Returns the transform matrix for hit testing.
    ///
    /// If `Some(matrix)`, the hit-test pipeline pushes this transform
    /// onto the `HitTestResult` stack before recursing into children.
    /// Default: `None` (no transform — uses identity).
    fn hit_test_transform(&self, size: flui_types::Size) -> Option<flui_types::Matrix4> {
        let _ = size;
        None
    }

    /// The pointer-event handler this render object contributes to its hit
    /// entry, if any.
    ///
    /// When a hit lands on this node, the pipeline attaches the returned handler
    /// to the node's [`HitTestEntry`](crate::hit_testing::HitTestEntry);
    /// [`HitTestResult::dispatch`](crate::hit_testing::HitTestResult) then
    /// invokes it with the (locally-transformed) [`PointerEvent`], honoring the
    /// returned [`EventPropagation`]. Default `None` — only a render object that
    /// listens for pointer events (e.g. `RenderListener`) overrides it. This is
    /// the arena analogue of Flutter's `RenderPointerListener` registering
    /// itself as the `HitTestEntry`'s target.
    ///
    /// [`PointerEvent`]: crate::hit_testing::PointerEvent
    /// [`EventPropagation`]: crate::hit_testing::EventPropagation
    fn pointer_event_handler(&self) -> Option<crate::hit_testing::PointerEventHandler> {
        None
    }

    /// The mouse cursor this render object contributes to its hit entry.
    ///
    /// Default `CursorIcon::Default`; `RenderMouseRegion` overrides this so
    /// [`MouseTracker`](crate::prelude::MouseTracker) can resolve the active
    /// platform cursor from the leaf-first hit-test path.
    fn mouse_cursor(&self) -> CursorIcon {
        CursorIcon::Default
    }

    /// Mouse-tracker annotation contributed to this render object's hit entry.
    ///
    /// The pipeline passes the entry's render id so the annotation can use the
    /// same stable region identity as the hit-test path. Default `None`.
    fn mouse_tracker_annotation(
        &self,
        id: flui_foundation::RenderId,
    ) -> Option<MouseTrackerAnnotation> {
        let _ = id;
        None
    }

    // ========================================================================
    // Semantics / Hot Reload
    // ========================================================================

    /// Describes semantic properties for accessibility.
    ///
    /// Called when building the semantics tree. Override to provide
    /// labels, actions, or other semantic information. Default: no-op.
    fn describe_semantics_configuration(&self, _config: &mut SemanticsConfiguration) {}

    /// Whether the semantics assembly walk should skip this render
    /// object's entire child subtree.
    ///
    /// Consulted by `flui-rendering`'s `run_semantics` assembly walk
    /// (ADR-0014 D5) before it recurses into children — the least-privilege
    /// counterpart of Flutter's `visitChildrenForSemantics` override that
    /// `RenderExcludeSemantics` uses to visit no children while excluding.
    /// This node's own config is still built and merged/boundary-decided
    /// normally; only its descendants are dropped from the walk.
    ///
    /// Default: `false` (children are always visited). Override on
    /// [`RenderBox`](crate::traits::RenderBox) or
    /// [`RenderSliver`](crate::traits::RenderSliver) — the blanket impls
    /// forward the call here.
    fn excludes_semantics_subtree(&self) -> bool {
        false
    }

    /// Marks this render object for reprocessing after hot reload.
    ///
    /// Default: no-op. See the *Hot-reload note* in the trait doc for the
    /// reason this is a documented FLUI divergence from Flutter semantics.
    fn reassemble(&mut self) {}

    // ========================================================================
    // Tree Lifecycle (ADR-0013)
    // ========================================================================

    /// Hands this render object a generational, least-privilege self-dirty
    /// handle when it enters the tree.
    ///
    /// Called exactly once, immediately after the pipeline assigns this
    /// node's [`RenderId`](flui_foundation::RenderId) and wires its tree
    /// links — see
    /// [`PipelineOwner::insert`](crate::pipeline::PipelineOwner::insert)
    /// and its sibling insertion methods. A render object that must mark
    /// **itself** dirty out-of-band subscribes to its source here and
    /// self-marks on notify via
    /// [`RepaintHandle::mark_needs_layout`](crate::pipeline::RepaintHandle::mark_needs_layout)
    /// or
    /// [`RepaintHandle::mark_needs_paint`](crate::pipeline::RepaintHandle::mark_needs_paint).
    ///
    /// Default: no-op — override on `RenderBox` or `RenderSliver`,
    /// mirroring `reassemble`. See the *Tree-lifecycle note* in the trait
    /// doc.
    fn attach(&mut self, handle: crate::pipeline::RepaintHandle) {
        let _ = handle;
    }

    /// Tears down whatever [`Self::attach`] subscribed to, before this
    /// render object leaves the tree.
    ///
    /// Called for every id in a removed subtree by
    /// [`PipelineOwner::remove_render_object`](crate::pipeline::PipelineOwner::remove_render_object),
    /// before the subtree's dirty-queue entries are evicted. Not a
    /// correctness prerequisite — the handle captured in `attach` is
    /// generational and already degrades to a silent no-op once this node
    /// is removed — but it is the point to drop a still-live
    /// `add_listener` subscription so a running notifier doesn't keep it
    /// alive for nothing.
    ///
    /// Default: no-op.
    fn detach(&mut self) {}

    // ========================================================================
    // Children Access (для pipeline/owner.rs)
    // ========================================================================

    /// Returns the number of children for painting.
    ///
    /// Note: This is separate from tree children. Render objects may have
    /// different numbers of logical vs tree children (e.g.,
    /// MultiChildRenderObjectWidget).
    ///
    /// Default: 0 (leaf nodes)
    fn child_count(&self) -> usize {
        0
    }

    // ========================================================================
    // Diagnostics
    // ========================================================================

    /// Stable static identifier for this render object.
    ///
    /// Used by the pipeline owner in error messages (specifically
    /// [`crate::error::RenderError::Poisoned`]) to identify the offending
    /// render object without holding a `String` or allocating per call.
    ///
    /// The default body monomorphizes per concrete `Self` and returns
    /// [`core::any::type_name::<Self>()`]. Vtables for `dyn
    /// RenderObject<P>` carry a pointer to that monomorphized default,
    /// so calling `obj.debug_name()` through a dyn pointer yields the
    /// concrete type name. Concrete impls may override to provide a
    /// shorter / more human-readable name.
    ///
    /// Mythos Step 12 (2026-05-20): introduced alongside the
    /// `std::panic::catch_unwind` plumbing that turns trait-call panics
    /// into `RenderError::Poisoned` rather than process aborts.
    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    // ========================================================================
    // Pipeline Integration
    // ========================================================================
    //
    // Historical note (U2 exemplar refactor, see docs/PORT.md): the trait
    // formerly carried a `set_was_repaint_boundary(&mut self, bool)` method.
    // It was a leaky abstraction -- framework bookkeeping that only existed
    // on the trait because Flutter's Dart classes are flat. The bit now lives
    // on `RenderState<P>::flags` as `WAS_REPAINT_BOUNDARY` (see
    // `crates/flui-rendering/src/storage/flags.rs`) and is flipped by the
    // paint phase via an atomic store, without acquiring a lock on the
    // trait object. Removing the method also removes the only paint-phase
    // `&mut` access to the trait surface.

    // Cycle 4 wave 5 R-21: `insert_into_pipeline` convenience method
    // removed. It was a default trait method gated by `Self: Sized`
    // (so unusable through `dyn RenderObject<P>`) that wrapped a
    // single line: `owner.insert(self)`. Workspace grep showed zero
    // production callsites -- the trait was paying compile-time and
    // API-stability cost for a convenience that earned nothing.
    //
    // Direct equivalent: `owner.insert(Box::new(render_object))`,
    // see [`crate::pipeline::PipelineOwner::insert`]. The real
    // load-bearing piece is the `From<Box<dyn RenderObject<P>>> for
    // RenderNode` impl in `storage/node.rs`, which the `insert`
    // method exercises via `.into()`.
}

impl_downcast!(sync RenderObject<P> where P: Protocol);

#[cfg(test)]
mod tests {
    use flui_types::Size;

    use super::*;
    use crate::protocol::BoxProtocol;

    /// Minimal `RenderObject<BoxProtocol>` implementer with no overrides,
    /// used to exercise this trait's default method bodies -- the contract
    /// every concrete render object gets "for free" until it opts in to
    /// something else.
    #[derive(Debug)]
    struct MinimalLeaf;

    impl Diagnosticable for MinimalLeaf {}

    impl RenderObject<BoxProtocol> for MinimalLeaf {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <BoxProtocol as Protocol>::LayoutCtxErased<'_>,
        ) -> crate::error::RenderResult<Size> {
            Ok(Size::ZERO)
        }

        fn paint_raw(
            &self,
            _recorder: &mut crate::context::FragmentRecorder,
            _child_count: usize,
            _size: Size,
        ) {
        }

        fn hit_test_raw(
            &self,
            _position: flui_types::Offset,
            _child_count: usize,
            _size: Size,
            _hit_child: &mut (dyn FnMut(usize, Option<flui_types::Offset>) -> bool + Send + Sync),
        ) -> HitTestOutcome {
            HitTestOutcome::miss()
        }
    }

    #[test]
    fn hit_test_outcome_constructors_set_the_expected_bits() {
        assert_eq!(HitTestOutcome::miss(), HitTestOutcome::new(false, false));
        assert_eq!(
            HitTestOutcome::from_hit(true),
            HitTestOutcome::new(true, true)
        );
        assert_eq!(
            HitTestOutcome::from_hit(false),
            HitTestOutcome::new(false, false)
        );
        assert_eq!(
            HitTestOutcome::add_self_without_blocking(),
            HitTestOutcome::new(true, false)
        );
    }

    #[test]
    fn default_optimization_boundary_flags_are_all_false() {
        let leaf = MinimalLeaf;
        assert!(!leaf.is_repaint_boundary());
        assert!(!leaf.is_relayout_boundary());
        assert!(!leaf.sized_by_parent());
        assert!(!leaf.always_needs_compositing());
    }

    #[test]
    fn default_effect_layer_hooks_are_inert() {
        let leaf = MinimalLeaf;
        assert_eq!(leaf.paint_alpha(), None);
        assert_eq!(leaf.paint_layer_blend(), None);
        assert!(!leaf.skip_paint());
        assert_eq!(leaf.paint_transform(Size::ZERO), None);
        assert_eq!(leaf.hit_test_transform(Size::ZERO), None);
        assert!(leaf.pointer_event_handler().is_none());
        assert_eq!(leaf.mouse_cursor(), CursorIcon::Default);
        assert!(
            leaf.mouse_tracker_annotation(flui_foundation::RenderId::new(1))
                .is_none()
        );
    }

    #[test]
    fn default_semantics_hook_does_not_mutate_the_configuration() {
        let leaf = MinimalLeaf;
        let mut config = SemanticsConfiguration::new();

        leaf.describe_semantics_configuration(&mut config);

        assert!(!config.is_semantics_boundary());
        assert!(!config.blocks_user_actions());
        assert!(!leaf.excludes_semantics_subtree());
    }

    #[test]
    fn default_reassemble_is_a_no_op() {
        let mut leaf = MinimalLeaf;
        leaf.reassemble();
    }

    #[test]
    fn default_child_count_is_zero() {
        let leaf = MinimalLeaf;
        assert_eq!(leaf.child_count(), 0);
    }

    #[test]
    fn default_debug_name_is_the_concrete_type_name() {
        let leaf = MinimalLeaf;
        assert!(leaf.debug_name().ends_with("MinimalLeaf"));
    }

    #[test]
    fn render_object_trait_object_downcasts_to_the_concrete_type() {
        let boxed: Box<dyn RenderObject<BoxProtocol>> = Box::new(MinimalLeaf);
        assert!(boxed.downcast_ref::<MinimalLeaf>().is_some());
    }
}
