//! Demonstration of the typed RenderObject system with compile-time arity constraints
//!
//! This example shows how the new typed system provides compile-time safety for child access.

// Now we can use flui_core directly since this is in flui_core/examples/
use flui_core::typed::{
    RenderObject, RenderArity,
    LeafArity, SingleArity, MultiArity,
    LayoutCx, PaintCx,
};
use flui_types::{Size, Offset};
use flui_types::constraints::BoxConstraints;

fn main() {
    println!("=== Typed RenderObject System Demonstration ===\n");

    // Create examples of each arity type
    let mut leaf = ExampleLeafRender { size: Size::new(100.0, 50.0) };
    let mut single = ExampleSingleRender { padding: 10.0 };
    let mut multi = ExampleMultiRender { spacing: 5.0 };

    // Create contexts
    let constraints = BoxConstraints::tight(Size::new(200.0, 150.0));
    let mut leaf_cx = LayoutCx::new_with_constraints(constraints);
    let mut single_cx = LayoutCx::new_with_constraints(constraints);
    let mut multi_cx = LayoutCx::new_with_constraints(constraints);

    // Layout each type
    println!("1. LEAF ARITY (no children)");
    println!("   Arity name: {}", LeafArity::name());
    println!("   Child count constraint: {:?}", LeafArity::CHILD_COUNT);
    let leaf_size = leaf.layout(&mut leaf_cx);
    println!("   Layout result: {:?}\n", leaf_size);

    println!("2. SINGLE ARITY (exactly one child)");
    println!("   Arity name: {}", SingleArity::name());
    println!("   Child count constraint: {:?}", SingleArity::CHILD_COUNT);
    let single_size = single.layout(&mut single_cx);
    println!("   Layout result: {:?}\n", single_size);

    println!("3. MULTI ARITY (zero or more children)");
    println!("   Arity name: {}", MultiArity::name());
    println!("   Child count constraint: {:?}", MultiArity::CHILD_COUNT);
    let multi_size = multi.layout(&mut multi_cx);
    println!("   Layout result: {:?}\n", multi_size);

    println!("=== KEY BENEFITS ===");
    println!("✓ Compile-time safety: Wrong child access = compile error");
    println!("✓ Zero-cost abstractions: No Box<dyn>, no downcast_mut");
    println!("✓ Better IDE support: Compiler knows available methods");
    println!("✓ No boilerplate: No manual child count checks needed");

    println!("\n=== COMPILE-TIME ERROR EXAMPLES ===");
    println!("If you uncomment these in the source, they won't compile:");
    println!("  - leaf_cx.child()     // ERROR: method not found!");
    println!("  - single_cx.children() // ERROR: method not found!");
    println!("  - multi_cx.child()     // ERROR: method not found!");
}

// ========== Example implementations ==========

/// Example: Leaf render object (no children)
///
/// Examples: Text, Image, ColoredBox
#[derive(Debug)]
struct ExampleLeafRender {
    size: Size,
}

impl RenderObject for ExampleLeafRender {
    type Arity = LeafArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        // Leaf objects just return their size, constrained by parent
        println!("   ExampleLeafRender::layout() called");
        println!("   - Can access: cx.constraints()");
        println!("   - CANNOT access: cx.child() or cx.children() (compile error!)");

        // This works:
        let constraints = cx.constraints();

        // These would be compile errors:
        // let child = cx.child(); // ERROR!
        // let children = cx.children(); // ERROR!

        constraints.constrain(self.size)
    }

    fn paint<'a>(&self, _cx: &mut PaintCx<'a, Self>) {
        // Paint the leaf element
    }
}

/// Example: Single-child render object
///
/// Examples: Padding, Opacity, Transform
#[derive(Debug)]
struct ExampleSingleRender {
    padding: f32,
}

impl RenderObject for ExampleSingleRender {
    type Arity = SingleArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        println!("   ExampleSingleRender::layout() called");
        println!("   - Can access: cx.child()");
        println!("   - CANNOT access: cx.children() (compile error!)");

        // This works:
        // let child = cx.child();

        // This would be compile error:
        // let children = cx.children(); // ERROR!

        // For demo, just return constraints
        cx.constraints().smallest()
    }

    fn paint<'a>(&self, _cx: &mut PaintCx<'a, Self>) {
        // Paint with padding around child
    }
}

/// Example: Multi-child render object
///
/// Examples: Row, Column, Stack, Flex
#[derive(Debug)]
struct ExampleMultiRender {
    spacing: f32,
}

impl RenderObject for ExampleMultiRender {
    type Arity = MultiArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        println!("   ExampleMultiRender::layout() called");
        println!("   - Can access: cx.children(), cx.child_count()");
        println!("   - CANNOT access: cx.child() (compile error!)");

        // This works:
        let _children = cx.children();
        let count = cx.child_count();
        println!("   - Child count: {}", count);

        // This would be compile error:
        // let child = cx.child(); // ERROR!

        // For demo, just return constraints
        cx.constraints().smallest()
    }

    fn paint<'a>(&self, _cx: &mut PaintCx<'a, Self>) {
        // Paint children with spacing
    }
}
