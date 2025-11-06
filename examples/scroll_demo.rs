//! Scroll Demo - Test SingleChildScrollView
//!
//! Demonstrates:
//! - SingleChildScrollView widget
//! - Vertical scrolling with long content
//! - Layout with infinite constraints
//! - Viewport clipping
//!
//! Run with: cargo run --example scroll_demo

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Column, Container, SingleChildScrollView, SizedBox, Text};

/// Scroll demo application
#[derive(Debug, Clone)]
struct ScrollDemoApp;

impl View for ScrollDemoApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create a column with many items
        let mut column = Column::builder()
            .main_axis_size(flui_types::layout::MainAxisSize::Min)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Stretch)
            .build();

        // Add title
        let mut title_container = Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(33, 150, 243))
            .build_container();

        title_container.child = Some(Box::new(
            Text::builder()
                .data("Scroll Demo")
                .size(28.0)
                .color(Color::WHITE)
                .build(),
        ));

        column.children.push(Box::new(title_container));

        // Add many items to make it scrollable
        for i in 0..50 {
            // Alternate colors
            let color = if i % 2 == 0 {
                Color::rgb(240, 240, 240)
            } else {
                Color::WHITE
            };

            let mut item_container = Container::builder()
                .padding(EdgeInsets::symmetric(16.0, 12.0))
                .color(color)
                .build_container();

            item_container.child = Some(Box::new(
                Text::builder()
                    .data(format!("Item #{} - This is a scrollable list item", i + 1))
                    .size(16.0)
                    .color(Color::rgb(33, 33, 33))
                    .build(),
            ));

            column.children.push(Box::new(item_container));

            // Add divider
            if i < 49 {
                let mut divider = SizedBox::builder().height(1.0).build();
                divider.child = Some(Box::new(
                    Container::builder()
                        .color(Color::rgb(200, 200, 200))
                        .build_container(),
                ));
                column.children.push(Box::new(divider));
            }
        }

        // Add footer
        let mut footer_container = Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(33, 150, 243))
            .build_container();

        footer_container.child = Some(Box::new(
            Text::builder()
                .data("End of list - You scrolled all the way!")
                .size(16.0)
                .color(Color::WHITE)
                .build(),
        ));

        column.children.push(Box::new(footer_container));

        // Wrap in SingleChildScrollView
        let scroll_view = SingleChildScrollView::vertical(column);

        // Center the scroll view
        let mut center = Center::builder().build();
        center.child = Some(Box::new(scroll_view));
        center
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== FLUI Scroll Demo ===");
    println!("Vertical scrolling with 50 items!");
    println!();
    println!("⚠️  Note: Mouse wheel scrolling not yet implemented");
    println!("   This demo shows layout with infinite constraints and clipping.");
    println!();

    run_app(Box::new(ScrollDemoApp))
}
