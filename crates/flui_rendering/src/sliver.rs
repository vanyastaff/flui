//! RenderSliver trait for sliver protocol render objects.
//!
//! This module provides the `RenderSliver<A>` trait for implementing scrollable
//! render objects with compile-time arity validation.
//!
//! # Flutter Protocol Compliance
//!
//! Follows Flutter's RenderSliver protocol exactly:
//! - **Geometry accuracy**: Must report scroll/paint/layout extents correctly
//! - **Idempotency**: Same constraints → same geometry
//! - **Paint extent limits**: paint_extent ≤ remaining_paint_extent
//! - **Scroll extent**: Represents total scrollable content
//! - **Layout extent**: Space consumed in viewport
//!
//! # Sliver Protocol
//!
//! Slivers are specialized render objects for scrollable content:
//! - **One-dimensional**: Scroll in main axis (vertical or horizontal)
//! - **Lazy loading**: Only layout/paint visible portions
//! - **Viewport clipping**: Automatically clip to visible region
//! - **Composability**: Multiple slivers in a single scrollable

use std::fmt;

use flui_foundation::{DiagnosticsProperty, ElementId};
use flui_interaction::HitTestResult;
use flui_types::{Rect, SliverConstraints, SliverGeometry};

use crate::arity::Arity;
use crate::hit_test_context::SliverHitTestContext;
use crate::layout_context::SliverLayoutContext;
use crate::object::RenderObject;
use crate::paint_context::SliverPaintContext;
use crate::RenderResult;

// ============================================================================
// CORE RENDER SLIVER TRAIT
// ============================================================================

