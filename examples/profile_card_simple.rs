//! Simplified Profile Card - Debugging version

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, MainAxisSize};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Column, Container, Padding, Scaffold, SizedBox, Text};

/// Simplified profile card for debugging
#[derive(Debug, Clone)]
struct SimpleProfileCardApp;

impl View for SimpleProfileCardApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Scaffold::builder()
            .background_color(Color::rgb(240, 240, 245))
            .body(
                Padding::builder()
                    .padding(EdgeInsets::all(40.0))
                    .child(
                        Center::builder()
                            .child(
                                Container::builder()
                                    .width(350.0)
                                    .padding(EdgeInsets::all(24.0))
                                    .color(Color::WHITE)
                                    .child(
                                        Column::builder()
                                            .main_axis_size(MainAxisSize::Min)
                                            .cross_axis_alignment(CrossAxisAlignment::Center)
                                            // Avatar placeholder
                                            .child(
                                                Container::builder()
                                                    .width(100.0)
                                                    .height(100.0)
                                                    .color(Color::rgb(100, 181, 246))
                                                    .child(
                                                        Center::builder()
                                                            .child(
                                                                Text::builder()
                                                                    .data("JD")
                                                                    .size(40.0)
                                                                    .color(Color::WHITE)
                                                                    .build(),
                                                            )
                                                            .build(),
                                                    )
                                                    .build(),
                                            )
                                            // Spacing
                                            .child(SizedBox::builder().height(16.0).build())
                                            // Name
                                            .child(
                                                Text::builder()
                                                    .data("John Doe")
                                                    .size(24.0)
                                                    .color(Color::rgb(33, 33, 33))
                                                    .build(),
                                            )
                                            // Spacing
                                            .child(SizedBox::builder().height(8.0).build())
                                            // Title
                                            .child(
                                                Text::builder()
                                                    .data("Senior Rust Developer")
                                                    .size(16.0)
                                                    .color(Color::rgb(117, 117, 117))
                                                    .build(),
                                            )
                                            .build(),
                                    )
                                    .build(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .build()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Simple Profile Card ===");
    run_app(Box::new(SimpleProfileCardApp))
}
