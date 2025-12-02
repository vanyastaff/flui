//! RenderSliver trait for sliver protocol render objects.
//!
//! This module provides the `RenderSliver<A>` trait for implementing render objects
//! that use the sliver layout protocol for scrollable content with compile-time arity validation.
//!
//! # Design Philosophy
//!
//! - **Simple and clean**: Minimal API surface, no unnecessary abstractions
//! - **Arity validation**: Compile-time child count constraints
//! - **Context-based**: All operations use typed contexts
//! - **Progressive disclosure**: Simple defaults, explicit when needed
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderObject (base trait)
//!     │
//!     └── RenderSliver<A> (sliver protocol with arity A)
//!             │
//!             ├── layout(ctx) -> SliverGeometry
//!             ├── paint(ctx)
//!             └── hit_test(ctx, result) -> bool
//! ```
//!
//! # Sliver Protocol
//!
//! Slivers are scrollable render objects that:
//! - Have one-dimensional scrolling (vertical or horizontal)
//! - Report scroll extents and paint extents
//! - Support lazy loading and viewport clipping
//! - Compose into scrollable lists, grids, and custom scrollables
//!
//! # Arity System
//!
//! | Arity | Children | Examples |
//! |-------|----------|----------|
//! | `Leaf` | 0 | SliverToBoxAdapter (wraps box child) |
//! | `Single` | 1 | SliverPadding |
//! | `Variable` | 0+ | SliverList, SliverGrid |
//!
//! # Examples
//!
//! ## Single Child Sliver
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderSliver, RenderObject, Single};
//!
//! #[derive(Debug)]
//! struct RenderSliverPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl RenderSliver<Single> for RenderSliverPadding {
//!     fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> SliverGeometry {
//!         // Layout child with adjusted scroll offset
//!         let child_constraints = SliverConstraints {
//!             scroll_offset: (ctx.constraints.scroll_offset - self.padding.before).max(0.0),
//!             ..ctx.constraints
//!         };
//!
//!         let mut child_geometry = ctx.layout_single_child_with(|_| child_constraints);
//!
//!         // Add padding to extents
//!         child_geometry.scroll_extent += self.padding.total();
//!         child_geometry.paint_extent += self.padding.after;
//!
//!         child_geometry
//!     }
//!
//!     fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
//!         let offset = Offset::new(0.0, self.padding.before);
//!         ctx.paint_single_child(offset);
//!     }
//! }
//!
//! impl RenderObject for RenderSliverPadding {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! ## Multi-Child List
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderSliverList {
//!     item_extent: f32,
//! }
//!
//! impl RenderSliver<Variable> for RenderSliverList {
//!     fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> SliverGeometry {
//!         let constraints = ctx.constraints;
//!         let mut geometry = SliverGeometry::zero();
//!
//!         // Calculate visible range
//!         let first_visible = (constraints.scroll_offset / self.item_extent).floor() as usize;
//!         let visible_count = (constraints.remaining_paint_extent / self.item_extent).ceil() as usize;
//!
//!         // Layout visible children
//!         for (i, child_id) in ctx.children().skip(first_visible).take(visible_count).enumerate() {
//!             let child_offset = (first_visible + i) as f32 * self.item_extent;
//!             let child_constraints = SliverConstraints {
//!                 scroll_offset: constraints.scroll_offset - child_offset,
//!                 remaining_paint_extent: self.item_extent.min(constraints.remaining_paint_extent),
//!                 ..constraints
//!             };
//!
//!             let child_geometry = ctx.layout_child(child_id, child_constraints);
//!             geometry.scroll_extent += child_geometry.scroll_extent;
//!             geometry.paint_extent += child_geometry.paint_extent;
//!         }
//!
//!         geometry
//!     }
//!
//!     fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
//!         ctx.paint_all_children();
//!     }
//! }
//!
//! impl RenderObject for RenderSliverList {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```

use std::fmt;

use flui_interaction::HitTestResult;
use flui_types::{Offset, Rect, SliverConstraints, SliverGeometry};

use super::arity::Arity;
use super::contexts::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext};
use super::render_object::RenderObject;

