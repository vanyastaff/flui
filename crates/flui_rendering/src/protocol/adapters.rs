//! Protocol adapters for cross-protocol communication.
//!
//! Adapters enable render objects from one protocol to be used within
//! another protocol's context. This is essential for features like:
//!
//! - Embedding box widgets inside scrollable lists (SliverToBoxAdapter)
//! - Custom layout protocols that bridge existing protocols
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's adapter pattern used in:
//! - `SliverToBoxAdapter` - wraps a box widget for use in a sliver context
//! - `SliverFillViewport` - sizes box children to fill viewport
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Viewport (Box)                        │
//! │  ┌─────────────────────────────────────────────────────┐│
//! │  │             Sliver List                              ││
//! │  │  ┌─────────────────────────────────────────────────┐││
//! │  │  │    SliverToBoxAdapter                           │││
//! │  │  │  ┌─────────────────────────────────────────────┐│││
//! │  │  │  │         Box Child (e.g., Container)         ││││
//! │  │  │  └─────────────────────────────────────────────┘│││
//! │  │  └─────────────────────────────────────────────────┘││
//! │  └─────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────┘
//! ```

use std::fmt::Debug;

use flui_types::{Axis, Size};

use super::{BoxProtocol, Protocol, SliverProtocol};
use crate::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};

/// Trait for adapting between protocols.
///
/// Protocol adapters convert constraints and geometry between different
/// layout protocols, enabling cross-protocol composition.
///
/// # Type Parameters
///
/// - `From`: Source protocol
/// - `To`: Target protocol
pub trait ProtocolAdapter<From: Protocol, To: Protocol>: Debug + Send + Sync {
    /// Converts constraints from source to target protocol.
    fn adapt_constraints(&self, constraints: &From::Constraints) -> To::Constraints;

    /// Converts geometry from target back to source protocol.
    fn adapt_geometry(
        &self,
        geometry: &To::Geometry,
        constraints: &From::Constraints,
    ) -> From::Geometry;
}

// ============================================================================
// SliverToBoxAdapter
// ============================================================================

