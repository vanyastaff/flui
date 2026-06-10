//! Protocol System - Type-safe render object protocols with composition.
//!
//! This module provides the protocol system based on capability composition:
//! - Protocol trait composes Layout and HitTest capabilities
//! - BoxProtocol for 2D cartesian layout (most widgets)
//! - SliverProtocol for scrollable content layout
//!
//! # Architecture
//!
//! ```text
//!              Protocol Trait (Composition)
//!                     │
//!        ┌────────────┴────────────┐
//!        ▼                         ▼
//!   LayoutCap                 HitTestCap
//!        │                         │
//!        ▼                         ▼
//!   BoxLayout                 BoxHitTest
//!   SliverLayout              SliverHitTest
//! ```
//!
//! # Module Structure
//!
//! - `protocol`: Core Protocol trait
//! - `capabilities`: LayoutCapability, HitTestCapability
//! - `box_protocol`: BoxProtocol with BoxLayout, BoxHitTest
//! - `sliver_protocol`: SliverProtocol with SliverLayout, SliverHitTest
//!
//! # Examples
//!
//! ```ignore
//! use flui_rendering::protocol::{BoxProtocol, Protocol, ProtocolConstraints};
//!
//! fn layout<P: Protocol>(constraints: &ProtocolConstraints<P>) {
//!     // Generic over any protocol
//! }
//! ```

// ============================================================================
// MODULE DECLARATIONS
// ============================================================================

// `box_protocol` and `sliver_protocol` are `pub` (rather than the
// historical private `mod`) so the pipeline-seam traits
// `BoxLayoutCtxErased` / `SliverLayoutCtxErased` are reachable to
// downstream consumers via the fully-qualified path
// `flui_rendering::protocol::box_protocol::BoxLayoutCtxErased` without
// being pulled into scope by a glob `use flui_rendering::protocol::*`.
// PR #141 Copilot review feedback (comment 3293746269): a glob re-export
// of the erased trait collides with `LayoutContextApi`'s method names
// (`constraints` / `layout_child` / `position_child` / `complete_layout`
// overlap by design) and triggers ambiguous-method E0034 in widget
// code. The submodule-pub approach surfaces the trait at one explicit
// path without polluting the common namespace. The most-used surfaces
// (`BoxLayoutCtx`, `BoxProtocol`, `LayoutContextApi`, …) are still
// re-exported at `protocol::*` below for ergonomics.
pub mod box_protocol;
mod capabilities;
mod into_render_object;
#[allow(clippy::module_inception)] // protocol.rs inside protocol/ contains core Protocol trait
mod protocol;
pub mod sliver_protocol;

// ============================================================================
// PROTOCOL TRAIT EXPORTS
// ============================================================================

