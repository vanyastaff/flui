//! Demo: Testing all converted widgets
//!
//! This demo tests widgets that have been converted to the new View architecture.
//! Note: visual_effects widgets (ClipOval, ClipRRect, etc.) are not yet converted.

use flui_core::view::children::Children;
use flui_widgets::basic::{
    Align, AspectRatio, Center, ColoredBox, ConstrainedBox, DecoratedBox, FittedBox, LimitedBox,
    Padding, SizedBox, Text,
};
use flui_widgets::layout::{Column, FractionallySizedBox, IndexedStack, Row, Stack, Wrap};

use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, StackFit};
use flui_types::{Alignment, Axis, Color, EdgeInsets};

fn main() {
    println!("=== Testing Converted Widgets ===\n");

    // Test 1: Text widget
    println!("1. Testing Text widget...");
    let _text = Text::builder()
        .data("Hello, Flui!")
        .size(16.0)
        .color(Color::BLACK)
        .build();
    println!("   ✓ Text widget created\n");

    // Test 2: SizedBox widget
    println!("2. Testing SizedBox widget...");
    let _sized_box = SizedBox::builder().width(100.0).height(50.0).build();
    println!("   ✓ SizedBox widget created (100x50)\n");

    // Test 3: ColoredBox widget
    println!("3. Testing ColoredBox widget...");
    let _colored_box = ColoredBox::builder()
        .color(Color::rgb(255, 100, 100))
        .build();
    println!("   ✓ ColoredBox widget created (red)\n");

    // Test 4: Padding widget
    println!("4. Testing Padding widget...");
    let _padding = Padding::builder().padding(EdgeInsets::all(10.0)).build();
    println!("   ✓ Padding widget created (10px all sides)\n");

    // Test 5: Center widget
    println!("5. Testing Center widget...");
    let _center = Center::builder().build();
    println!("   ✓ Center widget created\n");

    // Test 6: Align widget
    println!("6. Testing Align widget...");
    let _align = Align::builder().alignment(Alignment::TOP_LEFT).build();
    println!("   ✓ Align widget created (TOP_LEFT)\n");

    // Test 7: ConstrainedBox widget
    println!("7. Testing ConstrainedBox widget...");
    let _constrained = ConstrainedBox::builder().build();
    println!("   ✓ ConstrainedBox widget created\n");

    // Test 8: LimitedBox widget
    println!("8. Testing LimitedBox widget...");
    let _limited = LimitedBox::builder()
        .max_width(200.0)
        .max_height(100.0)
        .build();
    println!("   ✓ LimitedBox widget created (max 200x100)\n");

    // Test 9: FittedBox widget
    println!("9. Testing FittedBox widget...");
    let _fitted = FittedBox::builder().build();
    println!("   ✓ FittedBox widget created\n");

    // Test 10: DecoratedBox widget
    println!("10. Testing DecoratedBox widget...");
    let _decorated = DecoratedBox::builder().build();
    println!("   ✓ DecoratedBox widget created\n");

    // Test 11: AspectRatio widget
    println!("11. Testing AspectRatio widget...");
    let _aspect = AspectRatio::widescreen();
    println!("   ✓ AspectRatio widget created (16:9)\n");

    // Test 12: FractionallySizedBox widget
    println!("12. Testing FractionallySizedBox widget...");
    let _fractional = FractionallySizedBox::builder()
        .width_factor(0.5)
        .height_factor(0.5)
        .build();
    println!("   ✓ FractionallySizedBox widget created (50% x 50%)\n");

    println!("=== Multi-child Widgets ===\n");

    // Test 13: Row widget (homogeneous children)
    println!("13. Testing Row widget...");
    let _row = Row::builder()
        .main_axis_alignment(MainAxisAlignment::Center)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(vec![Text::new("A"), Text::new("B"), Text::new("C")])
        .build();
    println!("   ✓ Row widget created with 3 children\n");

    // Test 14: Column widget (homogeneous children)
    println!("14. Testing Column widget...");
    let _column = Column::builder()
        .main_axis_alignment(MainAxisAlignment::Start)
        .children(vec![Text::new("Item 1"), Text::new("Item 2")])
        .build();
    println!("   ✓ Column widget created with 2 children\n");

    // Test 15: Stack widget (heterogeneous children using Children)
    println!("15. Testing Stack widget...");
    let mut stack_children = Children::new();
    stack_children.push(ColoredBox::builder().color(Color::RED).build());
    stack_children.push(Text::new("Overlay"));
    let _stack = Stack::builder()
        .alignment(Alignment::CENTER)
        .fit(StackFit::Expand)
        .children(stack_children)
        .build();
    println!("   ✓ Stack widget created with 2 overlaid children\n");

    // Test 16: IndexedStack widget
    println!("16. Testing IndexedStack widget...");
    let _indexed_stack = IndexedStack::builder()
        .index(1)
        .children(vec![
            Text::new("Page 1"),
            Text::new("Page 2"),
            Text::new("Page 3"),
        ])
        .build();
    println!("   ✓ IndexedStack widget created with 3 pages (showing page 2)\n");

    // Test 17: Wrap widget
    println!("17. Testing Wrap widget...");
    let _wrap = Wrap::builder()
        .direction(Axis::Horizontal)
        .spacing(8.0)
        .run_spacing(8.0)
        .children(vec![
            Text::new("Tag1"),
            Text::new("Tag2"),
            Text::new("Tag3"),
        ])
        .build();
    println!("   ✓ Wrap widget created with 3 items\n");

    println!("=== Complex Widget Tree ===\n");
    println!("Building a complex widget hierarchy...");

    // Build heterogeneous children using Children::new() and push()
    let mut row_children = Children::new();
    row_children.push(
        ColoredBox::builder()
            .color(Color::rgb(255, 0, 0))
            .child(SizedBox::builder().width(50.0).height(50.0).build())
            .build(),
    );
    row_children.push(
        ColoredBox::builder()
            .color(Color::rgb(0, 255, 0))
            .child(SizedBox::builder().width(50.0).height(50.0).build())
            .build(),
    );
    row_children.push(
        ColoredBox::builder()
            .color(Color::rgb(0, 0, 255))
            .child(SizedBox::builder().width(50.0).height(50.0).build())
            .build(),
    );

    let mut stack_children2 = Children::new();
    stack_children2.push(
        ColoredBox::builder()
            .color(Color::rgba(0, 0, 0, 100))
            .child(SizedBox::builder().width(200.0).height(100.0).build())
            .build(),
    );
    stack_children2.push(
        Text::builder()
            .data("Stacked Text")
            .color(Color::WHITE)
            .build(),
    );

    let mut column_children = Children::new();
    column_children.push(
        Text::builder()
            .data("Welcome to Flui!")
            .size(24.0)
            .color(Color::BLACK)
            .build(),
    );
    column_children.push(SizedBox::builder().height(20.0).build());
    column_children.push(
        Row::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .children(row_children)
            .build(),
    );
    column_children.push(SizedBox::builder().height(20.0).build());
    column_children.push(
        Stack::builder()
            .alignment(Alignment::CENTER)
            .children(stack_children2)
            .build(),
    );

    // Build a complex tree using multiple widgets
    let _complex_widget = Center::builder()
        .child(
            Padding::builder()
                .padding(EdgeInsets::all(20.0))
                .child(
                    Column::builder()
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .children(column_children)
                        .build(),
                )
                .build(),
        )
        .build();

    println!("✓ Complex widget tree created successfully!\n");
    println!("Tree structure:");
    println!("  Center");
    println!("    └─ Padding (20px all)");
    println!("       └─ Column");
    println!("          ├─ Text ('Welcome to Flui!', 24px)");
    println!("          ├─ SizedBox (20px height spacer)");
    println!("          ├─ Row (3 colored boxes)");
    println!("          │  ├─ ColoredBox (red, 50x50)");
    println!("          │  ├─ ColoredBox (green, 50x50)");
    println!("          │  └─ ColoredBox (blue, 50x50)");
    println!("          ├─ SizedBox (20px height spacer)");
    println!("          └─ Stack");
    println!("             ├─ ColoredBox (semi-transparent black, 200x100)");
    println!("             └─ Text ('Stacked Text', white)\n");

    println!("=== All Tests Passed! ===");
    println!("✓ 17 widgets successfully tested with new View architecture");
    println!("✓ All widgets compile without errors");
    println!("✓ Complex widget hierarchies can be built");
    println!("\nNote: visual_effects widgets (Opacity, Transform, ClipRect, etc.)");
    println!("      are not yet converted to the new architecture.");
}
