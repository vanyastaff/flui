//! RenderSliver trait for scrollable content layout.

use flui_types::prelude::AxisDirection;
use flui_types::{Offset, Size};

use crate::constraints::{SliverConstraints, SliverGeometry};
use crate::pipeline::PaintingContext;
use crate::traits::RenderObject;

/// Trait for render objects that provide scrollable content.
///
/// RenderSliver is the layout protocol for scrollable content. Slivers:
/// - Receive [`SliverConstraints`] with scroll position and viewport info
/// - Compute what portion is visible and space consumed
/// - Return [`SliverGeometry`] with scroll/paint extents
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliver` abstract class.
///
/// # Key Concepts
///
/// - **Scroll Extent**: Total scrollable size of the sliver
/// - **Paint Extent**: How much the sliver paints in the viewport
/// - **Layout Extent**: How much the sliver consumes in the viewport
/// - **Cache Extent**: Extra area to keep rendered for smooth scrolling
pub trait RenderSliver: RenderObject {
    // ========================================================================
    // Semantics
    // ========================================================================

    /// Whether to ensure semantics is available for this sliver.
    ///
    /// If true, the sliver will always generate semantics information,
    /// even if it wouldn't normally be required.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.ensureSemantics` getter.
    fn ensure_semantics(&self) -> bool {
        false
    }
    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this sliver.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.performLayout` method.
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry;

    /// Returns the current geometry of this sliver.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.geometry` getter.
    fn geometry(&self) -> &SliverGeometry;

    /// Sets the geometry for this sliver.
    ///
    /// Called during `perform_layout` to report the computed geometry.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.geometry` setter.
    fn set_geometry(&mut self, geometry: SliverGeometry);

    /// Returns the constraints this sliver was laid out with.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.constraints` getter.
    fn constraints(&self) -> &SliverConstraints;

    // ========================================================================
    // Painting
    // ========================================================================

    /// Paints this sliver.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.paint` method.
    fn paint(&self, context: &mut PaintingContext, offset: Offset);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this sliver.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.hitTest` method.
    fn hit_test(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        let geometry = self.geometry();
        let constraints = self.constraints();

        if main_axis_position >= 0.0
            && main_axis_position < geometry.hit_test_extent
            && cross_axis_position >= 0.0
            && cross_axis_position < constraints.cross_axis_extent
        {
            self.hit_test_children(result, main_axis_position, cross_axis_position)
                || self.hit_test_self(main_axis_position, cross_axis_position)
        } else {
            false
        }
    }

    /// Hit tests just this sliver (not children).
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.hitTestSelf` method.
    fn hit_test_self(&self, _main: f32, _cross: f32) -> bool {
        false
    }

    /// Hit tests children of this sliver.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.hitTestChildren` method.
    fn hit_test_children(
        &self,
        _result: &mut SliverHitTestResult,
        _main: f32,
        _cross: f32,
    ) -> bool {
        false
    }

    // ========================================================================
    // Positioning
    // ========================================================================

    /// Returns the scroll offset adjustment for center slivers.
    ///
    /// This is used by viewports with a center sliver to adjust the
    /// scroll offset to account for slivers that grow in both directions.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.centerOffsetAdjustment` getter.
    fn center_offset_adjustment(&self) -> f32 {
        0.0
    }

    /// Returns the position of a child along the main axis.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.childMainAxisPosition` method.
    fn child_main_axis_position(&self, _child: &dyn RenderObject) -> f32 {
        0.0
    }

    /// Returns the position of a child along the cross axis.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.childCrossAxisPosition` method.
    fn child_cross_axis_position(&self, _child: &dyn RenderObject) -> f32 {
        0.0
    }

    /// Returns the scroll offset of a child.
    ///
    /// Returns the scroll offset needed to bring the leading edge
    /// of the given child into view.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.childScrollOffset` method.
    fn child_scroll_offset(&self, _child: &dyn RenderObject) -> Option<f32> {
        None
    }

    // ========================================================================
    // Paint/Cache Offset Calculation
    // ========================================================================

    /// Computes the portion of this sliver that is visible in the viewport.
    ///
    /// Given a `from` and `to` range in the sliver's coordinate space,
    /// this returns the offset at which the visible portion begins.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.calculatePaintOffset` method.
    fn calculate_paint_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(from <= to);
        let a = constraints.scroll_offset;
        let b = constraints.scroll_offset + constraints.remaining_paint_extent;
        (to.min(b) - from.max(a)).max(0.0)
    }

    /// Computes the portion of this sliver that is in the cache area.
    ///
    /// Similar to `calculate_paint_offset` but includes the cache extent.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.calculateCacheOffset` method.
    fn calculate_cache_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        debug_assert!(from <= to);
        let a = constraints.cache_origin;
        let b = constraints.cache_origin + constraints.remaining_cache_extent;
        (to.min(b) - from.max(a)).max(0.0)
    }

    // ========================================================================
    // Size Helpers
    // ========================================================================

    /// Returns the absolute size in the main and cross axis.
    ///
    /// Given a paint extent and cross axis extent, returns the
    /// absolute size as (width, height) based on the axis direction.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.getAbsoluteSize` method.
    fn get_absolute_size(&self) -> Size {
        let constraints = self.constraints();
        let geometry = self.geometry();
        let paint_extent = geometry.paint_extent;
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
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.getAbsoluteSizeRelativeToOrigin` method.
    fn get_absolute_size_relative_to_origin(&self) -> Size {
        let constraints = self.constraints();
        let geometry = self.geometry();
        let paint_extent = geometry.paint_extent;
        let cross_axis_extent = constraints.cross_axis_extent;

        match (constraints.axis_direction, constraints.growth_direction) {
            (AxisDirection::TopToBottom, _) | (AxisDirection::BottomToTop, _) => {
                Size::new(cross_axis_extent, paint_extent)
            }
            (AxisDirection::LeftToRight, _) | (AxisDirection::RightToLeft, _) => {
                Size::new(paint_extent, cross_axis_extent)
            }
        }
    }

    // ========================================================================
    // Transform
    // ========================================================================

    /// Applies the paint transform for the given child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.applyPaintTransform` method.
    fn apply_paint_transform_for_child(&self, child: &dyn RenderObject, transform: &mut [f32; 16]) {
        let _ = child;
        let _ = transform;
        // Default: identity transform
    }

    // ========================================================================
    // Debug Methods
    // ========================================================================

    /// Resets the size for debug purposes.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.debugResetSize` method.
    fn debug_reset_size(&mut self) {
        // Default: do nothing
        // Subclasses may clear cached geometry information
    }

    /// Debug assertion that this sliver's geometry meets its constraints.
    ///
    /// Called after layout to verify the geometry is valid.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.debugAssertDoesMeetConstraints` method.
    fn debug_assert_does_meet_constraints(&self) {
        let geometry = self.geometry();
        let constraints = self.constraints();

        // Verify geometry values are non-negative
        debug_assert!(
            geometry.scroll_extent >= 0.0,
            "scrollExtent must be non-negative, got {}",
            geometry.scroll_extent
        );
        debug_assert!(
            geometry.paint_extent >= 0.0,
            "paintExtent must be non-negative, got {}",
            geometry.paint_extent
        );
        debug_assert!(
            geometry.layout_extent >= 0.0,
            "layoutExtent must be non-negative, got {}",
            geometry.layout_extent
        );
        debug_assert!(
            geometry.cache_extent >= 0.0,
            "cacheExtent must be non-negative, got {}",
            geometry.cache_extent
        );
        debug_assert!(
            geometry.max_paint_extent >= 0.0,
            "maxPaintExtent must be non-negative, got {}",
            geometry.max_paint_extent
        );

        // Verify layout extent doesn't exceed paint extent
        debug_assert!(
            geometry.layout_extent <= geometry.paint_extent,
            "layoutExtent ({}) must not exceed paintExtent ({})",
            geometry.layout_extent,
            geometry.paint_extent
        );

        // Verify paint extent doesn't exceed remaining paint extent
        debug_assert!(
            geometry.paint_extent <= constraints.remaining_paint_extent,
            "paintExtent ({}) must not exceed remainingPaintExtent ({})",
            geometry.paint_extent,
            constraints.remaining_paint_extent
        );
    }

    /// Paints debugging visuals for this sliver.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.debugPaint` method.
    fn debug_paint(&self, _context: &mut PaintingContext, _offset: Offset) {
        // Default: do nothing
        // In debug mode, could paint overlays showing sliver extent
    }

    /// Returns whether this sliver needs to compute scroll offset.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliver.debugNeedsScrollOffset` getter.
    fn debug_needs_scroll_offset(&self) -> bool {
        false
    }
}

/// Result of a sliver hit test.
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    entries: Vec<SliverHitTestEntry>,
}

impl SliverHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry to the result.
    pub fn add(&mut self, entry: SliverHitTestEntry) {
        self.entries.push(entry);
    }

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[SliverHitTestEntry] {
        &self.entries
    }
}

/// An entry in a sliver hit test result.
#[derive(Debug)]
pub struct SliverHitTestEntry {
    /// Position along main axis.
    pub main_axis_position: f32,
    /// Position along cross axis.
    pub cross_axis_position: f32,
}

impl SliverHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(main_axis_position: f32, cross_axis_position: f32) -> Self {
        Self {
            main_axis_position,
            cross_axis_position,
        }
    }
}
