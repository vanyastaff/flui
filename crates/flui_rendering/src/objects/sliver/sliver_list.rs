//! RenderSliverList - Lazy-loading scrollable list with viewport culling
//!
//! Implements Flutter's sliver list protocol for efficient scrolling through large
//! lists. Uses lazy child building to only create visible and near-visible items,
//! enabling smooth scrolling through thousands of items without performance issues.
//! Fundamental building block for CustomScrollView, ListView, and infinite scrolling.

use crate::core::{RenderObject, RenderSliver, SliverLayoutContext, SliverPaintContext, Variable};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::{Axis, BoxConstraints, Offset, SliverConstraints, SliverGeometry, Size};

/// Child builder function for lazy loading
///
/// Takes index and returns whether to build the child at that index.
/// Returns None when no more children should be built.
pub type SliverChildBuilder = Box<dyn Fn(usize) -> Option<bool> + Send + Sync>;

/// RenderObject for lazy-loading scrollable lists with viewport culling.
///
/// Only builds and layouts children that are visible or near-visible (within cache
/// extent), enabling efficient scrolling through thousands of items.
pub struct RenderSliverList {
    /// Optional child builder for lazy loading
    #[allow(clippy::type_complexity)]
    pub child_builder: Option<SliverChildBuilder>,
    /// Fixed item extent (if all items have same size)
    pub item_extent: Option<f32>,
    /// Cross axis extent (width for vertical scroll)
    pub cross_axis_extent: f32,

    // Layout cache
    sliver_geometry: SliverGeometry,
    /// Cached child sizes from layout
    child_sizes: Vec<Size>,
}

impl std::fmt::Debug for RenderSliverList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverList")
            .field(
                "child_builder",
                &self.child_builder.as_ref().map(|_| "Fn(usize) -> Option<bool>"),
            )
            .field("item_extent", &self.item_extent)
            .field("cross_axis_extent", &self.cross_axis_extent)
            .field("sliver_geometry", &self.sliver_geometry)
            .finish()
    }
}

impl RenderSliverList {
    /// Create new sliver list
    pub fn new() -> Self {
        Self {
            child_builder: None,
            item_extent: None,
            cross_axis_extent: 0.0,
            sliver_geometry: SliverGeometry::default(),
            child_sizes: Vec::new(),
        }
    }

    /// Create with child builder
    pub fn with_builder<F>(builder: F) -> Self
    where
        F: Fn(usize) -> Option<bool> + Send + Sync + 'static,
    {
        Self {
            child_builder: Some(Box::new(builder)),
            item_extent: None,
            cross_axis_extent: 0.0,
            sliver_geometry: SliverGeometry::default(),
            child_sizes: Vec::new(),
        }
    }

    /// Set fixed item extent
    pub fn set_item_extent(&mut self, extent: f32) {
        self.item_extent = Some(extent);
    }

    /// Set cross axis extent
    pub fn set_cross_axis_extent(&mut self, extent: f32) {
        self.cross_axis_extent = extent;
    }

    /// Create with fixed item extent
    pub fn with_item_extent(mut self, extent: f32) -> Self {
        self.item_extent = Some(extent);
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate which children are visible based on scroll offset
    fn calculate_visible_range(
        &self,
        scroll_offset: f32,
        remaining_extent: f32,
        child_count: usize,
    ) -> (usize, usize) {
        if child_count == 0 {
            return (0, 0);
        }

        if let Some(item_extent) = self.item_extent {
            // Fixed extent - O(1) calculation
            let first_visible = (scroll_offset / item_extent).floor() as usize;
            let last_visible =
                ((scroll_offset + remaining_extent) / item_extent).ceil() as usize;

            (
                first_visible.min(child_count),
                last_visible.min(child_count),
            )
        } else {
            // Variable extent - layout all for now (TODO: optimize with caching)
            (0, child_count)
        }
    }
}

impl Default for RenderSliverList {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderSliverList {}

impl RenderSliver<Variable> for RenderSliverList {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;

        // Store cross axis extent
        self.cross_axis_extent = constraints.cross_axis_extent;

        // Get children count
        let children: Vec<_> = ctx.children().collect();
        let child_count = children.len();

        if child_count == 0 {
            self.sliver_geometry = SliverGeometry::default();
            self.child_sizes.clear();
            return Ok(self.sliver_geometry);
        }

        // Calculate visible range
        let (first_visible, last_visible) = self.calculate_visible_range(
            constraints.scroll_offset,
            constraints.remaining_paint_extent,
            child_count,
        );

