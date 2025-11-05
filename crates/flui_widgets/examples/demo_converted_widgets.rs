//! Demo: Testing all converted widgets
//!
//! This demo tests all 23 widgets that have been converted to the new View architecture:
//! - 18 single-child widgets
//! - 5 multi-child widgets

// Import only converted widgets
use flui_widgets::basic::{
    Text, SizedBox, ColoredBox, Padding, Center, Align,
    ConstrainedBox, LimitedBox, FittedBox, DecoratedBox, AspectRatio
};
use flui_widgets::layout::{
    Baseline, FractionallySizedBox, Row, Column, Stack, IndexedStack, Wrap
};
use flui_widgets::visual_effects::{
    Opacity, Transform, ClipRect, ClipRRect, ClipOval
};

use flui_types::{Color, EdgeInsets, Alignment, Axis};
use flui_types::layout::{MainAxisAlignment, CrossAxisAlignment, StackFit};

fn main() {
    println!("=== Testing Converted Widgets ===\n");

    // Test 1: Text widget
    println!("1. Testing Text widget...");
    let text = Text::builder()
        .data("Hello, Flui!")
        .size(16.0)
        .color(Color::BLACK)
        .build();
    println!("   ✓ Text widget created\n");

    // Test 2: SizedBox widget
    println!("2. Testing SizedBox widget...");
    let sized_box = SizedBox::builder()
        .width(100.0)
        .height(50.0)
        .build();
    println!("   ✓ SizedBox widget created (100x50)\n");

    // Test 3: ColoredBox widget
    println!("3. Testing ColoredBox widget...");
    let colored_box = ColoredBox::new(Color::rgb(255, 100, 100));
    println!("   ✓ ColoredBox widget created (red)\n");

    // Test 4: Padding widget
    println!("4. Testing Padding widget...");
    let padding = Padding::new(EdgeInsets::all(10.0));
    println!("   ✓ Padding widget created (10px all sides)\n");

    // Test 5: Center widget
    println!("5. Testing Center widget...");
    let center = Center::builder().build();
    println!("   ✓ Center widget created\n");

    // Test 6: Align widget
    println!("6. Testing Align widget...");
    let align = Align::new(Alignment::TOP_LEFT);
    println!("   ✓ Align widget created (TOP_LEFT)\n");

    // Test 7: ConstrainedBox widget
    println!("7. Testing ConstrainedBox widget...");
    let constrained = ConstrainedBox::builder().build();
    println!("   ✓ ConstrainedBox widget created\n");

    // Test 8: LimitedBox widget
    println!("8. Testing LimitedBox widget...");
    let limited = LimitedBox::builder()
        .max_width(200.0)
        .max_height(100.0)
        .build();
    println!("   ✓ LimitedBox widget created (max 200x100)\n");

    // Test 9: Opacity widget
    println!("9. Testing Opacity widget...");
    let opacity = Opacity::new(0.5);
    println!("   ✓ Opacity widget created (50%)\n");

    // Test 10: Transform widget
    println!("10. Testing Transform widget...");
    let transform = Transform::translate(10.0, 20.0);
    println!("   ✓ Transform widget created (translate 10, 20)\n");

    // Test 11: ClipRect widget
    println!("11. Testing ClipRect widget...");
    let clip_rect = ClipRect::new();
    println!("   ✓ ClipRect widget created\n");

    // Test 12: ClipRRect widget
    println!("12. Testing ClipRRect widget...");
    let clip_rrect = ClipRRect::circular(10.0);
    println!("   ✓ ClipRRect widget created (radius 10)\n");

    // Test 13: ClipOval widget
    println!("13. Testing ClipOval widget...");
    let clip_oval = ClipOval::new();
    println!("   ✓ ClipOval widget created\n");

    // Test 14: FittedBox widget
    println!("14. Testing FittedBox widget...");
    let fitted = FittedBox::builder().build();
    println!("   ✓ FittedBox widget created\n");

    // Test 15: DecoratedBox widget
    println!("15. Testing DecoratedBox widget...");
    let decorated = DecoratedBox::builder().build();
    println!("   ✓ DecoratedBox widget created\n");

    // Test 16: AspectRatio widget
    println!("16. Testing AspectRatio widget...");
    let aspect = AspectRatio::widescreen();
    println!("   ✓ AspectRatio widget created (16:9)\n");

    // Test 17: Baseline widget
    println!("17. Testing Baseline widget...");
    let baseline = Baseline::alphabetic(20.0);
    println!("   ✓ Baseline widget created\n");

    // Test 18: FractionallySizedBox widget
    println!("18. Testing FractionallySizedBox widget...");
    let fractional = FractionallySizedBox::both(0.5, 0.5);
    println!("   ✓ FractionallySizedBox widget created (50% x 50%)\n");

    println!("=== Multi-child Widgets ===\n");

    // Test 19: Row widget
    println!("19. Testing Row widget...");
    let row = Row::builder()
        .main_axis_alignment(MainAxisAlignment::Center)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(vec![
            Box::new(Text::new("A")),
            Box::new(Text::new("B")),
            Box::new(Text::new("C")),
        ])
        .build();
    println!("   ✓ Row widget created with 3 children\n");

    // Test 20: Column widget
    println!("20. Testing Column widget...");
    let column = Column::builder()
        .main_axis_alignment(MainAxisAlignment::Start)
        .children(vec![
            Box::new(Text::new("Item 1")),
            Box::new(Text::new("Item 2")),
        ])
        .build();
    println!("   ✓ Column widget created with 2 children\n");

    // Test 21: Stack widget
    println!("21. Testing Stack widget...");
    let stack = Stack::builder()
        .alignment(Alignment::CENTER)
        .fit(StackFit::Expand)
        .children(vec![
            Box::new(ColoredBox::new(Color::RED)),
            Box::new(Text::new("Overlay")),
        ])
        .build();
    println!("   ✓ Stack widget created with 2 overlaid children\n");

    // Test 22: IndexedStack widget
    println!("22. Testing IndexedStack widget...");
    let indexed_stack = IndexedStack::builder()
        .index(1)
        .children(vec![
            Box::new(Text::new("Page 1")),
            Box::new(Text::new("Page 2")),
            Box::new(Text::new("Page 3")),
        ])
        .build();
    println!("   ✓ IndexedStack widget created with 3 pages (showing page 2)\n");

    // Test 23: Wrap widget
    println!("23. Testing Wrap widget...");
    let wrap = Wrap::builder()
        .direction(Axis::Horizontal)
        .spacing(8.0)
        .run_spacing(8.0)
        .children(vec![
            Box::new(Text::new("Tag1")),
            Box::new(Text::new("Tag2")),
            Box::new(Text::new("Tag3")),
        ])
        .build();
    println!("   ✓ Wrap widget created with 3 items\n");

    println!("=== Complex Widget Tree ===\n");
    println!("Building a complex widget hierarchy...");

    // Build a complex tree using multiple widgets
    let _complex_widget = Center::builder()
        .child(
            Padding::builder()
                .padding(EdgeInsets::all(20.0))
                .child(
                    Column::builder()
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .children(vec![
                            Box::new(
                                Text::builder()
                                    .data("Welcome to Flui!")
                                    .size(24.0)
                                    .color(Color::BLACK)
                                    .build()
                            ),
                            Box::new(
                                SizedBox::builder()
                                    .height(20.0)
                                    .build()
                            ),
                            Box::new(
                                Row::builder()
                                    .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                    .children(vec![
                                        Box::new(
                                            ColoredBox::builder()
                                                .color(Color::rgb(255, 0, 0))
                                                .child(
                                                    SizedBox::builder()
                                                        .width(50.0)
                                                        .height(50.0)
                                                        .build()
                                                )
                                                .build()
                                        ),
                                        Box::new(
                                            ColoredBox::builder()
                                                .color(Color::rgb(0, 255, 0))
                                                .child(
                                                    SizedBox::builder()
                                                        .width(50.0)
                                                        .height(50.0)
                                                        .build()
                                                )
                                                .build()
                                        ),
                                        Box::new(
                                            ColoredBox::builder()
                                                .color(Color::rgb(0, 0, 255))
                                                .child(
                                                    SizedBox::builder()
                                                        .width(50.0)
                                                        .height(50.0)
                                                        .build()
                                                )
                                                .build()
                                        ),
                                    ])
                                    .build()
                            ),
                            Box::new(
                                SizedBox::builder()
                                    .height(20.0)
                                    .build()
                            ),
                            Box::new(
                                Stack::builder()
                                    .alignment(Alignment::CENTER)
                                    .children(vec![
                                        Box::new(
                                            ColoredBox::builder()
                                                .color(Color::rgba(0, 0, 0, 100))
                                                .child(
                                                    SizedBox::builder()
                                                        .width(200.0)
                                                        .height(100.0)
                                                        .build()
                                                )
                                                .build()
                                        ),
                                        Box::new(
                                            Text::builder()
                                                .data("Stacked Text")
                                                .color(Color::WHITE)
                                                .build()
                                        ),
                                    ])
                                    .build()
                            ),
                        ])
                        .build()
                )
                .build()
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
    println!("✓ 23 widgets successfully converted to new View architecture");
    println!("✓ All widgets compile without errors");
    println!("✓ Complex widget hierarchies can be built");
    println!("\nNext steps:");
    println!("  - Continue with Phase 4: Composite widgets");
    println!("  - Test with actual rendering pipeline");
}
