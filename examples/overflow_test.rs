//! Overflow Test Example
//!
//! Demonstrates the overflow indicator in debug mode.
//! Resize the window to make content overflow and see:
//! - Yellow-black stripes around overflowing containers
//! - Console warnings with overflow details
//!
//! This example shows what happens when content doesn't fit.

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

#[derive(Debug, Clone)]
struct OverflowTestApp;

flui_core::impl_into_widget!(OverflowTestApp, stateless);

impl StatelessWidget for OverflowTestApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(250, 250, 250))
            .child(
                Column::builder()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(vec![
                        // Title
                        Text::builder()
                            .data("Overflow Indicator Test")
                            .size(28.0)
                            .color(Color::rgb(33, 33, 33))
                            .build()
                            .into(),

                        SizedBox::builder().height(12.0).build().into(),

                        Text::builder()
                            .data("Resize the window smaller to see overflow indicators!")
                            .size(16.0)
                            .color(Color::rgb(117, 117, 117))
                            .build()
                            .into(),

                        SizedBox::builder().height(8.0).build().into(),

                        Text::builder()
                            .data("Debug mode: Yellow-black stripes + console warning")
                            .size(14.0)
                            .color(Color::rgb(244, 67, 54))
                            .build()
                            .into(),

                        SizedBox::builder().height(24.0).build().into(),

                        // Test Case 1: Very narrow container that will overflow easily
                        build_test_case(
                            "Test 1: Narrow Container (200px)",
                            "Container is 200px wide, children need 600px total",
                            Container::builder()
                                .width(200.0)
                                .padding(EdgeInsets::all(16.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(240, 240, 240)),
                                    border_radius: Some(BorderRadius::circular(8.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Row::builder()
                                        .main_axis_alignment(MainAxisAlignment::Start)
                                        .children(vec![
                                            // Fixed-width boxes that CANNOT shrink
                                            SizedBox::builder().width(100.0).height(40.0)
                                                .child(ColoredBox::builder()
                                                    .color(Color::rgb(33, 150, 243))
                                                    .build())
                                                .build().into(),
                                            SizedBox::builder().width(100.0).height(40.0)
                                                .child(ColoredBox::builder()
                                                    .color(Color::rgb(76, 175, 80))
                                                    .build())
                                                .build().into(),
                                            SizedBox::builder().width(100.0).height(40.0)
                                                .child(ColoredBox::builder()
                                                    .color(Color::rgb(255, 152, 0))
                                                    .build())
                                                .build().into(),
                                            SizedBox::builder().width(100.0).height(40.0)
                                                .child(ColoredBox::builder()
                                                    .color(Color::rgb(156, 39, 176))
                                                    .build())
                                                .build().into(),
                                            SizedBox::builder().width(100.0).height(40.0)
                                                .child(ColoredBox::builder()
                                                    .color(Color::rgb(244, 67, 54))
                                                    .build())
                                                .build().into(),
                                            SizedBox::builder().width(100.0).height(40.0)
                                                .child(ColoredBox::builder()
                                                    .color(Color::rgb(96, 125, 139))
                                                    .build())
                                                .build().into(),
                                        ])
                                        .build()
                                )
                                .build()
                        ),

                        SizedBox::builder().height(20.0).build().into(),

                        // Test Case 2: Medium container with buttons
                        build_test_case(
                            "Test 2: Medium Container (250px)",
                            "Multiple buttons that definitely overflow",
                            Container::builder()
                                .width(250.0)
                                .padding(EdgeInsets::all(16.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(240, 240, 240)),
                                    border_radius: Some(BorderRadius::circular(8.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Row::builder()
                                        .main_axis_alignment(MainAxisAlignment::Start)
                                        .children(vec![
                                            Button::builder("Save Changes")
                                                .color(Color::rgb(76, 175, 80))
                                                .build()
                                                .into(),
                                            Button::builder("Cancel")
                                                .color(Color::rgb(158, 158, 158))
                                                .build()
                                                .into(),
                                            Button::builder("Delete Forever")
                                                .color(Color::rgb(244, 67, 54))
                                                .build()
                                                .into(),
                                        ])
                                        .build()
                                )
                                .build()
                        ),

                        SizedBox::builder().height(20.0).build().into(),

                        // Test Case 3: Vertical overflow
                        build_test_case(
                            "Test 3: Vertical Overflow (150px height)",
                            "Column with limited height, content is taller",
                            Container::builder()
                                .width(300.0)
                                .height(150.0)
                                .padding(EdgeInsets::all(16.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(240, 240, 240)),
                                    border_radius: Some(BorderRadius::circular(8.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Column::builder()
                                        .main_axis_alignment(MainAxisAlignment::Start)
                                        .cross_axis_alignment(CrossAxisAlignment::Start)
                                        .children(vec![
                                            Text::builder()
                                                .data("Line 1: This is some text")
                                                .size(16.0)
                                                .build()
                                                .into(),
                                            SizedBox::builder().height(12.0).build().into(),
                                            Text::builder()
                                                .data("Line 2: More content here")
                                                .size(16.0)
                                                .build()
                                                .into(),
                                            SizedBox::builder().height(12.0).build().into(),
                                            Text::builder()
                                                .data("Line 3: Even more text")
                                                .size(16.0)
                                                .build()
                                                .into(),
                                            SizedBox::builder().height(12.0).build().into(),
                                            Text::builder()
                                                .data("Line 4: This won't fit!")
                                                .size(16.0)
                                                .build()
                                                .into(),
                                            SizedBox::builder().height(12.0).build().into(),
                                            Text::builder()
                                                .data("Line 5: Definitely overflowing")
                                                .size(16.0)
                                                .build()
                                                .into(),
                                        ])
                                        .build()
                                )
                                .build()
                        ),

                        SizedBox::builder().height(20.0).build().into(),

                        // Test Case 4: The Solution - Using Flexible
                        build_test_case(
                            "Solution: Using Flexible (200px container)",
                            "Same narrow width but with Flexible - no overflow!",
                            Container::builder()
                                .width(200.0)
                                .padding(EdgeInsets::all(16.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(232, 245, 233)),
                                    border_radius: Some(BorderRadius::circular(8.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Row::builder()
                                        .main_axis_alignment(MainAxisAlignment::Start)
                                        .children(vec![
                                            Flexible::builder()
                                                .child(build_badge("Badge 1", Color::rgb(33, 150, 243)))
                                                .build()
                                                .into(),
                                            Flexible::builder()
                                                .child(build_badge("Badge 2", Color::rgb(76, 175, 80)))
                                                .build()
                                                .into(),
                                            Flexible::builder()
                                                .child(build_badge("Badge 3", Color::rgb(255, 152, 0)))
                                                .build()
                                                .into(),
                                        ])
                                        .build()
                                )
                                .build()
                        ),
                    ])
                    .build()
            )
            .build()
    }
}

/// Build a test case container
fn build_test_case(title: &str, description: &str, content: Widget) -> Widget {
    Column::builder()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(vec![
            Text::builder()
                .data(title)
                .size(18.0)
                .color(Color::rgb(33, 33, 33))
                .build()
                .into(),
            SizedBox::builder().height(4.0).build().into(),
            Text::builder()
                .data(description)
                .size(14.0)
                .color(Color::rgb(117, 117, 117))
                .build()
                .into(),
            SizedBox::builder().height(12.0).build().into(),
            content,
        ])
        .build()
}

/// Build a colored badge
fn build_badge(text: &str, color: Color) -> Widget {
    Container::builder()
        .padding(EdgeInsets::symmetric(8.0, 16.0))
        .decoration(BoxDecoration {
            color: Some(color),
            border_radius: Some(BorderRadius::circular(16.0)),
            ..Default::default()
        })
        .child(
            Text::builder()
                .data(text)
                .size(14.0)
                .color(Color::WHITE)
                .build()
        )
        .build()
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Overflow Indicator Test ===");
    println!();
    println!("⚠️  DEBUG MODE ACTIVE");
    println!("    When content overflows:");
    println!("    • Yellow-black diagonal stripes appear on edges");
    println!("    • Warning printed to console");
    println!();
    println!("Instructions:");
    println!("  1. Look for yellow-black stripes on narrow containers");
    println!("  2. Check console for overflow warnings");
    println!("  3. Test 1-3 show overflow, Test 4 shows the fix");
    println!("  4. Try resizing window to see different overflow scenarios");
    println!();
    println!("Note: In release builds, overflow is silently clipped");
    println!("      (no performance cost)");
    println!();

    run_app(OverflowTestApp.into_widget())
}
