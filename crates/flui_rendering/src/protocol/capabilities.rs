//! Protocol capability traits.
//!
//! This module defines the capability traits that protocols compose:
//! - [`LayoutCapability`]: Layout input/output types
//! - [`HitTestCapability`]: Hit test input/output types
//! - [`PaintCapability`]: Paint component types

use std::fmt::Debug;
use std::hash::Hash;

use flui_foundation::painting::{Caching, Effects, Layering, Painter};
use flui_types::geometry::Offset;

use crate::arity::Arity;
use crate::parent_data::ParentData;

// ============================================================================
// LAYOUT CAPABILITY
// ============================================================================

/// Capability trait for layout operations.
///
/// Groups all types related to layout: constraints from parent,
/// geometry output, and the layout context GAT.
pub trait LayoutCapability: Send + Sync + 'static {
    /// Layout constraints from parent (must be hashable for caching).
    type Constraints: Clone + Debug + Send + Sync + Hash + Eq + 'static;

    /// Layout geometry output (returned to parent after layout).
    type Geometry: Clone + Debug + Default + Send + Sync + 'static;

    /// Layout context with Generic Associated Type.
    type Context<'ctx, A: Arity, P: ParentData>: LayoutContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;

    /// Returns the default geometry for uninitialized state.
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }

    /// Validates constraints before layout.
    fn validate_constraints(_constraints: &Self::Constraints) -> bool {
        true
    }

    /// Normalizes constraints for consistent cache keys.
    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints
    }
}

/// API for layout context operations.
pub trait LayoutContextApi<'ctx, L: LayoutCapability + ?Sized, A: Arity, P: ParentData>:
    Send + Sync
{
    /// Gets the layout constraints from parent.
    fn constraints(&self) -> &L::Constraints;

    /// Checks if layout is complete.
    fn is_complete(&self) -> bool;

    /// Marks layout complete with final geometry.
    fn complete_layout(&mut self, geometry: L::Geometry);

    /// Gets the number of children.
    fn child_count(&self) -> usize;

    /// Layouts a child with given constraints.
    fn layout_child(&mut self, index: usize, constraints: L::Constraints) -> L::Geometry;

    /// Positions a child at the given offset.
    fn position_child(&mut self, index: usize, offset: Offset);

    /// Gets a child's current geometry (after layout).
    fn child_geometry(&self, index: usize) -> Option<&L::Geometry>;

    /// Gets a child's parent data.
    fn child_parent_data(&self, index: usize) -> Option<&P>;

    /// Gets mutable reference to child's parent data.
    fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut P>;
}

// ============================================================================
// HIT TEST CAPABILITY
// ============================================================================

/// Capability trait for hit testing operations.
///
/// Groups all types related to hit testing: position input,
/// result accumulator, entry type, and the hit test context GAT.
pub trait HitTestCapability: Send + Sync + 'static {
    /// Hit test position type (usually Offset for 2D).
    type Position: Clone + Debug + Default + Send + Sync + 'static;

    /// Hit test result accumulator.
    type Result: Default + Send + Sync + 'static;

    /// Individual hit test entry.
    type Entry: Clone + Debug + Send + Sync + 'static;

    /// Hit test context with Generic Associated Type.
    type Context<'ctx, A: Arity, P: ParentData>: HitTestContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}

/// API for hit test context operations.
pub trait HitTestContextApi<'ctx, H: HitTestCapability + ?Sized, A: Arity, P: ParentData>:
    Send + Sync
{
    /// Gets the hit test position in local coordinates.
    fn position(&self) -> &H::Position;

    /// Gets the hit test result accumulator.
    fn result(&self) -> &H::Result;

    /// Gets mutable reference to hit test result.
    fn result_mut(&mut self) -> &mut H::Result;

    /// Adds a hit entry to the result.
    fn add_hit(&mut self, entry: H::Entry);

    /// Checks if position is inside the given bounds.
    fn is_hit(&self, bounds: flui_types::Rect) -> bool;

    /// Tests a child for hits with position transformation.
    fn hit_test_child(&mut self, index: usize, position: H::Position) -> bool;

    /// Adds a transform to the hit test path.
    fn push_transform(&mut self, transform: flui_types::Matrix4);

    /// Removes the most recent transform.
    fn pop_transform(&mut self);

    /// Adds an offset transform (convenience method).
    fn push_offset(&mut self, offset: Offset) {
        self.push_transform(flui_types::Matrix4::translation(offset.dx, offset.dy, 0.0));
    }
}

// ============================================================================
// PAINT CAPABILITY
// ============================================================================

/// Capability trait for paint operations.
///
/// Unlike Layout and HitTest which are protocol-specific, Paint is shared
/// across all protocols (Box and Sliver use the same painting system).
///
/// The capability composes 4 orthogonal concerns:
/// - **Painter**: What to draw (shapes, images, text)
/// - **Layering**: How layers are composed
/// - **Effects**: Visual effects applied
/// - **Caching**: Optimization strategies
pub trait PaintCapability: Send + Sync + 'static {
    /// The canvas/painter implementation for drawing primitives.
    type Painter: Painter;

    /// The layer composition strategy.
    type Layering: Layering;

    /// Visual effects implementation.
    type Effects: Effects;

    /// Caching strategy for paint optimization.
    type Caching: Caching;

    /// The paint context type, parameterized by lifetime, arity and parent data.
    type Context<'ctx, A: Arity, P: ParentData>: PaintContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}

/// API that paint contexts must provide.
pub trait PaintContextApi<'ctx, P: PaintCapability + ?Sized, A: Arity, PD: ParentData>:
    Send + Sync
{
    /// Get access to the underlying painter for direct drawing.
    fn painter(&mut self) -> &mut P::Painter;

    /// Get access to layering operations.
    fn layering(&mut self) -> &mut P::Layering;

    /// Get access to effects operations.
    fn effects(&mut self) -> &mut P::Effects;

    /// Get access to caching information.
    fn caching(&self) -> &P::Caching;

    /// Check if this render object is a repaint boundary.
    fn is_repaint_boundary(&self) -> bool;

    /// Get the current paint offset (accumulated from parent transforms).
    fn offset(&self) -> Offset;
}

// ============================================================================
// TYPE ALIASES (for Protocol usage)
// ============================================================================

use super::protocol::Protocol;

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
