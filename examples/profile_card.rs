//! Profile Card Example
//!
//! Demonstrates building a beautiful profile card using:
//! - Card widget for elevation and styling
//! - Row and Column for layout
//! - Container for spacing and decoration
//! - Text for content
//! - ClipOval for circular avatar
//! - Divider for visual separation

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

/// Profile card application
#[derive(Debug, Clone)]
struct ProfileCardApp;

flui_core::impl_into_widget!(ProfileCardApp, stateless);

impl StatelessWidget for ProfileCardApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(240, 240, 245))
            .child(
                Center::builder()
                    .child(
                        Card::builder()
                            .child(
                                Container::builder()
                                    .width(350.0)
                                    .padding(EdgeInsets::all(24.0))
                                    .child(
                                        Column::builder()
                                            .main_axis_size(MainAxisSize::Min)
                                            .cross_axis_alignment(CrossAxisAlignment::Center)
                                            .children(vec![
                                                // Avatar
                                                ClipOval::builder()
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
                                                                            .build()
                                                                    )
                                                                    .build()
                                                            )
                                                            .build()
                                                    )
                                                    .build()
                                                    .into(),

                                                SizedBox::builder()
                                                    .height(16.0)
                                                    .build()
                                                    .into(),

                                                // Name
                                                Text::builder()
                                                    .data("John Doe")
                                                    .size(24.0)
                                                    .color(Color::rgb(33, 33, 33))
                                                    .build()
                                                    .into(),

                                                SizedBox::builder()
                                                    .height(8.0)
                                                    .build()
                                                    .into(),

                                                // Title
                                                Text::builder()
                                                    .data("Senior Rust Developer")
                                                    .size(16.0)
                                                    .color(Color::rgb(117, 117, 117))
                                                    .build()
                                                    .into(),

                                                SizedBox::builder()
                                                    .height(16.0)
                                                    .build()
                                                    .into(),

                                                Divider::builder()
                                                    .color(Color::rgb(224, 224, 224))
                                                    .build()
                                                    .into(),

                                                SizedBox::builder()
                                                    .height(16.0)
                                                    .build()
                                                    .into(),

                                                // Stats Row
                                                Row::builder()
                                                    .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                                    .children(vec![
                                                        build_stat("128", "Posts"),
                                                        build_stat("2.5K", "Followers"),
                                                        build_stat("312", "Following"),
                                                    ])
                                                    .build()
                                                    .into(),

                                                SizedBox::builder()
                                                    .height(20.0)
                                                    .build()
                                                    .into(),

                                                // Action Buttons
                                                Row::builder()
                                                    .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                                    .children(vec![
                                                        Button::builder()
                                                            .text("Follow")
                                                            .color(Color::rgb(33, 150, 243))
                                                            .build()
                                                            .into(),
                                                        Button::builder()
                                                            .text("Message")
                                                            .color(Color::rgb(156, 39, 176))
                                                            .build()
                                                            .into(),
                                                    ])
                                                    .build()
                                                    .into(),
                                            ])
                                            .build()
                                    )
                                    .build()
                            )
                            .build()
                    )
                    .build()
            )
            .build()
    }
}

/// Helper function to build a stat widget
fn build_stat(value: &str, label: &str) -> Widget {
    Column::builder()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(vec![
            Text::builder()
                .data(value)
                .size(20.0)
                .color(Color::rgb(33, 33, 33))
                .build()
                .into(),
            SizedBox::builder()
                .height(4.0)
                .build()
                .into(),
            Text::builder()
                .data(label)
                .size(14.0)
                .color(Color::rgb(117, 117, 117))
                .build()
                .into(),
        ])
        .build()
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Profile Card Example ===");
    println!("Demonstrates:");
    println!("  • Card widget with elevation");
    println!("  • Row and Column layout");
    println!("  • ClipOval for circular avatar");
    println!("  • Divider for visual separation");
    println!("  • Button widgets for actions");
    println!();

    run_app(ProfileCardApp.into_widget())
}