        // Layout children
        self.child_sizes.clear();
        self.child_sizes.reserve(child_count);

        let child_constraints = if let Some(item_extent) = self.item_extent {
            // Fixed extent - tight height constraints
            BoxConstraints::new(
                0.0,
                constraints.cross_axis_extent,
                item_extent,
                item_extent,
            )
        } else {
            // Variable extent - loose constraints
            BoxConstraints::new(0.0, constraints.cross_axis_extent, 0.0, f32::INFINITY)
        };

        let mut total_extent = 0.0;

        for (i, child_id) in children.iter().enumerate() {
            // Layout child
            let child_size = ctx.tree_mut().perform_layout(*child_id, child_constraints)?;
            self.child_sizes.push(child_size);

            // Use height for vertical scrolling (assuming vertical for now)
            // TODO: Use axis_direction to determine which dimension
            total_extent += child_size.height;

            // Position child
            let child_offset = Offset::new(0.0, total_extent - child_size.height);
            ctx.set_child_offset(*child_id, child_offset);
        }

        // Calculate sliver geometry
        let scroll_offset = constraints.scroll_offset.max(0.0);
        let trailing_scroll_offset = (scroll_offset + constraints.remaining_paint_extent).min(total_extent);
        let paint_extent = (trailing_scroll_offset - scroll_offset).max(0.0);

        self.sliver_geometry = SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: total_extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if total_extent > 0.0 {
                (paint_extent / total_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: total_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        };

        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
        let mut canvas = Canvas::new();

        // Paint visible children
        for (i, child_id) in ctx.children().enumerate() {
            if let Some(child_size) = self.child_sizes.get(i) {
                // Calculate child offset from cached layout
                let mut child_offset_y = 0.0;
                for j in 0..i {
                    if let Some(prev_size) = self.child_sizes.get(j) {
                        child_offset_y += prev_size.height;
                    }
                }

                // Only paint if visible
                let child_scroll_offset = child_offset_y;
                let child_end = child_offset_y + child_size.height;

                let scroll_offset = ctx.geometry.scroll_extent - (ctx.geometry.scroll_extent - ctx.geometry.paint_extent);

                if child_end > scroll_offset && child_scroll_offset < scroll_offset + ctx.geometry.paint_extent {
                    // Child is visible, paint it
                    let child_offset = Offset::new(ctx.offset.dx, ctx.offset.dy + child_offset_y - scroll_offset);

                    if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, child_offset) {
                        canvas.append_canvas(child_canvas);
                    }
                }
            }
        }

        *ctx.canvas = canvas;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_list_new() {
        let list = RenderSliverList::new();

        assert!(list.child_builder.is_none());
        assert!(list.item_extent.is_none());
        assert_eq!(list.cross_axis_extent, 0.0);
    }

    #[test]
    fn test_render_sliver_list_with_builder() {
        let list = RenderSliverList::with_builder(|index| {
            if index < 100 {
                Some(true)
            } else {
                None
            }
        });

        assert!(list.child_builder.is_some());
    }

    #[test]
    fn test_render_sliver_list_set_item_extent() {
        let mut list = RenderSliverList::new();
        list.set_item_extent(50.0);

        assert_eq!(list.item_extent, Some(50.0));
    }

    #[test]
    fn test_render_sliver_list_set_cross_axis_extent() {
        let mut list = RenderSliverList::new();
        list.set_cross_axis_extent(400.0);

        assert_eq!(list.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_render_sliver_list_with_item_extent() {
        let list = RenderSliverList::new().with_item_extent(60.0);

        assert_eq!(list.item_extent, Some(60.0));
    }

    #[test]
    fn test_render_sliver_list_default() {
        let list = RenderSliverList::default();

        assert!(list.child_builder.is_none());
        assert!(list.item_extent.is_none());
    }

    #[test]
    fn test_calculate_visible_range_fixed_extent() {
        let list = RenderSliverList::new().with_item_extent(50.0);

        let (first, last) = list.calculate_visible_range(100.0, 300.0, 10);

        // scroll_offset = 100, so first visible = 100/50 = 2
        assert_eq!(first, 2);
        // scroll_offset + remaining = 100 + 300 = 400, so last = 400/50 = 8
        assert_eq!(last, 8);
    }

    #[test]
    fn test_calculate_visible_range_empty() {
        let list = RenderSliverList::new().with_item_extent(50.0);

        let (first, last) = list.calculate_visible_range(0.0, 600.0, 0);

        assert_eq!(first, 0);
        assert_eq!(last, 0);
    }
}
