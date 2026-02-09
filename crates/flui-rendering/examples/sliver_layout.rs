//! Example demonstrating SliverConstraints and SliverGeometry concepts.
//!
//! This example shows how sliver layout works:
//! 1. Slivers receive SliverConstraints with scroll position and viewport info
//! 2. Slivers compute what portion is visible and space consumed
//! 3. Slivers return SliverGeometry with scroll/paint extents
//!
//! Note: Full RenderSliver implementation requires additional setup.
//! This example focuses on understanding the constraint/geometry system.

use flui_rendering::constraints::{SliverConstraints, SliverGeometry};
use flui_types::prelude::AxisDirection;

// ============================================================================
// Sliver Simulation: SliverFixedExtentList
// ============================================================================

/// Simulates a sliver that displays items with fixed extent (height in vertical scroll).
struct SliverFixedExtentList {
    /// Number of items.
    item_count: usize,
    /// Extent of each item (height for vertical, width for horizontal).
    item_extent: f32,
}

impl SliverFixedExtentList {
    fn new(item_count: usize, item_extent: f32) -> Self {
        Self {
            item_count,
            item_extent,
        }
    }

    /// Total scroll extent (all items).
    fn total_extent(&self) -> f32 {
        self.item_count as f32 * self.item_extent
    }

    /// First visible item index based on scroll offset.
    fn first_visible_index(&self, scroll_offset: f32) -> usize {
        if self.item_extent <= 0.0 {
            return 0;
        }
        (scroll_offset / self.item_extent).floor() as usize
    }

    /// Number of visible items in viewport.
    fn visible_count(&self, viewport_extent: f32) -> usize {
        if self.item_extent <= 0.0 {
            return 0;
        }
        (viewport_extent / self.item_extent).ceil() as usize + 1
    }

    /// Simulates layout with given constraints, returns geometry.
    fn compute_geometry(&self, constraints: &SliverConstraints) -> SliverGeometry {
        let total_extent = self.total_extent();
        let remaining = constraints.remaining_paint_extent;

        // How much of our content is visible
        let paint_extent = remaining
            .min(total_extent - constraints.scroll_offset)
            .max(0.0);

        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            max_paint_extent: total_extent,
            layout_extent: paint_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: total_extent > remaining,
            ..Default::default()
        }
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    println!("=== Sliver Layout Concepts Example ===\n");

    // Create a sliver list simulation with 100 items, each 50px tall
    let sliver_list = SliverFixedExtentList::new(100, 50.0);

    println!("Created sliver list simulation:");
    println!("  item_count: {}", sliver_list.item_count);
    println!("  item_extent: {}", sliver_list.item_extent);
    println!("  total_extent: {}", sliver_list.total_extent());

    // Simulate viewport: 400px wide, 600px tall, at scroll offset 0
    println!("\n--- Layout at scroll_offset=0 ---");
    let constraints = SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_extent: 400.0,
        viewport_main_axis_extent: 600.0,
        remaining_paint_extent: 600.0,
        scroll_offset: 0.0,
        ..Default::default()
    };

    let geometry = sliver_list.compute_geometry(&constraints);
    println!("  Constraints:");
    println!("    axis_direction: {:?}", constraints.axis_direction);
    println!("    cross_axis_extent: {}", constraints.cross_axis_extent);
    println!(
        "    viewport_main_axis_extent: {}",
        constraints.viewport_main_axis_extent
    );
    println!("    scroll_offset: {}", constraints.scroll_offset);
    println!("  Geometry:");
    println!("    scroll_extent: {}", geometry.scroll_extent);
    println!("    paint_extent: {}", geometry.paint_extent);
    println!("    visible: {}", geometry.visible);
    println!("    has_visual_overflow: {}", geometry.has_visual_overflow);
    let first = sliver_list.first_visible_index(constraints.scroll_offset);
    let count = sliver_list.visible_count(constraints.remaining_paint_extent);
    println!(
        "  Visible items: {}..{} of {}",
        first,
        (first + count).min(sliver_list.item_count),
        sliver_list.item_count
    );

    // Scroll down 500px
    println!("\n--- Layout at scroll_offset=500 ---");
    let constraints = SliverConstraints {
        scroll_offset: 500.0,
        ..constraints.clone()
    };

    let geometry = sliver_list.compute_geometry(&constraints);
    let first = sliver_list.first_visible_index(constraints.scroll_offset);
    let count = sliver_list.visible_count(constraints.remaining_paint_extent);
    println!("  scroll_extent: {}", geometry.scroll_extent);
    println!("  paint_extent: {}", geometry.paint_extent);
    println!(
        "  Visible items: {}..{}",
        first,
        (first + count).min(sliver_list.item_count)
    );

    // Scroll to end
    println!("\n--- Layout at scroll_offset=4500 (near end) ---");
    let constraints = SliverConstraints {
        scroll_offset: 4500.0,
        ..constraints.clone()
    };

    let geometry = sliver_list.compute_geometry(&constraints);
    let first = sliver_list.first_visible_index(constraints.scroll_offset);
    let count = sliver_list.visible_count(constraints.remaining_paint_extent);
    println!("  paint_extent: {}", geometry.paint_extent);
    println!("  has_visual_overflow: {}", geometry.has_visual_overflow);
    println!(
        "  Visible items: {}..{}",
        first,
        (first + count).min(sliver_list.item_count)
    );

    // Demonstrate SliverConstraints properties
    println!("\n--- SliverConstraints key properties ---");
    let c = SliverConstraints::default();
    println!("  Default SliverConstraints:");
    println!("    axis_direction: {:?}", c.axis_direction);
    println!("    growth_direction: {:?}", c.growth_direction);
    println!("    scroll_offset: {}", c.scroll_offset);
    println!("    overlap: {}", c.overlap);
    println!("    remaining_paint_extent: {}", c.remaining_paint_extent);
    println!("    cross_axis_extent: {}", c.cross_axis_extent);
    println!(
        "    viewport_main_axis_extent: {}",
        c.viewport_main_axis_extent
    );

    println!("\n=== Example Complete ===");
}
