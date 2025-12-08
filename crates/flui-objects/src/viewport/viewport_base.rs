//! RenderViewportBase - Base functionality for viewport render objects
//!
//! This module provides the common infrastructure for viewports that contain
//! sliver children, including layout algorithms and parent data management.

use flui_rendering::ElementId;
use flui_types::constraints::{GrowthDirection, ScrollDirection};
use flui_types::layout::AxisDirection;
use flui_types::{Offset, Size, SliverConstraints, SliverGeometry};

/// Parent data for slivers in a viewport
///
/// Stores the paint offset for each sliver child, which is the offset
/// from the viewport's origin to where the sliver should be painted.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverPhysicalContainerParentData {
    /// Paint offset from viewport's top-left corner
    pub paint_offset: Offset,
}

impl SliverPhysicalContainerParentData {
    /// Create new parent data with zero offset
    pub const fn new() -> Self {
        Self {
            paint_offset: Offset::ZERO,
        }
    }

    /// Create parent data with specific offset
    pub const fn with_offset(offset: Offset) -> Self {
        Self {
            paint_offset: offset,
        }
    }
}

/// Result of laying out a sequence of slivers
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverLayoutResult {
    /// Total scroll extent of all laid out slivers
    pub total_scroll_extent: f32,
    /// Total paint extent consumed
    pub total_paint_extent: f32,
    /// Maximum scroll obstruction extent (for pinned headers)
    pub max_scroll_obstruction_extent: f32,
    /// Whether any sliver requested a scroll offset correction
    pub scroll_offset_correction: Option<f32>,
    /// Whether layout completed (no early exit)
    pub completed: bool,
}

/// Configuration for viewport layout
#[derive(Debug, Clone, Copy)]
pub struct ViewportLayoutConfig {
    /// Direction of the main axis
    pub axis_direction: AxisDirection,
    /// Direction of the cross axis
    pub cross_axis_direction: AxisDirection,
    /// Current scroll offset
    pub scroll_offset: f32,
    /// Viewport size on main axis
    pub viewport_main_axis_extent: f32,
    /// Viewport size on cross axis
    pub cross_axis_extent: f32,
    /// Cache extent for off-screen rendering
    pub cache_extent: f32,
    /// User scroll direction
    pub user_scroll_direction: ScrollDirection,
    /// Anchor position (0.0 to 1.0)
    pub anchor: f32,
}

impl Default for ViewportLayoutConfig {
    fn default() -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            cross_axis_direction: AxisDirection::LeftToRight,
            scroll_offset: 0.0,
            viewport_main_axis_extent: 0.0,
            cross_axis_extent: 0.0,
            cache_extent: 250.0,
            user_scroll_direction: ScrollDirection::Idle,
            anchor: 0.0,
        }
    }
}

/// Trait for viewport layout operations
///
/// This trait provides the core layout algorithm for viewports containing
/// sliver children. Implementors only need to provide child access methods.
pub trait ViewportLayoutDelegate {
    /// Get the number of sliver children
    fn sliver_child_count(&self) -> usize;

    /// Get sliver child at index
    fn sliver_child_at(&self, index: usize) -> Option<ElementId>;

    /// Layout a sliver child with constraints and return its geometry
    fn layout_sliver_child(
        &mut self,
        child_id: ElementId,
        constraints: &SliverConstraints,
    ) -> SliverGeometry;

    /// Set parent data for a sliver child
    fn set_sliver_parent_data(
        &mut self,
        child_id: ElementId,
        data: SliverPhysicalContainerParentData,
    );

    /// Get parent data for a sliver child
    fn get_sliver_parent_data(
        &self,
        child_id: ElementId,
    ) -> Option<SliverPhysicalContainerParentData>;
}

/// Calculate the main axis offset for a child based on growth direction
#[inline]
pub fn compute_child_main_axis_position(
    growth_direction: GrowthDirection,
    child_layout_position: f32,
    child_paint_extent: f32,
    viewport_main_axis_extent: f32,
) -> f32 {
    match growth_direction {
        GrowthDirection::Forward => child_layout_position,
        GrowthDirection::Reverse => {
            viewport_main_axis_extent - child_layout_position - child_paint_extent
        }
    }
}

/// Convert main axis position to paint offset
pub fn compute_paint_offset(
    axis_direction: AxisDirection,
    main_axis_position: f32,
    cross_axis_position: f32,
) -> Offset {
    match axis_direction {
        AxisDirection::TopToBottom => Offset::new(cross_axis_position, main_axis_position),
        AxisDirection::BottomToTop => Offset::new(cross_axis_position, main_axis_position),
        AxisDirection::LeftToRight => Offset::new(main_axis_position, cross_axis_position),
        AxisDirection::RightToLeft => Offset::new(main_axis_position, cross_axis_position),
    }
}

