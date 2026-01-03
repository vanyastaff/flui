//! Example demonstrating SliverWrapper usage with RenderSliver.
//!
//! This example shows how to:
//! 1. Implement RenderSliver for a custom sliver render object
//! 2. Wrap it with SliverWrapper for RenderTree storage
//! 3. Perform layout with sliver constraints (scroll-aware)

use flui_rendering::{
    arity::Leaf,
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext, SliverPaintContext},
    parent_data::SliverParentData,
    traits::{RenderObject, RenderSliver},
    wrapper::SliverWrapper,
};
use flui_types::Rect;

// ============================================================================
// Custom RenderSliver: SliverFixedExtentList
// ============================================================================

/// A sliver that displays items with fixed extent (height in vertical scroll).
#[derive(Debug)]
struct SliverFixedExtentList {
    /// Number of items.
    item_count: usize,
    /// Extent of each item (height for vertical, width for horizontal).
    item_extent: f32,
    /// Cached constraints from layout.
    constraints: SliverConstraints,
    /// Computed geometry.
    geometry: SliverGeometry,
}

impl SliverFixedExtentList {
    fn new(item_count: usize, item_extent: f32) -> Self {
        Self {
            item_count,
            item_extent,
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }

    /// Total scroll extent (all items).
    fn total_extent(&self) -> f32 {
        self.item_count as f32 * self.item_extent
    }

    /// First visible item index based on scroll offset.
    fn first_visible_index(&self) -> usize {
        if self.item_extent <= 0.0 {
            return 0;
        }
        (self.constraints.scroll_offset / self.item_extent).floor() as usize
    }

    /// Number of visible items in viewport.
    fn visible_count(&self) -> usize {
        if self.item_extent <= 0.0 {
            return 0;
        }
        let viewport = self.constraints.remaining_paint_extent;
        (viewport / self.item_extent).ceil() as usize + 1
    }
}

impl RenderSliver for SliverFixedExtentList {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, SliverParentData>) {
        self.constraints = ctx.constraints().clone();

        let total_extent = self.total_extent();
        let remaining = ctx.constraints().remaining_paint_extent;

        // How much of our content is visible
        let paint_extent = remaining
            .min(total_extent - ctx.constraints().scroll_offset)
            .max(0.0);

        let geometry = SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            max_paint_extent: total_extent,
            layout_extent: paint_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: total_extent > remaining,
            ..Default::default()
        };

        self.geometry = geometry.clone();
        ctx.complete(geometry);
    }

    fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.geometry = geometry;
    }

    fn paint(&mut self, _ctx: &mut SliverPaintContext<'_, Leaf, SliverParentData>) {
        let first = self.first_visible_index();
        let count = self.visible_count();
        println!(
            "  Paint SliverFixedExtentList: items {}..{} of {}, paint_extent={}",
            first,
            (first + count).min(self.item_count),
            self.item_count,
            self.geometry.paint_extent
        );
    }

    fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, SliverParentData>) -> bool {
        false
    }

    fn sliver_paint_bounds(&self) -> Rect {
        Rect::from_ltwh(
            0.0,
            0.0,
            self.constraints.cross_axis_extent,
            self.geometry.paint_extent,
        )
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    println!("=== SliverWrapper Example ===\n");

    // Create a sliver list with 100 items, each 50px tall
    let sliver_list = SliverFixedExtentList::new(100, 50.0);
    let mut wrapper = SliverWrapper::new(sliver_list);

    println!("Before layout:");
    println!("  needs_layout: {}", wrapper.needs_layout());
    println!("  item_count: {}", wrapper.inner().item_count);
    println!("  item_extent: {}", wrapper.inner().item_extent);
    println!("  total_extent: {}", wrapper.inner().total_extent());

    // Simulate viewport: 400px wide, 600px tall, at scroll offset 0
    println!("\n--- Layout at scroll_offset=0 ---");
    let constraints = SliverConstraints {
        cross_axis_extent: 400.0,
        viewport_main_axis_extent: 600.0,
        remaining_paint_extent: 600.0,
        scroll_offset: 0.0,
        ..Default::default()
    };
    wrapper.layout_sliver(constraints);

    println!("  needs_layout: {}", wrapper.needs_layout());
    println!("  geometry: {:?}", wrapper.inner().geometry());
    println!(
        "  visible items: {}..{}",
        wrapper.inner().first_visible_index(),
        wrapper.inner().first_visible_index() + wrapper.inner().visible_count()
    );

    // Scroll down 500px
    println!("\n--- Layout at scroll_offset=500 ---");
    let constraints = SliverConstraints {
        cross_axis_extent: 400.0,
        viewport_main_axis_extent: 600.0,
        remaining_paint_extent: 600.0,
        scroll_offset: 500.0,
        ..Default::default()
    };
    wrapper.layout_sliver(constraints);

    println!(
        "  geometry.scroll_extent: {}",
        wrapper.inner().geometry().scroll_extent
    );
    println!(
        "  geometry.paint_extent: {}",
        wrapper.inner().geometry().paint_extent
    );
    println!(
        "  visible items: {}..{}",
        wrapper.inner().first_visible_index(),
        wrapper.inner().first_visible_index() + wrapper.inner().visible_count()
    );

    // Scroll to end
    println!("\n--- Layout at scroll_offset=4500 (near end) ---");
    let constraints = SliverConstraints {
        cross_axis_extent: 400.0,
        viewport_main_axis_extent: 600.0,
        remaining_paint_extent: 600.0,
        scroll_offset: 4500.0,
        ..Default::default()
    };
    wrapper.layout_sliver(constraints);

    println!(
        "  geometry.paint_extent: {}",
        wrapper.inner().geometry().paint_extent
    );
    println!(
        "  has_visual_overflow: {}",
        wrapper.inner().geometry().has_visual_overflow
    );
    println!(
        "  visible items: {}..{}",
        wrapper.inner().first_visible_index(),
        (wrapper.inner().first_visible_index() + wrapper.inner().visible_count()).min(100)
    );

    // Check paint bounds
    println!("\n--- Paint bounds ---");
    println!("  paint_bounds: {:?}", wrapper.paint_bounds());

    // Check RenderObject trait
    println!("\n--- RenderObject trait ---");
    println!("  is_relayout_boundary: {}", wrapper.is_relayout_boundary());
    println!("  protocol: {}", wrapper.protocol_name());

    println!("\n=== Example Complete ===");
}
