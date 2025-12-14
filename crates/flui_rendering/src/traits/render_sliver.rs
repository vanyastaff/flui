//! RenderSliver trait for scrollable content layout.

use flui_types::{Offset, SliverConstraints, SliverGeometry};

use super::RenderObject;
use crate::pipeline::PaintingContext;

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
/// 1. Parent (viewport) calls `perform_layout()` with constraints
/// 2. Sliver determines visible portion based on scroll offset
/// 3. Sliver returns geometry describing how much space it consumes
/// 4. Viewport composes geometries to build scrollable view
///
/// # Key Concepts
///
/// - **Scroll Extent**: Total scrollable size of the sliver
/// - **Paint Extent**: How much the sliver paints in the viewport
/// - **Layout Extent**: How much the sliver consumes in the viewport
/// - **Cache Extent**: Extra area to keep rendered for smooth scrolling
pub trait RenderSliver: RenderObject {
    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this sliver.
    ///
    /// Called by the parent viewport with constraints that specify
    /// scroll position and viewport dimensions.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The sliver constraints from the viewport
    ///
    /// # Returns
    ///
    /// The computed geometry describing this sliver's space usage
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry;

    /// Returns the current geometry of this sliver.
    ///
    /// Only valid after `perform_layout` has been called.
    fn geometry(&self) -> &SliverGeometry;

    /// Returns the constraints this sliver was laid out with.
    ///
    /// Only valid after `perform_layout` has been called.
    fn constraints(&self) -> &SliverConstraints;

    // ========================================================================
    // Paint
    // ========================================================================

    /// Paints this sliver.
    ///
    /// Called after layout. Should only paint the visible portion.
    ///
    /// # Arguments
    ///
    /// * `context` - The painting context with canvas access
    /// * `offset` - The offset from the origin to paint at
    fn paint(&self, context: &mut PaintingContext, offset: Offset);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this sliver.
    ///
    /// Positions are in the sliver's coordinate system:
    /// - Main axis position: along scroll direction
    /// - Cross axis position: perpendicular to scroll direction
    ///
    /// # Arguments
    ///
    /// * `result` - The hit test result to add entries to
    /// * `main_axis_position` - Position along main (scroll) axis
    /// * `cross_axis_position` - Position along cross axis
    fn hit_test(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        let geometry = self.geometry();
        let constraints = self.constraints();

        if main_axis_position >= 0.0
            && main_axis_position < geometry.hit_test_extent()
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
    fn hit_test_self(&self, _main: f32, _cross: f32) -> bool {
        false
    }

    /// Hit tests children of this sliver.
    fn hit_test_children(
        &self,
        _result: &mut SliverHitTestResult,
        _main: f32,
        _cross: f32,
    ) -> bool {
        false
    }
}

// ============================================================================
// Supporting Types
// ============================================================================

/// Result of a sliver hit test.
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    /// The list of hit test entries.
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

// ============================================================================
// Proxy Sliver
// ============================================================================

/// Trait for slivers with a single sliver child.
pub trait RenderProxySliver: RenderSliver {
    /// Returns the child sliver, if any.
    fn child(&self) -> Option<&dyn RenderSliver>;

    /// Returns the child sliver mutably, if any.
    fn child_mut(&mut self) -> Option<&mut dyn RenderSliver>;

    /// Sets the child sliver.
    fn set_child(&mut self, child: Option<Box<dyn RenderSliver>>);
}

// ============================================================================
// Box Adapter
// ============================================================================

/// Trait for slivers that contain a single box child.
///
/// Used to embed box widgets inside scrollable content.
pub trait RenderSliverSingleBoxAdapter: RenderSliver {
    /// Returns the box child, if any.
    fn child(&self) -> Option<&dyn super::RenderBox>;

    /// Returns the box child mutably, if any.
    fn child_mut(&mut self) -> Option<&mut dyn super::RenderBox>;

    /// Sets the box child.
    fn set_child(&mut self, child: Option<Box<dyn super::RenderBox>>);
}

// ============================================================================
// Multi Box Adapter
// ============================================================================

/// Trait for slivers with multiple box children (lists, grids).
pub trait RenderSliverMultiBoxAdaptor: RenderSliver {
    /// Returns an iterator over box children.
    fn children(&self) -> Box<dyn Iterator<Item = &dyn super::RenderBox> + '_>;

    /// Returns a mutable iterator over box children.
    fn children_mut(&mut self) -> Box<dyn Iterator<Item = &mut dyn super::RenderBox> + '_>;

    /// Returns the number of children.
    fn child_count(&self) -> usize;
}
