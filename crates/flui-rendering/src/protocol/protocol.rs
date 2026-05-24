//! Core Protocol trait.
//!
//! This module defines the main [`Protocol`] trait that composes capabilities.
//! The protocol is the top-level abstraction that defines how a render object
//! performs layout and hit testing.

use std::fmt::Debug;

use flui_tree::Arity;

use super::capabilities::{
    HitTestCapability, LayoutCapability, ProtocolGeometry, ProtocolHitTestCtx, ProtocolLayoutCtx,
};
use crate::parent_data::ParentData;

// ============================================================================
// SEALED TRAIT
// ============================================================================

/// Private module for sealed trait pattern.
pub(crate) mod sealed {
    /// Sealed marker trait preventing external Protocol implementations.
    pub trait Sealed {}
}

// ============================================================================
// PROTOCOL TRAIT
// ============================================================================

/// Protocol trait that composes capabilities.
///
/// This trait composes two capability traits, each grouping related types:
///
/// - **Layout**: Constraints, Geometry, LayoutContext
/// - **HitTest**: Position, Result, Entry, HitTestContext
///
/// Paint is not part of the protocol - all render objects use the same
/// Canvas API for painting regardless of their protocol.
///
/// # Type Parameters
///
/// Each capability is a separate associated type, allowing different protocols
/// to have different layout and hit test behaviors.
///
/// # Example
///
/// ```ignore
/// pub struct BoxProtocol;
///
/// impl Protocol for BoxProtocol {
///     type Layout = BoxLayout;
///     type HitTest = BoxHitTest;
///     type DefaultParentData = BoxParentData;
///
///     fn name() -> &'static str { "Box" }
/// }
/// ```
pub trait Protocol: Send + Sync + Debug + Clone + Copy + sealed::Sealed + 'static {
    /// Layout capability defining constraints, geometry, and layout context.
    type Layout: LayoutCapability;

    /// Hit test capability defining position, result, and hit test context.
    type HitTest: HitTestCapability;

    /// Default parent data for child render objects.
    type DefaultParentData: ParentData + Default;

    /// Protocol-specific erased layout context — the trait-object form
    /// of the typed `<Self::Layout as LayoutCapability>::Context<'_, A, P>`
    /// without exposing the arity / parent-data type parameters.
    ///
    /// Used at the
    /// [`RenderObject<P>::perform_layout_raw`](crate::traits::RenderObject::perform_layout_raw)
    /// trait boundary so the pipeline can hand a typed layout context to
    /// a protocol-erased render-object trait method without per-protocol
    /// dispatch in the caller.
    ///
    /// **D-block PR-A1b U19 (companion memo D5):** for `BoxProtocol` this
    /// resolves to `dyn BoxLayoutCtxErased + 'ctx`; for `SliverProtocol`
    /// to `dyn SliverLayoutCtxErased + 'ctx`. Each per-protocol trait
    /// exposes the small protocol-shared surface (constraints, child ops,
    /// complete_layout) that the [`RenderObject`](crate::traits::RenderObject)
    /// blanket impl needs to reconstruct a typed layout context via a
    /// `Proxy` storage variant on the typed context type.
    type LayoutCtxErased<'ctx>: ?Sized
    where
        Self: 'ctx;

    /// Protocol name for debugging and diagnostics.
    fn name() -> &'static str;

    /// Default geometry for uninitialized state.
    fn default_geometry() -> <Self::Layout as LayoutCapability>::Geometry {
        <Self::Layout as LayoutCapability>::default_geometry()
    }

    /// Validate constraints before layout (returns true if valid).
    fn validate_constraints(constraints: &<Self::Layout as LayoutCapability>::Constraints) -> bool {
        <Self::Layout as LayoutCapability>::validate_constraints(constraints)
    }

    /// Bootstrap the per-instance `IS_RELAYOUT_BOUNDARY` storage flag after
    /// a successful layout pass.
    ///
    /// Default implementation is a no-op (slivers don't use relayout-boundary
    /// semantics today; that's deferred to Core.2). The `BoxProtocol`
    /// override calls [`RenderState::<BoxProtocol>::compute_relayout_boundary`]
    /// with `parent_uses_size = false` and `sized_by_parent = false` —
    /// Flutter parity for those parameters lands later in Core.2 alongside
    /// the intrinsic-dimension protocol.
    ///
    /// Added in D-block PR-A1 U17 (companion memo D3) so that
    /// [`PipelineOwner::mark_needs_layout`] (U15) has a meaningful
    /// `is_relayout_boundary` answer to read once nodes have laid out at
    /// least once. Pre-bootstrap, all nodes report `false` and propagation
    /// runs to root — the correct fallback (root is the implicit boundary).
    ///
    /// [`RenderState::<BoxProtocol>::compute_relayout_boundary`]: crate::storage::RenderState::compute_relayout_boundary
    /// [`PipelineOwner::mark_needs_layout`]: crate::pipeline::PipelineOwner::mark_needs_layout
    fn bootstrap_relayout_boundary(state: &crate::storage::RenderState<Self>, has_parent: bool)
    where
        Self: Sized,
    {
        let _ = (state, has_parent);
    }

    /// Constructs a leaf-mode (no children, no layout callback) erased
    /// layout context from protocol-typed constraints, then invokes `f`
    /// with a `&mut Self::LayoutCtxErased<'_>` referencing it.
    ///
    /// Closure shape (`FnOnce`) keeps the typed context's lifetime scoped
    /// to the call — the typed `BoxLayoutCtx::new(...)` value lives on
    /// the caller's stack inside the trait method, the erased coercion
    /// borrows it, and the borrow expires when `f` returns.
    ///
    /// **D-block PR-A1b U19 (companion memo D5):** used by
    /// [`RenderEntry::layout`](crate::storage::RenderEntry::layout) for
    /// the leaf / single-node layout path. The pipeline's
    /// `layout_dirty_root` (U20) constructs its own typed context with
    /// children access via disjoint borrows and bypasses this helper.
    fn with_leaf_erased_ctx<R>(
        constraints: <Self::Layout as LayoutCapability>::Constraints,
        f: impl FnOnce(&mut Self::LayoutCtxErased<'_>) -> R,
    ) -> R
    where
        Self: Sized;
}

