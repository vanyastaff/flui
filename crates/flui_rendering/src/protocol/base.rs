//! Protocol trait with composition-based architecture.
//!
//! This module defines the Protocol trait that composes capabilities:
//! - Layout: Constraints, Geometry, LayoutContext
//! - HitTest: Position, Result, HitTestContext
//! - Paint: Painter, Layering, Effects, Caching, PaintContext
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  trait Protocol {                                               │
//! │      type Layout: LayoutCapability;     // Constraints+Geometry │
//! │      type HitTest: HitTestCapability;   // Position+Result      │
//! │      type Paint: PaintCapability;       // 4 painting traits    │
//! │      type DefaultParentData;            // Per-child metadata   │
//! │  }                                                              │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!               ┌───────────────┴───────────────┐
//!               ▼                               ▼
//!        BoxProtocol                     SliverProtocol
//!        Layout=BoxLayout                Layout=SliverLayout
//!        HitTest=BoxHitTest              HitTest=SliverHitTest
//!        Paint=StandardPaint  ◄─SHARED─► Paint=StandardPaint
//! ```
//!
//! # Benefits
//!
//! 1. **Composition over inheritance** - Protocol composes capabilities
//! 2. **Clear type grouping** - Each capability groups related types
//! 3. **Paint sharing** - BoxProtocol and SliverProtocol share StandardPaint
//! 4. **Easy extension** - New protocol = new capability combination
//! 5. **Backend swap** - Replace only Canvas component in Paint

use std::fmt::Debug;

use crate::arity::Arity;
use crate::parent_data::ParentData;
use crate::protocol::capabilities::{HitTestCapability, LayoutCapability, PaintCapability};

// ============================================================================
// SEALED TRAIT
// ============================================================================

/// Private module for sealed trait pattern.
mod sealed {
    /// Sealed marker trait preventing external Protocol implementations.
    pub trait Sealed {}
}

// ============================================================================
// PROTOCOL TRAIT
// ============================================================================

/// Protocol trait that composes capabilities.
///
/// This trait composes three capability traits, each grouping related types:
///
/// - **Layout**: Constraints, Geometry, LayoutContext
/// - **HitTest**: Position, Result, Entry, HitTestContext
/// - **Paint**: Painter, Layering, Effects, Caching, PaintContext
///
/// # Type Parameters
///
/// Each capability is a separate associated type, allowing different protocols
/// to share components. For example, BoxProtocol and SliverProtocol both use
/// StandardPaint for painting.
///
/// # Example
///
/// ```ignore
/// pub struct MyProtocol;
///
/// impl sealed::Sealed for MyProtocol {}
///
/// impl Protocol for MyProtocol {
///     type Layout = MyLayout;      // Has Constraints, Geometry, Context
///     type HitTest = MyHitTest;    // Has Position, Result, Context
///     type Paint = StandardPaint;  // Shared painting system
///     type DefaultParentData = MyParentData;
///
///     fn name() -> &'static str { "my_protocol" }
/// }
/// ```
pub trait Protocol: Send + Sync + Debug + Clone + Copy + sealed::Sealed + 'static {
    /// Layout capability defining constraints, geometry, and layout context.
    type Layout: LayoutCapability;

    /// Hit test capability defining position, result, and hit test context.
    type HitTest: HitTestCapability;

    /// Paint capability composing Painter, Layering, Effects, and Caching.
    type Paint: PaintCapability;

    /// Default parent data for child render objects.
    type DefaultParentData: ParentData + Default;

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
}

// ============================================================================
// TYPE ALIASES FOR CONVENIENCE
// ============================================================================

/// Constraints type for a protocol.
pub type ProtocolConstraints<P> = <<P as Protocol>::Layout as LayoutCapability>::Constraints;

/// Geometry type for a protocol.
pub type ProtocolGeometry<P> = <<P as Protocol>::Layout as LayoutCapability>::Geometry;

/// Hit test position type for a protocol.
pub type ProtocolPosition<P> = <<P as Protocol>::HitTest as HitTestCapability>::Position;

/// Hit test result type for a protocol.
pub type ProtocolHitResult<P> = <<P as Protocol>::HitTest as HitTestCapability>::Result;

/// Layout context type for a protocol.
pub type ProtocolLayoutCtx<'ctx, P, A, PD> =
    <<P as Protocol>::Layout as LayoutCapability>::Context<'ctx, A, PD>;

/// Hit test context type for a protocol.
pub type ProtocolHitTestCtx<'ctx, P, A, PD> =
    <<P as Protocol>::HitTest as HitTestCapability>::Context<'ctx, A, PD>;

/// Paint context type for a protocol.
pub type ProtocolPaintCtx<'ctx, P, A, PD> =
    <<P as Protocol>::Paint as PaintCapability>::Context<'ctx, A, PD>;

