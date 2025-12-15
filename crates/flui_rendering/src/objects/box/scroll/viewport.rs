//! RenderViewport - standard scrollable viewport render object.
//!
//! This module implements the standard viewport that displays a portion of its
//! sliver children based on a scroll offset. The viewport has a fixed extent
//! determined by its parent's constraints.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `RenderViewport` in `rendering/viewport.dart`.

use std::any::Any;
use std::sync::Arc;

use flui_types::prelude::AxisDirection;
use flui_types::{Axis, Offset, Rect, Size};

use crate::constraints::{BoxConstraints, GrowthDirection, SliverConstraints};
use crate::containers::Viewport;
use crate::parent_data::ParentData;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::sliver::SliverHitTestResult;
use crate::traits::{
    BoxHitTestResult, DiagnosticPropertiesBuilder, RenderBox, RenderObject, RenderSliver,
};
use crate::view::{
    CacheExtentStyle, RenderAbstractViewport, RevealedOffset, ScrollDirection, ViewportOffset,
};

// ============================================================================
// RenderViewport
// ============================================================================

/// A render object that displays a subset of its sliver children.
///
/// `RenderViewport` is the workhorse of scrollable views. It takes sliver
/// children and lays them out along a main axis, displaying only the portion
/// that falls within its viewport.
///
/// # Key Properties
///
/// - **axis_direction**: The direction in which the scroll offset increases
/// - **cross_axis_direction**: The direction perpendicular to scrolling
/// - **offset**: The current scroll position
/// - **anchor**: Where the zero scroll offset is located (0.0 = start, 1.0 = end)
/// - **cache_extent**: Extra area to keep rendered for smooth scrolling
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderViewport` class.
#[derive(Debug)]
pub struct RenderViewport {
    /// Storage for sliver children and viewport configuration.
    viewport: Viewport,

    /// Current size after layout.
    size: Size,

    /// Cached constraints from parent.
    constraints: Option<BoxConstraints>,

    /// Whether this render object is attached to a pipeline.
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

    /// Whether this render object needs compositing.
    needs_compositing: bool,

    /// Whether the center sliver has been set.
    has_center: bool,
}

// Safety: RenderViewport uses raw pointers only for parent/owner references
// which are managed by the tree structure and never dereferenced unsafely.
unsafe impl Send for RenderViewport {}
unsafe impl Sync for RenderViewport {}

impl RenderViewport {
    /// Creates a new `RenderViewport` with the given axis direction.
    ///
    /// # Arguments
    ///
    /// * `axis_direction` - The direction in which the scroll offset increases
    pub fn new(axis_direction: AxisDirection) -> Self {
        Self {
            viewport: Viewport::new(axis_direction),
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
            has_center: false,
        }
    }

    /// Creates a new `RenderViewport` with all configuration options.
    #[allow(clippy::too_many_arguments)]
    pub fn with_config(
        axis_direction: AxisDirection,
        cross_axis_direction: AxisDirection,
        offset: Arc<dyn ViewportOffset>,
        anchor: f32,
        cache_extent: f32,
        cache_extent_style: CacheExtentStyle,
    ) -> Self {
        let mut viewport = Viewport::new(axis_direction);
        viewport.set_cross_axis_direction(cross_axis_direction);
        viewport.set_offset(offset);
        viewport.set_anchor(anchor);
        viewport.set_cache_extent(cache_extent);
        viewport.set_cache_extent_style(cache_extent_style);

        Self {
            viewport,
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
            has_center: false,
        }
    }

    // ========================================================================
    // Configuration Getters/Setters
    // ========================================================================

    /// Returns the axis direction.
    pub fn axis_direction(&self) -> AxisDirection {
        self.viewport.axis_direction()
    }

    /// Sets the axis direction.
    pub fn set_axis_direction(&mut self, direction: AxisDirection) {
        if self.viewport.axis_direction() != direction {
            self.viewport.set_axis_direction(direction);
            self.mark_needs_layout();
        }
    }

    /// Returns the cross axis direction.
    pub fn cross_axis_direction(&self) -> AxisDirection {
        self.viewport.cross_axis_direction()
    }

    /// Sets the cross axis direction.
    pub fn set_cross_axis_direction(&mut self, direction: AxisDirection) {
        if self.viewport.cross_axis_direction() != direction {
            self.viewport.set_cross_axis_direction(direction);
            self.mark_needs_layout();
        }
    }

    /// Returns the scroll axis.
    pub fn axis(&self) -> Axis {
        self.viewport.axis()
    }

    /// Returns the viewport offset (scroll position controller).
    pub fn offset(&self) -> Option<&Arc<dyn ViewportOffset>> {
        self.viewport.offset()
    }

    /// Sets the viewport offset.
    pub fn set_offset(&mut self, offset: Option<Arc<dyn ViewportOffset>>) {
        if let Some(o) = offset {
            self.viewport.set_offset(o);
        } else {
            self.viewport.clear_offset();
        }
        self.mark_needs_layout();
    }

    /// Returns the anchor point (0.0 = start, 1.0 = end).
    pub fn anchor(&self) -> f32 {
        self.viewport.anchor()
    }

    /// Sets the anchor point.
    pub fn set_anchor(&mut self, anchor: f32) {
        if (self.viewport.anchor() - anchor).abs() > f32::EPSILON {
            self.viewport.set_anchor(anchor);
            self.mark_needs_layout();
        }
    }

    /// Returns the cache extent in logical pixels.
    pub fn cache_extent(&self) -> f32 {
        self.viewport.cache_extent()
    }

    /// Sets the cache extent.
    pub fn set_cache_extent(&mut self, extent: f32) {
        if (self.viewport.cache_extent() - extent).abs() > f32::EPSILON {
            self.viewport.set_cache_extent(extent);
            self.mark_needs_layout();
        }
    }

    /// Returns the cache extent style.
    pub fn cache_extent_style(&self) -> CacheExtentStyle {
        self.viewport.cache_extent_style()
    }

    /// Sets the cache extent style.
    pub fn set_cache_extent_style(&mut self, style: CacheExtentStyle) {
        if self.viewport.cache_extent_style() != style {
            self.viewport.set_cache_extent_style(style);
            self.mark_needs_layout();
        }
    }

    /// Returns the index of the center sliver.
    pub fn center_index(&self) -> Option<usize> {
        self.viewport.center_index()
    }

    /// Sets the center sliver index.
    pub fn set_center_index(&mut self, index: Option<usize>) {
        if self.viewport.center_index() != index {
            self.viewport.set_center_index(index);
            self.has_center = index.is_some();
            self.mark_needs_layout();
        }
    }

    // ========================================================================
    // Child Management
    // ========================================================================

    /// Returns the number of sliver children.
    pub fn child_count(&self) -> usize {
        self.viewport.children().len()
    }

    /// Adds a sliver child to this viewport.
    pub fn add_child(&mut self, child: Box<dyn RenderSliver>) {
        self.viewport.children_mut().push(child);
        self.mark_needs_layout();
    }

    /// Removes a sliver child at the given index.
    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn RenderSliver>> {
        if index < self.viewport.children().len() {
            let child = self.viewport.children_mut().remove_child(index);
            self.mark_needs_layout();
            Some(child)
        } else {
            None
        }
    }

    /// Clears all children.
    pub fn clear_children(&mut self) {
        self.viewport.children_mut().clear();
        self.mark_needs_layout();
    }

    // ========================================================================
    // Layout Helpers
    // ========================================================================

    /// Returns the main axis extent of the viewport.
    fn main_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Horizontal => self.size.width,
            Axis::Vertical => self.size.height,
        }
    }

    /// Returns the cross axis extent of the viewport.
    fn cross_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Horizontal => self.size.height,
            Axis::Vertical => self.size.width,
        }
    }

    /// Computes the offset for a given main axis position.
    fn compute_child_paint_offset(&self, layout_offset: f32) -> Offset {
        let main_axis_unit = match self.axis_direction() {
            AxisDirection::TopToBottom => Offset::new(0.0, 1.0),
            AxisDirection::BottomToTop => Offset::new(0.0, -1.0),
            AxisDirection::LeftToRight => Offset::new(1.0, 0.0),
            AxisDirection::RightToLeft => Offset::new(-1.0, 0.0),
        };

        // Apply layout offset along main axis
        Offset::new(
            main_axis_unit.dx * layout_offset,
            main_axis_unit.dy * layout_offset,
        )
    }

    /// Computes paint offset for a given layout offset and axis direction.
    fn compute_paint_offset_for_axis(axis_direction: AxisDirection, layout_offset: f32) -> Offset {
        let main_axis_unit = match axis_direction {
            AxisDirection::TopToBottom => Offset::new(0.0, 1.0),
            AxisDirection::BottomToTop => Offset::new(0.0, -1.0),
            AxisDirection::LeftToRight => Offset::new(1.0, 0.0),
            AxisDirection::RightToLeft => Offset::new(-1.0, 0.0),
        };

        Offset::new(
            main_axis_unit.dx * layout_offset,
            main_axis_unit.dy * layout_offset,
        )
    }

    /// Lays out sliver children and computes their paint offsets.
    fn layout_child_sequence(
        &mut self,
        constraints: &SliverConstraints,
        child_start: usize,
        child_end: usize,
        advance: i32,
        mut layout_offset: f32,
        scroll_offset: f32,
        overlap: f32,
    ) -> f32 {
        let _ = overlap; // For future use with overlap handling

        // Capture axis direction before mutable borrow
        let axis_direction = self.axis_direction();

        let mut index = child_start as i32;
        let end = child_end as i32;

        while (advance > 0 && index < end) || (advance < 0 && index > end) {
            let child_index = index as usize;

            // Get child and compute its constraints
            if let Some((child, data)) = self.viewport.children_mut().get_with_data_mut(child_index)
            {
                let child_scroll_offset = (scroll_offset - layout_offset).max(0.0);

                let child_constraints = SliverConstraints {
                    axis_direction: constraints.axis_direction,
                    growth_direction: constraints.growth_direction,
                    user_scroll_direction: constraints.user_scroll_direction,
                    scroll_offset: child_scroll_offset,
                    preceding_scroll_extent: constraints.preceding_scroll_extent,
                    overlap: 0.0,
                    remaining_paint_extent: (constraints.remaining_paint_extent - layout_offset)
                        .max(0.0),
                    cross_axis_extent: constraints.cross_axis_extent,
                    cross_axis_direction: constraints.cross_axis_direction,
                    viewport_main_axis_extent: constraints.viewport_main_axis_extent,
                    remaining_cache_extent: constraints.remaining_cache_extent,
                    cache_origin: constraints.cache_origin,
                };

                // Layout the child
                let geometry = child.perform_layout(child_constraints);

                // Update paint offset using static method
                data.paint_offset =
                    Self::compute_paint_offset_for_axis(axis_direction, layout_offset);

                // Advance layout offset
                layout_offset += geometry.layout_extent;
            }

            index += advance;
        }

        layout_offset
    }
}

// ============================================================================
// RenderObject Implementation
// ============================================================================

impl RenderObject for RenderViewport {
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

        // Attach all children
        self.viewport.children_mut().for_each_mut(|child, _data| {
            child.attach(owner);
        });
    }

    fn detach(&mut self) {
        self.owner = None;
        self.attached = false;

        // Detach all children
        self.viewport.children_mut().for_each_mut(|child, _data| {
            child.detach();
        });
    }

    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {
        // Children are managed through the Viewport container
        self.mark_needs_layout();
    }

    fn drop_child(&mut self, _child: &mut dyn RenderObject) {
        // Children are managed through the Viewport container
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
        true // Viewports are always repaint boundaries
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
        self.viewport.children().for_each(|child, _data| {
            visitor(child);
        });
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        self.viewport.children_mut().for_each_mut(|child, _data| {
            visitor(child);
        });
    }

    fn paint_bounds(&self) -> Rect {
        Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height)
    }

    fn debug_fill_properties(&self, properties: &mut DiagnosticPropertiesBuilder) {
        properties.add_string("axisDirection", format!("{:?}", self.axis_direction()));
        properties.add_string(
            "crossAxisDirection",
            format!("{:?}", self.cross_axis_direction()),
        );
        properties.add_float("anchor", self.anchor() as f64);
        properties.add_float("cacheExtent", self.cache_extent() as f64);
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

impl RenderBox for RenderViewport {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);

        // Viewports expand to fill available space
        let size = if self.axis() == Axis::Vertical {
            Size::new(
                constraints.max_width,
                constraints.constrain_height(f32::INFINITY),
            )
        } else {
            Size::new(
                constraints.constrain_width(f32::INFINITY),
                constraints.max_height,
            )
        };

        self.size = size;
        self.viewport.set_size(size);

        // Get scroll offset
        let scroll_offset = self.viewport.pixels();

        // Calculate main axis extent
        let main_axis_extent = self.main_axis_extent();
        let cross_axis_extent = self.cross_axis_extent();

        // Calculate cache extent
        let calculated_cache_extent = self.viewport.calculated_cache_extent();

        // Determine the center index (default to 0)
        let center_index = self.center_index().unwrap_or(0);

        // Create base sliver constraints
        let base_constraints = SliverConstraints {
            axis_direction: self.axis_direction(),
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: main_axis_extent,
            cross_axis_extent,
            cross_axis_direction: self.cross_axis_direction(),
            viewport_main_axis_extent: main_axis_extent,
            remaining_cache_extent: main_axis_extent + calculated_cache_extent * 2.0,
            cache_origin: -calculated_cache_extent,
        };

        // Layout slivers after center (forward)
        let child_count = self.child_count();
        if child_count > 0 {
            let _forward_extent = self.layout_child_sequence(
                &base_constraints,
                center_index,
                child_count,
                1,   // advance forward
                0.0, // start at layout offset 0
                scroll_offset,
                0.0, // no initial overlap
            );

            // Layout slivers before center (reverse) - if there's a center
            if center_index > 0 {
                let reverse_constraints = SliverConstraints {
                    growth_direction: GrowthDirection::Reverse,
                    ..base_constraints
                };

                let _reverse_extent = self.layout_child_sequence(
                    &reverse_constraints,
                    center_index - 1,
                    0,
                    -1,  // advance backward
                    0.0, // start at layout offset 0
                    scroll_offset,
                    0.0,
                );
            }
        }

        self.clear_needs_layout();
        size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
        self.viewport.set_size(size);
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Paint each visible sliver child
        self.viewport.children().for_each(|child, data| {
            let child_offset = Offset::new(
                offset.dx + data.paint_offset.dx,
                offset.dy + data.paint_offset.dy,
            );

            // Only paint if child has visible geometry
            let geometry = child.geometry();
            if geometry.visible {
                child.paint(context, child_offset);
            }
        });
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Check if position is within bounds
        if position.dx < 0.0
            || position.dy < 0.0
            || position.dx >= self.size.width
            || position.dy >= self.size.height
        {
            return false;
        }

        // Hit test children in reverse paint order (front to back)
        let mut hit = false;
        self.viewport.children().for_each_rev(|child, data| {
            if !hit {
                let child_offset = data.paint_offset;
                let local_position =
                    Offset::new(position.dx - child_offset.dx, position.dy - child_offset.dy);

                // Convert to main/cross axis for sliver hit testing
                let (main_axis_pos, cross_axis_pos) = match self.axis() {
                    Axis::Vertical => (local_position.dy, local_position.dx),
                    Axis::Horizontal => (local_position.dx, local_position.dy),
                };

                let mut sliver_result = SliverHitTestResult::new();
                if child.hit_test(&mut sliver_result, main_axis_pos, cross_axis_pos) {
                    hit = true;
                }
            }
        });

        // Viewport itself can be hit
        hit || self.hit_test_self(position)
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true // Viewport accepts hits
    }
}

// ============================================================================
// RenderAbstractViewport Implementation
// ============================================================================

impl RenderAbstractViewport for RenderViewport {
    fn get_offset_to_reveal(
        &self,
        target: &dyn RenderObject,
        alignment: f32,
        rect: Option<Rect>,
        _axis: Option<Axis>,
    ) -> RevealedOffset {
        // Get target's paint bounds or use provided rect
        let target_rect = rect.unwrap_or_else(|| target.paint_bounds());

        // For now, return a simple calculation
        // In a full implementation, this would traverse the render tree
        // to find the target's position relative to the viewport

        let main_axis_extent = self.main_axis_extent();
        let leading_scroll_offset = match self.axis() {
            Axis::Vertical => target_rect.min_y(),
            Axis::Horizontal => target_rect.min_x(),
        };

        let target_main_axis_extent = match self.axis() {
            Axis::Vertical => target_rect.height(),
            Axis::Horizontal => target_rect.width(),
        };

        // Calculate offset based on alignment
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
    fn test_render_viewport_new() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.axis_direction(), AxisDirection::TopToBottom);
        assert_eq!(viewport.axis(), Axis::Vertical);
        assert_eq!(viewport.child_count(), 0);
    }

    #[test]
    fn test_render_viewport_axis_direction() {
        let mut viewport = RenderViewport::new(AxisDirection::LeftToRight);
        assert_eq!(viewport.axis(), Axis::Horizontal);

        viewport.set_axis_direction(AxisDirection::TopToBottom);
        assert_eq!(viewport.axis(), Axis::Vertical);
        assert!(viewport.needs_layout());
    }

    #[test]
    fn test_render_viewport_anchor() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.anchor(), 0.0);

        viewport.set_anchor(0.5);
        assert_eq!(viewport.anchor(), 0.5);
        assert!(viewport.needs_layout());
    }

    #[test]
    fn test_render_viewport_cache_extent() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.set_cache_extent(500.0);
        assert_eq!(viewport.cache_extent(), 500.0);
    }

    #[test]
    fn test_render_viewport_is_repaint_boundary() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom);
        assert!(viewport.is_repaint_boundary());
    }

    #[test]
    fn test_render_viewport_layout() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);

        let constraints = BoxConstraints {
            min_width: 0.0,
            max_width: 400.0,
            min_height: 0.0,
            max_height: 800.0,
        };

        let size = viewport.perform_layout(constraints);
        assert_eq!(size.width, 400.0);
        assert_eq!(size.height, 800.0);
    }

    #[test]
    fn test_render_viewport_paint_bounds() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.set_size(Size::new(400.0, 800.0));

        let bounds = viewport.paint_bounds();
        assert_eq!(bounds.width(), 400.0);
        assert_eq!(bounds.height(), 800.0);
    }
}
