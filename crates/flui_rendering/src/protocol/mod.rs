//! Elite Protocol System - Type-safe render object protocols with GATs.
//!
//! Enhanced protocol system providing:
//! - Generic Associated Types (GATs) for protocol-specific contexts
//! - Ambassador delegation for zero-cost trait implementation
//! - Compile-time protocol validation and compatibility checking
//! - Constraint normalization for efficient caching
//! - SIMD-optimized conversions where applicable
//! - Sealed traits for controlled extensibility
//!
//! # Architecture
//!
//! ```text
//!              Protocol Trait (GATs)
//!                     │
//!        ┌────────────┼────────────┐
//!        ▼            ▼            ▼
//!   BoxProtocol  SliverProtocol Custom
//!        │            │            │
//!        ▼            ▼            ▼
//!   Intrinsic    Viewport    Context
//!   Baseline      Aware       (GAT)
//!   Bi-dir
//! ```
//!
//! # Type Flow
//!
//! ```text
//! Protocol<P>
//!   ├─ Object: dyn RenderObject (trait object type)
//!   ├─ Constraints: Input (hashable for cache)
//!   ├─ ParentData: Child metadata
//!   ├─ Geometry: Output
//!   ├─ LayoutContext<'ctx>: GAT for layout
//!   ├─ PaintContext<'ctx>: GAT for painting
//!   └─ HitTestContext<'ctx>: GAT for hit testing
//! ```
//!
//! # Examples
//!
//! ## Basic Protocol Usage
//!
//! ```ignore
//! use flui_rendering::protocol::{BoxProtocol, Protocol};
//! use flui_elite::prelude::*;
//!
//! let arena: EliteArena<Box<dyn RenderBox>, RenderId> = EliteArena::new();
//! let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
//! let normalized = BoxProtocol::normalize_constraints(constraints);
//! ```
//!
//! ## Using GAT Contexts
//!
//! ```ignore
//! fn layout<'ctx>(mut ctx: BoxLayoutContext<'ctx>) -> Size {
//!     let constraints = ctx.constraints();
//!
//!     for i in 0..ctx.child_count() {
//!         let size = ctx.layout_child(i, child_constraints)?;
//!         ctx.position_child(i, Offset::new(0.0, y_offset));
//!         y_offset += size.height;
//!     }
//!
//!     let size = Size::new(width, y_offset);
//!     ctx.complete_layout(size);
//!     size
//! }
//! ```
//!
//! ## Protocol Adapters
//!
//! ```ignore
//! use flui_rendering::protocol::{SliverToBoxAdapter, ProtocolAdapter};
//!
//! let adapter = SliverToBoxAdapter::new();
//! let box_constraints = adapter.adapt_constraints(&sliver_constraints);
//! ```

// ============================================================================
// MODULE DECLARATIONS
// ============================================================================

mod adapters;
mod base;
mod box_protocol;
pub mod sliver;

/// Sealed trait module for protocol safety.
pub mod sealed {
    pub use super::base::Sealed;
}

// ============================================================================
// CORE EXPORTS
// ============================================================================

// Protocol trait and marker traits
pub use base::{
    BaselineProtocol, BidirectionalProtocol, Canvas, ConstrainedProtocol, DelegateProtocolOps,
    HitTestContext, HitTestTarget, IntrinsicProtocol, LayoutContext, PaintContext, Protocol,
    ProtocolCompatible, ProtocolId, ProtocolRegistry,
};

// Default context implementations
pub use base::{DefaultHitTestContext, DefaultLayoutContext, DefaultPaintContext};

// Error types
pub use base::LayoutError;

// BoxProtocol and contexts
pub use box_protocol::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, BoxProtocol, ChildAccessor,
};

// SliverProtocol and contexts
pub use sliver::{
    SliverChildAccessor, SliverHitTestContext, SliverLayoutContext, SliverPaintContext,
    SliverProtocol,
};

// Helper functions (Sliver)
pub use sliver::{calculate_cache_extent, calculate_paint_extent, is_visible};

// Protocol adapters
pub use adapters::{
    BoxToSliverAdapter, ComposableAdapter, ProtocolAdapter, SliverToBoxAdapter, TypedAdapter,
};

#[cfg(feature = "cache")]
pub use adapters::CachedAdapter;

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
        // Core traits
        BaselineProtocol,
        BidirectionalProtocol,
        // Concrete protocols
        BoxProtocol,
        // Adapters
        BoxToSliverAdapter,
        HitTestContext,
        IntrinsicProtocol,
        LayoutContext,
        PaintContext,
        Protocol,

        ProtocolAdapter,
        SliverProtocol,

        SliverToBoxAdapter,
    };

    // Re-export ambassador for adapter delegation
    pub use ambassador::{delegatable_trait, Delegate};
}

// ============================================================================
// GLOBAL PROTOCOL REGISTRY
// ============================================================================

/// Global protocol registry implementation using TypeId for unique IDs.
pub struct GlobalProtocolRegistry;

impl ProtocolRegistry for GlobalProtocolRegistry {
    fn protocol_id<P: Protocol>() -> ProtocolId {
        use std::any::TypeId;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let type_id = TypeId::of::<P>();
        let mut hasher = DefaultHasher::new();
        type_id.hash(&mut hasher);

        ProtocolId::new(hasher.finish() as u32)
    }
}

// ============================================================================
// PROTOCOL COMPATIBILITY IMPLS
// ============================================================================

// Self-compatibility
impl ProtocolCompatible<BoxProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true
    }
}

impl ProtocolCompatible<SliverProtocol> for SliverProtocol {
    fn is_compatible() -> bool {
        true
    }
}

// Cross-protocol compatibility (via adapters)
impl ProtocolCompatible<SliverProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true // Via BoxToSliverAdapter
    }
}

impl ProtocolCompatible<BoxProtocol> for SliverProtocol {
    fn is_compatible() -> bool {
        true // Via SliverToBoxAdapter
    }
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
///
/// # Example
///
/// ```ignore
/// assert_compatible::<BoxProtocol, SliverProtocol>(); // Compiles
/// ```
pub fn assert_compatible<From, To>()
where
    From: Protocol + ProtocolCompatible<To>,
    To: Protocol,
{
    // Compile-time assertion via trait bounds
    const _: () = {};
}

// ============================================================================
// FEATURE FLAGS
// ============================================================================

/// Check if async protocol support is enabled.
pub const fn has_async_support() -> bool {
    cfg!(feature = "async")
}

/// Check if adapter caching is enabled.
pub const fn has_adapter_cache() -> bool {
    cfg!(feature = "cache")
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
    fn test_protocol_ids() {
        let box_id = BoxProtocol::protocol_id();
        let sliver_id = SliverProtocol::protocol_id();

        assert_ne!(box_id, sliver_id);
        assert_eq!(box_id, BoxProtocol::protocol_id()); // Consistent
        assert_eq!(sliver_id, SliverProtocol::protocol_id());
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

    #[test]
    fn test_feature_flags() {
        let _ = has_async_support();
        let _ = has_adapter_cache();
    }
}
