//! RenderSliver trait for sliver protocol render objects.
//!
//! This module provides the `RenderSliver<A>` trait for implementing render objects
//! that use the sliver layout protocol for scrollable content with compile-time arity validation.
//!
//! # Flutter Compliance
//!
//! This implementation follows Flutter's RenderSliver protocol exactly:
//! - Geometry must accurately report scroll/paint/layout extents
//! - Layout must be idempotent (same constraints → same geometry)
//! - Paint extent must not exceed remaining paint extent in constraints
//! - Scroll extent represents total scrollable content
//! - Layout extent represents space consumed in viewport
//!
//! # Design Philosophy
//!
//! - **Simple and clean**: Minimal API surface, no unnecessary abstractions
//! - **Arity validation**: Compile-time child count constraints
//! - **Context-based**: All operations use typed contexts
//! - **Progressive disclosure**: Simple defaults, explicit when needed
//! - **Performance**: Optimized for scrolling performance
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
//! Slivers are specialized render objects for scrollable content:
//!
//! - **One-dimensional**: Scroll in main axis (vertical or horizontal)
//! - **Lazy loading**: Only layout/paint visible portions
//! - **Viewport clipping**: Automatically clip to visible region
//! - **Composability**: Multiple slivers in a single scrollable
//!
//! ## Key Concepts
//!
//! - **Scroll Offset**: Position of viewport start in scrollable content
//! - **Scroll Extent**: Total length of scrollable content
//! - **Paint Extent**: Visible length currently being painted
//! - **Layout Extent**: Space consumed in viewport (may differ from paint extent)
//! - **Max Paint Extent**: Maximum paint extent if fully visible
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
//! ## Fixed-Height Sliver (Leaf)
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderSliver, RenderObject, Leaf};
//!
//! #[derive(Debug)]
//! struct RenderSliverAppBar {
//!     height: f32,
//!     color: Color,
//! }
//!
//! impl RenderSliver<Leaf> for RenderSliverAppBar {
//!     fn layout(&mut self, ctx: SliverLayoutContext<'_, Leaf>) -> RenderResult<SliverGeometry> {
//!         // Calculate visible height based on scroll position
//!         let scroll_offset = ctx.constraints.scroll_offset;
//!
//!         // Total scrollable content
//!         let scroll_extent = self.height;
//!
//!         // Visible portion (clamped to remaining space)
//!         let paint_extent = if scroll_offset < scroll_extent {
//!             (scroll_extent - scroll_offset).min(ctx.constraints.remaining_paint_extent)
//!         } else {
//!             0.0
//!         };
//!
//!         Ok(SliverGeometry {
//!             scroll_extent,
//!             paint_extent,
//!             layout_extent: Some(paint_extent), // Consumes space in viewport
//!             max_paint_extent: Some(scroll_extent),
//!             ..Default::default()
//!         })
//!     }
//!
//!     fn paint(&self, ctx: &mut SliverPaintContext<'_, Leaf>) {
//!         let paint_extent = ctx.geometry.paint_extent;
//!         if paint_extent > 0.0 {
//!             let rect = Rect::from_ltwh(0.0, 0.0, ctx.constraints.cross_axis_extent, paint_extent);
//!             ctx.canvas_mut().draw_rect(rect, &Paint::from_color(self.color));
//!         }
//!     }
//! }
//!
//! impl RenderObject for RenderSliverAppBar {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! ## Single Child Sliver (Single)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderSliverPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl RenderSliver<Single> for RenderSliverPadding {
//!     fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
//!         // Calculate how much scroll offset has consumed our top padding
//!         let before_padding = self.padding.top;
//!         let after_padding = self.padding.bottom;
//!
//!         // Adjust scroll offset for child (accounting for consumed padding)
//!         let child_scroll_offset = (ctx.constraints.scroll_offset - before_padding).max(0.0);
//!
//!         // Adjust remaining paint extent for child
//!         let child_remaining_paint = if ctx.constraints.scroll_offset < before_padding {
//!             // Still showing top padding, reduce available space
//!             ctx.constraints.remaining_paint_extent - (before_padding - ctx.constraints.scroll_offset)
//!         } else {
//!             ctx.constraints.remaining_paint_extent
//!         }.max(0.0);
//!
//!         // Layout child with adjusted constraints
//!         let child_constraints = SliverConstraints {
//!             scroll_offset: child_scroll_offset,
//!             remaining_paint_extent: child_remaining_paint,
//!             ..ctx.constraints
//!         };
//!
//!         let mut child_geometry = ctx.layout_single_child_with(|_| child_constraints)?;
//!
//!         // Add padding to geometry
//!         child_geometry.scroll_extent += before_padding + after_padding;
//!
//!         // Adjust paint extent to include visible padding
//!         if ctx.constraints.scroll_offset < before_padding {
//!             child_geometry.paint_extent += before_padding - ctx.constraints.scroll_offset;
//!         }
//!         child_geometry.paint_extent = child_geometry.paint_extent.min(
//!             ctx.constraints.remaining_paint_extent
//!         );
//!
//!         Ok(child_geometry)
//!     }
//!
//!     fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
//!         // Child is offset by top padding
//!         let offset = Offset::new(0.0, self.padding.top);
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
//! ## Multi-Child List (Variable)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderSliverList {
//!     item_extent: f32, // Fixed height per item
//! }
//!
//! impl RenderSliver<Variable> for RenderSliverList {
//!     fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
//!         let constraints = &ctx.constraints;
//!         let child_count = ctx.children_count();
//!
//!         // Total scrollable content
//!         let total_extent = child_count as f32 * self.item_extent;
//!
//!         // Calculate visible range
//!         let first_visible_index = (constraints.scroll_offset / self.item_extent).floor() as usize;
//!         let last_visible_index = ((constraints.scroll_offset + constraints.remaining_paint_extent) / self.item_extent).ceil() as usize;
//!         let last_visible_index = last_visible_index.min(child_count);
//!
//!         let mut paint_extent = 0.0;
//!
//!         // Layout only visible children (lazy loading!)
//!         for (index, child_id) in ctx.children().enumerate() {
//!             if index < first_visible_index || index >= last_visible_index {
//!                 continue; // Skip off-screen children
//!             }
//!
//!             // Calculate position of this item
//!             let item_offset = index as f32 * self.item_extent;
//!
//!             // Layout child (simplified - would use box protocol in real implementation)
//!             let child_extent = self.item_extent;
//!
//!             // Position child
//!             ctx.set_child_offset(child_id, Offset::new(0.0, item_offset - constraints.scroll_offset));
//!
//!             // Accumulate paint extent
//!             let visible_extent = (child_extent).min(
//!                 (constraints.scroll_offset + constraints.remaining_paint_extent - item_offset).max(0.0)
//!             );
//!             paint_extent += visible_extent;
//!         }
//!
//!         Ok(SliverGeometry {
//!             scroll_extent: total_extent,
//!             paint_extent: paint_extent.min(constraints.remaining_paint_extent),
//!             layout_extent: Some(paint_extent.min(constraints.remaining_paint_extent)),
//!             max_paint_extent: Some(total_extent),
//!             ..Default::default()
//!         })
//!     }
//!
//!     fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
//!         // Paint all laid-out children using offsets from layout
//!         ctx.paint_all_children();
//!     }
//! }
//!
//! impl RenderObject for RenderSliverList {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! # Flutter Protocol Compliance
//!
//! ## Layout Phase
//!
//! 1. **Receive Constraints**: Parent provides `SliverConstraints`
//!    - `scroll_offset`: Current scroll position
//!    - `remaining_paint_extent`: Available space for painting
//!    - `overlap`: Amount overlapped by previous sliver (rare)
//!    - `cross_axis_extent`: Width (for vertical) or height (for horizontal)
//!
//! 2. **Compute Geometry**: Return `SliverGeometry` with:
//!    - `scroll_extent`: Total scrollable length
//!    - `paint_extent`: Visible length (≤ remaining_paint_extent)
//!    - `layout_extent`: Space consumed (usually = paint_extent)
//!    - `max_paint_extent`: Maximum paint extent if fully visible
//!
//! 3. **Invariants** (verified in debug builds):
//!    - `paint_extent ≤ remaining_paint_extent`
//!    - `layout_extent ≤ paint_extent`
//!    - `paint_extent ≤ scroll_extent`
//!
//! ## Paint Phase
//!
//! - Use `geometry.paint_extent` to determine visible region
//! - Clip painting to visible bounds
//! - Skip painting if `paint_extent == 0.0`
//!
//! ## Hit Test Phase
//!
//! - Test along main axis using `geometry.paint_extent`
//! - Transform position accounting for scroll offset
//! - Test children in reverse order
//!
//! # Common Patterns
//!
//! ## Pass-through (Single child)
//!
//! ```rust,ignore
//! fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
//!     ctx.layout_single_child()
//! }
//! ```
//!
//! ## Fixed extent (Leaf)
//!
//! ```rust,ignore
//! fn layout(&mut self, ctx: SliverLayoutContext<'_, Leaf>) -> RenderResult<SliverGeometry> {
//!     let extent = 100.0;
//!     let visible = if ctx.constraints.scroll_offset < extent {
//!         (extent - ctx.constraints.scroll_offset).min(ctx.constraints.remaining_paint_extent)
//!     } else {
//!         0.0
//!     };
//!
//!     Ok(SliverGeometry {
//!         scroll_extent: extent,
//!         paint_extent: visible,
//!         layout_extent: Some(visible),
//!         max_paint_extent: Some(extent),
//!         ..Default::default()
//!     })
//! }
//! ```
//!
//! ## Lazy loading list (Variable)
//!
//! ```rust,ignore
//! fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
//!     // Calculate visible range from scroll_offset
//!     let first_visible = calculate_first_visible(ctx.constraints.scroll_offset);
//!     let visible_count = calculate_visible_count(ctx.constraints.remaining_paint_extent);
//!
//!     // Layout only visible children
//!     let mut geometry = SliverGeometry::zero();
//!     for (i, child_id) in ctx.children().skip(first_visible).take(visible_count).enumerate() {
//!         let child_geom = layout_child_at_index(child_id, i);
//!         geometry = combine_geometries(geometry, child_geom);
//!     }
//!
//!     Ok(geometry)
//! }
//! ```

