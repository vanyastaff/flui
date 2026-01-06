//! Example demonstrating RenderBox and IntoRenderObject usage.
//!
//! This example shows how to:
//! 1. Implement RenderBox for a custom render object
//! 2. Convert it to RenderNode via IntoRenderObject for RenderTree storage
//! 3. Build a tree structure with parent-child relationships

use flui_rendering::{
    arity::Leaf,
    constraints::{BoxConstraints, Constraints},
    context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext},
    parent_data::BoxParentData,
    protocol::IntoRenderObject,
    storage::RenderTree,
    traits::RenderBox,
};
use flui_tree::TreeWrite;
use flui_types::Size;

// ============================================================================
// Custom RenderBox: ColoredBox
// ============================================================================

/// A simple colored box with a preferred size.
#[derive(Debug)]
struct ColoredBox {
    /// Preferred size (will be constrained by parent).
    preferred_size: Size,
    /// Actual size after layout.
    size: Size,
    /// Color as RGBA.
    color: [f32; 4],
}

impl ColoredBox {
    fn new(width: f32, height: f32, color: [f32; 4]) -> Self {
        Self {
            preferred_size: Size::new(width, height),
            size: Size::ZERO,
            color,
        }
    }

    fn red(width: f32, height: f32) -> Self {
        Self::new(width, height, [1.0, 0.0, 0.0, 1.0])
    }

    fn green(width: f32, height: f32) -> Self {
        Self::new(width, height, [0.0, 1.0, 0.0, 1.0])
    }

    fn blue(width: f32, height: f32) -> Self {
        Self::new(width, height, [0.0, 0.0, 1.0, 1.0])
    }
}

impl flui_foundation::Diagnosticable for ColoredBox {}

impl RenderBox for ColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        // Constrain preferred size to parent's constraints
        let constrained = ctx.constrain(self.preferred_size);
        self.size = constrained;
        ctx.complete_with_size(constrained);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Leaf, BoxParentData>) {
        // In real implementation, would draw a colored rectangle
        println!(
            "  Paint ColoredBox: size={:?}, color={:?}",
            self.size, self.color
        );
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        // Simple bounds check would go here
        false
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    println!("=== RenderBox with IntoRenderObject Example ===\n");

    // Create a RenderTree to store our render objects
    let mut tree = RenderTree::new();

    // Create colored boxes
    let red_box = ColoredBox::red(100.0, 50.0);
    let green_box = ColoredBox::green(200.0, 100.0);
    let blue_box = ColoredBox::blue(150.0, 75.0);

    println!("Created boxes:");
    println!("  Red:   preferred={:?}", red_box.preferred_size);
    println!("  Green: preferred={:?}", green_box.preferred_size);
    println!("  Blue:  preferred={:?}", blue_box.preferred_size);

    // Convert to RenderNodes via IntoRenderObject and insert into tree
    let red_node = red_box.into_render_node();
    let green_node = green_box.into_render_node();
    let blue_node = blue_box.into_render_node();

    println!("\nNode types:");
    println!("  Red is_box:   {}", red_node.is_box());
    println!("  Green is_box: {}", green_node.is_box());
    println!("  Blue is_box:  {}", blue_node.is_box());

    // Insert all nodes into tree as independent nodes
    // (Full parent-child setup requires PipelineOwner integration)
    let red_id = tree.insert(red_node);
    let green_id = tree.insert(green_node);
    let blue_id = tree.insert(blue_node);

    println!("\nTree structure:");
    println!("  Tree length: {}", tree.len());
    println!("  Red id: {:?}", red_id);
    println!("  Green id: {:?}", green_id);
    println!("  Blue id: {:?}", blue_id);

    // Demonstrate BoxConstraints
    println!("\n--- BoxConstraints examples ---");

    let tight = BoxConstraints::tight(Size::new(80.0, 40.0));
    println!("  Tight(80x40): is_tight={}", tight.is_tight());

    let loose = BoxConstraints::loose(Size::new(300.0, 200.0));
    println!(
        "  Loose(max 300x200): min=({},{}), max=({},{})",
        loose.min_width, loose.min_height, loose.max_width, loose.max_height
    );

    let bounded = BoxConstraints::new(50.0, 100.0, 25.0, 50.0);
    println!(
        "  Bounded(50-100 x 25-50): min=({},{}), max=({},{})",
        bounded.min_width, bounded.min_height, bounded.max_width, bounded.max_height
    );

    // Test constraint operations
    println!("\n--- Constraint operations ---");
    let test_size = Size::new(200.0, 200.0);
    println!("  Size to constrain: {:?}", test_size);
    println!("  Tight constrains to: {:?}", tight.constrain(test_size));
    println!("  Loose constrains to: {:?}", loose.constrain(test_size));
    println!(
        "  Bounded constrains to: {:?}",
        bounded.constrain(test_size)
    );

    println!("\n=== Example Complete ===");
}
