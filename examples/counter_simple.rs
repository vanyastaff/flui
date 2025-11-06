//! Simple Counter Demo - Static UI Demo
//!
//! Demonstrates basic UI layout without state management.
//! This is a simplified version to demonstrate FLUI widgets.
//!
//! Run with: cargo run --example counter_simple

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Button, Center, Column, Container, SizedBox, Text};

/// Simple counter app (static UI)
#[derive(Debug, Clone)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut container = Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(245, 245, 250))
            .build_container();

        container.child = Some(Box::new(CounterContent));
        container
    }
}

/// Content of the counter
#[derive(Debug, Clone)]
struct CounterContent;

impl View for CounterContent {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut center = Center::builder().build();

        // Create column
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

        column.children.push(Box::new(
            SizedBox::builder().height(20.0).build(),
        ));

        // Counter display
        let mut counter_container = Container::builder()
            .padding(EdgeInsets::symmetric(32.0, 16.0))
            .color(Color::rgb(33, 150, 243))
            .build_container();

        counter_container.child = Some(Box::new(
            Text::builder()
                .data("0")
                .size(48.0)
                .color(Color::WHITE)
                .build(),
        ));

        column.children.push(Box::new(counter_container));

        column.children.push(Box::new(
            SizedBox::builder().height(20.0).build(),
        ));

        // Increment button
        column.children.push(Box::new(
            Button::builder("Increment (+)")
                .color(Color::rgb(76, 175, 80))
                .on_tap(|| {
                    println!("Increment clicked!");
                })
                .build(),
        ));

        column.children.push(Box::new(
            SizedBox::builder().height(8.0).build(),
        ));

        // Decrement button
        column.children.push(Box::new(
            Button::builder("Decrement (-)")
                .color(Color::rgb(244, 67, 54))
                .on_tap(|| {
                    println!("Decrement clicked!");
                })
                .build(),
        ));

        column.children.push(Box::new(
            SizedBox::builder().height(8.0).build(),
        ));

        // Reset button
        column.children.push(Box::new(
            Button::builder("Reset")
                .color(Color::rgb(158, 158, 158))
                .on_tap(|| {
                    println!("Reset clicked!");
                })
                .build(),
        ));

        center.child = Some(Box::new(column));
        center
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== FLUI Counter Demo (Simple) ===");
    println!("Static UI demonstration");
    println!("Note: This version doesn't have state management yet");
    println!();

    run_app(Box::new(CounterApp))
}