// ============================================================================
// BOX PROTOCOL EXPORTS
// ============================================================================
pub use box_protocol::{
    // Cache key
    BoxConstraintsCacheKey,
    // Hit test
    BoxHitTest,
    BoxHitTestCtx,
    BoxHitTestEntry,
    BoxHitTestResult,
    // Layout
    BoxLayout,
    BoxLayoutCtx,
    // Protocol
    BoxProtocol,
    // Per-child layout state (moved here from the deleted
    // children_access.rs in Mythos Step 5b)
    ChildState,
    ErasedBoxLayoutCtx,
    ErasedChildState,
};
// Erased layout-context trait (D-block PR-A1b U19 / memo D5).
//
// **Deliberately NOT re-exported** at the `protocol::*` level (PR #141
// Copilot review feedback, comment 3293746269):
// `BoxLayoutCtxErased::constraints` / `layout_child` / `position_child`
// / `complete_layout` overlap with `LayoutContextApi`'s method names by
// design (both view the same operations from different angles); putting
// the trait into `protocol::*` would force user widget code that does
// `use flui_rendering::protocol::*;` into ambiguous-method E0034 on
// `ctx.constraints()` etc. The trait stays declared `pub` at its own
// module path so a future `pub mod box_protocol;` lift (when an
// external Protocol impl outside the sealed set is allowed) is a
// one-line change; today no such consumer exists, so the trait is
// effectively crate-internal — the blanket impl on `BoxLayoutCtx` and
// the `from_erased` ctor reach it via the local
// `super::box_protocol::BoxLayoutCtxErased` path within
// `protocol/box_protocol.rs` itself.
// ============================================================================
// CAPABILITY EXPORTS
// ============================================================================
pub use capabilities::{
    // Capability traits
    HitTestCapability,
    HitTestContextApi,
    LayoutCapability,
    LayoutContextApi,
    // Type aliases
    ProtocolConstraints,
    ProtocolGeometry,
    ProtocolHitResult,
    ProtocolHitTestCtx,
    ProtocolLayoutCtx,
    ProtocolPosition,
};
// ============================================================================
// INTO RENDER OBJECT EXPORTS
// ============================================================================
pub use into_render_object::IntoRenderObject;
pub use protocol::{
    // Marker traits
    BidirectionalProtocol,
    // Protocol trait
    Protocol,
    ProtocolCompatible,
    ProtocolRenderObject,
};
// ============================================================================
// SLIVER PROTOCOL EXPORTS
// ============================================================================
pub use sliver_protocol::{
    // Hit test
    MainAxisPosition,
    // Cache key
    SliverConstraintsCacheKey,
    SliverHitTest,
    SliverHitTestCtx,
    SliverHitTestEntry,
    SliverHitTestResult,
    // Layout
    SliverLayout,
    SliverLayoutCtx,
    // Protocol
    SliverProtocol,
};
// Sliver counterpart to `BoxLayoutCtxErased` — same no-public-re-export
// rationale; see the BoxLayoutCtxErased note above.

// ============================================================================
// RENDER OBJECT TRAIT EXPORTS (re-exported from traits module)
// ============================================================================
pub use crate::traits::RenderObject;

// ============================================================================
// PRELUDE MODULE
// ============================================================================

/// Convenient imports for protocol system.
///
/// ```ignore
/// use flui_rendering::protocol::prelude::*;
/// ```
pub mod prelude {
    pub use super::{
        // Marker traits
        BidirectionalProtocol,
        // Concrete capabilities
        BoxHitTest,
        BoxLayout,
        // Protocols
        BoxProtocol,
        // Capability traits
        HitTestCapability,
        HitTestContextApi,
        LayoutCapability,
        LayoutContextApi,
        // Protocol trait
        Protocol,
        ProtocolCompatible,
        // Type aliases
        ProtocolConstraints,
        ProtocolGeometry,
        ProtocolHitResult,
        ProtocolHitTestCtx,
        ProtocolLayoutCtx,
        ProtocolPosition,
        ProtocolRenderObject,
        SliverHitTest,
        SliverLayout,
        SliverProtocol,
    };
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Check if two protocols are compatible at runtime.
pub fn are_protocols_compatible<P1, P2>() -> bool
where
    P1: Protocol + ProtocolCompatible<P2>,
    P2: Protocol,
{
    P1::is_compatible()
}

/// Assert protocol compatibility at compile time.
pub fn assert_compatible<From, To>()
where
    From: Protocol + ProtocolCompatible<To>,
    To: Protocol,
{
    // Compile-time assertion via trait bounds
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_names() {
        assert_eq!(BoxProtocol::name(), "box");
        assert_eq!(SliverProtocol::name(), "sliver");
    }

    #[test]
    fn test_protocol_compatibility() {
        assert!(are_protocols_compatible::<BoxProtocol, BoxProtocol>());
        assert!(are_protocols_compatible::<SliverProtocol, SliverProtocol>());
        assert!(are_protocols_compatible::<BoxProtocol, SliverProtocol>());
        assert!(are_protocols_compatible::<SliverProtocol, BoxProtocol>());
    }

    #[test]
    fn test_compile_time_compatibility() {
        assert_compatible::<BoxProtocol, BoxProtocol>();
        assert_compatible::<SliverProtocol, SliverProtocol>();
        assert_compatible::<BoxProtocol, SliverProtocol>();
        assert_compatible::<SliverProtocol, BoxProtocol>();
    }
}
