//! RenderSliver trait for scrollable content layout.

use flui_types::prelude::AxisDirection;
use flui_types::{Rect, Size};

use crate::arity::Arity;
use crate::constraints::{SliverConstraints, SliverGeometry};
use crate::context::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext};
use crate::parent_data::ParentData;
use crate::traits::RenderObject;

// ============================================================================
// RenderSliver Trait
// ============================================================================

/// Trait for render objects that provide scrollable content.
///
/// RenderSliver is the layout protocol for scrollable content. Slivers:
/// - Receive [`SliverConstraints`] with scroll position and viewport info
/// - Compute what portion is visible and space consumed
/// - Return [`SliverGeometry`] with scroll/paint extents
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliver` abstract class in
/// `rendering/sliver.dart`.
///
/// # Layout Protocol
///
/// 1. Parent (viewport) calls `perform_layout()` with context
/// 2. Sliver determines visible portion based on scroll offset
/// 3. Sliver completes layout via `ctx.complete(geometry)`
/// 4. Viewport composes geometries to build scrollable view
///
/// # Key Concepts
///
/// - **Scroll Extent**: Total scrollable size of the sliver
/// - **Paint Extent**: How much the sliver paints in the viewport
/// - **Layout Extent**: How much the sliver consumes in the viewport
/// - **Cache Extent**: Extra area to keep rendered for smooth scrolling
///
/// # Example
///
/// ```ignore
/// impl RenderSliver for MySliverList {
///     type Arity = Variable;
///     type ParentData = SliverMultiBoxAdaptorParentData;
///
///     fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<Variable, Self::ParentData>) {
///         let scroll_offset = ctx.constraints().scroll_offset;
///         // ... compute visible items ...
///         ctx.complete(SliverGeometry { ... });
///     }
/// }
/// ```
///
/// # Note
///
/// `RenderSliver` extends `Renderable` with `Protocol = SliverProtocol`, so any type
/// implementing `RenderSliver` can be used with `Wrapper<T>` directly.
pub trait RenderSliver: Send + Sync + std::fmt::Debug + 'static {
    /// The arity of this render sliver (Leaf, Optional, Variable, etc.)
    type Arity: Arity;

    /// The parent data type for children of this render sliver.
    type ParentData: ParentData + Default;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this sliver.
    ///
    /// The context provides:
    /// - Constraints via `ctx.constraints()`
    /// - Child layout via `ctx.layout_child()`
    /// - Completion via `ctx.complete(geometry)`
    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Self::Arity, Self::ParentData>);

    /// Returns the current geometry of this sliver.
    ///
    /// Only valid after `perform_layout` has been called.
    fn geometry(&self) -> &SliverGeometry;

    /// Returns the constraints this sliver was laid out with.
    ///
    /// Only valid after `perform_layout` has been called.
    fn constraints(&self) -> &SliverConstraints;

    /// Sets the geometry for this sliver.
    ///
    /// Called during `perform_layout` to report the computed geometry.
    fn set_geometry(&mut self, geometry: SliverGeometry);

    // ========================================================================
    // Positioning
    // ========================================================================

    /// Returns the scroll offset adjustment for center slivers.
    ///
    /// This is used by viewports with a center sliver to adjust the
    /// scroll offset to account for slivers that grow in both directions.
    /// Only the center sliver and slivers before it should return a non-zero value.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.centerOffsetAdjustment` in Flutter.
    fn center_offset_adjustment(&self) -> f32 {
        0.0
    }

    /// Computes the portion of this sliver that is visible in the viewport.
    ///
    /// Given a `from` and `to` range in the sliver's coordinate space,
    /// this returns the offset at which the visible portion begins.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the range in sliver coordinates
    /// * `to` - End of the range in sliver coordinates
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.calculatePaintOffset` in Flutter.
    fn calculate_paint_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(from <= to);
        let remaining_painted_extent = constraints.remaining_paint_extent;
        let scroll_offset = constraints.scroll_offset;

        let a = scroll_offset;
        let b = scroll_offset + remaining_painted_extent;

        (to.min(b) - from.max(a)).max(0.0)
    }

    /// Computes the portion of this sliver that is in the cache area.
    ///
    /// Similar to `calculate_paint_offset` but includes the cache extent.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the range in sliver coordinates
    /// * `to` - End of the range in sliver coordinates
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.calculateCacheOffset` in Flutter.
    fn calculate_cache_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(from <= to);
        let remaining_cache_extent = constraints.remaining_cache_extent;
        let cache_origin = constraints.cache_origin;

        let a = cache_origin;
        let b = cache_origin + remaining_cache_extent;

        (to.min(b) - from.max(a)).max(0.0)
    }

    /// Returns the position of a child along the main axis.
    ///
    /// # Arguments
    ///
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childMainAxisPosition` in Flutter.
    fn child_main_axis_position(&self, child: &dyn RenderObject) -> f32 {
        let _ = child;
        0.0
    }

    /// Returns the position of a child along the cross axis.
    ///
    /// # Arguments
    ///
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childCrossAxisPosition` in Flutter.
    fn child_cross_axis_position(&self, child: &dyn RenderObject) -> f32 {
        let _ = child;
        0.0
    }

    /// Returns the scroll offset of a child.
    ///
    /// Returns the scroll offset needed to bring the leading edge
    /// of the given child into view.
    ///
    /// # Arguments
    ///
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childScrollOffset` in Flutter.
    fn child_scroll_offset(&self, child: &dyn RenderObject) -> Option<f32> {
        let _ = child;
        None
    }

    // ========================================================================
    // Size Helpers
    // ========================================================================

    /// Returns the absolute size in the main and cross axis.
    ///
    /// Given a paint extent and cross axis extent, returns the
    /// absolute size as (width, height) based on the axis direction.
    ///
    /// # Arguments
    ///
    /// * `paint_extent` - The extent along the main axis
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.getAbsoluteSize` in Flutter.
    fn get_absolute_size(&self, paint_extent: f32) -> Size {
        let constraints = self.constraints();
        let cross_axis_extent = constraints.cross_axis_extent;

        match constraints.axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => {
                Size::new(cross_axis_extent, paint_extent)
            }
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => {
                Size::new(paint_extent, cross_axis_extent)
            }
        }
    }

    /// Returns the absolute size relative to the origin.
    ///
    /// Like `get_absolute_size`, but takes into account the growth
    /// direction and axis direction to position relative to origin.
    ///
    /// # Arguments
    ///
    /// * `paint_extent` - The extent along the main axis
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.getAbsoluteSizeRelativeToOrigin` in Flutter.
    fn get_absolute_size_relative_to_origin(&self, paint_extent: f32) -> Size {
        // By default, same as get_absolute_size
        // Override for slivers that need special handling
        self.get_absolute_size(paint_extent)
    }

    // ========================================================================
    // Paint
    // ========================================================================

    /// Paints this sliver.
    ///
    /// Called after layout. Should only paint the visible portion.
    ///
    /// The context provides:
    /// - Canvas access via `ctx.canvas()`
    /// - Current offset via `ctx.offset()`
    /// - Children access via `ctx.children_mut()`
    fn paint(&mut self, ctx: &mut SliverPaintContext<'_, Self::Arity, Self::ParentData>);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this sliver.
    ///
    /// The context provides:
    /// - Position via `ctx.main_axis()`, `ctx.cross_axis()`
    /// - Child testing via `ctx.hit_test_child()`
    /// - Result management via `ctx.add_self(id)`
    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool;

    /// Hit tests just this sliver (not children).
    fn hit_test_self(&self, _main: f32, _cross: f32) -> bool {
        false
    }

    // ========================================================================
    // Paint Bounds
    // ========================================================================

    /// Returns the paint bounds of this render sliver.
    fn sliver_paint_bounds(&self) -> Rect {
        let geometry = self.geometry();
        let size = self.get_absolute_size(geometry.paint_extent);
        Rect::new(0.0, 0.0, size.width, size.height)
    }

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Creates default parent data for a child.
    fn create_default_parent_data() -> Self::ParentData {
        Self::ParentData::default()
    }
}

// ============================================================================
// Proxy Sliver
// ============================================================================

/// Trait for slivers with a single sliver child.
///
/// Generic over the child type `C` which must implement `RenderSliver`.
pub trait RenderProxySliver<C: RenderSliver>: RenderSliver {
    /// Returns the child sliver, if any.
    fn child(&self) -> Option<&C>;

    /// Returns the child sliver mutably, if any.
    fn child_mut(&mut self) -> Option<&mut C>;

    /// Sets the child sliver.
    fn set_child(&mut self, child: Option<C>);
}
