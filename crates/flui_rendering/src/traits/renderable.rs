//! Renderable trait - base trait for protocol-based render objects.
//!
//! This module defines the `Renderable` trait that abstracts over layout protocols
//! (Box, Sliver, etc.). Both `RenderBox` and `RenderSliver` extend this trait.

use std::fmt::Debug;

use flui_types::Rect;

use crate::arity::Arity;
use crate::context::{HitTestContext, LayoutContext, PaintContext};
use crate::parent_data::ParentData;
use crate::protocol::{LayoutCapability, Protocol};

// ============================================================================
// Renderable Trait
// ============================================================================

/// Base trait for all render objects that use a layout protocol.
///
/// `Renderable` abstracts over the protocol (Box, Sliver) and provides the
/// core methods needed by `Wrapper<T>` to implement `RenderObject`.
///
/// # Architecture
///
/// ```text
/// Renderable  (base trait)
///     │
///     ├── RenderBox : Renderable<Protocol = BoxProtocol>
///     │       (+intrinsics, +baseline, +dry_layout)
///     │
///     └── RenderSliver : Renderable<Protocol = SliverProtocol>
///             (+scroll offset, +cache extent)
///
/// Wrapper<T: Renderable> → implements RenderObject
/// ```
///
/// # Why This Design
///
/// Instead of separate `BoxWrapper` and `SliverWrapper`, we use a single
/// `Wrapper<T>` that works with any `Renderable`. The protocol's
/// associated types (Constraints, Geometry, Context) are accessed via GATs.
///
/// # Example
///
/// ```ignore
/// // User implements RenderBox (which extends Renderable)
/// impl RenderBox for MyWidget {
///     type Arity = Leaf;
///     type ParentData = BoxParentData;
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Leaf, BoxParentData>) {
///         ctx.complete_with_size(Size::new(100.0, 50.0));
///     }
/// }
///
/// // Wrapper<MyWidget> automatically implements RenderObject
/// let wrapper = Wrapper::new(MyWidget::new());
/// ```
pub trait Renderable: Send + Sync + Debug + 'static {
    /// The layout protocol for this renderable (BoxProtocol, SliverProtocol).
    type Protocol: Protocol;

    /// The arity of this renderable (Leaf, Single, Optional, Variable).
    type Arity: Arity;

    /// The parent data type for children of this renderable.
    type ParentData: ParentData + Default;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Performs layout using the protocol-specific context.
    ///
    /// The context type is the high-level `LayoutContext` wrapper for the protocol.
    fn perform_layout(
        &mut self,
        ctx: &mut LayoutContext<'_, Self::Protocol, Self::Arity, Self::ParentData>,
    );

    /// Returns the geometry after layout.
    ///
    /// For BoxProtocol, this returns Size.
    /// For SliverProtocol, this returns SliverGeometry.
    fn geometry(&self) -> <<Self::Protocol as Protocol>::Layout as LayoutCapability>::Geometry;

    /// Sets the geometry.
    fn set_geometry(
        &mut self,
        geometry: <<Self::Protocol as Protocol>::Layout as LayoutCapability>::Geometry,
    );

    // ========================================================================
    // Paint
    // ========================================================================

    /// Paints this renderable using the protocol-specific paint context.
    fn paint(&mut self, ctx: &mut PaintContext<'_, Self::Protocol, Self::Arity, Self::ParentData>);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this renderable using the protocol-specific context.
    fn hit_test(
        &self,
        ctx: &mut HitTestContext<'_, Self::Protocol, Self::Arity, Self::ParentData>,
    ) -> bool;

    // ========================================================================
    // Paint Bounds
    // ========================================================================

    /// Returns the paint bounds for this renderable.
    fn paint_bounds(&self) -> Rect;

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Creates default parent data for a child.
    fn create_default_parent_data() -> Self::ParentData {
        Self::ParentData::default()
    }
}

// ============================================================================
// Protocol-Specific Constraints/Geometry Access
// ============================================================================

/// Extension trait to access protocol constraints type.
pub trait RenderableConstraints: Renderable {
    /// The constraints type for this renderable's protocol.
    type Constraints: Clone + Send + Sync;
}

impl<T: Renderable> RenderableConstraints for T {
    type Constraints = <<T::Protocol as Protocol>::Layout as LayoutCapability>::Constraints;
}

/// Extension trait to access protocol geometry type.
pub trait RenderableGeometry: Renderable {
    /// The geometry type for this renderable's protocol.
    type Geometry: Clone + Send + Sync;
}

impl<T: Renderable> RenderableGeometry for T {
    type Geometry = <<T::Protocol as Protocol>::Layout as LayoutCapability>::Geometry;
}
