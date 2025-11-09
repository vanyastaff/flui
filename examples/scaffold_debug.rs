//! Scaffold Debug - Test positioning with visible borders
//!
//! This test adds colored borders to each section to see exactly where
//! each element is positioned and if they overlap.

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Column, Container, Text};

#[derive(Debug, Clone)]
struct ScaffoldDebugApp;

impl View for ScaffoldDebugApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut main_column = Column::builder().build();

        // Top box (AppBar) - RED with height 64
        let mut top = Container::builder()
            .height(64.0)
            .color(Color::rgb(255, 0, 0)) // RED
            .padding(EdgeInsets::all(8.0))
            .build();
        top.child = Some(Box::new(
            Text::builder()
                .data("AppBar (64px RED)")
                .size(16.0)
                .color(Color::WHITE)
                .build(),
        ));
        main_column.children.push(Box::new(top));

        // Middle box (Body) - GREEN, should fill remaining space
        let mut middle = Container::builder()
            .color(Color::rgb(0, 255, 0)) // GREEN
            .padding(EdgeInsets::all(16.0))
            .build();
        middle.child = Some(Box::new(
            Text::builder()
                .data("Body (GREEN - should fill)")
                .size(16.0)
                .color(Color::BLACK)
                .build(),
        ));

        // Wrap middle in Expanded to fill space
        main_column
            .children
            .push(Box::new(flui_widgets::Expanded::new(Box::new(middle))));

        // Bottom box (BottomNav) - BLUE with height 56
        let mut bottom = Container::builder()
            .height(56.0)
            .color(Color::rgb(0, 0, 255)) // BLUE
            .padding(EdgeInsets::all(8.0))
            .build();
        bottom.child = Some(Box::new(
            Text::builder()
                .data("BottomNav (56px BLUE)")
                .size(14.0)
                .color(Color::WHITE)
                .build(),
        ));
        main_column.children.push(Box::new(bottom));

        // Wrap everything in ColoredBox with light gray background
        let mut with_background = flui_widgets::ColoredBox::builder()
            .color(Color::rgb(245, 245, 245))
            .build();
        with_background.child = Some(Box::new(main_column));

        with_background
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Scaffold Debug Test ===");
    println!("Expected layout:");
    println!("  • RED bar at top (64px)");
    println!("  • GREEN body filling middle");
    println!("  • BLUE bar at bottom (56px)");
    println!("  • Light gray background (245, 245, 245)");
    println!();
    println!("Total height should be: 64 + middle + 56 = window height");
    println!();

    run_app(Box::new(ScaffoldDebugApp))
}
