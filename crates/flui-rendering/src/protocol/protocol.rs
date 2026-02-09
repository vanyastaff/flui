//! Core Protocol trait.
//!
//! This module defines the main [`Protocol`] trait that composes capabilities.
//! The protocol is the top-level abstraction that defines how a render object
//! performs layout and hit testing.

use std::fmt::Debug;

use crate::parent_data::ParentData;

use super::capabilities::{
    HitTestCapability, LayoutCapability, ProtocolConstraints, ProtocolGeometry, ProtocolHitTestCtx,
    ProtocolLayoutCtx,
};
use crate::arity::Arity;

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
