//! Profile Card Example with Macros - Compact declarative UI
//!
//! Demonstrates building a beautiful profile card using:
//! - flui_widgets::text! macro for ultra-compact text widgets
//! - flui_widgets::sized_box! macro for spacing
//! - Mix of macros and builders for optimal ergonomics

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{
    Button, Card, Center, ClipOval, Column, Container, Divider, Padding, Row, Scaffold,
};

/// Profile card application - Compact with macros
#[derive(Debug, Clone)]
struct ProfileCardMacrosApp;

impl View for ProfileCardMacrosApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Using improved flui_widgets::scaffold! macro with builder-style properties
        flui_widgets::scaffold! {
            background_color: Color::rgb(240, 240, 245),
            body:
                Padding::builder()
                    .padding(EdgeInsets::all(40.0))
                    .child(
                        Center::builder().child(
                            Card::builder()
                                .elevation(2.0)
                                .child(
                                    Container::builder()
                                        .width(350.0)
                                        .padding(EdgeInsets::all(24.0))
                                        .child(
                                            Column::builder()
                                                .main_axis_size(MainAxisSize::Min)
                                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                                // Avatar
                                                .child(
                                                    ClipOval::builder().child(
                                                        Container::builder()
                                                            .width(100.0)
                                                            .height(100.0)
                                                            .color(Color::rgb(100, 181, 246))
                                                            .child(
                                                                Center::builder().child(flui_widgets::text! {
                                                                    data: "JD",
                                                                    size: 40.0,
                                                                    color: Color::WHITE,
                                                                })
                                                                .build(),
                                                            )
                                                            .build(),
                                                    )
                                                    .build(),
                                                )
                                                // Spacing
                                                .child(flui_widgets::sized_box! { height: 16.0 })
                                                // Name
                                                .child(flui_widgets::text! {
                                                    data: "John Doe",
                                                    size: 24.0,
                                                    color: Color::rgb(33, 33, 33),
                                                })
                                                // Spacing
                                                .child(flui_widgets::sized_box! { height: 8.0 })
                                                // Title
                                                .child(flui_widgets::text! {
                                                    data: "Senior Rust Developer",
                                                    size: 16.0,
                                                    color: Color::rgb(117, 117, 117),
                                                })
                                                // Spacing
                                                .child(flui_widgets::sized_box! { height: 16.0 })
                                                // Divider
                                                .child(
                                                    Divider::builder()
                                                        .color(Color::rgb(224, 224, 224))
                                                        .build(),
                                                )
                                                // Spacing
                                                .child(flui_widgets::sized_box! { height: 16.0 })
                                                // Stats row
                                                .child(
                                                    Row::builder()
                                                        .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                                        // Posts stat
                                                        .child(
                                                            Column::builder()
                                                                .main_axis_size(MainAxisSize::Min)
                                                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                                                .child(flui_widgets::text! {
                                                                    data: "128",
                                                                    size: 20.0,
                                                                    color: Color::rgb(33, 33, 33),
                                                                })
                                                                .child(flui_widgets::sized_box! { height: 4.0 })
                                                                .child(flui_widgets::text! {
                                                                    data: "Posts",
                                                                    size: 14.0,
                                                                    color: Color::rgb(117, 117, 117),
                                                                })
                                                                .build(),
                                                        )
                                                        // Followers stat
                                                        .child(
                                                            Column::builder()
                                                                .main_axis_size(MainAxisSize::Min)
                                                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                                                .child(flui_widgets::text! {
                                                                    data: "2.5K",
                                                                    size: 20.0,
                                                                    color: Color::rgb(33, 33, 33),
                                                                })
                                                                .child(flui_widgets::sized_box! { height: 4.0 })
                                                                .child(flui_widgets::text! {
                                                                    data: "Followers",
                                                                    size: 14.0,
                                                                    color: Color::rgb(117, 117, 117),
                                                                })
                                                                .build(),
                                                        )
                                                        // Following stat
                                                        .child(
                                                            Column::builder()
                                                                .main_axis_size(MainAxisSize::Min)
                                                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                                                .child(flui_widgets::text! {
                                                                    data: "312",
                                                                    size: 20.0,
                                                                    color: Color::rgb(33, 33, 33),
                                                                })
                                                                .child(flui_widgets::sized_box! { height: 4.0 })
                                                                .child(flui_widgets::text! {
                                                                    data: "Following",
                                                                    size: 14.0,
                                                                    color: Color::rgb(117, 117, 117),
                                                                })
                                                                .build(),
                                                        )
                                                        .build(),
                                                )
                                                // Spacing
                                                .child(flui_widgets::sized_box! { height: 20.0 })
                                                // Action buttons
                                                .child(
                                                    Row::builder()
                                                        .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                                        .child(
                                                            Button::builder("Follow")
                                                                .color(Color::rgb(33, 150, 243))
                                                                .build(),
                                                        )
                                                        .child(
                                                            Button::builder("Message")
                                                                .color(Color::rgb(156, 39, 176))
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
                        .build(),
                    )
                    .build()
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Profile Card with Macros - Compact declarative UI ===");
    println!("Demonstrates:");
    println!("  • flui_widgets::text! macro for ultra-compact text widgets");
    println!("  • flui_widgets::sized_box! macro for spacing");
    println!("  • Builder chaining with .child() methods");
    println!("  • Mix of macros and builders for optimal ergonomics");
    println!();

    run_app(Box::new(ProfileCardMacrosApp))
}
