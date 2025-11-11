//! RenderSliver trait - for sliver-based render objects
//!
//! This module provides the `RenderSliver` trait for implementing scrollable
//! render objects that use SliverConstraints instead of BoxConstraints.
//!
//! # Architecture
//!
//! Slivers are specialized render objects designed for scrollable content:
//! - Use **SliverConstraints** instead of BoxConstraints
//! - Return **SliverGeometry** instead of Size
//! - Support lazy loading and viewport awareness
//! - Enable efficient infinite scrolling
//!
//! # Sliver vs Box RenderObjects
//!
//! | Aspect | Box (Render) | Sliver (RenderSliver) |
//! |--------|--------------|------------------------|
//! | **Constraints** | BoxConstraints (width/height) | SliverConstraints (scroll state) |
//! | **Output** | Size (width × height) | SliverGeometry (scroll/paint extents) |
//! | **Use Case** | Static layouts | Scrollable content |
//! | **Examples** | Padding, Container, Row | SliverList, SliverGrid, SliverAppBar |
//!
//! # Usage Patterns
//!
//! ## Leaf Sliver (0 children)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderSliverToBoxAdapter {
//!     child_size: Size,
//! }
//!
//! impl RenderSliver for RenderSliverToBoxAdapter {
//!     fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
//!         let extent = match ctx.constraints.axis() {
//!             Axis::Vertical => self.child_size.height,
//!             Axis::Horizontal => self.child_size.width,
//!         };
//!
//!         SliverGeometry::simple(extent, extent.min(ctx.constraints.remaining_paint_extent))
//!     }
//!
//!     fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
//!         let mut canvas = Canvas::new();
//!         // Paint content...
//!         canvas
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Exact(0)  // No children
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//! ```
//!
//! ## Single Child Sliver
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderSliverPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl RenderSliver for RenderSliverPadding {
//!     fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
//!         let child_id = ctx.children.single();
//!
//!         // Adjust constraints for padding
//!         let child_constraints = ctx.constraints.copy_with_scroll_offset(
//!             (ctx.constraints.scroll_offset - self.padding.vertical_total()).max(0.0)
//!         );
//!
//!         // Layout child
//!         let child_geometry = ctx.layout_child(child_id, child_constraints);
//!
//!         // Add padding to geometry
//!         SliverGeometry {
//!             scroll_extent: child_geometry.scroll_extent + self.padding.vertical_total(),
//!             paint_extent: child_geometry.paint_extent + self.padding.vertical_total(),
//!             ..child_geometry
//!         }
//!     }
//!
//!     fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
//!         let child_id = ctx.children.single();
//!         let child_offset = ctx.offset + self.padding.top_left_offset();
//!         ctx.paint_child(child_id, child_offset)
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Exact(1)  // Exactly one child
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//! ```
//!
//! ## Multiple Children Sliver
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderSliverList {
//!     item_extent: f32,
//! }
//!
//! impl RenderSliver for RenderSliverList {
//!     fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
//!         let children = ctx.children.as_slice();
//!         let total_extent = children.len() as f32 * self.item_extent;
//!
//!         // Calculate visible range
//!         let first_visible = (ctx.constraints.scroll_offset / self.item_extent).floor() as usize;
//!         let visible_count = (ctx.constraints.remaining_paint_extent / self.item_extent).ceil() as usize;
//!
//!         // Layout only visible children
//!         for i in first_visible..(first_visible + visible_count).min(children.len()) {
//!             let child_id = children[i];
//!             // Layout child...
//!         }
//!
//!         SliverGeometry::simple(
//!             total_extent,
//!             total_extent.min(ctx.constraints.remaining_paint_extent),
//!         )
//!     }
//!
//!     fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
//!         let mut canvas = Canvas::new();
//!         // Paint visible children...
//!         canvas
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Variable  // Any number of children
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//! ```

use crate::render::{Arity, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::SliverGeometry;
use std::fmt::Debug;

/// RenderSliver trait for sliver-based render objects
///
/// The RenderSliver trait is FLUI's abstraction for scrollable layout and painting.
/// Unlike the `Render` trait which uses BoxConstraints and returns Size,
/// RenderSliver uses SliverConstraints and returns SliverGeometry.
///
/// # What is a Sliver?
///
/// Slivers are specialized render objects for scrollable content:
/// - **Viewport-aware**: Know their position relative to the viewport
/// - **Lazy loading**: Only layout/paint visible content
/// - **Efficient scrolling**: Support infinite lists without memory overhead
/// - **Variable extent**: Can grow/shrink based on scroll position
///
/// Similar to:
/// - **Flutter**: RenderSliver (base class for SliverList, SliverGrid, etc.)
/// - **SwiftUI**: LazyVStack/LazyHStack with viewport awareness
/// - **Web**: Virtual scrolling (react-window, react-virtualized)
///
/// # Three Sliver Patterns
///
/// FLUI supports three patterns based on child count:
///
/// | Pattern | Children | Arity | Example |
/// |---------|----------|-------|---------|
/// | **Leaf** | 0 | `Arity::Exact(0)` | SliverToBoxAdapter |
/// | **Single** | 1 | `Arity::Exact(1)` | SliverPadding, SliverOpacity |
/// | **Multi** | N | `Arity::Variable` | SliverList, SliverGrid |
///
/// All three patterns use the same `RenderSliver` trait - just differ in how they
/// access children via `SliverLayoutContext` and `SliverPaintContext`.
///
/// # Required Methods
///
/// 1. **`layout`**: Compute sliver geometry given constraints
///    - Input: `SliverLayoutContext` (contains sliver constraints and children)
///    - Output: `SliverGeometry` (scroll/paint/cache extents)
///    - Side effects: Updates children's geometry via `ctx.layout_child()`
///
/// 2. **`paint`**: Generate canvas for rendering
///    - Input: `SliverPaintContext` (contains offset and children)
///    - Output: `Canvas` (recorded drawing commands)
///    - Side effects: Paints children via `ctx.paint_child()`
///
/// 3. **`as_any`**: Enable downcasting for metadata access
///    - Required for type-safe metadata
///
/// 4. **`arity`**: Specify expected child count
///    - Default: `Arity::Variable` (any number of children)
///    - Override with `Arity::Exact(n)` for strict validation
///
/// # Optional Methods
///
/// - `debug_name`: Get debug name for diagnostics
///
/// # Thread Safety
///
/// All sliver renderers must be `Send + Sync + 'static`:
/// - **`Send`**: Can be moved between threads
/// - **`Sync`**: Can be accessed concurrently from multiple threads
/// - **`'static`**: No borrowed data (owns all state)
///
/// This enables parallel layout and concurrent rendering.
///
/// # Coordinate System
///
/// Slivers use a scroll-aware coordinate system:
/// - **scroll_offset**: Distance scrolled from the start
/// - **paint_extent**: Visible space that should be painted
/// - **cache_extent**: Additional space to cache for smooth scrolling
///
/// # Examples
///
/// See module-level documentation for detailed examples.
pub trait RenderSliver: Send + Sync + Debug + 'static {
    /// Compute sliver layout with context
    ///
    /// This method is called during the layout phase to compute the sliver geometry
    /// given the sliver constraints from the viewport.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Sliver layout context providing access to:
    ///   - `ctx.tree`: Element tree for child layout
    ///   - `ctx.children`: Children enum (None/Single/Multi)
    ///   - `ctx.constraints`: Sliver constraints from viewport
    ///
    /// # Returns
    ///
    /// The computed sliver geometry describing:
    /// - `scroll_extent`: Total scrollable extent
    /// - `paint_extent`: Currently visible extent
    /// - `layout_extent`: Extent that was laid out (including cached)
    /// - And other sliver-specific properties
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
    ///     // For leaf slivers: compute intrinsic extent
    ///     let extent = self.compute_extent();
    ///     SliverGeometry::simple(extent, extent.min(ctx.constraints.remaining_paint_extent))
    ///
    ///     // For single child: delegate and adjust
    ///     let child_id = ctx.children.single();
    ///     let child_geometry = ctx.layout_child(child_id, adjusted_constraints);
    ///     self.adjust_geometry(child_geometry)
    ///
    ///     // For multiple children: layout visible range
    ///     let visible_range = self.calculate_visible_range(&ctx.constraints);
    ///     for &child_id in &ctx.children.as_slice()[visible_range] {
    ///         let child_geometry = ctx.layout_child(child_id, constraints);
    ///         // Accumulate geometry...
    ///     }
    ///     total_geometry
    /// }
    /// ```
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry;

    /// Paint sliver with context
    ///
    /// This method is called during the paint phase to generate a Canvas
    /// with recorded drawing commands for this sliver and its visible children.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Sliver paint context providing access to:
    ///   - `ctx.tree`: Element tree for child painting
    ///   - `ctx.children`: Children enum (None/Single/Multi)
    ///   - `ctx.offset`: Paint offset in viewport's coordinate space
    ///
    /// # Returns
    ///
    /// A Canvas containing recorded drawing commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
    ///     // For leaf slivers: paint self
    ///     let mut canvas = Canvas::new();
    ///     canvas.translate(ctx.offset.dx, ctx.offset.dy);
    ///     canvas.draw_rect(rect, &paint);
    ///     canvas
    ///
    ///     // For single child: paint child with offset
    ///     let child_id = ctx.children.single();
    ///     ctx.paint_child(child_id, ctx.offset + padding_offset)
    ///
    ///     // For multiple children: paint visible range
    ///     let mut canvas = Canvas::new();
    ///     for (i, &child_id) in self.visible_children.iter().enumerate() {
    ///         let offset = ctx.offset + self.child_offsets[i];
    ///         let child_canvas = ctx.paint_child(child_id, offset);
    ///         canvas.draw_canvas(&child_canvas, offset);
    ///     }
    ///     canvas
    /// }
    /// ```
    fn paint(&self, ctx: &SliverPaintContext) -> Canvas;

    /// Get arity (expected child count)
    ///
    /// Returns the arity specification for this sliver renderer.
    /// Used for runtime validation during element mounting.
    ///
    /// # Default Implementation
    ///
    /// Returns `Arity::Variable` (allows any number of children).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Leaf sliver - no children
    /// fn arity(&self) -> Arity {
    ///     Arity::Exact(0)
    /// }
    ///
    /// // Single child sliver
    /// fn arity(&self) -> Arity {
    ///     Arity::Exact(1)
    /// }
    ///
    /// // Multi-child sliver (default)
    /// fn arity(&self) -> Arity {
    ///     Arity::Variable
    /// }
    /// ```
    fn arity(&self) -> Arity {
        Arity::Variable
    }

    /// Downcast to Any for metadata access
    ///
    /// Allows parent sliver renderers to downcast children to access metadata.
    /// This is used by layouts like SliverGrid to query child-specific metadata.
    ///
    /// # Implementation
    ///
    /// All implementations should simply return `self`:
    ///
    /// ```rust,ignore
    /// fn as_any(&self) -> &dyn std::any::Any {
    ///     self
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Parent accessing child metadata
    /// if let Some(sliver_item) = child_render.as_any().downcast_ref::<RenderSliverItem>() {
    ///     let extent = sliver_item.metadata.extent;
    ///     // Use extent...
    /// }
    /// ```
    fn as_any(&self) -> &dyn std::any::Any;

    /// Debug name for diagnostics
    ///
    /// Returns a human-readable name for this sliver render object.
    /// Used in debug output, error messages, and dev tools.
    ///
    /// # Default Implementation
    ///
    /// Returns the type name (e.g., "my_crate::RenderSliverPadding").
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn debug_name(&self) -> &'static str {
    ///     "RenderSliverPadding"
    /// }
    /// ```
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestLeafSliver;

    impl RenderSliver for TestLeafSliver {
        fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
            SliverGeometry::simple(100.0, 100.0_f32.min(ctx.constraints.remaining_paint_extent))
        }

        fn paint(&self, _ctx: &SliverPaintContext) -> Canvas {
            Canvas::new()
        }

        fn arity(&self) -> Arity {
            Arity::Exact(0)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct TestSingleSliver;

    impl RenderSliver for TestSingleSliver {
        fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
            let child_id = ctx.children.single();
            ctx.layout_child(child_id, ctx.constraints)
        }

        fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
            let child_id = ctx.children.single();
            ctx.paint_child(child_id, ctx.offset)
        }

        fn arity(&self) -> Arity {
            Arity::Exact(1)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct TestMultiSliver;

    impl RenderSliver for TestMultiSliver {
        fn layout(&mut self, _ctx: &SliverLayoutContext) -> SliverGeometry {
            SliverGeometry::simple(500.0, 500.0)
        }

        fn paint(&self, _ctx: &SliverPaintContext) -> Canvas {
            Canvas::new()
        }

        fn arity(&self) -> Arity {
            Arity::Variable
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_leaf_sliver_arity() {
        let sliver = TestLeafSliver;
        assert_eq!(sliver.arity(), Arity::Exact(0));
    }

    #[test]
    fn test_single_sliver_arity() {
        let sliver = TestSingleSliver;
        assert_eq!(sliver.arity(), Arity::Exact(1));
    }

    #[test]
    fn test_multi_sliver_arity() {
        let sliver = TestMultiSliver;
        assert_eq!(sliver.arity(), Arity::Variable);
    }

    #[test]
    fn test_debug_name() {
        let sliver = TestLeafSliver;
        let name = sliver.debug_name();
        assert!(name.contains("TestLeafSliver"));
    }
}
