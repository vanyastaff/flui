//! Protocol System - Type-safe render object protocols with composition.
//!
//! This module provides the protocol system based on capability composition:
//! - Protocol trait composes Layout, HitTest, and Paint capabilities
//! - BoxProtocol for 2D cartesian layout (most widgets)
//! - SliverProtocol for scrollable content layout
//!
//! # Architecture
//!
//! ```text
//!              Protocol Trait (Composition)
//!                     │
//!        ┌────────────┼────────────┐
//!        ▼            ▼            ▼
//!   LayoutCap    HitTestCap    PaintCap
//!        │            │            │
//!        ▼            ▼            ▼
//!   BoxLayout    BoxHitTest   StandardPaint
//!   SliverLayout SliverHitTest    (shared)
//! ```
//!
//! # Capabilities
//!
//! Each capability groups related types:
//! - **LayoutCapability**: Constraints, Geometry, LayoutContext
//! - **HitTestCapability**: Position, Result, Entry, HitTestContext
//! - **PaintCapability**: Painter, Layering, Effects, Caching, PaintContext
//!
//! # Examples
//!
//! ## Using Protocol Types
//!
//! ```ignore
//! use flui_rendering::protocol::{BoxProtocol, Protocol, ProtocolConstraints};
//!
//! fn layout<P: Protocol>(constraints: &ProtocolConstraints<P>) {
//!     // Generic over any protocol
//! }
//! ```
//!
//! ## Implementing a RenderObject
//!
//! ```ignore
//! use flui_rendering::protocol::{BoxProtocol, ProtocolRenderObject};
//! use flui_rendering::arity::Single;
//!
//! struct MyRenderBox { /* ... */ }
//!
//! impl ProtocolRenderObject<BoxProtocol, Single> for MyRenderBox {
//!     fn perform_layout(&mut self, ctx: &mut ProtocolLayoutCtx<'_, BoxProtocol, Single, BoxParentData>) {
//!         // Layout implementation
//!     }
//!     // ... other methods
//! }
//! ```

// ============================================================================
// MODULE DECLARATIONS
// ============================================================================

mod base;
pub mod capabilities;

// ============================================================================
// CORE EXPORTS
// ============================================================================

// Protocol trait and implementations
pub use base::{
    BaselineProtocol, BidirectionalProtocol, BoxProtocol, IntrinsicProtocol, Protocol,
    ProtocolCompatible, ProtocolRenderObject, SliverProtocol,
};

// Type aliases for convenience
pub use base::{
    ProtocolConstraints, ProtocolGeometry, ProtocolHitResult, ProtocolHitTestCtx,
    ProtocolLayoutCtx, ProtocolPaintCtx, ProtocolPosition,
};

// Capability traits
pub use capabilities::{
    HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi, PaintCapability,
    PaintContextApi,
};

// Concrete capabilities
pub use capabilities::{
    BoxHitTest, BoxHitTestCtx, BoxHitTestEntry, BoxHitTestResult, BoxLayout, BoxLayoutCtx,
    DynCaching, DynEffects, DynLayering, DynPainter, SliverHitTest, SliverHitTestCtx,
    SliverHitTestEntry, SliverHitTestResult, SliverLayout, SliverLayoutCtx, StandardPaint,
    StandardPaintCtx,
};

// Hit test position types
pub use capabilities::MainAxisPosition;

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
        // Protocol trait and marker traits
        BaselineProtocol,
        BidirectionalProtocol,
        // Concrete capabilities
        BoxHitTest,
        BoxLayout,
        // Concrete protocols
        BoxProtocol,
        // Capability traits
        HitTestCapability,
        HitTestContextApi,
        IntrinsicProtocol,
        LayoutCapability,
        LayoutContextApi,
        PaintCapability,
        PaintContextApi,
        Protocol,
        ProtocolCompatible,
        // Type aliases
        ProtocolConstraints,
        ProtocolGeometry,
        ProtocolHitResult,
        ProtocolHitTestCtx,
        ProtocolLayoutCtx,
        ProtocolPaintCtx,
        ProtocolPosition,
        ProtocolRenderObject,
        SliverHitTest,
        SliverLayout,
        SliverProtocol,
        StandardPaint,
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
///
/// Fails to compile if protocols aren't compatible.
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
