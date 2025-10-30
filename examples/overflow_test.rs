//! Overflow Test Example
//!
//! Demonstrates the overflow indicator in debug mode.
//! Resize the window to make content overflow and see:
//! - Red border around overflowing containers
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
                            .data("Debug mode: Red border + console warning")
                            .size(14.0)
                            .color(Color::rgb(244, 67, 54))
                            .build()
                            .into(),

                        SizedBox::builder().height(24.0).build().into(),

                        // Test Case 1: Fixed width container with overflowing Row
                        build_test_case(
                            "Test 1: Fixed Width Container",
                            "Container is 350px wide, content needs more space",
                            Container::builder()
                                .width(350.0)
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
                                            build_badge("Badge 1", Color::rgb(33, 150, 243)),
                                            build_badge("Badge 2", Color::rgb(76, 175, 80)),
                                            build_badge("Badge 3", Color::rgb(255, 152, 0)),
                                            build_badge("Badge 4", Color::rgb(156, 39, 176)),
                                            build_badge("Badge 5", Color::rgb(244, 67, 54)),
                                        ])
                                        .build()
                                )
                                .build()
                        ),

                        SizedBox::builder().height(20.0).build().into(),

                        // Test Case 2: Buttons in a Row
                        build_test_case(
                            "Test 2: Button Row",
                            "Multiple buttons that overflow on small screens",
                            Container::builder()
                                .width(400.0)
                                .padding(EdgeInsets::all(16.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(240, 240, 240)),
                                    border_radius: Some(BorderRadius::circular(8.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Row::builder()
                                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                                        .children(vec![
                                            Button::builder("Save")
                                                .color(Color::rgb(76, 175, 80))
                                                .build()
                                                .into(),
                                            Button::builder("Cancel")
                                                .color(Color::rgb(158, 158, 158))
                                                .build()
                                                .into(),
                                            Button::builder("Delete")
                                                .color(Color::rgb(244, 67, 54))
                                                .build()
                                                .into(),
                                        ])
                                        .build()
                                )
                                .build()
                        ),

                        SizedBox::builder().height(20.0).build().into(),

                        // Test Case 3: The Solution - Using Flexible
                        build_test_case(
                            "Solution: Using Flexible",
                            "Same content but with Flexible - no overflow!",
                            Container::builder()
                                .width(350.0)
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
    println!("    • Red border appears around container");
    println!("    • Warning printed to console");
    println!();
    println!("Instructions:");
    println!("  1. Resize window to make it smaller");
    println!("  2. Watch for red borders on containers");
    println!("  3. Check console for overflow warnings");
    println!("  4. Compare Test 1/2 (overflow) vs Solution (Flexible)");
    println!();
    println!("Note: In release builds, overflow is silently clipped");
    println!("      (no performance cost)");
    println!();

    run_app(OverflowTestApp.into_widget())
}
