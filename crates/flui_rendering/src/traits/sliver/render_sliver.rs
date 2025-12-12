//! RenderSliver trait for scrollable content layout

use crate::constraints::SliverConstraints;
use crate::geometry::SliverGeometry;
use crate::traits::RenderObject;
use flui_types::Offset;

/// Trait for render objects using the Sliver protocol
///
/// RenderSliver objects are used for scrollable content. They receive
/// SliverConstraints (which include scroll position and viewport info)
/// and produce SliverGeometry (which describes scroll and paint extents).
///
/// # Layout Process
///
/// 1. Parent calls `perform_layout(constraints)` on child sliver
/// 2. Child computes its geometry based on constraints and scroll offset
/// 3. Child returns SliverGeometry describing its extents
/// 4. Parent can query geometry later via `geometry()`
///
/// # Coordinate System
///
/// - **Main Axis**: Direction of scrolling (vertical or horizontal)
/// - **Cross Axis**: Perpendicular to scrolling
/// - **Scroll Offset**: How far user has scrolled along main axis
/// - **Paint Extent**: How much of the sliver is currently visible
///
/// # Example
///
/// ```ignore
/// impl RenderSliver for RenderSliverList {
///     fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
///         let scroll_offset = constraints.scroll_offset;
///         let remaining_extent = constraints.remaining_paint_extent;
///
///         // Calculate how many items are visible
///         let visible_items = calculate_visible_items(scroll_offset, remaining_extent);
///
///         // Compute geometry
///         let geometry = SliverGeometry {
///             scroll_extent: self.total_height,
///             paint_extent: visible_items.height.min(remaining_extent),
///             max_paint_extent: self.total_height,
///             ..Default::default()
///         };
///
///         self._geometry = geometry.clone();
///         geometry
///     }
///
///     fn geometry(&self) -> SliverGeometry {
///         self._geometry.clone()
///     }
/// }
/// ```
pub trait RenderSliver: RenderObject {
    // ===== Layout =====

    /// Computes the geometry of this sliver given the constraints
    ///
    /// This is the core layout method. The implementation must:
    /// - Respect scroll offset and viewport constraints
    /// - Compute scroll extent (total scrollable size)
    /// - Compute paint extent (currently visible size)
    /// - Store the geometry for later access via `geometry()`
    ///
    /// # Arguments
    ///
    /// - `constraints`: Scroll position and viewport info from parent
    ///
    /// # Returns
    ///
    /// The computed geometry describing this sliver's extents.
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry;

    /// Returns the current geometry of this sliver
    ///
    /// This must return the geometry computed during the most recent layout.
    /// Only valid after `perform_layout` has been called.
    fn geometry(&self) -> SliverGeometry;

    // ===== Paint =====

    /// Paints this sliver at the given offset
    ///
    /// # Arguments
    ///
    /// - `context`: Painting context providing canvas and layer management
    /// - `offset`: Base offset for painting
    ///
    /// # Notes
    ///
    /// - Only paint the visible portion (based on paint_extent)
    /// - Children should be painted via `context.paint_child`
    /// - Apply scroll offset when painting children
    fn paint(&self, context: &mut dyn SliverPaintingContext, offset: Offset);

    // ===== Hit Testing =====

    /// Tests whether a pointer event at the given position hits this sliver
    ///
    /// # Arguments
    ///
    /// - `result`: Accumulates hit test results
    /// - `main_axis_position`: Position along the scroll axis
    /// - `cross_axis_position`: Position perpendicular to scroll axis
    ///
    /// # Returns
    ///
    /// `true` if the position hits this sliver or a child.
    fn hit_test(
        &self,
        result: &mut dyn SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        let geometry = self.geometry();
        let hit_test_extent = geometry.hit_test_extent();

        if main_axis_position >= 0.0
            && main_axis_position < hit_test_extent
            && cross_axis_position >= 0.0
        {
            self.hit_test_children(result, main_axis_position, cross_axis_position)
                || self.hit_test_self(main_axis_position, cross_axis_position)
        } else {
            false
        }
    }

    /// Tests whether this sliver itself (not children) is hit
    fn hit_test_self(&self, _main_axis_position: f32, _cross_axis_position: f32) -> bool {
        false
    }

    /// Tests whether any children are hit
    fn hit_test_children(
        &self,
        _result: &mut dyn SliverHitTestResult,
        _main_axis_position: f32,
        _cross_axis_position: f32,
    ) -> bool {
        false
    }

    // ===== Constraints Access =====

    /// Returns the constraints used for the current layout
    ///
    /// This is needed for various calculations during paint and hit testing.
    fn constraints(&self) -> &SliverConstraints;

    // ===== Child Transforms =====

    /// Computes the paint transform for a box child of this sliver
    ///
    /// Used when a sliver contains box children (e.g., SliverToBoxAdapter).
    /// The transform maps from the child's coordinate space to the sliver's.
    fn apply_paint_transform_for_box_child(
        &self,
        _child: &dyn super::super::r#box::RenderBox,
        _transform: &mut Transform,
    ) {
        // Default: no transform
    }
}

/// Simplified transform type (will be properly implemented later)
#[derive(Debug, Clone)]
pub struct Transform;

/// Trait for sliver painting context (simplified for now)
pub trait SliverPaintingContext {
    // Sliver painting context methods will be implemented later
    // For now, this is a placeholder
}

/// Trait for sliver hit test results (simplified for now)
pub trait SliverHitTestResult {
    // Hit test result methods will be implemented later
    // For now, this is a placeholder
}
