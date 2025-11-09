//! Counter Demo - Interactive Counter with Hooks
//!
//! Demonstrates:
//! - use_signal hook for reactive state
//! - Button widget with callbacks
//! - Column layout for vertical arrangement
//! - Text widget for displaying state
//!
//! Run with: cargo run --example counter --features="flui_app,flui_widgets"

use flui_app::run_app;
use flui_core::hooks::signal::use_signal;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Button, Center, Column, Container, SizedBox, Text};

/// Counter application with interactive buttons
#[derive(Debug, Clone)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Create reactive counter state
        let count = use_signal(ctx, 0i32);

        // No clones needed! Signal is Copy

        // Build UI
        let mut center = Center::builder().build();

        // Create column with counter display and buttons
        let mut column = Column::builder()
            .main_axis_size(flui_types::layout::MainAxisSize::Min)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Center)
            .build();

        // Title
        column.children.push(Box::new(
            Text::builder()
                .data("Counter Demo")
                .size(24.0)
                .color(Color::rgb(33, 33, 33))
                .build(),
        ));

        // Spacing
        column
            .children
            .push(Box::new(SizedBox::builder().height(20.0).build()));

        // Counter display with styled background
        let mut counter_container = Container::builder()
            .padding(EdgeInsets::symmetric(32.0, 16.0))
            .color(Color::rgb(33, 150, 243))
            .build();

        // Get current count value for display (untracked read - doesn't create dependency)
        let current_count = count.get_untracked();

        counter_container.child = Some(Box::new(
            Text::builder()
                .data(format!("{}", current_count))
                .size(48.0)
                .color(Color::WHITE)
                .build(),
        ));

        column.children.push(Box::new(counter_container));

        // Spacing
        column
            .children
            .push(Box::new(SizedBox::builder().height(20.0).build()));

        // Increment button
        column.children.push(Box::new(
            Button::builder("Increment (+)")
                .color(Color::rgb(76, 175, 80))
                .on_tap(move || {
                    count.update(|n| n + 1);
                })
                .build(),
        ));

        // Spacing
        column
            .children
            .push(Box::new(SizedBox::builder().height(8.0).build()));

        // Decrement button
        column.children.push(Box::new(
            Button::builder("Decrement (-)")
                .color(Color::rgb(244, 67, 54))
                .on_tap(move || {
                    count.update(|n| n - 1);
                })
                .build(),
        ));

        // Spacing
        column
            .children
            .push(Box::new(SizedBox::builder().height(8.0).build()));

        // Reset button
        column.children.push(Box::new(
            Button::builder("Reset")
                .color(Color::rgb(158, 158, 158))
                .on_tap(move || {
                    count.set(0);
                })
                .build(),
        ));

        center.child = Some(Box::new(column));

        // Wrap in container with padding and background
        let mut container = Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(245, 245, 250))
            .build();
        container.child = Some(Box::new(center));
        container
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== FLUI Counter Demo ===");
    println!("Interactive counter with hooks!");
    println!();

    run_app(Box::new(CounterApp))
}