/// Render trait for sliver protocol with compile-time arity validation.
///
/// # Required Trait Bounds
///
/// - `RenderObject`: Base trait for type erasure
/// - `Debug`: Required for debugging
/// - `Send + Sync`: Required for thread safety
///
/// # Type Parameters
///
/// - `A`: Arity type constraining number of children
///
/// # Required Methods
///
/// - [`layout`](Self::layout) - Computes geometry given constraints
/// - [`paint`](Self::paint) - Draws to canvas
///
/// # Optional Methods (with defaults)
///
/// - [`hit_test`](Self::hit_test) - Pointer event detection
/// - [`child_scroll_offset`](Self::child_scroll_offset) - Child positioning
/// - [`child_main_axis_position`](Self::child_main_axis_position) - Visible edge distance
/// - [`child_cross_axis_position`](Self::child_cross_axis_position) - Cross-axis offset
/// - [`calculate_paint_offset`](Self::calculate_paint_offset) - Visible region calculation
/// - [`calculate_cache_offset`](Self::calculate_cache_offset) - Cache region calculation
/// - [`center_offset_adjustment`](Self::center_offset_adjustment) - Center sliver adjustment
/// - [`has_visual_overflow`](Self::has_visual_overflow) - Overflow indicator
/// - [`local_bounds`](Self::local_bounds) - Bounding rectangle
pub trait RenderSliver<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    // ============================================================================
    // REQUIRED METHODS
    // ============================================================================

    /// Computes the geometry of this sliver given constraints.
    ///
    /// # Flutter Contract
    ///
    /// The returned geometry MUST satisfy these invariants:
    /// - `paint_extent ≤ constraints.remaining_paint_extent`
    /// - `layout_extent ≤ paint_extent`
    /// - `paint_extent ≤ scroll_extent` (unless pinned/floating)
    /// - `max_paint_extent ≥ paint_extent`
    ///
    /// Violations cause assertion failures in debug builds.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Layout context with constraints and tree access
    ///
    /// # Returns
    ///
    /// SliverGeometry describing this sliver's dimensions.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, ctx: SliverLayoutContext<'_, Leaf>) -> RenderResult<SliverGeometry> {
    ///     let extent = 100.0;
    ///     let scroll_offset = ctx.constraints.scroll_offset;
    ///
    ///     let visible = if scroll_offset < extent {
    ///         (extent - scroll_offset).min(ctx.constraints.remaining_paint_extent)
    ///     } else {
    ///         0.0
    ///     };
    ///
    ///     Ok(SliverGeometry {
    ///         scroll_extent: extent,
    ///         paint_extent: visible,
    ///         layout_extent: Some(visible),
    ///         max_paint_extent: Some(extent),
    ///         ..Default::default()
    ///     })
    /// }
    /// ```
    fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> RenderResult<SliverGeometry>;

    /// Paints this sliver to canvas.
    ///
    /// # Flutter Contract
    ///
    /// - Use `geometry.paint_extent` to determine visible region
    /// - Clip painting to visible bounds
    /// - Skip if `paint_extent == 0.0`
    ///
    /// # Arguments
    ///
    /// * `ctx` - Paint context with canvas and geometry
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
    ///     if ctx.geometry.paint_extent > 0.0 {
    ///         ctx.paint_single_child(Offset::ZERO);
    ///     }
    /// }
    /// ```
    fn paint(&self, ctx: &mut SliverPaintContext<'_, A>);

    // ============================================================================
    // HIT TESTING (Optional)
    // ============================================================================

    /// Hit tests at position (optional).
    ///
    /// Default: tests along main axis using paint_extent.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Hit test context
    /// * `result` - Accumulator for hit entries
    ///
    /// # Returns
    ///
    /// `true` if hit, `false` otherwise.
    fn hit_test(&self, _ctx: &SliverHitTestContext<'_, A>, _result: &mut HitTestResult) -> bool {
        false // Default: no hit testing
    }

    // ============================================================================
    // CHILD POSITIONING (Flutter Methods)
    // ============================================================================

    /// Distance from parent's zero scroll offset to child's zero scroll offset.
    ///
    /// This differs from `child_main_axis_position` - `childScrollOffset` gives
    /// distance from the sliver's zero scroll offset (unaffected by scrolling),
    /// whereas `child_main_axis_position` gives distance from visible leading edge.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// double childScrollOffset(RenderObject child) {
    ///   return _childOffsets[indexOf(child)];
    /// }
    /// ```
    ///
    /// # When to override
    ///
    /// Override if children are positioned anywhere other than scroll offset zero.
    /// For example:
    /// - SliverList: each child has cumulative offset
    /// - SliverGrid: children positioned in 2D grid
    /// - SliverPadding: child offset by padding amount
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // SliverList - children stacked vertically
    /// fn child_scroll_offset(&self, child_id: ElementId) -> Option<f32> {
    ///     let index = self.index_of(child_id)?;
    ///     Some(self.cumulative_heights[index])
    /// }
    ///
    /// // SliverPadding - child offset by padding
    /// fn child_scroll_offset(&self, _child_id: ElementId) -> Option<f32> {
    ///     Some(self.padding.top)
    /// }
    /// ```
    fn child_scroll_offset(&self, _child_id: ElementId) -> Option<f32> {
        Some(0.0) // Default: child aligned with parent's zero offset
    }

    /// Distance from parent's visible leading edge to child's visible leading edge.
    ///
    /// If the actual leading edge is not visible, uses the edge defined by
    /// intersection of sliver with viewport's leading edge.
    ///
    /// # Difference from child_scroll_offset
    ///
    /// - `child_scroll_offset`: Absolute position (unaffected by scroll)
    /// - `child_main_axis_position`: Relative to visible edge (changes with scroll)
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// double childMainAxisPosition(RenderObject child) {
    ///   return childScrollOffset(child)! - constraints.scrollOffset;
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn child_main_axis_position(&self, child_id: ElementId) -> Option<f32> {
    ///     let scroll_offset = self.child_scroll_offset(child_id)?;
    ///     Some(scroll_offset - self.current_scroll_offset)
    /// }
    /// ```
    fn child_main_axis_position(&self, _child_id: ElementId) -> Option<f32> {
        Some(0.0) // Default: child at visible leading edge
    }

    /// Distance along cross axis from parent's edge to child's edge.
    ///
    /// For vertical scrolling, this is the horizontal offset (left side).
    /// For horizontal scrolling, this is the vertical offset (top side).
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// @override
    /// double childCrossAxisPosition(RenderObject child) {
    ///   return 0.0;  // Usually centered or aligned to edge
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // SliverGrid - children positioned in grid
    /// fn child_cross_axis_position(&self, child_id: ElementId) -> Option<f32> {
    ///     let index = self.index_of(child_id)?;
    ///     let column = index % self.cross_axis_count;
    ///     Some(column as f32 * self.cell_width)
    /// }
    /// ```
    fn child_cross_axis_position(&self, _child_id: ElementId) -> Option<f32> {
        Some(0.0) // Default: aligned to parent's cross-axis edge
    }

    // ============================================================================
    // VIEWPORT CALCULATIONS (Flutter Helpers)
    // ============================================================================

    /// Computes the visible portion of region from `from` to `to`.
    ///
    /// Returns the extent that is within the viewport's paint region, assuming:
    /// - Only region from `scroll_offset` with `remaining_paint_extent` is visible
    /// - Linear relationship between scroll offsets and paint offsets
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// double calculatePaintOffset(
    ///   SliverConstraints constraints, {
    ///   required double from,
    ///   required double to,
    /// }) {
    ///   final double targetLastScrollOffset = to;
    ///   final double targetFirstScrollOffset = from;
    ///   final double clampedPaintExtent = (targetLastScrollOffset -
    ///       math.max(constraints.scrollOffset, targetFirstScrollOffset))
    ///       .clamp(0.0, constraints.remainingPaintExtent);
    ///   return clampedPaintExtent;
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `constraints` - Current sliver constraints
    /// * `from` - Start of region (scroll offset)
    /// * `to` - End of region (scroll offset)
    ///
    /// # Returns
    ///
    /// Visible extent within viewport (0.0 if not visible).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let visible = self.calculate_paint_offset(&ctx.constraints, 100.0, 200.0);
    /// // If scroll_offset = 150 and remaining_paint_extent = 500:
    /// // visible = (200 - max(150, 100)).min(500) = 50.0
    /// ```
    fn calculate_paint_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        let target_last = to;
        let target_first = from;
        let scroll_offset = constraints.scroll_offset;
        let remaining_paint = constraints.remaining_paint_extent;

        (target_last - scroll_offset.max(target_first))
            .max(0.0)
            .min(remaining_paint)
    }

    /// Computes the cached portion of region from `from` to `to`.
    ///
    /// Similar to `calculate_paint_offset` but for cache extent instead of
    /// paint extent. Used for preloading content outside visible region.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// // Flutter equivalent:
    /// double calculateCacheOffset(
    ///   SliverConstraints constraints, {
    ///   required double from,
    ///   required double to,
    /// }) {
    ///   final double cacheExtent = constraints.cacheOrigin + constraints.remainingCacheExtent;
    ///   final double targetLastScrollOffset = to;
    ///   final double targetFirstScrollOffset = from;
    ///   return (targetLastScrollOffset - math.max(constraints.cacheOrigin, targetFirstScrollOffset))
    ///       .clamp(0.0, cacheExtent);
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `constraints` - Current sliver constraints
    /// * `from` - Start of region
    /// * `to` - End of region
    ///
    /// # Returns
    ///
    /// Cached extent (for preloading).
    fn calculate_cache_offset(&self, _constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        // Note: SliverConstraints doesn't have cache_origin and remaining_cache_extent.
        // This is a simplified implementation that returns the region extent.
        // In a full implementation, these would come from viewport configuration.
        let cache_origin: f32 = 0.0;
        let cache_extent: f32 = f32::INFINITY; // Full caching by default

        let target_last = to;
        let target_first = from;

        (target_last - cache_origin.max(target_first))
            .max(0.0)
            .min(cache_extent)
    }

    // ============================================================================
    // CENTER SLIVER SUPPORT (Optional)
    // ============================================================================

    /// Offset applied to viewport's center sliver (for centering).
    ///
    /// This implicitly shifts neighboring slivers. Positive offsets shift
    /// the sliver opposite to axis direction.
    ///
    /// # When to override
    ///
    /// Only override if this sliver is designed to be the center of a viewport
    /// and needs custom centering behavior.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn center_offset_adjustment(&self) -> f32 {
    ///     // Shift center sliver by half its extent
    ///     self.total_extent / 2.0
    /// }
    /// ```
    fn center_offset_adjustment(&self) -> f32 {
        0.0 // Default: no adjustment
    }

    // ============================================================================
    // OVERFLOW AND BOUNDS (Optional)
    // ============================================================================

    /// Whether this sliver has visual overflow beyond its paint bounds.
    ///
    /// Used by scrollable to decide whether to clip content.
    fn has_visual_overflow(&self) -> bool {
        false // Default: no overflow
    }

    /// Bounding rectangle in local coordinates.
    ///
    /// For slivers, typically uses paint_extent for height/width.
    fn local_bounds(&self) -> Rect {
        Rect::ZERO // Default: empty bounds
    }

    // ============================================================================
    // DEBUG UTILITIES (Optional)
    // ============================================================================

    /// Fills diagnostic properties (Flutter debugFillProperties).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
    ///     properties.push(DiagnosticsProperty::new("scroll_extent", self.scroll_extent));
    ///     properties.push(DiagnosticsProperty::new("paint_extent", self.paint_extent));
    /// }
    /// ```
    #[cfg(debug_assertions)]
    fn debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Override to add custom properties
    }

    /// Paints debug visualization.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn debug_paint(&self, canvas: &mut Canvas, geometry: &SliverGeometry) {
    ///     // Draw bounds of visible region
    ///     let rect = Rect::from_ltwh(0.0, 0.0, 0.0, geometry.paint_extent);
    ///     canvas.rect(rect, &Paint::stroke(Color::BLUE, 1.0));
    /// }
    /// ```
    #[cfg(debug_assertions)]
    fn debug_paint(&self, _canvas: &mut flui_painting::Canvas, _geometry: &SliverGeometry) {
        // Override for custom debug visualization
    }
}