// ============================================================================
// PROTOCOL MARKER TRAITS
// ============================================================================

/// Protocols supporting bidirectional layout (both main-axis directions).
pub trait BidirectionalProtocol: Protocol {}

/// Protocols supporting intrinsic dimension queries before layout.
pub trait IntrinsicProtocol: Protocol {
    /// Compute minimum intrinsic main-axis extent for given cross-axis extent.
    fn compute_min_intrinsic_main_axis(
        constraints: &ProtocolConstraints<Self>,
        cross_axis: f32,
    ) -> f32;

    /// Compute maximum intrinsic main-axis extent for given cross-axis extent.
    fn compute_max_intrinsic_main_axis(
        constraints: &ProtocolConstraints<Self>,
        cross_axis: f32,
    ) -> f32;
}

/// Protocols supporting baseline alignment for text and inline content.
pub trait BaselineProtocol: Protocol {
    /// Distance from top edge to baseline, or None if no baseline.
    fn get_distance_to_baseline(geometry: &ProtocolGeometry<Self>) -> Option<f32>;
}

// ============================================================================
// BOX PROTOCOL
// ============================================================================

use crate::parent_data::BoxParentData;
use crate::protocol::capabilities::{BoxHitTest, BoxLayout, StandardPaint};

/// Box protocol using 2D constraints and sizes.
///
/// This is the most common protocol for 2D layout with width/height constraints.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxProtocol;

impl sealed::Sealed for BoxProtocol {}

impl Protocol for BoxProtocol {
    type Layout = BoxLayout;
    type HitTest = BoxHitTest;
    type Paint = StandardPaint;
    type DefaultParentData = BoxParentData;

    fn name() -> &'static str {
        "box"
    }
}

impl BidirectionalProtocol for BoxProtocol {}

// ============================================================================
// SLIVER PROTOCOL
// ============================================================================

use crate::parent_data::SliverParentData;
use crate::protocol::capabilities::{SliverHitTest, SliverLayout};

/// Sliver protocol for scrollable viewport children.
///
/// Slivers are laid out along a single scrolling axis with viewport constraints.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverProtocol;

impl sealed::Sealed for SliverProtocol {}

impl Protocol for SliverProtocol {
    type Layout = SliverLayout;
    type HitTest = SliverHitTest;
    type Paint = StandardPaint;
    type DefaultParentData = SliverParentData;

    fn name() -> &'static str {
        "sliver"
    }
}

// ============================================================================
// PROTOCOL COMPATIBILITY
// ============================================================================

/// Trait for checking protocol compatibility (for adapters).
pub trait ProtocolCompatible<Other: Protocol>: Protocol {
    /// Returns true if protocols can be adapted together.
    fn is_compatible() -> bool {
        false // Default: not compatible
    }
}

// Box and Sliver can be adapted together (e.g., SliverToBoxAdapter)
impl ProtocolCompatible<SliverProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true
    }
}

impl ProtocolCompatible<BoxProtocol> for SliverProtocol {
    fn is_compatible() -> bool {
        true
    }
}

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
    PD: ParentData = <P as Protocol>::DefaultParentData,
>: Send + Sync
{
    /// Perform layout with the given context.
    fn perform_layout(&mut self, ctx: &mut ProtocolLayoutCtx<'_, P, A, PD>);

    /// Paint this render object.
    fn paint(&self, ctx: &mut ProtocolPaintCtx<'_, P, A, PD>);

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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_protocol() {
        assert_eq!(BoxProtocol::name(), "box");
    }

    #[test]
    fn test_sliver_protocol() {
        assert_eq!(SliverProtocol::name(), "sliver");
    }

    #[test]
    fn test_protocol_compatibility() {
        assert!(BoxProtocol::is_compatible::<SliverProtocol>());
        assert!(SliverProtocol::is_compatible::<BoxProtocol>());
        assert!(BoxProtocol::is_compatible::<BoxProtocol>());
        assert!(SliverProtocol::is_compatible::<SliverProtocol>());
    }

    #[test]
    fn test_protocol_types() {
        // Verify type aliases work
        fn _check_types<P: Protocol>() {
            fn _layout_ctx<'a, P: Protocol, A: Arity, PD: ParentData>(
                _: ProtocolLayoutCtx<'a, P, A, PD>,
            ) {
            }
            fn _hit_test_ctx<'a, P: Protocol, A: Arity, PD: ParentData>(
                _: ProtocolHitTestCtx<'a, P, A, PD>,
            ) {
            }
            fn _paint_ctx<'a, P: Protocol, A: Arity, PD: ParentData>(
                _: ProtocolPaintCtx<'a, P, A, PD>,
            ) {
            }
        }

        _check_types::<BoxProtocol>();
        _check_types::<SliverProtocol>();
    }
}