/// Adapter for embedding box render objects inside sliver contexts.
///
/// This is the most common adapter, used whenever you need to place
/// a regular widget (box protocol) inside a scrollable list (sliver protocol).
///
/// # Layout Behavior
///
/// 1. Receives `SliverConstraints` from parent viewport
/// 2. Converts to `BoxConstraints` based on cross-axis extent
/// 3. Lays out box child with converted constraints
/// 4. Converts resulting `Size` back to `SliverGeometry`
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliverToBoxAdapter`.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::protocol::{SliverToBoxAdapter, ProtocolAdapter};
/// use flui_rendering::constraints::SliverConstraints;
///
/// let adapter = SliverToBoxAdapter::default();
/// let sliver_constraints = /* ... */;
/// let box_constraints = adapter.adapt_constraints(&sliver_constraints);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverToBoxAdapter;

impl SliverToBoxAdapter {
    /// Creates a new sliver-to-box adapter.
    pub fn new() -> Self {
        Self
    }

    /// Computes box constraints from sliver constraints.
    ///
    /// The main axis is unbounded (child determines its own extent),
    /// while the cross axis is constrained to match the viewport.
    pub fn compute_box_constraints(constraints: &SliverConstraints) -> BoxConstraints {
        let axis = constraints.axis_direction.axis();

        match axis {
            Axis::Vertical => BoxConstraints {
                min_width: constraints.cross_axis_extent,
                max_width: constraints.cross_axis_extent,
                min_height: 0.0,
                max_height: f32::INFINITY,
            },
            Axis::Horizontal => BoxConstraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: constraints.cross_axis_extent,
                max_height: constraints.cross_axis_extent,
            },
        }
    }

    /// Computes sliver geometry from box child size.
    ///
    /// The child's main axis extent becomes the sliver's scroll extent
    /// and paint extent (clamped to remaining paint extent).
    pub fn compute_sliver_geometry(
        child_size: Size,
        constraints: &SliverConstraints,
    ) -> SliverGeometry {
        let axis = constraints.axis_direction.axis();

        let child_extent = match axis {
            Axis::Vertical => child_size.height,
            Axis::Horizontal => child_size.width,
        };

        // Calculate how much of the child is visible
        let paint_extent = (child_extent - constraints.scroll_offset)
            .clamp(0.0, constraints.remaining_paint_extent);

        // Calculate layout extent (how much space this sliver consumes)
        let layout_extent = paint_extent;

        // Calculate max paint extent (child's full extent)
        let max_paint_extent = child_extent;

        SliverGeometry {
            scroll_extent: child_extent,
            paint_extent,
            layout_extent,
            max_paint_extent,
            paint_origin: 0.0,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: child_extent > constraints.remaining_paint_extent,
            cache_extent: child_extent, // Cache entire child
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None, // Uses constraint's cross axis extent
            scroll_offset_correction: None,
        }
    }
}

impl ProtocolAdapter<SliverProtocol, BoxProtocol> for SliverToBoxAdapter {
    fn adapt_constraints(&self, constraints: &SliverConstraints) -> BoxConstraints {
        Self::compute_box_constraints(constraints)
    }

    fn adapt_geometry(&self, geometry: &Size, constraints: &SliverConstraints) -> SliverGeometry {
        Self::compute_sliver_geometry(*geometry, constraints)
    }
}

// ============================================================================
// BoxToSliverAdapter (for completeness)
// ============================================================================

/// Adapter for embedding sliver render objects inside box contexts.
///
/// This is less common but useful for creating fixed-size scrollable
/// regions within a box layout.
///
/// # Note
///
/// This adapter requires knowing the viewport size upfront, as box
/// constraints don't naturally map to sliver constraints.
#[derive(Debug, Clone, Copy)]
pub struct BoxToSliverAdapter {
    /// The viewport extent to use for the main axis.
    pub viewport_extent: f32,
}

impl BoxToSliverAdapter {
    /// Creates a new box-to-sliver adapter with the given viewport extent.
    pub fn new(viewport_extent: f32) -> Self {
        Self { viewport_extent }
    }
}

impl Default for BoxToSliverAdapter {
    fn default() -> Self {
        Self {
            viewport_extent: 0.0,
        }
    }
}

impl ProtocolAdapter<BoxProtocol, SliverProtocol> for BoxToSliverAdapter {
    fn adapt_constraints(&self, constraints: &BoxConstraints) -> SliverConstraints {
        use crate::constraints::GrowthDirection;
        use crate::view::ScrollDirection;
        use flui_types::prelude::AxisDirection;

        SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: self.viewport_extent,
            cross_axis_extent: constraints.max_width,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: self.viewport_extent,
            remaining_cache_extent: self.viewport_extent,
            cache_origin: 0.0,
        }
    }

    fn adapt_geometry(&self, geometry: &SliverGeometry, _constraints: &BoxConstraints) -> Size {
        // Convert sliver geometry back to size
        Size::new(0.0, geometry.layout_extent)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::GrowthDirection;
    use crate::view::ScrollDirection;
    use flui_types::prelude::AxisDirection;

    fn create_test_sliver_constraints() -> SliverConstraints {
        SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 850.0,
            cache_origin: -250.0,
        }
    }

    #[test]
    fn test_sliver_to_box_adapter_vertical() {
        let adapter = SliverToBoxAdapter::new();
        let sliver_constraints = create_test_sliver_constraints();

        let box_constraints = adapter.adapt_constraints(&sliver_constraints);

        // Cross axis should be tight
        assert_eq!(box_constraints.min_width, 400.0);
        assert_eq!(box_constraints.max_width, 400.0);

        // Main axis should be unbounded
        assert_eq!(box_constraints.min_height, 0.0);
        assert!(box_constraints.max_height.is_infinite());
    }

    #[test]
    fn test_sliver_to_box_adapter_horizontal() {
        let mut sliver_constraints = create_test_sliver_constraints();
        sliver_constraints.axis_direction = AxisDirection::LeftToRight;

        let box_constraints = SliverToBoxAdapter::compute_box_constraints(&sliver_constraints);

        // Cross axis should be tight
        assert_eq!(box_constraints.min_height, 400.0);
        assert_eq!(box_constraints.max_height, 400.0);

        // Main axis should be unbounded
        assert_eq!(box_constraints.min_width, 0.0);
        assert!(box_constraints.max_width.is_infinite());
    }

    #[test]
    fn test_sliver_to_box_geometry_conversion() {
        let sliver_constraints = create_test_sliver_constraints();
        let child_size = Size::new(400.0, 200.0);

        let geometry = SliverToBoxAdapter::compute_sliver_geometry(child_size, &sliver_constraints);

        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 200.0);
        assert_eq!(geometry.layout_extent, 200.0);
        assert!(geometry.visible);
        assert!(!geometry.has_visual_overflow);
    }

    #[test]
    fn test_sliver_to_box_geometry_scrolled() {
        let mut sliver_constraints = create_test_sliver_constraints();
        sliver_constraints.scroll_offset = 100.0;
        sliver_constraints.remaining_paint_extent = 500.0;

        let child_size = Size::new(400.0, 200.0);

        let geometry = SliverToBoxAdapter::compute_sliver_geometry(child_size, &sliver_constraints);

        assert_eq!(geometry.scroll_extent, 200.0);
        // Only 100 pixels visible (200 - 100 scroll offset)
        assert_eq!(geometry.paint_extent, 100.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_sliver_to_box_geometry_scrolled_out() {
        let mut sliver_constraints = create_test_sliver_constraints();
        sliver_constraints.scroll_offset = 250.0; // Past the child

        let child_size = Size::new(400.0, 200.0);

        let geometry = SliverToBoxAdapter::compute_sliver_geometry(child_size, &sliver_constraints);

        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_box_to_sliver_adapter() {
        let adapter = BoxToSliverAdapter::new(600.0);

        let box_constraints = BoxConstraints {
            min_width: 0.0,
            max_width: 400.0,
            min_height: 0.0,
            max_height: 600.0,
        };

        let sliver_constraints = adapter.adapt_constraints(&box_constraints);

        assert_eq!(sliver_constraints.viewport_main_axis_extent, 600.0);
        assert_eq!(sliver_constraints.cross_axis_extent, 400.0);
    }
}