// ============================================================================
// CORE RENDER SLIVER TRAIT
// ============================================================================

/// Render trait for sliver protocol with arity validation.
///
/// This trait provides the foundation for implementing scrollable render objects
/// that use the sliver layout protocol.
///
/// # Type Parameters
///
/// - `A`: Arity type constraining the number of children
///
/// # Required Methods
///
/// - [`layout`](Self::layout) - Computes geometry given constraints
/// - [`paint`](Self::paint) - Draws to canvas
///
/// # Optional Methods
///
/// - [`hit_test`](Self::hit_test) - Pointer event detection
///
/// # Context API
///
/// All methods use contexts with sensible defaults:
///
/// ```rust,ignore
/// // Minimal - uses default SliverProtocol
/// fn layout(&mut self, ctx: LayoutContext<'_, Single, SliverProtocol>) -> SliverGeometry
///
/// // Type alias - explicit protocol
/// fn layout(&mut self, ctx: SliverLayoutContext<'_, Variable>) -> SliverGeometry
/// ```
pub trait RenderSliver<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    /// Computes the geometry of this sliver.
    ///
    /// # Context
    ///
    /// - `ctx.constraints` - Sliver constraints from parent
    ///   - `scroll_offset`: Current scroll position
    ///   - `remaining_paint_extent`: Available space for painting
    ///   - `overlap`: Overlap from previous sliver
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.layout_child(id, c)` - Layout a child (returns SliverGeometry, fallback on error)
    /// - `ctx.set_child_offset(id, offset)` - Position a child
    ///
    /// # Returns
    ///
    /// `SliverGeometry` with:
    /// - `scroll_extent`: Total scrollable length
    /// - `paint_extent`: Amount currently painted (visible)
    /// - `layout_extent`: Amount consuming viewport space
    /// - `max_paint_extent`: Maximum paint extent if fully visible
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Fixed-height sliver
    /// fn layout(&mut self, ctx: SliverLayoutContext<'_, Leaf>) -> SliverGeometry {
    ///     let height = 100.0;
    ///     let visible = height.min(ctx.constraints.remaining_paint_extent);
    ///
    ///     SliverGeometry {
    ///         scroll_extent: height,
    ///         paint_extent: visible,
    ///         layout_extent: visible,
    ///         max_paint_extent: height,
    ///         ..Default::default()
    ///     }
    /// }
    ///
    /// // Single child wrapper
    /// fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> SliverGeometry {
    ///     ctx.layout_single_child()
    /// }
    ///
    /// // Multiple children list
    /// fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> SliverGeometry {
    ///     let mut geometry = SliverGeometry::zero();
    ///
    ///     for child_id in ctx.children() {
    ///         let child_geometry = ctx.layout_child(child_id, ctx.constraints);
    ///         geometry.scroll_extent += child_geometry.scroll_extent;
    ///         geometry.paint_extent += child_geometry.paint_extent;
    ///     }
    ///
    ///     geometry
    /// }
    /// ```
    fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> SliverGeometry;

    /// Paints this sliver to the canvas.
    ///
    /// # Context
    ///
    /// - `ctx.offset` - Position in parent coordinates
    /// - `ctx.geometry` - Geometry from layout (SliverGeometry)
    /// - `ctx.canvas_mut()` - Mutable canvas for drawing
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.paint_child(id, offset)` - Paint a child
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Fixed content
    /// fn paint(&self, ctx: &mut SliverPaintContext<'_, Leaf>) {
    ///     let paint_extent = ctx.paint_extent();
    ///     ctx.canvas_mut().draw_rect(Rect::from_ltwh(0.0, 0.0, 100.0, paint_extent));
    /// }
    ///
    /// // Single child
    /// fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
    ///     ctx.paint_single_child(Offset::ZERO);
    /// }
    ///
    /// // Multiple children
    /// fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
    ///     ctx.paint_all_children();
    /// }
    /// ```
    fn paint(&self, ctx: &mut SliverPaintContext<'_, A>);

    /// Performs hit testing for pointer events.
    ///
    /// # Context
    ///
    /// - `ctx.position` - Position to test
    /// - `ctx.geometry` - Geometry from layout
    /// - `ctx.children()` - Iterator over children
    /// - `ctx.hit_test_child(id, pos, result)` - Test a child
    /// - `ctx.hit_test_self(result)` - Add self to result
    ///
    /// # Default Implementation
    ///
    /// The default tests children in reverse order and adds self if hit.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default behavior works for most slivers
    /// // Override only if you need custom hit testing logic
    /// fn hit_test(&self, ctx: &SliverHitTestContext<'_, Variable>, result: &mut HitTestResult) -> bool {
    ///     // Test children in reverse z-order
    ///     for child_id in ctx.children_reverse() {
    ///         if ctx.hit_test_child(child_id, ctx.position, result) {
    ///             return true;
    ///         }
    ///     }
    ///
    ///     // Check if position is within paint extent
    ///     if ctx.position.dy >= 0.0 && ctx.position.dy < ctx.geometry.paint_extent {
    ///         ctx.hit_test_self(result);
    ///         true
    ///     } else {
    ///         false
    ///     }
    /// }
    /// ```
    fn hit_test(&self, ctx: &SliverHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        // Default: test children in reverse z-order
        for child_id in ctx.children_reverse() {
            if ctx.hit_test_child(child_id, ctx.position, result) {
                return true;
            }
        }

        // Check if position is within this sliver's paint extent
        // Slivers are laid out along the main axis (usually vertical)
        if ctx.position.dy >= 0.0 && ctx.position.dy < ctx.geometry.paint_extent {
            ctx.hit_test_self(result);
            true
        } else {
            false
        }
    }

    /// Returns the child count that should be kept alive even when off-screen.
    ///
    /// This is used for performance optimization in scrollable lists.
    /// Default is 0 (no keep-alive).
    fn child_keep_alive_count(&self) -> usize {
        0
    }

    /// Returns whether this sliver has visual overflow.
    ///
    /// Default is false.
    fn has_visual_overflow(&self) -> bool {
        false
    }

    /// Gets the local bounding rectangle.
    ///
    /// For slivers, this is typically based on paint extent.
    fn local_bounds(&self) -> Rect {
        (self as &dyn RenderObject).local_bounds()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single, Variable};
    use std::marker::PhantomData;

    // Simple test sliver
    #[derive(Debug)]
    struct TestRenderSliver<A: Arity> {
        extent: f32,
        _phantom: PhantomData<A>,
    }

    impl<A: Arity> TestRenderSliver<A> {
        fn new(extent: f32) -> Self {
            Self {
                extent,
                _phantom: PhantomData,
            }
        }
    }

    impl<A: Arity> RenderSliver<A> for TestRenderSliver<A> {
        fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> SliverGeometry {
            let visible = self.extent.min(ctx.constraints.remaining_paint_extent);

            SliverGeometry {
                scroll_extent: self.extent,
                paint_extent: visible,
                layout_extent: visible,
                max_paint_extent: self.extent,
                ..Default::default()
            }
        }

        fn paint(&self, _ctx: &mut SliverPaintContext<'_, A>) {
            // Do nothing
        }
    }

    impl<A: Arity> RenderObject for TestRenderSliver<A> {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_render_sliver_arity() {
        let _leaf: TestRenderSliver<Leaf> = TestRenderSliver::new(100.0);
        let _single: TestRenderSliver<Single> = TestRenderSliver::new(100.0);
        let _variable: TestRenderSliver<Variable> = TestRenderSliver::new(100.0);
        // Compiles = arity system works
    }

    #[test]
    fn test_default_keep_alive() {
        let sliver = TestRenderSliver::<Leaf>::new(100.0);
        assert_eq!(sliver.child_keep_alive_count(), 0);
    }

    #[test]
    fn test_default_visual_overflow() {
        let sliver = TestRenderSliver::<Leaf>::new(100.0);
        assert!(!sliver.has_visual_overflow());
    }
}
