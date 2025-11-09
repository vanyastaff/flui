//! Viewport Demo - Test Viewport widget
//!
//! Demonstrates:
//! - Viewport widget with fixed offset
//! - Displaying portion of large content
//! - Vertical and horizontal viewports
//! - Clipping behavior
//!
//! Run with: cargo run --example viewport_demo

use flui_app::run_app;
use flui_core::hooks::use_signal;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets, Offset};
use flui_widgets::{Button, Column, Container, Row, SizedBox, Text, Viewport};

/// Viewport demo application
#[derive(Debug, Clone)]
struct ViewportDemoApp;

impl View for ViewportDemoApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // State for viewport offset
        let offset_y = use_signal(ctx, 0.0_f32);

        // Create large content (taller than viewport)
        let mut content_column = Column::builder().build();

        // Add title
        let mut title = Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(33, 150, 243))
            .build();
        title.child = Some(Box::new(
            Text::builder()
                .data("Large Content - 30 Items")
                .size(24.0)
                .color(Color::WHITE)
                .build(),
        ));
        content_column.children.push(Box::new(title));

        // Add many items
        for i in 0..30 {
            let color = if i % 2 == 0 {
                Color::rgb(250, 250, 250)
            } else {
                Color::rgb(255, 255, 255)
            };

            let mut item = Container::builder()
                .padding(EdgeInsets::symmetric(16.0, 12.0))
                .color(color)
                .build();

            item.child = Some(Box::new(
                Text::builder()
                    .data(format!("Item #{}", i + 1))
                    .size(16.0)
                    .color(Color::rgb(33, 33, 33))
                    .build(),
            ));

            content_column.children.push(Box::new(item));
        }

        // Create viewport with current offset
        let current_offset = offset_y.get_untracked();
        let viewport = Viewport::builder()
            .axis(flui_types::layout::Axis::Vertical)
            .offset(Offset::new(0.0, current_offset))
            .child(content_column)
            .build();

        // Wrap viewport in SizedBox to give it fixed size
        let mut sized_viewport = SizedBox::builder().width(600.0).height(400.0).build();
        sized_viewport.child = Some(Box::new(viewport));

        // Add border around viewport
        let mut viewport_container = Container::builder()
            .color(Color::rgb(220, 220, 220))
            .padding(EdgeInsets::all(2.0))
            .build();
        viewport_container.child = Some(Box::new(sized_viewport));

        // Create control buttons
        let mut buttons_row = Row::builder().build();

        let offset_up = offset_y.clone();
        let up_button = Button::builder("↑ Scroll Up (50px)")
            .on_tap(move || {
                let current = offset_up.get_untracked();
                offset_up.set((current - 50.0).max(0.0));
            })
            .build();
        buttons_row.children.push(Box::new(up_button));

        buttons_row
            .children
            .push(Box::new(SizedBox::builder().width(10.0).build()));

        let offset_down = offset_y.clone();
        let down_button = Button::builder("↓ Scroll Down (50px)")
            .on_tap(move || {
                let current = offset_down.get_untracked();
                offset_down.set((current + 50.0).min(1000.0));
            })
            .build();
        buttons_row.children.push(Box::new(down_button));

        buttons_row
            .children
            .push(Box::new(SizedBox::builder().width(20.0).build()));

        let offset_reset = offset_y.clone();
        let reset_button = Button::builder("⟲ Reset")
            .on_tap(move || {
                offset_reset.set(0.0);
            })
            .build();
        buttons_row.children.push(Box::new(reset_button));

        // Wrap buttons in container
        let mut buttons_container = Container::builder()
            .padding(EdgeInsets::all(16.0))
            .color(Color::rgb(240, 240, 240))
            .build();
        buttons_container.child = Some(Box::new(buttons_row));

        // Info text
        let mut info_container = Container::builder().padding(EdgeInsets::all(10.0)).build();
        info_container.child = Some(Box::new(
            Text::builder()
                .data(format!(
                    "Viewport Offset: {:.0}px",
                    offset_y.get_untracked()
                ))
                .size(14.0)
                .color(Color::rgb(100, 100, 100))
                .build(),
        ));

        // Main layout
        let mut main_column = Column::builder().build();

        let mut title_container = Container::builder().padding(EdgeInsets::all(20.0)).build();
        title_container.child = Some(Box::new(
            Text::builder()
                .data("Viewport Demo")
                .size(32.0)
                .color(Color::rgb(33, 33, 33))
                .build(),
        ));
        main_column.children.push(Box::new(title_container));

        main_column.children.push(Box::new(buttons_container));
        main_column.children.push(Box::new(info_container));

        let mut content_container = Container::builder().padding(EdgeInsets::all(20.0)).build();
        content_container.child = Some(Box::new(viewport_container));
        main_column.children.push(Box::new(content_container));

        main_column
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== FLUI Viewport Demo ===");
    println!("Controls:");
    println!("  • ↑ Scroll Up - Move viewport up by 50px");
    println!("  • ↓ Scroll Down - Move viewport down by 50px");
    println!("  • ⟲ Reset - Reset to top");
    println!();

    run_app(Box::new(ViewportDemoApp))
}