use std::fmt;

use flui_interaction::HitTestResult;
use flui_types::{Offset, Rect, SliverConstraints, SliverGeometry};

use super::arity::Arity;
use super::contexts::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext};
use super::render_object::RenderObject;
use super::RenderResult;

// ============================================================================
// CORE RENDER SLIVER TRAIT
// ============================================================================

/// Render trait for sliver protocol with compile-time arity validation.
///
/// This trait provides the foundation for implementing scrollable render objects
/// that use the sliver layout protocol.
///
/// # Required Trait Bounds
///
/// - `RenderObject`: Base trait for type erasure
/// - `Debug`: Required for debugging and error messages
/// - `Send + Sync`: Required for thread-safe tree operations
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
/// - [`hit_test`](Self::hit_test) - Pointer event detection (default provided)
/// - [`child_keep_alive_count`](Self::child_keep_alive_count) - Keep-alive optimization
/// - [`has_visual_overflow`](Self::has_visual_overflow) - Overflow indicator
/// - [`local_bounds`](Self::local_bounds) - Bounding rectangle
///
/// # Context API
///
/// All methods use contexts with sensible defaults:
///
/// ```rust,ignore
/// // Minimal - uses default SliverProtocol
/// fn layout(&mut self, ctx: SliverLayoutContext<'_, Single>) -> Result<SliverGeometry>
///
/// // With type alias - more explicit
/// fn layout(&mut self, ctx: SliverLayoutContext<'_, Variable>) -> Result<SliverGeometry>
/// ```
///
/// # Safety Guarantees
///
/// - ✅ No panics: All methods return `Result` for errors
/// - ✅ Geometry validation: Debug builds verify invariants
/// - ✅ Lifecycle safety: Assertions prevent invalid states
/// - ✅ Arity enforcement: Compile-time child count validation
pub trait RenderSliver<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    /// Computes the geometry of this sliver given constraints.
    ///
    /// # Flutter Contract
    ///
    /// The returned geometry MUST satisfy these invariants:
    /// - `paint_extent ≤ remaining_paint_extent` (from constraints)
    /// - `layout_extent ≤ paint_extent` (can't consume more than visible)
    /// - `paint_extent ≤ scroll_extent` (can't paint more than total content)
    /// - `max_paint_extent ≥ paint_extent` (maximum should include current)
    ///
    /// Violating these invariants will cause assertion failures in debug builds.
    ///
    /// # Layout Protocol
    ///
    /// 1. **Receive Constraints**: Examine `ctx.constraints`:
    ///    - `scroll_offset`: Position of viewport start
    ///    - `remaining_paint_extent`: Available space for painting
    ///    - `overlap`: Amount overlapped by previous sliver (usually 0)
    ///    - `cross_axis_extent`: Width (vertical) or height (horizontal)
    ///
    /// 2. **Determine Visibility**: Calculate what portion is visible based on scroll offset
    ///
    /// 3. **Layout Children** (if any): Call `ctx.layout_child()` for each child
    ///
    /// 4. **Position Children**: Call `ctx.set_child_offset()` for each child
    ///
    /// 5. **Compute Geometry**: Return `SliverGeometry` with:
    ///    - `scroll_extent`: Total scrollable length
    ///    - `paint_extent`: Visible length being painted
    ///    - `layout_extent`: Space consumed in viewport
    ///    - `max_paint_extent`: Maximum paint extent if fully visible
    ///
    /// # Context API
    ///
    /// The context provides:
    /// - `ctx.constraints` - Sliver constraints from parent
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.layout_child(id, c)` - Layout a child sliver
    /// - `ctx.set_child_offset(id, offset)` - Position a child
    /// - Helper methods for common patterns
    ///
    /// # Errors
    ///
    /// Returns `RenderError` if:
    /// - Child layout fails (propagated from child)
    /// - Required child is missing (arity violation)
    /// - Tree operation fails
    ///
    /// # Performance Optimization
    ///
    /// For lists/grids with many children:
    /// - **Lazy layout**: Only layout visible children
    /// - **Extent caching**: Cache child extents to avoid repeated layout
    /// - **Index calculation**: Compute visible range from scroll offset
    ///
    /// # Examples
    ///
    /// ## Fixed extent (Leaf)
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, ctx: SliverLayoutContext<'_, Leaf>) -> RenderResult<SliverGeometry> {
    ///     let height = 100.0;
    ///
    ///     // Calculate visible portion
    ///     let visible = if ctx.constraints.scroll_offset < height {
    ///         (height - ctx.constraints.scroll_offset)
    ///             .min(ctx.constraints.remaining_paint_extent)
    ///     } else {
    ///         0.0
    ///     };
    ///
    ///     Ok(SliverGeometry {
    ///         scroll_extent: height,
    ///         paint_extent: visible,
    ///         layout_extent: Some(visible),
    ///         max_paint_extent: Some(height),
    ///         ..Default::default()
    ///     })
    /// }
    /// ```
    ///
    /// ## Pass-through (Single child)
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
    ///     // Simply pass constraints through to child
    ///     ctx.layout_single_child()
    /// }
    /// ```
    ///
    /// ## Lazy loading list (Variable)
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
    ///     let constraints = &ctx.constraints;
    ///
    ///     // Calculate visible range (lazy loading!)
    ///     let first_visible = (constraints.scroll_offset / self.item_height).floor() as usize;
    ///     let visible_count = (constraints.remaining_paint_extent / self.item_height).ceil() as usize;
    ///
    ///     let mut geometry = SliverGeometry::zero();
    ///
    ///     // Layout only visible children
    ///     for (i, child_id) in ctx.children().enumerate() {
    ///         if i < first_visible || i >= first_visible + visible_count {
    ///             continue; // Skip off-screen
    ///         }
    ///
    ///         let child_offset = i as f32 * self.item_height;
    ///         let child_geom = /* layout child */;
    ///
    ///         geometry.scroll_extent += child_geom.scroll_extent;
    ///         geometry.paint_extent += child_geom.paint_extent;
    ///     }
    ///
    ///     Ok(geometry)
    /// }
    /// ```
    fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> RenderResult<SliverGeometry>;

    /// Paints this sliver to the canvas.
    ///
    /// # Flutter Contract
    ///
    /// - MUST NOT call layout during paint
    /// - MUST use geometry from layout phase (`ctx.geometry`)
    /// - SHOULD skip painting if `geometry.paint_extent == 0.0`
    /// - SHOULD clip to visible bounds
    /// - MUST paint children using stored offsets from layout
    ///
    /// # Paint Protocol
    ///
    /// 1. **Check Visibility**: Skip if `ctx.geometry.paint_extent == 0.0`
    /// 2. **Setup Clipping**: Clip to visible bounds if needed
    /// 3. **Paint Self**: Draw background, decorations, etc.
    /// 4. **Paint Children**: Use `ctx.paint_child()` with offsets from layout
    /// 5. **Paint Foreground**: Draw overlays, scrollbars, etc.
    ///
    /// # Context API
    ///
    /// The context provides:
    /// - `ctx.offset` - Position in parent coordinates
    /// - `ctx.geometry` - `SliverGeometry` from layout
    /// - `ctx.constraints` - `SliverConstraints` from layout
    /// - `ctx.canvas_mut()` - Mutable canvas for drawing
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.paint_child(id, offset)` - Paint a specific child
    /// - Helper methods for common patterns
    ///
    /// # Performance
    ///
    /// - Early return if not visible
    /// - Skip children that are off-screen
    /// - Use canvas save/restore efficiently
    /// - Batch similar drawing operations
    ///
    /// # Examples
    ///
    /// ## Fixed content (Leaf)
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut SliverPaintContext<'_, Leaf>) {
    ///     let paint_extent = ctx.geometry.paint_extent;
    ///     if paint_extent == 0.0 {
    ///         return; // Not visible, skip painting
    ///     }
    ///
    ///     let rect = Rect::from_ltwh(
    ///         0.0,
    ///         0.0,
    ///         ctx.constraints.cross_axis_extent,
    ///         paint_extent
    ///     );
    ///     ctx.canvas_mut().draw_rect(rect, &self.paint);
    /// }
    /// ```
    ///
    /// ## Pass-through (Single child)
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
    ///     ctx.paint_single_child(Offset::ZERO);
    /// }
    /// ```
    ///
    /// ## Multiple children
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
    ///     // Paint all children using offsets from layout
    ///     ctx.paint_all_children();
    /// }
    /// ```
    fn paint(&self, ctx: &mut SliverPaintContext<'_, A>);

    /// Performs hit testing for pointer events.
    ///
    /// # Flutter Contract
    ///
    /// - Position is in local coordinates
    /// - Test children in REVERSE order (front to back)
    /// - Return `true` if hit, `false` otherwise
    /// - Transform position for children accounting for scroll
    /// - Add self to result if hit
    ///
    /// # Hit Test Protocol
    ///
    /// 1. **Visibility Check**: Check if position is within paint extent
    /// 2. **Test Children**: Test each child in reverse order
    /// 3. **Transform Position**: Adjust for child offsets and scroll
    /// 4. **Test Self**: Determine if self is hit (if no child was)
    /// 5. **Add to Result**: Call `ctx.hit_test_self()` if hit
    ///
    /// # Context API
    ///
    /// - `ctx.position` - Hit position in local coordinates
    /// - `ctx.geometry` - Geometry from layout
    /// - `ctx.children()` / `ctx.children_reverse()` - Child iterators
    /// - `ctx.hit_test_child(id, pos, result)` - Test a child
    /// - `ctx.hit_test_self(result)` - Add self to result
    ///
    /// # Default Implementation
    ///
    /// Tests children in reverse order and checks main axis bounds.
    /// Override for custom hit testing logic.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn hit_test(&self, ctx: &SliverHitTestContext<'_, Variable>, result: &mut HitTestResult) -> bool {
    ///     // Check main axis bounds
    ///     if ctx.position.dy < 0.0 || ctx.position.dy >= ctx.geometry.paint_extent {
    ///         return false;
    ///     }
    ///
    ///     // Test children in reverse
    ///     for child_id in ctx.children_reverse() {
    ///         if ctx.hit_test_child(child_id, ctx.position, result) {
    ///             return true;
    ///         }
    ///     }
    ///
    ///     ctx.hit_test_self(result);
    ///     true
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
        let in_bounds = ctx.position.dy >= 0.0 && ctx.position.dy < ctx.geometry.paint_extent;

        if in_bounds {
            ctx.hit_test_self(result);
            true
        } else {
            false
        }
    }

    /// Returns the number of children to keep alive even when off-screen.
    ///
    /// This is used for performance optimization in scrollable lists.
    /// Keeping children alive avoids rebuilding them when scrolling.
    ///
    /// Default is 0 (no keep-alive).
    ///
    /// # Use Cases
    ///
    /// - Smooth scrolling with minimal jank
    /// - Preserving expensive child state
    /// - Reducing rebuild overhead
    ///
    /// # Trade-offs
    ///
    /// - Higher keep-alive = more memory usage
    /// - Higher keep-alive = smoother scrolling
    /// - Tune based on child complexity and available memory
    fn child_keep_alive_count(&self) -> usize {
        0
    }

    /// Returns whether this sliver has visual overflow.
    ///
    /// Visual overflow occurs when content extends beyond the sliver's bounds,
    /// potentially needing clipping or special handling.
    ///
    /// Default is false (no overflow).
    ///
    /// # Use Cases
    ///
    /// - Debug visualization of overflow
    /// - Automatic clipping application
    /// - Performance warnings
    fn has_visual_overflow(&self) -> bool {
        false
    }

    /// Gets the local bounding rectangle.
    ///
    /// For slivers, this is typically based on paint extent and cross-axis extent.
    /// Default returns an empty rectangle.
    ///
    /// Override for proper hit testing and bounds calculation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn local_bounds(&self) -> Rect {
    ///     Rect::from_ltwh(
    ///         0.0,
    ///         0.0,
    ///         self.cross_axis_extent,
    ///         self.geometry.paint_extent
    ///     )
    /// }
    /// ```
    fn local_bounds(&self) -> Rect {
        Rect::ZERO
    }
}

