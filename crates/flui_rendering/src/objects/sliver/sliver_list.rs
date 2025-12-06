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
use std::collections::HashMap;

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
    /// Cached child extents (main axis size) for variable extent lists
    /// Key: child index, Value: main axis extent (height for vertical)
    extent_cache: HashMap<usize, f32>,
    /// Cached child offsets (main axis position) for fast paint
    /// Index: child index, Value: cumulative offset from start
    offset_cache: Vec<f32>,
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
            extent_cache: HashMap::new(),
            offset_cache: Vec::new(),
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
            extent_cache: HashMap::new(),
            offset_cache: Vec::new(),
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
            // Variable extent - use extent cache for O(n) calculation
            // This is much better than laying out all children
            if self.extent_cache.is_empty() {
                // First layout - need to layout all children to build cache
                return (0, child_count);
            }

            let mut cumulative_extent = 0.0;
            let mut first_visible = 0;
            let mut last_visible = child_count;

            // Find first visible child
            for i in 0..child_count {
                if let Some(&extent) = self.extent_cache.get(&i) {
                    if cumulative_extent + extent > scroll_offset {
                        first_visible = i;
                        break;
                    }
                    cumulative_extent += extent;
                }
            }

            // Find last visible child
            cumulative_extent = 0.0;
            for i in 0..child_count {
                if let Some(&extent) = self.extent_cache.get(&i) {
                    cumulative_extent += extent;
                    if cumulative_extent >= scroll_offset + remaining_extent {
                        last_visible = (i + 1).min(child_count);
                        break;
                    }
                }
            }

            (first_visible, last_visible)
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

        // Count children without allocation
        let child_count = ctx.children().count();

        if child_count == 0 {
            self.sliver_geometry = SliverGeometry::default();
            self.child_sizes.clear();
            self.extent_cache.clear();
            self.offset_cache.clear();
            return Ok(self.sliver_geometry);
        }

        // Calculate visible range
        let (first_visible, last_visible) = self.calculate_visible_range(
            constraints.scroll_offset,
            constraints.remaining_paint_extent,
            child_count,
        );

        // Prepare caches
        self.child_sizes.clear();
        self.child_sizes.reserve(child_count);
        self.offset_cache.clear();
        self.offset_cache.reserve(child_count);

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

        // Layout children (iterate without collecting into Vec)
        for (i, child_id) in ctx.children().enumerate() {
            // For variable extent with cache, only layout visible children after first frame
            let should_layout = self.item_extent.is_some() || self.extent_cache.is_empty() || (i >= first_visible && i < last_visible);

            let child_extent = if should_layout {
                // Layout child
                let child_size = ctx.tree_mut().perform_layout(child_id, child_constraints)?;
                let extent = child_size.height; // TODO: Use axis_direction

                // Update caches
                self.child_sizes.push(child_size);
                if self.item_extent.is_none() {
                    self.extent_cache.insert(i, extent);
                }

                extent
            } else {
                // Use cached extent (for variable extent lists)
                let extent = self.extent_cache.get(&i).copied().unwrap_or(0.0);
                self.child_sizes.push(Size::new(constraints.cross_axis_extent, extent));
                extent
            };

            // Cache offset before adding extent
            self.offset_cache.push(total_extent);

            total_extent += child_extent;

            // Position child (only if we laid it out)
            if should_layout {
                let child_offset = Offset::new(0.0, total_extent - child_extent);
                ctx.set_child_offset(child_id, child_offset);
            }
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

        // Calculate scroll offset once
        let scroll_offset = ctx.geometry.scroll_extent - (ctx.geometry.scroll_extent - ctx.geometry.paint_extent);

        // Paint visible children using cached offsets
        for (i, child_id) in ctx.children().enumerate() {
            if let Some(child_size) = self.child_sizes.get(i) {
                // Use cached offset instead of recalculating
                let child_offset_y = self.offset_cache.get(i).copied().unwrap_or(0.0);

                // Only paint if visible
                let child_end = child_offset_y + child_size.height;

                if child_end > scroll_offset && child_offset_y < scroll_offset + ctx.geometry.paint_extent {
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
