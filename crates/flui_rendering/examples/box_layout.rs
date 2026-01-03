//! Example demonstrating BoxWrapper usage with RenderBox.
//!
//! This example shows how to:
//! 1. Implement RenderBox for a custom render object
//! 2. Wrap it with BoxWrapper for RenderTree storage
//! 3. Perform layout with constraints

use flui_rendering::{
    arity::Leaf,
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext},
    parent_data::BoxParentData,
    traits::{RenderBox, RenderObject},
    wrapper::BoxWrapper,
};
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

impl RenderBox for ColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        // Constrain preferred size to parent's constraints
        let constrained = ctx.constrain(self.preferred_size);
        self.size = constrained;
        ctx.complete_with_size(constrained);
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
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
    println!("=== BoxWrapper Example ===\n");

    // Create colored boxes
    let red_box = ColoredBox::red(100.0, 50.0);
    let green_box = ColoredBox::green(200.0, 100.0);
    let blue_box = ColoredBox::blue(150.0, 75.0);

    // Wrap them for RenderTree storage
    let mut red_wrapper = BoxWrapper::new(red_box);
    let mut green_wrapper = BoxWrapper::new(green_box);
    let mut blue_wrapper = BoxWrapper::new(blue_box);

    println!("Before layout:");
    println!("  Red:   needs_layout={}", red_wrapper.needs_layout());
    println!("  Green: needs_layout={}", green_wrapper.needs_layout());
    println!("  Blue:  needs_layout={}", blue_wrapper.needs_layout());

    // Layout with tight constraints (exact size)
    println!("\n--- Layout with tight constraints (80x40) ---");
    let tight = BoxConstraints::tight(Size::new(80.0, 40.0));
    red_wrapper.layout(tight, true);
    println!(
        "  Red after layout: size={:?}, needs_layout={}",
        red_wrapper.inner().size(),
        red_wrapper.needs_layout()
    );

    // Layout with loose constraints (max size)
    println!("\n--- Layout with loose constraints (max 300x200) ---");
    let loose = BoxConstraints::loose(Size::new(300.0, 200.0));
    green_wrapper.layout(loose, true);
    println!(
        "  Green after layout: size={:?}",
        green_wrapper.inner().size()
    );

    // Layout with bounded constraints
    println!("\n--- Layout with bounded constraints (50-100 x 25-50) ---");
    let bounded = BoxConstraints::new(50.0, 100.0, 25.0, 50.0);
    blue_wrapper.layout(bounded, true);
    println!(
        "  Blue after layout: size={:?}",
        blue_wrapper.inner().size()
    );

    // Check paint bounds
    println!("\n--- Paint bounds ---");
    println!("  Red paint_bounds:   {:?}", red_wrapper.paint_bounds());
    println!("  Green paint_bounds: {:?}", green_wrapper.paint_bounds());
    println!("  Blue paint_bounds:  {:?}", blue_wrapper.paint_bounds());

    // Demonstrate RenderObject trait
    println!("\n--- RenderObject trait ---");
    println!("  Red depth:   {}", red_wrapper.depth());
    println!(
        "  Red is_repaint_boundary: {}",
        red_wrapper.is_repaint_boundary()
    );

    // Create with repaint boundary
    let boundary_box = ColoredBox::new(50.0, 50.0, [1.0, 1.0, 0.0, 1.0]);
    let boundary_wrapper = BoxWrapper::with_repaint_boundary(boundary_box);
    println!(
        "  Boundary is_repaint_boundary: {}",
        boundary_wrapper.is_repaint_boundary()
    );

    println!("\n=== Example Complete ===");
}
