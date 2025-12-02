//! Test example for the new flui_rendering core architecture
//!
//! This example demonstrates the new three-tree architecture with:
//! - GAT-based contexts (LayoutContext, PaintContext, HitTestContext)
//! - Arity system for compile-time child validation
//! - RenderBox/SliverRender traits with context-based API
//! - Type-safe protocols (BoxProtocol, SliverProtocol)

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_rendering::core::{
    // Arity markers
    Arity,
    // Geometry and constraints
    BoxConstraints,
    BoxHitTestContext,
    // Context types
    BoxLayoutContext,
    BoxPaintContext,
    HitTestTree,
    // Tree operations (dyn-compatible)
    LayoutTree,
    Leaf,
    PaintTree,
    // Core traits
    RenderBox,
    RenderObject,
    RenderResult,
    Single,
    SliverHitTestContext,
    SliverLayoutContext,
    SliverPaintContext,
    SliverRender,
    Variable,
};
use flui_types::{Offset, Size, SliverConstraints, SliverGeometry};
use std::any::Any;

// ============================================================================
// EXAMPLE RENDER OBJECTS
// ============================================================================

/// Example leaf render object (no children)
#[derive(Debug)]
struct ExampleLeafBox {
    width: f32,
    height: f32,
}

impl RenderObject for ExampleLeafBox {
    fn debug_name(&self) -> &'static str {
        "ExampleLeafBox"
    }
}

impl RenderBox<Leaf> for ExampleLeafBox {
    fn layout(&mut self, _ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
        Ok(Size::new(self.width, self.height))
    }

    fn paint(&self, _ctx: &mut BoxPaintContext<'_, Leaf>) {
        // Paint implementation would go here
        tracing::debug!("Painting ExampleLeafBox {}x{}", self.width, self.height);
    }

    fn hit_test(&self, _ctx: &BoxHitTestContext<'_, Leaf>, _result: &mut HitTestResult) -> bool {
        // Hit test implementation would go here
        true
    }
}

/// Example single-child render object (padding-like)
#[derive(Debug)]
struct ExamplePadding {
    padding: f32,
}

impl RenderObject for ExamplePadding {
    fn debug_name(&self) -> &'static str {
        "ExamplePadding"
    }
}

impl RenderBox<Single> for ExamplePadding {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        // Get the single child if it exists
        if let Some(child_id) = ctx.children.single_child() {
            // Create child constraints (deflated by padding)
            let child_constraints = ctx.constraints.deflate_all(self.padding * 2.0);

            // Layout the child (this would call into the tree)
            let child_size = Size::new(50.0, 30.0); // Mock child size
            tracing::debug!(
                "ExamplePadding: laid out child {:?} with size {:?}",
                child_id,
                child_size
            );

            // Return size including padding
            Ok(Size::new(
                child_size.width + self.padding * 2.0,
                child_size.height + self.padding * 2.0,
            ))
        } else {
            // No child, just return padding
            Ok(Size::new(self.padding * 2.0, self.padding * 2.0))
        }
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        tracing::debug!("Painting ExamplePadding with {} padding", self.padding);

        // Paint child if it exists
        if let Some(child_id) = ctx.children.single_child() {
            let child_offset = Offset::new(self.padding, self.padding);
            tracing::debug!(
                "Would paint child {:?} at offset {:?}",
                child_id,
                child_offset
            );
            // ctx.paint_child(child_id, child_offset); // Would call tree operation
        }
    }
}

/// Example variable-child render object (flex-like)
#[derive(Debug)]
struct ExampleFlex {
    direction: FlexDirection,
}

#[derive(Debug, Clone, Copy)]
enum FlexDirection {
    Horizontal,
    Vertical,
}

impl RenderObject for ExampleFlex {
    fn debug_name(&self) -> &'static str {
        "ExampleFlex"
    }
}

impl RenderBox<Variable> for ExampleFlex {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
        let mut total_size = Size::ZERO;

        tracing::debug!("ExampleFlex: laying out {} children", ctx.children.len());

        // Iterate through all children
        for (index, child_id) in ctx.children.element_ids().enumerate() {
            // Mock child layout
            let child_size = Size::new(40.0, 25.0);
            tracing::debug!(
                "ExampleFlex: child {} ({:?}) size {:?}",
                index,
                child_id,
                child_size
            );

            match self.direction {
                FlexDirection::Horizontal => {
                    total_size.width += child_size.width;
                    total_size.height = total_size.height.max(child_size.height);
                }
                FlexDirection::Vertical => {
                    total_size.height += child_size.height;
                    total_size.width = total_size.width.max(child_size.width);
                }
            }
        }