/// Layout a sequence of slivers in a viewport
///
/// This implements the core sliver layout algorithm:
/// 1. For each sliver, compute constraints based on remaining space
/// 2. Layout the sliver and get its geometry
/// 3. Update running totals and check for scroll offset corrections
/// 4. Continue until all slivers are laid out or space is exhausted
///
/// # Arguments
///
/// * `delegate` - Object providing sliver access and layout methods
/// * `config` - Viewport configuration
/// * `growth_direction` - Whether laying out forward or reverse slivers
/// * `children` - Indices of children to layout (in layout order)
/// * `initial_scroll_offset` - Starting scroll offset for this sequence
/// * `initial_layout_offset` - Starting layout offset for positioning
///
/// # Returns
///
/// `SliverLayoutResult` containing totals and any scroll offset correction
pub fn layout_sliver_sequence<D: ViewportLayoutDelegate>(
    delegate: &mut D,
    config: &ViewportLayoutConfig,
    growth_direction: GrowthDirection,
    children: &[usize],
    initial_scroll_offset: f32,
    initial_layout_offset: f32,
) -> SliverLayoutResult {
    let mut result = SliverLayoutResult::default();
    let mut scroll_offset = initial_scroll_offset;
    let mut layout_offset = initial_layout_offset;
    let mut remaining_paint_extent = config.viewport_main_axis_extent;
    let mut remaining_cache_extent = config.cache_extent;
    let mut preceding_scroll_extent = 0.0_f32;

    for &child_index in children {
        let Some(child_id) = delegate.sliver_child_at(child_index) else {
            continue;
        };

        // Compute overlap from scroll offset
        let overlap = (-scroll_offset).max(0.0);

        // Compute cache origin
        let cache_origin = if scroll_offset < 0.0 {
            scroll_offset
        } else {
            0.0
        };

        // Create constraints for this sliver
        let constraints = SliverConstraints {
            axis_direction: config.axis_direction,
            growth_direction,
            user_scroll_direction: config.user_scroll_direction,
            scroll_offset: scroll_offset.max(0.0),
            preceding_scroll_extent,
            overlap,
            remaining_paint_extent,
            cross_axis_extent: config.cross_axis_extent,
            cross_axis_direction: config.cross_axis_direction,
            viewport_main_axis_extent: config.viewport_main_axis_extent,
            remaining_cache_extent,
            cache_origin,
        };

        // Layout the sliver
        let geometry = delegate.layout_sliver_child(child_id, &constraints);

        // Check for scroll offset correction
        if let Some(correction) = geometry.scroll_offset_correction {
            result.scroll_offset_correction = Some(correction);
            result.completed = false;
            return result;
        }

        // Update totals
        result.total_scroll_extent += geometry.scroll_extent;
        result.total_paint_extent += geometry.paint_extent;
        result.max_scroll_obstruction_extent = result
            .max_scroll_obstruction_extent
            .max(geometry.max_scroll_obstruction_extent);

        // Compute paint offset for this child
        let main_axis_position = compute_child_main_axis_position(
            growth_direction,
            layout_offset,
            geometry.paint_extent,
            config.viewport_main_axis_extent,
        );

        let paint_offset = compute_paint_offset(config.axis_direction, main_axis_position, 0.0);

        delegate.set_sliver_parent_data(
            child_id,
            SliverPhysicalContainerParentData::with_offset(paint_offset),
        );

        // Update for next iteration
        scroll_offset -= geometry.scroll_extent;
        layout_offset += geometry.layout_extent;
        remaining_paint_extent -= geometry.layout_extent;
        remaining_cache_extent -= geometry.cache_extent;
        preceding_scroll_extent += geometry.scroll_extent;

        // Early exit if no more space
        if remaining_paint_extent <= 0.0 {
            break;
        }
    }

    result.completed = true;
    result
}

/// Compute the size of a viewport based on constraints
pub fn compute_viewport_size(
    axis_direction: AxisDirection,
    constraints: &flui_types::BoxConstraints,
) -> Size {
    // Viewports typically expand to fill available space
    match axis_direction.axis() {
        flui_types::layout::Axis::Vertical => Size::new(
            constraints.constrain_width(f32::INFINITY),
            constraints.constrain_height(f32::INFINITY),
        ),
        flui_types::layout::Axis::Horizontal => Size::new(
            constraints.constrain_width(f32::INFINITY),
            constraints.constrain_height(f32::INFINITY),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_physical_container_parent_data_new() {
        let data = SliverPhysicalContainerParentData::new();
        assert_eq!(data.paint_offset, Offset::ZERO);
    }

    #[test]
    fn test_sliver_physical_container_parent_data_with_offset() {
        let offset = Offset::new(10.0, 20.0);
        let data = SliverPhysicalContainerParentData::with_offset(offset);
        assert_eq!(data.paint_offset, offset);
    }

    #[test]
    fn test_compute_child_main_axis_position_forward() {
        let pos = compute_child_main_axis_position(GrowthDirection::Forward, 100.0, 50.0, 600.0);
        assert_eq!(pos, 100.0);
    }

    #[test]
    fn test_compute_child_main_axis_position_reverse() {
        let pos = compute_child_main_axis_position(GrowthDirection::Reverse, 100.0, 50.0, 600.0);
        assert_eq!(pos, 450.0); // 600 - 100 - 50
    }

    #[test]
    fn test_compute_paint_offset_vertical() {
        let offset = compute_paint_offset(AxisDirection::TopToBottom, 100.0, 20.0);
        assert_eq!(offset.dx, 20.0);
        assert_eq!(offset.dy, 100.0);
    }

    #[test]
    fn test_compute_paint_offset_horizontal() {
        let offset = compute_paint_offset(AxisDirection::LeftToRight, 100.0, 20.0);
        assert_eq!(offset.dx, 100.0);
        assert_eq!(offset.dy, 20.0);
    }

    #[test]
    fn test_viewport_layout_config_default() {
        let config = ViewportLayoutConfig::default();
        assert_eq!(config.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(config.scroll_offset, 0.0);
        assert_eq!(config.cache_extent, 250.0);
        assert_eq!(config.anchor, 0.0);
    }

    #[test]
    fn test_sliver_layout_result_default() {
        let result = SliverLayoutResult::default();
        assert_eq!(result.total_scroll_extent, 0.0);
        assert_eq!(result.total_paint_extent, 0.0);
        assert!(result.scroll_offset_correction.is_none());
        assert!(!result.completed);
    }
}
