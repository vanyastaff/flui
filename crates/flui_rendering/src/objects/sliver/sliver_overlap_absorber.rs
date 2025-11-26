//! RenderSliverOverlapAbsorber - Absorbs overlap for nested scroll views

use crate::core::{RuntimeArity, LegacySliverRender, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::SliverGeometry;
use std::sync::{Arc, Mutex};

/// Handle for communicating absorbed overlap between RenderSliverOverlapAbsorber
/// and RenderSliverOverlapInjector.
///
/// This handle stores the overlap extent that has been absorbed by a
/// RenderSliverOverlapAbsorber and needs to be injected elsewhere in the
/// scroll view (typically by a RenderSliverOverlapInjector).
#[derive(Debug, Clone)]
pub struct SliverOverlapAbsorberHandle {
    /// The absorbed overlap extent (shared between absorber and injector)
    extent: Arc<Mutex<f32>>,
}

impl SliverOverlapAbsorberHandle {
    /// Create a new handle with zero initial overlap
    pub fn new() -> Self {
        Self {
            extent: Arc::new(Mutex::new(0.0)),
        }
    }

    /// Get the current absorbed overlap extent
    pub fn get_extent(&self) -> f32 {
        *self.extent.lock().unwrap()
    }

    /// Set the absorbed overlap extent (internal use)
    fn set_extent(&self, extent: f32) {
        *self.extent.lock().unwrap() = extent;
    }
}

impl Default for SliverOverlapAbsorberHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject that absorbs overlap from a child sliver
///
/// This sliver wraps another sliver and treats its layout extent as overlap.
/// The absorbed overlap is reported to a `SliverOverlapAbsorberHandle`, which
/// can be used by a `RenderSliverOverlapInjector` to inject the overlap
/// elsewhere in the scroll view.
///
/// # Use Cases
///
/// - Nested scroll views with overlapping headers
/// - Coordinating pinned headers across multiple scroll views
/// - Managing overlap in complex scrollable layouts
/// - SliverAppBar with NestedScrollView
///
/// # Implementation Notes
///
/// The absorbed overlap is the difference between:
/// - The child's `maxScrollObstructionExtent` (content that overlaps)
/// - The overlap reported by this widget (zero)
///
/// This difference is stored in the handle for use by other widgets.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSliverOverlapAbsorber, SliverOverlapAbsorberHandle};
///
/// let handle = SliverOverlapAbsorberHandle::new();
/// let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
///
/// // Later, an injector can read the absorbed overlap
/// let overlap = handle.get_extent();
/// ```
#[derive(Debug)]
pub struct RenderSliverOverlapAbsorber {
    /// Handle for communicating absorbed overlap
    pub handle: SliverOverlapAbsorberHandle,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverOverlapAbsorber {
    /// Create new overlap absorber with given handle
    ///
    /// # Arguments
    /// * `handle` - Handle for storing absorbed overlap extent
    pub fn new(handle: SliverOverlapAbsorberHandle) -> Self {
        Self {
            handle,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry for overlap absorption
    ///
    /// The absorbed overlap is the child's maxScrollObstructionExtent.
    /// We modify the child's geometry to:
    /// - Reduce scroll_extent by the obstruction extent
    /// - Set layout_extent to max(0, paint_extent - obstruction)
    fn calculate_sliver_geometry(&self, child_geometry: SliverGeometry) -> SliverGeometry {
        // The obstruction extent is the overlap we're absorbing
        let obstruction_extent = child_geometry.max_paint_extent.min(
            child_geometry
                .hit_test_extent
                .unwrap_or(child_geometry.paint_extent),
        );

        // Store the absorbed overlap in the handle
        self.handle.set_extent(obstruction_extent);

        // Modify child geometry to account for absorbed overlap
        SliverGeometry {
            // Reduce scroll extent by absorbed overlap
            scroll_extent: (child_geometry.scroll_extent - obstruction_extent).max(0.0),

            // Layout extent excludes the obstruction
            layout_extent: (child_geometry.paint_extent - obstruction_extent).max(0.0),

            // Other fields pass through from child
            paint_extent: child_geometry.paint_extent,
            paint_origin: child_geometry.paint_origin,
            max_paint_extent: child_geometry.max_paint_extent,
            max_scroll_obsolescence: child_geometry.max_scroll_obsolescence,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: child_geometry.cross_axis_extent,
            cache_extent: child_geometry.cache_extent,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry.hit_test_extent,
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl Default for RenderSliverOverlapAbsorber {
    fn default() -> Self {
        Self::new(SliverOverlapAbsorberHandle::new())
    }
}

impl LegacySliverRender for RenderSliverOverlapAbsorber {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        // If no child, return zero geometry and clear handle
        let Some(child_id) = ctx.children.try_single() else {
            self.handle.set_extent(0.0);
            self.sliver_geometry = SliverGeometry::default();
            return self.sliver_geometry;
        };

        // Layout child with unchanged constraints
        let child_geometry = ctx.tree.layout_sliver_child(child_id, ctx.constraints);

        // Calculate our geometry by absorbing child's overlap
        self.sliver_geometry = self.calculate_sliver_geometry(child_geometry);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Paint child if present
        if let Some(child_id) = ctx.children.try_single() {
            return ctx.tree.paint_child(child_id, ctx.offset);
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_overlap_absorber_handle_new() {
        let handle = SliverOverlapAbsorberHandle::new();
        assert_eq!(handle.get_extent(), 0.0);
    }

    #[test]
    fn test_sliver_overlap_absorber_handle_set_extent() {
        let handle = SliverOverlapAbsorberHandle::new();
        handle.set_extent(50.0);
        assert_eq!(handle.get_extent(), 50.0);
    }

    #[test]
    fn test_sliver_overlap_absorber_handle_clone() {
        let handle1 = SliverOverlapAbsorberHandle::new();
        handle1.set_extent(100.0);

        let handle2 = handle1.clone();
        assert_eq!(handle2.get_extent(), 100.0);

        // Verify they share the same underlying data
        handle1.set_extent(200.0);
        assert_eq!(handle2.get_extent(), 200.0);
    }

    #[test]
    fn test_render_sliver_overlap_absorber_new() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        assert_eq!(absorber.handle.get_extent(), 0.0);
    }

    #[test]
    fn test_render_sliver_overlap_absorber_default() {
        let absorber = RenderSliverOverlapAbsorber::default();
        assert_eq!(absorber.handle.get_extent(), 0.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_basic() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        let child_geometry = SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: 150.0,
            paint_origin: 0.0,
            layout_extent: 150.0,
            max_paint_extent: 200.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 1.0,
            cross_axis_extent: 400.0,
            cache_extent: 150.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: Some(150.0),
            scroll_offset_correction: None,
        };

        let geometry = absorber.calculate_sliver_geometry(child_geometry);

        // Obstruction extent should be min(max_paint_extent, hit_test_extent)
        // = min(200.0, 150.0) = 150.0
        assert_eq!(handle.get_extent(), 150.0);

        // scroll_extent = child_scroll_extent - obstruction = 200 - 150 = 50
        assert_eq!(geometry.scroll_extent, 50.0);

        // layout_extent = max(0, paint_extent - obstruction) = max(0, 150 - 150) = 0
        assert_eq!(geometry.layout_extent, 0.0);

        // paint_extent passes through
        assert_eq!(geometry.paint_extent, 150.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_with_obstruction() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        let child_geometry = SliverGeometry {
            scroll_extent: 300.0,
            paint_extent: 200.0,
            paint_origin: 0.0,
            layout_extent: 200.0,
            max_paint_extent: 300.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 1.0,
            cross_axis_extent: 400.0,
            cache_extent: 200.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: Some(100.0), // Obstruction is 100px
            scroll_offset_correction: None,
        };

        let geometry = absorber.calculate_sliver_geometry(child_geometry);

        // Obstruction extent = min(300.0, 100.0) = 100.0
        assert_eq!(handle.get_extent(), 100.0);

        // scroll_extent = 300 - 100 = 200
        assert_eq!(geometry.scroll_extent, 200.0);

        // layout_extent = max(0, 200 - 100) = 100
        assert_eq!(geometry.layout_extent, 100.0);

        assert_eq!(geometry.paint_extent, 200.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_no_hit_test_extent() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        let child_geometry = SliverGeometry {
            scroll_extent: 250.0,
            paint_extent: 180.0,
            paint_origin: 0.0,
            layout_extent: 180.0,
            max_paint_extent: 250.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 1.0,
            cross_axis_extent: 400.0,
            cache_extent: 180.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: None, // Falls back to paint_extent
            scroll_offset_correction: None,
        };

        let geometry = absorber.calculate_sliver_geometry(child_geometry);

        // Obstruction extent = min(250.0, 180.0) = 180.0
        assert_eq!(handle.get_extent(), 180.0);

        // scroll_extent = 250 - 180 = 70
        assert_eq!(geometry.scroll_extent, 70.0);

        // layout_extent = max(0, 180 - 180) = 0
        assert_eq!(geometry.layout_extent, 0.0);
    }

    #[test]
    fn test_arity_is_single_child() {
        let absorber = RenderSliverOverlapAbsorber::default();
        assert_eq!(absorber.arity(), RuntimeArity::Exact(1));
    }
}
