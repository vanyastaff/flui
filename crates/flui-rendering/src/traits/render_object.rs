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
use flui_types::{Offset, Rect};

use crate::{
    protocol::{
        Protocol, ProtocolConstraints, ProtocolGeometry, ProtocolHitResult, ProtocolPosition,
    },
    semantics::SemanticsConfiguration,
};

// ============================================================================
// Capability Traits (Mythos Step 11 extension-trait split)
// ============================================================================

/// Optional paint-effect hooks (alpha + transform layers).
///
/// Only render objects that actually emit OpacityLayer / TransformLayer
/// implement non-default behaviour here -- RenderOpacity overrides
/// `paint_alpha`, RenderTransform overrides `paint_transform`. Other
/// concrete types get the default `None` for both and the paint phase
/// skips the layer-wrapping path.
///
/// This trait is a supertrait of `RenderObject<P>`; the pipeline reads
/// these methods through a `&dyn RenderObject<P>` and the call resolves
/// to whichever impl the concrete type provided.
pub trait PaintEffectsCapability {
    /// Returns the alpha value to apply to children.
    ///
    /// If `Some(alpha)`, the painting pipeline wraps children in an
    /// OpacityLayer. Used by `RenderOpacity` to implement opacity
    /// animations. Default: `None` (no opacity effect).
    fn paint_alpha(&self) -> Option<u8> {
        None
    }

    /// Returns the transform matrix to apply to children.
    ///
    /// If `Some(matrix)`, the painting pipeline wraps children in a
    /// TransformLayer. Used by `RenderTransform` to implement transform
    /// animations. Default: `None` (no transform effect).
    fn paint_transform(&self) -> Option<flui_types::Matrix4> {
        None
    }
}

/// Optional semantics-tree contribution.
///
/// Render objects that describe themselves to the accessibility tree
/// override `describe_semantics_configuration`. Default no-op.
///
/// This trait is a supertrait of `RenderObject<P>`.
pub trait SemanticsCapability {
    /// Describes semantic properties for accessibility.
    ///
    /// Called when building the semantics tree. Override to provide
    /// labels, actions, or other semantic information. Default: no-op.
    fn describe_semantics_configuration(&self, _config: &mut SemanticsConfiguration) {}
}

/// Optional hot-reload reassembly hook.
///
/// Default no-op. Used by render objects that need to invalidate
/// cached state after a hot-reload.
///
/// This trait is a supertrait of `RenderObject<P>`.
pub trait HotReloadCapability {
    /// Marks this render object for reprocessing after hot reload.
    ///
    /// Called by the framework after code changes. The storage layer
    /// will mark this node dirty and reprocess it. Default: no-op.
    fn reassemble(&mut self) {}
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
/// # Capability Traits
///
/// `RenderObject<P>` carries three capability supertraits whose methods
/// are reachable through any `&dyn RenderObject<P>`:
///
/// - [`PaintEffectsCapability`] -- `paint_alpha`, `paint_transform`
/// - [`SemanticsCapability`] -- `describe_semantics_configuration`
/// - [`HotReloadCapability`] -- `reassemble`
///
/// All three default to no-op / `None`. Concrete render objects opt in
/// by writing `impl <Capability> for MyRenderObject { ... }`; the empty
/// `impl <Capability> for MyRenderObject {}` is the explicit opt-out
/// (uses all defaults).
///
/// # Storage Integration
///
/// Render objects are wrapped in `RenderEntry<P>` which adds:
/// - Tree structure via `NodeLinks` (parent, children, depth)
/// - Dirty state via `RenderState<P>` (needs_layout, needs_paint, etc)
/// - Thread-safe access via `RwLock`
///
/// The storage layer calls these trait methods to drive the rendering pipeline.
pub trait RenderObject<P: Protocol>:
    Diagnosticable
    + DowncastSync
    + Send
    + Sync
    + 'static
    + PaintEffectsCapability
    + SemanticsCapability
    + HotReloadCapability
{
    // ========================================================================
    // Core Operations
    // ========================================================================

    /// Performs layout with raw protocol constraints.
    ///
    /// Called by `RenderEntry::layout()`. Returns the computed geometry.
    ///
    /// **Users don't implement this directly.** Protocol traits like
    /// `RenderBox` provide blanket implementations that create typed
    /// contexts and call the typed `perform_layout()` method.
    fn perform_layout_raw(&mut self, constraints: ProtocolConstraints<P>) -> ProtocolGeometry<P>;

    /// Paints this render object.
    ///
    /// Called by the painting pipeline after layout. The offset is this node's
    /// position relative to the parent's origin.
    ///
    /// Protocol traits may provide typed paint methods with better APIs.
    fn paint(&self, context: &mut crate::pipeline::CanvasContext, offset: Offset);

    /// Hit tests this render object with raw protocol types.
    ///
    /// Called by the hit testing pipeline. Returns true if the position hits
    /// this render object or any of its children.
    ///
    /// **Users don't implement this directly.** Protocol traits provide
    /// blanket implementations that create typed contexts.
    fn hit_test_raw(
        &self,
        result: &mut ProtocolHitResult<P>,
        position: ProtocolPosition<P>,
    ) -> bool;

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

    /// Returns the current geometry after layout.
    ///
    /// For Box protocol: `Size`
    /// For Sliver protocol: `SliverGeometry`
    fn geometry(&self) -> &ProtocolGeometry<P>;

    /// Sets the geometry (called by storage layer after layout).
    fn set_geometry(&mut self, geometry: ProtocolGeometry<P>);

    // ========================================================================
    // Effect Layers
    // ========================================================================
    //
    // paint_alpha and paint_transform live on PaintEffectsCapability
    // (Mythos Step 11 extension-trait split). They are still reachable
    // through `&dyn RenderObject<P>` because PaintEffectsCapability is a
    // supertrait, but a render object that doesn't apply any effect
    // layers no longer has to carry the default impls on the core trait.

    // ========================================================================
    // Paint Bounds
    // ========================================================================

    /// Returns the bounds within which this object paints.
    ///
    /// Used for clipping and culling. Should include all pixels this
    /// render object might paint, including effects like shadows.
    fn paint_bounds(&self) -> Rect;

    // ========================================================================
    // Semantics / Hot Reload
    // ========================================================================
    //
    // describe_semantics_configuration lives on SemanticsCapability.
    // reassemble lives on HotReloadCapability. Both are supertraits of
    // RenderObject<P>; reachable through the dyn pointer.

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

    /// Returns the paint offset for the child at the given index.
    ///
    /// Called during painting to position children. The offset is relative
    /// to this node's origin and is typically set during layout via
    /// position_child().
    ///
    /// Default: Offset::ZERO
    fn child_offset(&self, _index: usize) -> Offset {
        Offset::ZERO
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