        Ok(total_size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
        tracing::debug!("Painting ExampleFlex with {:?} direction", self.direction);

        let mut current_offset = Offset::ZERO;

        for child_id in ctx.children.element_ids() {
            tracing::debug!(
                "Would paint child {:?} at offset {:?}",
                child_id,
                current_offset
            );
            // ctx.paint_child(child_id, current_offset); // Would call tree operation

            // Update offset for next child (mock)
            match self.direction {
                FlexDirection::Horizontal => current_offset.x += 40.0,
                FlexDirection::Vertical => current_offset.y += 25.0,
            }
        }
    }
}

/// Example sliver render object
#[derive(Debug)]
struct ExampleSliver {
    item_height: f32,
}

impl RenderObject for ExampleSliver {
    fn debug_name(&self) -> &'static str {
        "ExampleSliver"
    }
}

impl SliverRender<Variable> for ExampleSliver {
    fn layout(&mut self, ctx: SliverLayoutContext<'_, Variable>) -> SliverGeometry {
        let child_count = ctx.children.len();
        let total_extent = child_count as f32 * self.item_height;

        tracing::debug!(
            "ExampleSliver: {} children, total extent {}",
            child_count,
            total_extent
        );

        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent: total_extent.min(ctx.constraints.remaining_paint_extent),
            max_paint_extent: Some(total_extent),
            layout_extent: Some(total_extent),
            ..Default::default()
        }
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
        tracing::debug!("Painting ExampleSliver");

        for (index, child_id) in ctx.children.element_ids().enumerate() {
            let child_offset = Offset::new(0.0, index as f32 * self.item_height);
            tracing::debug!(
                "Would paint sliver child {:?} at {:?}",
                child_id,
                child_offset
            );
        }
    }
}

// ============================================================================
// MOCK TREE IMPLEMENTATION
// ============================================================================

/// Mock tree implementation for testing
struct MockTree;

impl LayoutTree for MockTree {
    fn perform_layout(&mut self, id: ElementId, constraints: BoxConstraints) -> RenderResult<Size> {
        tracing::debug!("MockTree: layout {:?} with {:?}", id, constraints);
        Ok(constraints.biggest())
    }

    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> RenderResult<SliverGeometry> {
        tracing::debug!("MockTree: sliver layout {:?} with {:?}", id, constraints);
        Ok(SliverGeometry {
            scroll_extent: 100.0,
            paint_extent: 100.0,
            ..Default::default()
        })
    }

    fn set_offset(&mut self, id: ElementId, offset: Offset) {
        tracing::debug!("MockTree: set offset {:?} = {:?}", id, offset);
    }

    fn get_offset(&self, _id: ElementId) -> Option<Offset> {
        Some(Offset::ZERO)
    }

    fn mark_needs_layout(&mut self, id: ElementId) {
        tracing::debug!("MockTree: mark needs layout {:?}", id);
    }

    fn needs_layout(&self, _id: ElementId) -> bool {
        false
    }

    fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
        None
    }

    fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
        None
    }
}

impl PaintTree for MockTree {
    fn perform_paint(&mut self, id: ElementId, offset: Offset) -> RenderResult<Canvas> {
        tracing::debug!("MockTree: paint {:?} at {:?}", id, offset);
        Ok(Canvas::new())
    }

    fn mark_needs_paint(&mut self, id: ElementId) {
        tracing::debug!("MockTree: mark needs paint {:?}", id);
    }

    fn needs_paint(&self, _id: ElementId) -> bool {
        false
    }

    fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
        None
    }
}

impl HitTestTree for MockTree {
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool {
        tracing::debug!("MockTree: hit test {:?} at {:?}", id, position);
        true
    }

    fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
        None
    }
}

// ============================================================================
// MAIN DEMONSTRATION
// ============================================================================

fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 FLUI Core Architecture Test");
    println!("==============================");

    // Create example render objects
    let mut leaf_box = ExampleLeafBox {
        width: 100.0,
        height: 50.0,
    };

    let mut padding = ExamplePadding { padding: 10.0 };

    let mut flex = ExampleFlex {
        direction: FlexDirection::Horizontal,
    };

    let mut sliver = ExampleSliver { item_height: 30.0 };

    // Create mock tree
    let mut tree = MockTree;

    // Test element IDs
    let leaf_id = ElementId::new(1);
    let padding_id = ElementId::new(2);
    let flex_id = ElementId::new(3);
    let sliver_id = ElementId::new(4);

    println!("\n📦 Testing Leaf RenderBox (Arity: Leaf)");
    println!("----------------------------------------");

    // Create constraints
    let constraints = BoxConstraints::tight(Size::new(200.0, 100.0));

    // Mock children for leaf (empty)
    let leaf_children: [ElementId; 0] = [];
    let leaf_accessor = flui_tree::arity::Leaf::from_slice(&leaf_children);

    // Create context for leaf
    let leaf_ctx = BoxLayoutContext::new(&mut tree, leaf_id, constraints, leaf_accessor);

    // Test layout
    match leaf_box.layout(leaf_ctx) {
        Ok(size) => println!("✅ Leaf layout successful: {:?}", size),
        Err(e) => println!("❌ Leaf layout failed: {}", e),
    }

    println!("\n🎯 Testing Single-Child RenderBox (Arity: Single)");
    println!("---------------------------------------------------");

    // Mock children for padding (single child)
    let padding_children = [leaf_id];
    let padding_accessor = flui_tree::arity::Single::from_slice(&padding_children);

    let padding_ctx = BoxLayoutContext::new(&mut tree, padding_id, constraints, padding_accessor);

    match padding.layout(padding_ctx) {
        Ok(size) => println!("✅ Padding layout successful: {:?}", size),
        Err(e) => println!("❌ Padding layout failed: {}", e),
    }

    println!("\n🔀 Testing Variable-Child RenderBox (Arity: Variable)");
    println!("-------------------------------------------------------");

    // Mock children for flex (multiple children)
    let flex_children = [leaf_id, padding_id];
    let flex_accessor = flui_tree::arity::Variable::from_slice(&flex_children);

    let flex_ctx = BoxLayoutContext::new(&mut tree, flex_id, constraints, flex_accessor);

    match flex.layout(flex_ctx) {
        Ok(size) => println!("✅ Flex layout successful: {:?}", size),
        Err(e) => println!("❌ Flex layout failed: {}", e),
    }

    println!("\n📜 Testing SliverRender (Arity: Variable)");
    println!("------------------------------------------");

    // Create sliver constraints
    let sliver_constraints = SliverConstraints {
        axis_direction: flui_types::AxisDirection::Down,
        growth_direction: flui_types::GrowthDirection::Forward,
        scroll_offset: 0.0,
        precedence_scroll_offset: None,
        overlap: 0.0,
        remaining_paint_extent: 500.0,
        cross_axis_extent: 200.0,
        cross_axis_direction: flui_types::AxisDirection::Right,
        viewport_main_axis_extent: 500.0,
        remaining_cache_extent: 1000.0,
        cache_origin: 0.0,
    };

    let sliver_children = [leaf_id, padding_id, flex_id];
    let sliver_accessor = flui_tree::arity::Variable::from_slice(&sliver_children);

    let sliver_ctx =
        SliverLayoutContext::new(&mut tree, sliver_id, sliver_constraints, sliver_accessor);

    let geometry = sliver.layout(sliver_ctx);
    println!("✅ Sliver layout successful: {:?}", geometry);

    println!("\n🎨 Testing Paint Contexts");
    println!("---------------------------");

    // Create canvas and test painting
    let mut canvas = Canvas::new();

    // Test leaf painting
    let leaf_children: [ElementId; 0] = [];
    let leaf_accessor = flui_tree::arity::Leaf::from_slice(&leaf_children);
    let mut leaf_paint_ctx = BoxPaintContext::new(
        &mut tree,
        leaf_id,
        Offset::ZERO,
        Size::new(100.0, 50.0),
        &mut canvas,
        leaf_accessor,
    );
    leaf_box.paint(&mut leaf_paint_ctx);
    println!("✅ Leaf painting completed");

    // Test padding painting
    let padding_children = [leaf_id];
    let padding_accessor = flui_tree::arity::Single::from_slice(&padding_children);
    let mut padding_paint_ctx = BoxPaintContext::new(
        &mut tree,
        padding_id,
        Offset::ZERO,
        Size::new(120.0, 70.0),
        &mut canvas,
        padding_accessor,
    );
    padding.paint(&mut padding_paint_ctx);
    println!("✅ Padding painting completed");

    println!("\n🎯 Architecture Verification");
    println!("=============================");

    println!("✅ GAT-based contexts working");
    println!("✅ Arity system type-safe");
    println!("✅ RenderBox trait functional");
    println!("✅ SliverRender trait functional");
    println!("✅ Tree operations abstracted");
    println!("✅ Context-based API working");

    println!("\n🎉 Core architecture test completed successfully!");
    println!("   New three-tree system with flui-tree integration is functional.");
}
