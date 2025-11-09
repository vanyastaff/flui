//! Scaffold Demo - Test Scaffold widget with AppBar and FAB
//!
//! Demonstrates:
//! - Scaffold structure with AppBar
//! - Body with main content
//! - Floating Action Button (FAB)
//! - Bottom navigation bar
//! - Proper Material Design layout
//!
//! Run with: cargo run --example scaffold_demo

use flui_app::run_app;
use flui_core::hooks::use_signal;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{
    AppBar, Button, Card, Center, Column, Container, Row, Scaffold, SizedBox, Text,
};

/// Scaffold demo application
#[derive(Debug, Clone)]
struct ScaffoldDemoApp;

impl View for ScaffoldDemoApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Counter state
        let counter = use_signal(ctx, 0);

        // Create AppBar
        let app_bar = AppBar::builder()
            .title(
                Text::builder()
                    .data("Scaffold Demo")
                    .size(20.0)
                    .color(Color::WHITE)
                    .build(),
            )
            .background_color(Color::rgb(25, 118, 210)) // Material Blue 700
            .elevation(4.0)
            .build();

        // Create main content
        let mut content_column = Column::builder()
            .main_axis_alignment(flui_types::layout::MainAxisAlignment::Center)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Center)
            .build();

        // Welcome card
        let mut welcome_card = Card::builder().elevation(2.0).color(Color::WHITE).build();

        let mut welcome_content = Container::builder().padding(EdgeInsets::all(24.0)).build();

        let mut welcome_column = Column::builder()
            .main_axis_size(flui_types::layout::MainAxisSize::Min)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Center)
            .build();

        welcome_column.children.push(Box::new(
            Text::builder()
                .data("Welcome to Scaffold Demo!")
                .size(24.0)
                .color(Color::rgb(33, 33, 33))
                .build(),
        ));

        welcome_column
            .children
            .push(Box::new(SizedBox::builder().height(16.0).build()));

        welcome_column.children.push(Box::new(
            Text::builder()
                .data("Scaffold provides a standard app structure")
                .size(16.0)
                .color(Color::rgb(100, 100, 100))
                .build(),
        ));

        welcome_content.child = Some(Box::new(welcome_column));
        welcome_card.child = Some(Box::new(welcome_content));
        content_column.children.push(Box::new(welcome_card));

        // Spacer
        content_column
            .children
            .push(Box::new(SizedBox::builder().height(32.0).build()));

        // Counter card
        let mut counter_card = Card::builder().elevation(2.0).color(Color::WHITE).build();

        let mut counter_content = Container::builder().padding(EdgeInsets::all(24.0)).build();

        let mut counter_column = Column::builder()
            .main_axis_size(flui_types::layout::MainAxisSize::Min)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Center)
            .build();

        counter_column.children.push(Box::new(
            Text::builder()
                .data("Counter")
                .size(20.0)
                .color(Color::rgb(33, 33, 33))
                .build(),
        ));

        counter_column
            .children
            .push(Box::new(SizedBox::builder().height(16.0).build()));

        counter_column.children.push(Box::new(
            Text::builder()
                .data(format!("{}", counter.get_untracked()))
                .size(48.0)
                .color(Color::rgb(25, 118, 210))
                .build(),
        ));

        counter_column
            .children
            .push(Box::new(SizedBox::builder().height(16.0).build()));

        let counter_btn = counter.clone();
        counter_column.children.push(Box::new(
            Button::builder("Increment")
                .on_tap(move || {
                    let current = counter_btn.get_untracked();
                    counter_btn.set(current + 1);
                })
                .build(),
        ));

        counter_content.child = Some(Box::new(counter_column));
        counter_card.child = Some(Box::new(counter_content));
        content_column.children.push(Box::new(counter_card));

        // Wrap content in container with padding
        let mut body_container = Container::builder().padding(EdgeInsets::all(16.0)).build();
        body_container.child = Some(Box::new(Center::builder().child(content_column).build()));

        // Create FAB (Floating Action Button)
        let fab_counter = counter.clone();
        let mut fab = Container::builder()
            .width(56.0)
            .height(56.0)
            .color(Color::rgb(25, 118, 210))
            .build();
        fab.child = Some(Box::new(
            Center::builder()
                .child(
                    Button::builder("+")
                        .on_tap(move || {
                            let current = fab_counter.get_untracked();
                            fab_counter.set(current + 1);
                        })
                        .build(),
                )
                .build(),
        ));

        // Create bottom navigation bar
        let mut bottom_nav = Container::builder()
            .height(56.0)
            .color(Color::rgb(250, 250, 250))
            .build();

        let mut nav_row = Row::builder()
            .main_axis_alignment(flui_types::layout::MainAxisAlignment::SpaceAround)
            .cross_axis_alignment(flui_types::layout::CrossAxisAlignment::Center)
            .build();

        for label in &["Home", "Search", "Profile"] {
            let mut nav_item = Container::builder().padding(EdgeInsets::all(12.0)).build();
            nav_item.child = Some(Box::new(
                Text::builder()
                    .data(*label)
                    .size(14.0)
                    .color(Color::rgb(100, 100, 100))
                    .build(),
            ));
            nav_row.children.push(Box::new(nav_item));
        }

        bottom_nav.child = Some(Box::new(nav_row));

        // Build scaffold
        Scaffold::builder()
            .app_bar(Box::new(app_bar))
            .body(body_container)
            .floating_action_button(Box::new(fab))
            .bottom_navigation_bar(Box::new(bottom_nav))
            .background_color(Color::rgb(245, 245, 245))
            .build()
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    println!("=== FLUI Scaffold Demo ===");
    println!("Demonstrates Material Design layout structure:");
    println!("  • AppBar at the top");
    println!("  • Body with main content");
    println!("  • Floating Action Button (bottom-right)");
    println!("  • Bottom Navigation Bar");
    println!();

    run_app(Box::new(ScaffoldDemoApp))
}