// ============================================================================
// PROTOCOL MARKER TRAITS
// ============================================================================

/// Protocols supporting bidirectional layout (both main-axis directions).
pub trait BidirectionalProtocol: Protocol {}

// ============================================================================
// PROTOCOL COMPATIBILITY
// ============================================================================

/// Trait for checking protocol compatibility (for adapters).
pub trait ProtocolCompatible<Other: Protocol>: Protocol {
    /// Returns true if protocols can be adapted together.
    fn is_compatible() -> bool {
        false
    }
}

// ============================================================================
// RENDER OBJECT TRAIT
// ============================================================================

/// Render object that works with protocols.
///
/// This trait is parameterized by:
/// - `P`: The protocol (BoxProtocol, SliverProtocol)
/// - `A`: The arity (Leaf, Single, Optional, Variable)
/// - `PD`: The parent data type (defaults to protocol's DefaultParentData)
pub trait ProtocolRenderObject<
    P: Protocol,
    A: Arity,
    PD: ParentData + Default = <P as Protocol>::DefaultParentData,
>: Send + Sync
{
    /// Perform layout with the given context.
    fn perform_layout(&mut self, ctx: &mut ProtocolLayoutCtx<'_, P, A, PD>);

    /// Hit test at the given position.
    fn hit_test(&self, ctx: &mut ProtocolHitTestCtx<'_, P, A, PD>) -> bool;

    /// Get the current geometry (after layout).
    fn geometry(&self) -> &ProtocolGeometry<P>;

    /// Check if layout is needed.
    fn needs_layout(&self) -> bool;

    /// Check if paint is needed.
    fn needs_paint(&self) -> bool;

    /// Mark as needing layout.
    fn mark_needs_layout(&mut self);

    /// Mark as needing paint.
    fn mark_needs_paint(&mut self);
}