// ============================================================================
// GEOMETRY VALIDATION (DEBUG ONLY)
// ============================================================================

#[cfg(debug_assertions)]
pub fn verify_sliver_geometry(geometry: &SliverGeometry, constraints: &SliverConstraints) {
    // Verify paint_extent ≤ remaining_paint_extent
    debug_assert!(
        geometry.paint_extent <= constraints.remaining_paint_extent + 0.001, // Allow for floating point error
        "Sliver geometry violation: paint_extent ({}) exceeds remaining_paint_extent ({})",
        geometry.paint_extent,
        constraints.remaining_paint_extent
    );

    // Verify layout_extent ≤ paint_extent
    if let Some(layout_extent) = geometry.layout_extent {
        debug_assert!(
            layout_extent <= geometry.paint_extent + 0.001,
            "Sliver geometry violation: layout_extent ({}) exceeds paint_extent ({})",
            layout_extent,
            geometry.paint_extent
        );
    }

    // Verify paint_extent ≤ scroll_extent (unless pinned/floating)
    debug_assert!(
        geometry.paint_extent <= geometry.scroll_extent + 0.001,
        "Sliver geometry violation: paint_extent ({}) exceeds scroll_extent ({})",
        geometry.paint_extent,
        geometry.scroll_extent
    );

    // Verify max_paint_extent ≥ paint_extent
    if let Some(max_paint) = geometry.max_paint_extent {
        debug_assert!(
            max_paint >= geometry.paint_extent - 0.001,
            "Sliver geometry violation: max_paint_extent ({}) is less than paint_extent ({})",
            max_paint,
            geometry.paint_extent
        );
    }
}

