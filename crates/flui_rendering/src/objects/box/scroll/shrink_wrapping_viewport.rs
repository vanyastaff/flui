//! RenderShrinkWrappingViewport - viewport that sizes to its content.
//!
//! This module implements a viewport that shrinks to fit its sliver children
//! rather than expanding to fill available space. Useful for nested scrolling
//! scenarios and cases where the viewport size should be determined by content.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `RenderShrinkWrappingViewport` in `rendering/viewport.dart`.

use std::any::Any;
use std::sync::Arc;

use flui_types::prelude::AxisDirection;
use flui_types::{Axis, Offset, Rect, Size};

use crate::constraints::{BoxConstraints, GrowthDirection, SliverConstraints};
use crate::containers::Children;
use crate::parent_data::{ParentData, SliverParentData};
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::protocol::SliverProtocol;
use crate::traits::sliver::SliverHitTestResult;
use crate::traits::{
    BoxHitTestResult, DiagnosticPropertiesBuilder, RenderBox, RenderObject, RenderSliver,
};
use crate::view::{
    CacheExtentStyle, RenderAbstractViewport, RevealedOffset, ScrollDirection, ViewportOffset,
};

// ============================================================================
// RenderShrinkWrappingViewport
// ============================================================================

/// A viewport that sizes itself to its sliver children.
///
/// Unlike [`RenderViewport`](super::RenderViewport), which expands to fill
/// available space, `RenderShrinkWrappingViewport` shrinks to fit its content.
/// This is useful for:
///
/// - Nested scrollable views
/// - Viewports in dialogs or sheets
/// - Cases where content size should determine viewport size
///
/// # Key Differences from RenderViewport
///
/// - Uses `SliverParentData` (logical offsets) instead of `SliverPhysicalParentData`
/// - Sizes to content rather than constraints
/// - No center sliver concept (all slivers grow forward)
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderShrinkWrappingViewport` class.
#[derive(Debug)]
pub struct RenderShrinkWrappingViewport {
    /// Sliver children with logical parent data.
    children: Children<SliverProtocol, SliverParentData>,

    /// Direction of scroll offset increase.
    axis_direction: AxisDirection,

    /// Direction perpendicular to scrolling.
    cross_axis_direction: AxisDirection,

    /// Scroll position controller.
    offset: Option<Arc<dyn ViewportOffset>>,

    /// Extra area to keep rendered.
    cache_extent: f32,

    /// How to interpret cache_extent.
    cache_extent_style: CacheExtentStyle,

    /// Current size after layout.
    size: Size,

    /// Cached constraints from parent.
    constraints: Option<BoxConstraints>,

    /// Whether this render object is attached.
    attached: bool,

    /// The pipeline owner.
    owner: Option<*const PipelineOwner>,

    /// Parent render object.
    parent: Option<*const dyn RenderObject>,

    /// Depth in the render tree.
    depth: usize,

    /// Parent data set by our parent.
    parent_data: Option<Box<dyn ParentData>>,

    /// Whether layout is needed.
    needs_layout: bool,

    /// Whether paint is needed.
    needs_paint: bool,

    /// Whether compositing bits need update.
    needs_compositing_bits_update: bool,

    /// Whether compositing is needed.
    needs_compositing: bool,
}

// Safety: Uses raw pointers only for parent/owner references
unsafe impl Send for RenderShrinkWrappingViewport {}
unsafe impl Sync for RenderShrinkWrappingViewport {}

impl RenderShrinkWrappingViewport {
    /// Creates a new shrink-wrapping viewport with the given axis direction.
    pub fn new(axis_direction: AxisDirection) -> Self {
        Self {
            children: Children::new(),
            axis_direction,
            cross_axis_direction: Self::default_cross_axis(axis_direction),
            offset: None,
            cache_extent: 250.0, // Default cache extent in pixels
            cache_extent_style: CacheExtentStyle::Pixel,
            size: Size::ZERO,
            constraints: None,
            attached: false,
            owner: None,
            parent: None,
            depth: 0,
            parent_data: None,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: true,
            needs_compositing: false,
        }
    }

    /// Creates a new shrink-wrapping viewport with all configuration options.
    pub fn with_config(
        axis_direction: AxisDirection,
        cross_axis_direction: AxisDirection,
        offset: Arc<dyn ViewportOffset>,
        cache_extent: f32,
        cache_extent_style: CacheExtentStyle,
    ) -> Self {
        Self {
            children: Children::new(),
            axis_direction,
            cross_axis_direction,
            offset: Some(offset),
            cache_extent,
            cache_extent_style,
            size: Size::ZERO,
            constraints: None,
            attached: false,
            owner: None,
            parent: None,
            depth: 0,
            parent_data: None,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: true,
            needs_compositing: false,
        }
    }

    /// Returns the default cross axis direction for a given axis direction.
    fn default_cross_axis(axis_direction: AxisDirection) -> AxisDirection {
        match axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => AxisDirection::LeftToRight,
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => AxisDirection::TopToBottom,
        }
    }

    // ========================================================================
    // Configuration
    // ========================================================================

    /// Returns the axis direction.
    pub fn axis_direction(&self) -> AxisDirection {
        self.axis_direction
    }

    /// Sets the axis direction.
    pub fn set_axis_direction(&mut self, direction: AxisDirection) {
        if self.axis_direction != direction {
            self.axis_direction = direction;
            self.mark_needs_layout();
        }
    }

    /// Returns the cross axis direction.
    pub fn cross_axis_direction(&self) -> AxisDirection {
        self.cross_axis_direction
    }

    /// Sets the cross axis direction.
    pub fn set_cross_axis_direction(&mut self, direction: AxisDirection) {
        if self.cross_axis_direction != direction {
            self.cross_axis_direction = direction;
            self.mark_needs_layout();
        }
    }

    /// Returns the scroll axis.
    pub fn axis(&self) -> Axis {
        match self.axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => Axis::Vertical,
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => Axis::Horizontal,
        }
    }

    /// Returns the viewport offset.
    pub fn offset(&self) -> Option<&Arc<dyn ViewportOffset>> {
        self.offset.as_ref()
    }

    /// Sets the viewport offset.
    pub fn set_offset(&mut self, offset: Option<Arc<dyn ViewportOffset>>) {
        self.offset = offset;
        self.mark_needs_layout();
    }

    /// Returns the current scroll offset in pixels.
    pub fn pixels(&self) -> f32 {
        self.offset.as_ref().map_or(0.0, |o| o.pixels())
    }

    /// Returns the cache extent.
    pub fn cache_extent(&self) -> f32 {
        self.cache_extent
    }

    /// Sets the cache extent.
    pub fn set_cache_extent(&mut self, extent: f32) {
        if (self.cache_extent - extent).abs() > f32::EPSILON {
            self.cache_extent = extent;
            self.mark_needs_layout();
        }
    }

    /// Returns the cache extent style.
    pub fn cache_extent_style(&self) -> CacheExtentStyle {
        self.cache_extent_style
    }

    /// Sets the cache extent style.
    pub fn set_cache_extent_style(&mut self, style: CacheExtentStyle) {
        if self.cache_extent_style != style {
            self.cache_extent_style = style;
            self.mark_needs_layout();
        }
    }

    // ========================================================================
    // Child Management
    // ========================================================================

    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns the children container.
    pub fn children(&self) -> &Children<SliverProtocol, SliverParentData> {
        &self.children
    }

    /// Returns the children container mutably.
    pub fn children_mut(&mut self) -> &mut Children<SliverProtocol, SliverParentData> {
        &mut self.children
    }

    /// Adds a sliver child.
    pub fn add_child(&mut self, child: Box<dyn RenderSliver>) {
        self.children.push(child);
        self.mark_needs_layout();
    }

    /// Removes a child at the given index.
    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn RenderSliver>> {
        if index < self.children.len() {
            let child = self.children.remove_child(index);
            self.mark_needs_layout();
            Some(child)
        } else {
            None
        }
    }

    /// Clears all children.
    pub fn clear_children(&mut self) {
        self.children.clear();
        self.mark_needs_layout();
    }

    // ========================================================================
    // Layout Helpers
    // ========================================================================

    /// Returns the cross axis extent based on current size.
    fn cross_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Horizontal => self.size.height,
            Axis::Vertical => self.size.width,
        }
    }

    /// Calculates the cache extent in pixels.
    fn calculated_cache_extent(&self, main_axis_extent: f32) -> f32 {
        match self.cache_extent_style {
            CacheExtentStyle::Pixel => self.cache_extent,
            CacheExtentStyle::Viewport => self.cache_extent * main_axis_extent,
        }
    }

    /// Converts a layout offset to a paint offset.
    fn layout_offset_to_paint_offset(&self, layout_offset: f32) -> Offset {
        match self.axis_direction {
            AxisDirection::TopToBottom => Offset::new(0.0, layout_offset),
            AxisDirection::BottomToTop => Offset::new(0.0, self.size.height - layout_offset),
            AxisDirection::LeftToRight => Offset::new(layout_offset, 0.0),
            AxisDirection::RightToLeft => Offset::new(self.size.width - layout_offset, 0.0),
        }
    }
}

// ============================================================================
// RenderObject Implementation
// ============================================================================

impl RenderObject for RenderShrinkWrappingViewport {
    fn parent(&self) -> Option<&dyn RenderObject> {
        self.parent.map(|p| unsafe { &*p })
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        self.owner.map(|p| unsafe { &*p })
    }

    fn set_parent(&mut self, parent: Option<*const dyn RenderObject>) {
        self.parent = parent;
    }

    fn attach(&mut self, owner: &PipelineOwner) {
        self.owner = Some(owner as *const PipelineOwner);
        self.attached = true;

        self.children.for_each_mut(|child, _data| {
            child.attach(owner);
        });
    }

    fn detach(&mut self) {
        self.owner = None;
        self.attached = false;

        self.children.for_each_mut(|child, _data| {
            child.detach();
        });
    }

    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {
        self.mark_needs_layout();
    }

    fn drop_child(&mut self, _child: &mut dyn RenderObject) {
        self.mark_needs_layout();
    }

    fn redepth_child(&mut self, child: &mut dyn RenderObject) {
        if child.depth() <= self.depth {
            child.set_depth(self.depth + 1);
            child.redepth_children();
        }
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint
    }

    fn needs_compositing_bits_update(&self) -> bool {
        self.needs_compositing_bits_update
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint = true;
    }

    fn mark_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = true;
    }

    fn mark_needs_semantics_update(&mut self) {
        // Would mark semantics dirty
    }

    fn clear_needs_layout(&mut self) {
        self.needs_layout = false;
    }

    fn clear_needs_paint(&mut self) {
        self.needs_paint = false;
    }

    fn clear_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = false;
    }

    fn mark_parent_needs_layout(&mut self) {
        if let Some(parent) = self.parent {
            unsafe {
                let parent_mut = parent as *mut dyn RenderObject;
                (*parent_mut).mark_needs_layout();
            }
        }
    }

    fn schedule_initial_layout(&mut self) {
        self.mark_needs_layout();
    }

    fn schedule_initial_paint(&mut self) {
        self.mark_needs_paint();
    }

    fn needs_compositing(&self) -> bool {
        self.needs_compositing
    }

    fn set_needs_compositing(&mut self, value: bool) {
        self.needs_compositing = value;
    }

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_ref().map(|d| d.as_ref())
    }

    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_mut().map(|d| d.as_mut())
    }

    fn set_parent_data(&mut self, data: Box<dyn ParentData>) {
        self.parent_data = Some(data);
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        self.children.for_each(|child, _data| {
            visitor(child);
        });
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        self.children.for_each_mut(|child, _data| {
            visitor(child);
        });
    }

    fn paint_bounds(&self) -> Rect {
        Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height)
    }

    fn debug_fill_properties(&self, properties: &mut DiagnosticPropertiesBuilder) {
        properties.add_string("axisDirection", format!("{:?}", self.axis_direction));
        properties.add_string(
            "crossAxisDirection",
            format!("{:?}", self.cross_axis_direction),
        );
        properties.add_float("cacheExtent", self.cache_extent as f64);
        properties.add_int("childCount", self.child_count() as i64);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// RenderBox Implementation
// ============================================================================

impl RenderBox for RenderShrinkWrappingViewport {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);

        // Get cross axis extent from constraints
        let cross_axis_extent = match self.axis() {
            Axis::Vertical => constraints.max_width,
            Axis::Horizontal => constraints.max_height,
        };

        // Get scroll offset
        let scroll_offset = self.pixels();

        // For shrink-wrapping, we use unbounded main axis
        let viewport_main_axis_extent = f32::INFINITY;
        let cache_extent = self.calculated_cache_extent(viewport_main_axis_extent);

        // Track total extent used by slivers
        let mut total_layout_extent = 0.0_f32;
        let mut layout_offset = 0.0_f32;

        // Layout each sliver
        let child_count = self.child_count();
        for i in 0..child_count {
            if let Some((child, data)) = self.children.get_with_data_mut(i) {
                let child_scroll_offset = (scroll_offset - layout_offset).max(0.0);

                let sliver_constraints = SliverConstraints {
                    axis_direction: self.axis_direction,
                    growth_direction: GrowthDirection::Forward,
                    user_scroll_direction: ScrollDirection::Idle,
                    scroll_offset: child_scroll_offset,
                    preceding_scroll_extent: layout_offset,
                    overlap: 0.0,
                    remaining_paint_extent: f32::INFINITY,
                    cross_axis_extent,
                    cross_axis_direction: self.cross_axis_direction,
                    viewport_main_axis_extent,
                    remaining_cache_extent: f32::INFINITY,
                    cache_origin: -cache_extent,
                };

                // Layout child
                let geometry = child.perform_layout(sliver_constraints);

                // Store layout offset in parent data
                data.layout_offset = Some(layout_offset);

                // Accumulate extent
                layout_offset += geometry.layout_extent;
                total_layout_extent = total_layout_extent.max(layout_offset);
            }
        }

        // Calculate final size - shrink to content
        let main_axis_extent = total_layout_extent;
        let size = match self.axis() {
            Axis::Vertical => Size::new(
                cross_axis_extent,
                constraints.constrain_height(main_axis_extent),
            ),
            Axis::Horizontal => Size::new(
                constraints.constrain_width(main_axis_extent),
                cross_axis_extent,
            ),
        };

        self.size = size;
        self.clear_needs_layout();
        size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Paint each child at its layout offset
        self.children.for_each(|child, data| {
            if let Some(layout_offset) = data.layout_offset {
                let paint_offset = self.layout_offset_to_paint_offset(layout_offset);
                let child_offset =
                    Offset::new(offset.dx + paint_offset.dx, offset.dy + paint_offset.dy);

                // Only paint if visible
                let geometry = child.geometry();
                if geometry.visible {
                    child.paint(context, child_offset);
                }
            }
        });
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Check bounds
        if position.dx < 0.0
            || position.dy < 0.0
            || position.dx >= self.size.width
            || position.dy >= self.size.height
        {
            return false;
        }

        // Hit test children in reverse order
        let mut hit = false;
        let children_snapshot: Vec<_> = self.children.iter_with_data().collect();

        for (child, data) in children_snapshot.into_iter().rev() {
            if hit {
                break;
            }

            if let Some(layout_offset) = data.layout_offset {
                let paint_offset = self.layout_offset_to_paint_offset(layout_offset);
                let local_position =
                    Offset::new(position.dx - paint_offset.dx, position.dy - paint_offset.dy);

                let (main_axis_pos, cross_axis_pos) = match self.axis() {
                    Axis::Vertical => (local_position.dy, local_position.dx),
                    Axis::Horizontal => (local_position.dx, local_position.dy),
                };

                let mut sliver_result = SliverHitTestResult::new();
                if child.hit_test(&mut sliver_result, main_axis_pos, cross_axis_pos) {
                    hit = true;
                }
            }
        }

        hit || self.hit_test_self(position)
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

// ============================================================================
// RenderAbstractViewport Implementation
// ============================================================================

impl RenderAbstractViewport for RenderShrinkWrappingViewport {
    fn get_offset_to_reveal(
        &self,
        target: &dyn RenderObject,
        alignment: f32,
        rect: Option<Rect>,
        _axis: Option<Axis>,
    ) -> RevealedOffset {
        let target_rect = rect.unwrap_or_else(|| target.paint_bounds());

        let main_axis_extent = match self.axis() {
            Axis::Vertical => self.size.height,
            Axis::Horizontal => self.size.width,
        };

        let leading_scroll_offset = match self.axis() {
            Axis::Vertical => target_rect.min_y(),
            Axis::Horizontal => target_rect.min_x(),
        };

        let target_main_axis_extent = match self.axis() {
            Axis::Vertical => target_rect.height(),
            Axis::Horizontal => target_rect.width(),
        };

        let aligned_offset =
            leading_scroll_offset - (main_axis_extent - target_main_axis_extent) * alignment;

        RevealedOffset::new(aligned_offset, target_rect)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shrink_wrapping_viewport_new() {
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.axis_direction(), AxisDirection::TopToBottom);
        assert_eq!(viewport.axis(), Axis::Vertical);
        assert_eq!(viewport.child_count(), 0);
    }

    #[test]
    fn test_shrink_wrapping_viewport_axis() {
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::LeftToRight);
        assert_eq!(viewport.axis(), Axis::Horizontal);

        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.axis(), Axis::Vertical);
    }

    #[test]
    fn test_shrink_wrapping_viewport_cache_extent() {
        let mut viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        viewport.set_cache_extent(300.0);
        assert_eq!(viewport.cache_extent(), 300.0);
    }

    #[test]
    fn test_shrink_wrapping_viewport_is_repaint_boundary() {
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        assert!(viewport.is_repaint_boundary());
    }

    #[test]
    fn test_shrink_wrapping_viewport_layout_empty() {
        let mut viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);

        let constraints = BoxConstraints {
            min_width: 0.0,
            max_width: 400.0,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };

        let size = viewport.perform_layout(constraints);
        // With no children, should shrink to minimum
        assert_eq!(size.width, 400.0);
        assert_eq!(size.height, 0.0);
    }

    #[test]
    fn test_shrink_wrapping_viewport_default_cross_axis() {
        // Vertical scrolling should have horizontal cross axis
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.cross_axis_direction(), AxisDirection::LeftToRight);

        // Horizontal scrolling should have vertical cross axis
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::LeftToRight);
        assert_eq!(viewport.cross_axis_direction(), AxisDirection::TopToBottom);
    }
}
