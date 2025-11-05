//! Example 03: RenderObject Integration
//!
//! This example demonstrates how Views create RenderElements:
//! - Creating LeafRender, SingleRender, MultiRender objects
//! - RenderNode enum for wrapping render objects
//! - Layout and paint responsibilities
//!
//! Run with: `cargo run --example 03_render_object`

use flui_core::{BuildContext, Element};
use flui_core::view::{View, ChangeFlags};
use flui_core::render::{LeafRender, SingleRender, MultiRender, RenderNode};
use flui_core::element::{RenderElement, ElementBase, ElementTree, ElementId};
use flui_engine::{BoxedLayer, ContainerLayer};
use flui_types::{Size, Offset, constraints::BoxConstraints, Color, EdgeInsets};

// ============================================================================
// Simple LeafRender Example
// ============================================================================

/// A simple colored box render object (leaf - no children)
#[derive(Debug, Clone)]
pub struct RenderColoredBox {
    color: Color,
    width: f32,
    height: f32,
}

impl RenderColoredBox {
    pub fn new(color: Color, width: f32, height: f32) -> Self {
        Self {
            color,
            width,
            height,
        }
    }
}

impl LeafRender for RenderColoredBox {
    type Metadata = ();

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        println!("RenderColoredBox::layout() called");
        println!("  Constraints: {:?}", constraints);

        let size = Size::new(self.width, self.height);
        let constrained = constraints.constrain(size);

        println!("  Desired size: {:?}", size);
        println!("  Constrained size: {:?}", constrained);

        constrained
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        println!("RenderColoredBox::paint() called");
        println!("  Offset: {:?}", offset);
        println!("  Color: {:?}", self.color);

        // In a real implementation, this would create a layer with the colored box
        Box::new(ContainerLayer::new())
    }
}

// ============================================================================
// ColoredBox View
// ============================================================================

/// A view that creates a colored box
#[derive(Debug, Clone, PartialEq)]
pub struct ColoredBox {
    color: Color,
    width: f32,
    height: f32,
}

impl ColoredBox {
    pub fn new(color: Color, width: f32, height: f32) -> Self {
        Self {
            color,
            width,
            height,
        }
    }
}

impl View for ColoredBox {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        println!("\nColoredBox::build() called");
        println!("  Color: {:?}, Size: {}x{}", self.color, self.width, self.height);

        // 1. Create the RenderObject
        let render_object = RenderColoredBox::new(
            self.color,
            self.width,
            self.height,
        );

        // 2. Wrap in RenderNode::Leaf
        let render_node = RenderNode::new_leaf(Box::new(render_object));

        // 3. Create RenderElement
        let render_element = RenderElement::new(render_node);

        // 4. Wrap in Element enum
        let element = Element::Render(render_element);

