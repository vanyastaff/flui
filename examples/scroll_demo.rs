//! Scroll Demo - Test SingleChildScrollView with ScrollController
//!
//! Demonstrates:
//! - SingleChildScrollView widget
//! - ScrollController for programmatic scrolling
//! - Vertical scrolling with long content
//! - Layout with infinite constraints
//! - Viewport clipping
//! - Scroll buttons for user interaction
//!
//! Run with: cargo run --example scroll_demo

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{
    Button, Column, Container, Expanded, Row, ScrollController, SingleChildScrollView, SizedBox,
    Text,
};

/// Scroll demo application with interactive controls
#[derive(Debug, Clone)]
struct ScrollDemoApp;

impl View for ScrollDemoApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create scroll controller (Arc-based, cheap to clone)
        let controller = ScrollController::new();

        // Create scroll buttons row
        let scroll_up_ctrl = controller.clone();
        let scroll_down_ctrl = controller.clone();
        let scroll_to_top_ctrl = controller.clone();
        let scroll_to_bottom_ctrl = controller.clone();

        let mut buttons_row = Row::builder().build();

        // Scroll Up button
        let up_button = Button::builder("↑ Up")
            .on_tap(move || {
                scroll_up_ctrl.scroll_by(-50.0);
            })
            .build();
        buttons_row.children.push(Box::new(up_button));

        // Scroll Down button
        let down_button = Button::builder("↓ Down")
            .on_tap(move || {
                scroll_down_ctrl.scroll_by(50.0);
            })
            .build();
        buttons_row.children.push(Box::new(down_button));

        buttons_row.children.push(Box::new(
            SizedBox::builder().width(20.0).build(), // Spacer
        ));

        // Scroll to Top button
        let top_button = Button::builder("⇈ Top")
            .on_tap(move || {
                scroll_to_top_ctrl.scroll_to_start();
            })
            .build();
        buttons_row.children.push(Box::new(top_button));

        // Scroll to Bottom button
        let bottom_button = Button::builder("⇊ Bottom")
            .on_tap(move || {
                scroll_to_bottom_ctrl.scroll_to_end();
            })
            .build();
        buttons_row.children.push(Box::new(bottom_button));

        // Wrap buttons in a container
        let mut buttons_container = Container::builder()
            .padding(EdgeInsets::all(16.0))
            .color(Color::rgb(240, 240, 240))
            .build();
        buttons_container.child = Some(Box::new(buttons_row));

        // Create scrollable content column
        let mut content_column = Column::builder()
            .main_axis_size(flui_types::layout::MainAxisSize::Min)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Stretch)
            .build();

        // Add title
        let mut title_container = Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(33, 150, 243))
            .build();

        title_container.child = Some(Box::new(
            Text::builder()
                .data("Scroll Demo - 50 Items")
                .size(24.0)
                .color(Color::WHITE)
                .build(),
        ));

        content_column.children.push(Box::new(title_container));

        // Add many items to make it scrollable
        for i in 0..50 {
            // Alternate colors
            let color = if i % 2 == 0 {
                Color::rgb(250, 250, 250)
            } else {
                Color::rgb(255, 255, 255)
            };

            let mut item_container = Container::builder()
                .padding(EdgeInsets::symmetric(16.0, 12.0))
                .color(color)
                .build();

            item_container.child = Some(Box::new(
                Text::builder()
                    .data(format!("Item #{} - Scroll with buttons above", i + 1))
                    .size(16.0)
                    .color(Color::rgb(33, 33, 33))
                    .build(),
            ));

            content_column.children.push(Box::new(item_container));

            // Add divider
            if i < 49 {
                let mut divider = SizedBox::builder().height(1.0).build();
                divider.child = Some(Box::new(
                    Container::builder()
                        .color(Color::rgb(220, 220, 220))
                        .build(),
                ));
                content_column.children.push(Box::new(divider));
            }
        }

        // Add footer
        let mut footer_container = Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(76, 175, 80))
            .build();

        footer_container.child = Some(Box::new(
            Text::builder()
                .data("🎉 End of list - You made it!")
                .size(18.0)
                .color(Color::WHITE)
                .build(),
        ));

        content_column.children.push(Box::new(footer_container));

        // Wrap content_column in SizedBox to enforce width matching viewport
        let mut sized_content = SizedBox::builder()
            .width(f32::INFINITY) // Expand to fill available width
            .build();
        sized_content.child = Some(Box::new(content_column));

        // Wrap in SingleChildScrollView with controller
        let scroll_view =
            SingleChildScrollView::vertical(sized_content).with_controller(controller);

        // Wrap scroll view in Expanded to fill remaining space
        let expanded_scroll = Expanded::new(Box::new(scroll_view));

        // Create main column with buttons and scroll view
        let mut main_column = Column::builder().build();
        main_column.children.push(Box::new(buttons_container));
        main_column.children.push(Box::new(expanded_scroll));

        main_column
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== FLUI Scroll Demo ===");
    println!("Use the buttons at the top to scroll:");
    println!("  • ↑ Up / ↓ Down - Scroll by 50 pixels");
    println!("  • ⇈ Top / ⇊ Bottom - Jump to start/end");
    println!();
    println!("You can also use mouse wheel scrolling:");
    println!("  • Scroll wheel - Scroll smoothly through content");
    println!();

    run_app(Box::new(ScrollDemoApp))
}
