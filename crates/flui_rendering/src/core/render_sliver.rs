//! Sliver protocol render trait with context-based API.
//!
//! This module provides the `RenderSliver<A>` trait for implementing render objects
//! that participate in scrollable layouts (viewports).
//!
//! # Naming Convention
//!
//! This trait is named `RenderSliver` (not `SliverRender`) for consistency with
//! `RenderBox`. Both follow the pattern `Render{Protocol}`.
//!
//! # Sliver vs Box
//!
//! - **Box**: Fixed 2D layout with `BoxConstraints` → `Size`
//! - **Sliver**: Scrollable layout with `SliverConstraints` → `SliverGeometry`
//!
//! # Architecture
//!
//! ```text
//! RenderSliver<A> trait
//! ├── layout(ctx: SliverLayoutContext<'_, A>) → SliverGeometry
//! ├── paint(ctx: &mut SliverPaintContext<'_, A>)
//! └── hit_test(ctx: &SliverHitTestContext<'_, A>, result) → bool
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::{RenderSliver, SliverLayoutContext, Variable};
//! use flui_types::SliverGeometry;
//!
//! impl RenderSliver<Variable> for RenderSliverList {
//!     fn layout(&mut self, ctx: SliverLayoutContext<'_, Variable>) -> SliverGeometry {
//!         let mut total_extent = 0.0;
//!         for child in ctx.children.element_ids() {
//!             let child_geom = ctx.layout_child(child, ctx.constraints);
//!             total_extent += child_geom.scroll_extent;
//!         }
//!         SliverGeometry {
//!             scroll_extent: total_extent,
//!             paint_extent: total_extent.min(ctx.constraints.remaining_paint_extent),
//!             ..Default::default()
//!         }
//!     }
//!
//!     fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
//!         // Paint visible children
//!     }
//! }
//! ```

use flui_interaction::HitTestResult;
use flui_types::{Offset, SliverGeometry};
use std::any::Any;
use std::fmt::Debug;

use super::arity::Arity;
use super::contexts::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext};

// ============================================================================
// RENDER SLIVER TRAIT
// ============================================================================

/// Sliver protocol render trait with context-based API.
///
/// Implement this trait for render objects that participate in scrollable layouts.
///
/// # Naming
///
/// Named `RenderSliver` for consistency with `RenderBox`. Both follow the
/// pattern `Render{Protocol}`.
///
/// # Type Parameters
///
/// - `A`: Arity - compile-time child count (Leaf, Single, Variable, etc.)
///
/// # Context Objects
///
/// All methods receive context objects that provide:
/// - Access to children via typed accessors (`ctx.children`)
/// - Tree operations (`ctx.layout_child()`, `ctx.paint_child()`)
/// - Protocol-specific data (`ctx.constraints`, `ctx.offset`, etc.)
pub trait RenderSliver<A: Arity>: Send + Sync + Debug + 'static {
    /// Computes the sliver geometry given constraints.
    ///
    /// # Context
    ///
    /// - `ctx.constraints` - Sliver constraints from viewport
    /// - `ctx.children` - Typed accessor for child ElementIds
    /// - `ctx.layout_child(id, constraints)` - Layout a child sliver
    ///
    /// # Returns
    ///
    /// `SliverGeometry` describing scroll extent, paint extent, layout extent,
    /// and other properties for viewport integration.
    fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> SliverGeometry;

    /// Paints the sliver to a canvas.
    ///
    /// # Context
    ///
    /// - `ctx.offset` - Offset in parent's coordinate space
    /// - `ctx.children` - Typed accessor for child ElementIds
    /// - `ctx.canvas()` - Mutable access to the canvas
    /// - `ctx.paint_child(id, offset)` - Paint a child
    fn paint(&self, ctx: &mut SliverPaintContext<'_, A>);

    /// Performs hit testing for pointer events.
    ///
    /// # Context
    ///
    /// - `ctx.position` - Position in local coordinates
    /// - `ctx.geometry` - Computed sliver geometry from layout
    /// - `ctx.children` - Typed accessor for child ElementIds
    /// - `ctx.hit_test_child(id, position, result)` - Hit test a child
    ///
    /// # Returns
    ///
    /// `true` if this element or any child was hit.
    ///
    /// # Default Implementation
    ///
    /// Tests children first, then self via `hit_test_self`.
    fn hit_test(&self, ctx: &SliverHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        // Test children first
        let child_ids: Vec<_> = ctx.children.element_ids().collect();
        for child_id in child_ids.into_iter().rev() {
            if ctx.hit_test_child(child_id, ctx.position, result) {
                return true;
            }
        }

        // Test self
        if self.hit_test_self(ctx.position, &ctx.geometry) {
            ctx.add_to_result(result);
            return true;
        }

        false
    }

    /// Tests if the position hits this sliver (excluding children).
    ///
    /// Default returns `false` (transparent to hit testing).
    fn hit_test_self(&self, _position: Offset, _geometry: &SliverGeometry) -> bool {
        false
    }

    /// Returns a debug name for this render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcasts to concrete type for inspection.
    fn as_any(&self) -> &dyn Any;

    /// Downcasts to mutable concrete type for mutation.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for ergonomic sliver render operations.
pub trait RenderSliverExt<A: Arity>: RenderSliver<A> {
    /// Checks if position is within the paint extent.
    fn is_in_paint_extent(&self, position: Offset, geometry: &SliverGeometry) -> bool {
        position.dy >= 0.0 && position.dy < geometry.paint_extent
    }
}

impl<A: Arity, R: RenderSliver<A>> RenderSliverExt<A> for R {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::Leaf;

    #[derive(Debug)]
    struct TestSliverBox {
        extent: f32,
    }

    impl RenderSliver<Leaf> for TestSliverBox {
        fn layout(&mut self, ctx: SliverLayoutContext<'_, Leaf>) -> SliverGeometry {
            let paint_extent = self.extent.min(ctx.constraints.remaining_paint_extent);
            SliverGeometry {
                scroll_extent: self.extent,
                paint_extent,
                layout_extent: Some(paint_extent),
                max_paint_extent: Some(self.extent),
                ..Default::default()
            }
        }

        fn paint(&self, _ctx: &mut SliverPaintContext<'_, Leaf>) {
            // Nothing to paint in this test
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_sliver_extension() {
        let sliver = TestSliverBox { extent: 200.0 };

        let geometry = SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: 100.0,
            ..Default::default()
        };

        assert!(sliver.is_in_paint_extent(Offset::new(0.0, 50.0), &geometry));
        assert!(!sliver.is_in_paint_extent(Offset::new(0.0, 150.0), &geometry));
    }
}
