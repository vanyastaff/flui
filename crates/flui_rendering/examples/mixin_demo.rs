//! Demonstration of the mixin-based render object architecture
//!
//! This example shows how the new Ambassador + Deref pattern makes it easy
//! to create render objects with minimal boilerplate.
//!
//! Run with: cargo run --example mixin_demo -p flui_rendering

use flui_rendering::mixins::*;
use flui_types::{BoxConstraints, Color, EdgeInsets, Offset, Size};

// ============================================================================
// Demo: How to use the mixin architecture
// ============================================================================

fn main() {
    println!("🎭 Mixin Architecture Demo\n");
    println!("This demonstrates the new ambassador-based mixin pattern");
    println!("for creating render objects with minimal boilerplate.\n");

    demo_colored_box();
    demo_opacity();
    demo_padding();
    demo_column();

    print_benefits();
}

// ============================================================================
// Example 1: LeafBox - ColoredBox (no children)
// ============================================================================

fn demo_colored_box() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 1: ColoredBox (LeafBox - no children)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    #[derive(Clone, Debug)]
    struct ColoredBoxData {
        color: Color,
    }

    // Create a LeafBox
    let mut colored_box = LeafBox::new(ColoredBoxData {
        color: Color::rgb(255, 0, 0), // Red
    });

    // Access data via Deref
    println!("Color: {:?}", colored_box.color);

    // Set geometry
    colored_box.set_size(Size::new(200.0, 100.0));
    println!("Size: {:?}", colored_box.size());

    println!("✨ Benefits:");
    println!("   - Direct field access via Deref: self.color");
    println!("   - Geometry management via Ambassador: self.set_size()");
    println!("   - Must override perform_layout() and paint()\n");
}

// ============================================================================
// Example 2: ProxyBox - Opacity (delegates to child)
// ============================================================================

fn demo_opacity() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 2: Opacity (ProxyBox - delegates to child)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    #[derive(Clone, Debug)]
    struct OpacityData {
        alpha: f32,
    }

    let mut opacity = ProxyBox::new(OpacityData { alpha: 0.5 });

    println!("Initial alpha: {}", opacity.alpha); // Deref!
    opacity.alpha = 0.7; // DerefMut!
    println!("Updated alpha: {}", opacity.alpha);

    println!("Has child: {}", opacity.has_child()); // Ambassador!
    println!("Size: {:?}", opacity.size());

    println!("✨ Benefits:");
    println!("   - Inherits default perform_layout (delegates to child)");
    println!("   - Override only paint() to apply opacity effect");
    println!("   - Child management via Ambassador\n");
}

// ============================================================================
// Example 3: ShiftedBox - Padding (offset transform)
// ============================================================================

fn demo_padding() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 3: Padding (ShiftedBox - offset transform)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    #[derive(Clone, Debug)]
    struct PaddingData {
        padding: EdgeInsets,
    }

    let mut padding = ShiftedBox::new(PaddingData {
        padding: EdgeInsets::all(16.0),
    });

    // Set child offset
    padding.set_child_offset(Offset::new(16.0, 16.0));

    println!("Padding: {:?}", padding.padding); // Deref!
    println!("Child offset: {:?}", padding.child_offset()); // Ambassador!
    println!("Has child: {}", padding.has_child());

    println!("✨ Benefits:");
    println!("   - Must override perform_layout() to compute offset");
    println!("   - paint() and hit_test() auto-apply child_offset");
    println!("   - Offset management via Ambassador\n");
}

// ============================================================================
// Example 4: ContainerBox - Column (multiple children)
// ============================================================================

fn demo_column() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 4: Column (ContainerBox - multiple children)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    use flui_foundation::RenderId;

    #[derive(Clone, Debug)]
    struct ColumnData {
        spacing: f32,
    }

    #[derive(Default, Clone, Debug)]
    struct ColumnParentData {
        offset: Offset,
    }

    let mut column = ContainerBox::<ColumnData, ColumnParentData>::new(ColumnData { spacing: 8.0 });

    // Add children
    column.children_mut().push(RenderId::new(1), ColumnParentData::default());
    column.children_mut().push(RenderId::new(2), ColumnParentData::default());
    column.children_mut().push(RenderId::new(3), ColumnParentData::default());

    println!("Child count: {}", column.child_count()); // Ambassador!
    println!("Has children: {}", column.has_children());
    println!("Spacing: {}px", column.spacing); // Deref!

    // Access children with data
    println!("\nChildren:");
    for (i, (child_id, parent_data)) in column.children().iter_with_data().enumerate() {
        println!("  {}. RenderId({:?}) - offset: {:?}", i + 1, child_id, parent_data.offset);
    }

    println!("\n✨ Benefits:");
    println!("   - Must override perform_layout() to position children");
    println!("   - paint() and hit_test() auto-iterate children");
    println!("   - Children management via Ambassador\n");
}

// ============================================================================
// Summary
// ============================================================================

fn print_benefits() {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✨ Key Benefits of Mixin Architecture:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("1. 🎯 Minimal Boilerplate");
    println!("   Define data struct + override what you need (~20 lines vs 200+)");
    println!();
    println!("2. 🤖 Automatic Trait Delegation (Ambassador)");
    println!("   - HasChild: child(), child_mut(), has_child()");
    println!("   - HasBoxGeometry: size(), set_size()");
    println!("   - HasChildren: children(), children_mut(), child_count()");
    println!("   - HasOffset: child_offset(), set_child_offset()");
    println!();
    println!("3. ✨ Clean Field Access (Deref)");
    println!("   self.alpha instead of self.data.alpha");
    println!("   self.padding instead of self.data.padding");
    println!();
    println!("4. 🛡️  Type Safety");
    println!("   Compile-time guarantees for protocols (Box vs Sliver)");
    println!("   Generic over data type T and parent data PD");
    println!();
    println!("5. 🔧 Easy to Extend");
    println!("   Override only what differs from default behavior");
    println!("   Default implementations for common patterns");
    println!();
    println!("6. 🦋 Flutter-like Patterns");
    println!("   Familiar API but with Rust idioms and zero-cost abstractions");
    println!();

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📋 Pattern Summary:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("ProxyBox<T>        → Delegates all to child (e.g., Opacity, Transform)");
    println!("ShiftedBox<T>      → Applies offset to child (e.g., Padding, Positioned)");
    println!("AligningShiftedBox → Adds alignment support (e.g., Align, Center)");
    println!("ContainerBox<T,PD> → Multiple children (e.g., Flex, Stack, Wrap)");
    println!("LeafBox<T>         → No children, paints itself (e.g., ColoredBox, Image)");
    println!();
    println!("ProxySliver<T>     → Sliver version of ProxyBox");
    println!("ShiftedSliver<T>   → Sliver version of ShiftedBox");
    println!("ContainerSliver<T> → Sliver version of ContainerBox");
    println!("LeafSliver<T>      → Sliver version of LeafBox");
    println!();
}