        println!("  Created Render element with LeafRender");

        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Rebuild if color or size changed
        if self.color != prev.color || self.width != prev.width || self.height != prev.height {
            println!("ColoredBox::rebuild() - properties changed");
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD | ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT
        } else {
            println!("ColoredBox::rebuild() - no changes");
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Simple SingleRender Example (Padding)
// ============================================================================

/// A simple padding render object (single child)
#[derive(Debug, Clone)]
pub struct RenderPaddingSimple {
    padding: EdgeInsets,
}

impl RenderPaddingSimple {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl SingleRender for RenderPaddingSimple {
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        println!("RenderPaddingSimple::layout() called");
        println!("  Padding: {:?}", self.padding);
        println!("  Constraints: {:?}", constraints);

        // Deflate constraints by padding
        let child_constraints = constraints.deflate(&self.padding);
        println!("  Child constraints: {:?}", child_constraints);

        // Layout child
        let child_size = tree.layout_child(child_id, child_constraints);
        println!("  Child size: {:?}", child_size);

        // Add padding to size
        let size = Size::new(
            child_size.width + self.padding.horizontal_total(),
            child_size.height + self.padding.vertical_total(),
        );

        println!("  Final size: {:?}", size);

        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        println!("RenderPaddingSimple::paint() called");
        println!("  Offset: {:?}", offset);

        // Apply padding offset
        let child_offset = Offset::new(self.padding.left, self.padding.top);
        let final_offset = offset + child_offset;

        println!("  Child offset: {:?}", final_offset);

        tree.paint_child(child_id, final_offset)
    }
}

// ============================================================================
// Simple MultiRender Example (Column)
// ============================================================================

/// A simple vertical column render object (multiple children)
#[derive(Debug, Clone)]
pub struct RenderColumnSimple {
    spacing: f32,
}

impl RenderColumnSimple {
    pub fn new(spacing: f32) -> Self {
        Self { spacing }
    }
}

impl MultiRender for RenderColumnSimple {
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        println!("RenderColumnSimple::layout() called");
        println!("  Children count: {}", children.len());
        println!("  Spacing: {}", self.spacing);
        println!("  Constraints: {:?}", constraints);

        let mut total_height: f32 = 0.0;
        let mut max_width: f32 = 0.0;

        for (i, &child) in children.iter().enumerate() {
            println!("  Laying out child {}...", i);

            // Layout each child
            let child_size = tree.layout_child(child, constraints);

            println!("    Child {} size: {:?}", i, child_size);

            max_width = max_width.max(child_size.width);
            total_height += child_size.height;

            // Add spacing between children
            if i < children.len() - 1 {
                total_height += self.spacing;
            }
        }

        let size = constraints.constrain(Size::new(max_width, total_height));

        println!("  Final column size: {:?}", size);

        size
    }

    fn paint(&self, tree: &ElementTree, children: &[ElementId], offset: Offset) -> BoxedLayer {
        println!("RenderColumnSimple::paint() called");
        println!("  Offset: {:?}", offset);

        let mut current_y = 0.0;

        for (i, &child) in children.iter().enumerate() {
            let child_offset = Offset::new(offset.x, offset.y + current_y);

            println!("  Painting child {} at {:?}", i, child_offset);

            tree.paint_child(child, child_offset);

            // Move down for next child (simplified - assumes we know child height)
            current_y += 50.0 + self.spacing;  // Placeholder height
        }

        Box::new(ContainerLayer::new())
    }
}

// ============================================================================
// Demo Application
// ============================================================================

fn main() {
    println!("=== Example 03: RenderObject Integration ===\n");

    println!("--- Demo 1: LeafRender (Colored Box) ---");
    let colored_box = ColoredBox::new(Color::BLUE, 100.0, 50.0);
    println!("Created ColoredBox view: {:?}", colored_box);
    println!("\nThis would create:");
    println!("  1. RenderColoredBox (implements LeafRender)");
    println!("  2. RenderNode::Leaf wrapping the render object");
    println!("  3. RenderElement owning the RenderNode");
    println!("  4. Element::Render variant");

    println!("\n--- Demo 2: SingleRender (Padding) ---");
    println!("Padding pattern:");
    println!("  1. Create RenderPaddingSimple with EdgeInsets");
    println!("  2. In layout():");
    println!("     - Deflate constraints by padding");
    println!("     - Layout child with deflated constraints");
    println!("     - Add padding to child's size");
    println!("  3. In paint():");
    println!("     - Apply padding offset");
    println!("     - Paint child at adjusted position");

    println!("\n--- Demo 3: MultiRender (Column) ---");
    println!("Column pattern:");
    println!("  1. Create RenderColumnSimple with spacing");
    println!("  2. In layout():");
    println!("     - Layout each child sequentially");
    println!("     - Track max width and total height");
    println!("     - Add spacing between children");
    println!("  3. In paint():");
    println!("     - Paint each child at its position");
    println!("     - Stack vertically with spacing");

    println!("\n=== Render Object Architecture ===");
    println!("\n1. Three Render Traits:");
    println!("   • LeafRender   - No children (Text, Image, etc.)");
    println!("   • SingleRender - One child (Padding, Opacity, etc.)");
    println!("   • MultiRender  - Multiple children (Row, Column, etc.)");

    println!("\n2. RenderNode Enum:");
    println!("   • Wraps trait objects");
    println!("   • Leaf(Box<dyn LeafRender<Metadata = ()>>)");
    println!("   • Single {{ render, child }}");
    println!("   • Multi {{ render, children }}");

    println!("\n3. GAT Metadata Pattern:");
    println!("   • type Metadata: Any + Send + Sync");
    println!("   • Use () for zero-cost when not needed");
    println!("   • Use custom type for parent communication");
    println!("   • Example: FlexItemMetadata for Flex layout");

    println!("\n=== Layout Protocol ===");
    println!("1. Parent receives BoxConstraints");
    println!("2. Parent modifies constraints for child");
    println!("3. Child calls tree.layout_child()");
    println!("4. Child returns its Size");
    println!("5. Parent computes own Size");

    println!("\n=== Paint Protocol ===");
    println!("1. Parent receives Offset");
    println!("2. Parent creates BoxedLayer");
    println!("3. Parent computes child offsets");
    println!("4. Parent calls tree.paint_child()");
    println!("5. Parent returns composed layer");

    println!("\n=== Key Takeaways ===");
    println!("✅ Views create RenderElements");
    println!("✅ RenderElements own RenderNodes");
    println!("✅ RenderNodes wrap RenderObjects");
    println!("✅ Choose trait based on child count");
    println!("✅ Use Metadata = () unless needed");
    println!("✅ Cache layout results for paint");
    println!("✅ Tree provides layout_child/paint_child");

    println!("\n=== Common Patterns ===");
    println!("\nPass-through (e.g., Padding):");
    println!("  layout: deflate constraints → layout child → inflate size");
    println!("  paint: adjust offset → paint child");

    println!("\nSizing (e.g., SizedBox):");
    println!("  layout: constrain child → return fixed size");
    println!("  paint: center child → paint");

    println!("\nStacking (e.g., Column):");
    println!("  layout: layout all children → sum sizes");
    println!("  paint: paint each at calculated offset");
}
