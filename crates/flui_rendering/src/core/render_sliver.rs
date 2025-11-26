//! Sliver protocol render trait.
//!
//! This module provides the `SliverRender<A>` trait for implementing render objects
//! that participate in scrollable layouts (viewports).
//!
//! # Sliver vs Box
//!
//! - **Box**: Fixed 2D layout with `BoxConstraints` → `Size`
//! - **Sliver**: Scrollable layout with `SliverConstraints` → `SliverGeometry`
//!
//! # Architecture
//!
//! ```text
//! SliverRender<A> trait
//! ├── layout() → SliverGeometry
//! ├── paint() → ()
//! └── hit_test() → bool
//! ```

use flui_types::SliverGeometry;
use std::any::Any;
use std::fmt::Debug;

use super::arity::Arity;
use super::contexts::{HitTestContext, LayoutContext, PaintContext};
use super::protocol::SliverProtocol;
use super::render_tree::{HitTestTree, LayoutTree, PaintTree};
use flui_interaction::HitTestResult;

// ============================================================================
// SLIVER RENDER TRAIT
// ============================================================================

/// Sliver protocol render trait.
///
/// Implement this trait for render objects that participate in scrollable layouts.
///
/// # Type Parameters
///
/// - `A`: Arity - compile-time child count (Leaf, Single, Variable, etc.)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SliverRender, Variable, LayoutContext, SliverProtocol};
///
/// impl SliverRender<Variable> for RenderSliverList {
///     fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, SliverProtocol>) -> SliverGeometry
///     where
///         T: LayoutTree,
///     {
///         let mut scroll_extent = 0.0;
///
///         for child_id in ctx.children.iter() {
///             let child_geometry = ctx.layout_child(child_id, ctx.constraints);
///             scroll_extent += child_geometry.scroll_extent;
///         }
///
///         SliverGeometry {
///             scroll_extent,
///             paint_extent: scroll_extent.min(ctx.constraints.remaining_paint_extent),
///             ..Default::default()
///         }
///     }
/// }
/// ```
pub trait SliverRender<A: Arity>: Send + Sync + Debug + 'static {
    /// Computes the sliver geometry given constraints.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Layout context with constraints, children access, and tree operations
    ///
    /// # Returns
    ///
    /// `SliverGeometry` describing scroll extent, paint extent, layout extent,
    /// and other properties for viewport integration.
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, A, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree;

    /// Paints the sliver to a canvas.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Paint context with offset, children access, canvas, and tree operations
    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, A>)
    where
        T: PaintTree;

    /// Performs hit testing for pointer events.
    ///
    /// Default implementation returns `false` (no hit).
    ///
    /// # Arguments
    ///
    /// * `ctx` - Hit test context with position, geometry, and children access
    /// * `result` - Result accumulator for hit test entries
    ///
    /// # Returns
    ///
    /// `true` if this element or any child was hit.
    fn hit_test<T>(
        &self,
        _ctx: &HitTestContext<'_, T, A, SliverProtocol>,
        _result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        // Default: no hit (transparent to hit testing)
        false
    }

    /// Returns a debug name for this render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcasts to concrete type for inspection.
    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    /// Downcasts to mutable concrete type for mutation.
    fn as_any_mut(&mut self) -> &mut dyn Any
    where
        Self: Sized,
    {
        self
    }
}

// ============================================================================
// SLIVER RENDER EXT
// ============================================================================

/// Extension trait for SliverRender with helper methods.
pub trait SliverRenderExt<A: Arity>: SliverRender<A> {
    /// Returns the protocol type (always SliverProtocol).
    fn protocol(&self) -> SliverProtocol {
        SliverProtocol
    }
}

/// Blanket implementation of SliverRenderExt for all SliverRender types.
impl<A: Arity, T: SliverRender<A>> SliverRenderExt<A> for T {}