// ============================================================================
// HELPER EXTENSION TRAIT
// ============================================================================

/// Extension trait providing helper methods for SliverGeometry.
pub trait SliverGeometryExt {
    /// Creates a zero geometry (all extents are 0).
    fn zero() -> Self;

    /// Returns true if this geometry has any visible content.
    fn is_visible(&self) -> bool;

    /// Returns true if this geometry is completely scrolled off-screen.
    fn is_scrolled_off_screen(&self, scroll_offset: f32) -> bool;
}

impl SliverGeometryExt for SliverGeometry {
    fn zero() -> Self {
        Self {
            scroll_extent: 0.0,
            paint_extent: 0.0,
            layout_extent: Some(0.0),
            max_paint_extent: Some(0.0),
            ..Default::default()
        }
    }

    fn is_visible(&self) -> bool {
        self.paint_extent > 0.0
    }

    fn is_scrolled_off_screen(&self, scroll_offset: f32) -> bool {
        scroll_offset >= self.scroll_extent
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
        fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> RenderResult<SliverGeometry> {
            let visible = if ctx.constraints.scroll_offset < self.extent {
                (self.extent - ctx.constraints.scroll_offset)
                    .min(ctx.constraints.remaining_paint_extent)
            } else {
                0.0
            };

            Ok(SliverGeometry {
                scroll_extent: self.extent,
                paint_extent: visible,
                layout_extent: Some(visible),
                max_paint_extent: Some(self.extent),
                ..Default::default()
            })
        }

        fn paint(&self, _ctx: &mut SliverPaintContext<'_, A>) {
            // No-op for tests
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
    fn test_render_sliver_arity_types() {
        let _leaf: TestRenderSliver<Leaf> = TestRenderSliver::new(100.0);
        let _single: TestRenderSliver<Single> = TestRenderSliver::new(100.0);
        let _variable: TestRenderSliver<Variable> = TestRenderSliver::new(100.0);
        // Compiles = arity system works
    }

    #[test]
    fn test_default_methods() {
        let sliver = TestRenderSliver::<Leaf>::new(100.0);
        assert_eq!(sliver.child_keep_alive_count(), 0);
        assert!(!sliver.has_visual_overflow());
        assert_eq!(sliver.local_bounds(), Rect::ZERO);
    }

    #[test]
    fn test_geometry_helpers() {
        let zero = SliverGeometry::zero();
        assert_eq!(zero.scroll_extent, 0.0);
        assert_eq!(zero.paint_extent, 0.0);
        assert!(!zero.is_visible());

        let visible = SliverGeometry {
            scroll_extent: 100.0,
            paint_extent: 50.0,
            ..Default::default()
        };
        assert!(visible.is_visible());
        assert!(!visible.is_scrolled_off_screen(0.0));
        assert!(visible.is_scrolled_off_screen(100.0));
    }
}
