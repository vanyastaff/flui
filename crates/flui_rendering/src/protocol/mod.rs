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

mod box_protocol;
mod capabilities;
mod into_render_object;
mod protocol;
mod sliver_protocol;

// ============================================================================
// PROTOCOL TRAIT EXPORTS
// ============================================================================

pub use protocol::{
    // Marker traits
    BaselineProtocol,
    BidirectionalProtocol,
    IntrinsicProtocol,
    // Protocol trait
    Protocol,
    ProtocolCompatible,
    ProtocolRenderObject,
};

// ============================================================================
// RENDER OBJECT TRAIT EXPORTS (re-exported from traits module)
// ============================================================================

pub use crate::traits::RenderObject;

// ============================================================================
// INTO RENDER OBJECT EXPORTS
// ============================================================================

pub use into_render_object::IntoRenderObject;

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
        BaselineProtocol,
        BidirectionalProtocol,
        // Concrete capabilities
        BoxHitTest,
        BoxLayout,
        // Protocols
        BoxProtocol,
        // Capability traits
        HitTestCapability,
        HitTestContextApi,
        IntrinsicProtocol,
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
