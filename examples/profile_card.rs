//! Profile Card Example - Flutter-style declarative UI
//!
//! Demonstrates building a beautiful profile card using:
//! - Card widget for elevation and styling
//! - Row and Column for layout
//! - Container for spacing and decoration
//! - Text for content
//! - ClipOval for circular avatar
//! - Divider for visual separation

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{
    Button, Card, Center, ClipOval, Column, Container, Divider, Padding, Row, Scaffold, SizedBox,
    Text,
};

/// Profile card application - Flutter-style inline composition
#[derive(Debug, Clone)]
struct ProfileCardApp;

impl View for ProfileCardApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Scaffold with gray background
        Scaffold::builder()
            .background_color(Color::rgb(240, 240, 245))
            .body(
                Padding::builder()
                    .padding(EdgeInsets::all(40.0))
                    .child(
                        // Center the card
                        Center::builder()
                            .child(
                                // Card with elevation
                                Card::builder()
                                    .elevation(2.0)
                                    .child(
                                        // Container for card content
                                        Container::builder()
                                            .width(350.0)
                                            .padding(EdgeInsets::all(24.0))
                                            .child(
                                    // Main column layout
                                    Column::builder()
                                        .main_axis_size(MainAxisSize::Min)
                                        .cross_axis_alignment(CrossAxisAlignment::Center)
                                        // Avatar
                                        .child(
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
                                                                        .build(),
                                                                )
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
                                        // Spacing
                                        .child(SizedBox::builder().height(16.0).build())
                                        // Divider
                                        .child(
                                            Divider::builder()
                                                .color(Color::rgb(224, 224, 224))
                                                .build(),
                                        )
                                        // Spacing
                                        .child(SizedBox::builder().height(16.0).build())
                                        // Stats row
                                        .child(
                                            Row::builder()
                                                .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                                // Posts stat
                                                .child(
                                                    Column::builder()
                                                        .main_axis_size(MainAxisSize::Min)
                                                        .cross_axis_alignment(CrossAxisAlignment::Center)
                                                        .child(
                                                            Text::builder()
                                                                .data("128")
                                                                .size(20.0)
                                                                .color(Color::rgb(33, 33, 33))
                                                                .build(),
                                                        )
                                                        .child(SizedBox::builder().height(4.0).build())
                                                        .child(
                                                            Text::builder()
                                                                .data("Posts")
                                                                .size(14.0)
                                                                .color(Color::rgb(117, 117, 117))
                                                                .build(),
                                                        )
                                                        .build(),
                                                )
                                                // Followers stat
                                                .child(
                                                    Column::builder()
                                                        .main_axis_size(MainAxisSize::Min)
                                                        .cross_axis_alignment(CrossAxisAlignment::Center)
                                                        .child(
                                                            Text::builder()
                                                                .data("2.5K")
                                                                .size(20.0)
                                                                .color(Color::rgb(33, 33, 33))
                                                                .build(),
                                                        )
                                                        .child(SizedBox::builder().height(4.0).build())
                                                        .child(
                                                            Text::builder()
                                                                .data("Followers")
                                                                .size(14.0)
                                                                .color(Color::rgb(117, 117, 117))
                                                                .build(),
                                                        )
                                                        .build(),
                                                )
                                                // Following stat
                                                .child(
                                                    Column::builder()
                                                        .main_axis_size(MainAxisSize::Min)
                                                        .cross_axis_alignment(CrossAxisAlignment::Center)
                                                        .child(
                                                            Text::builder()
                                                                .data("312")
                                                                .size(20.0)
                                                                .color(Color::rgb(33, 33, 33))
                                                                .build(),
                                                        )
                                                        .child(SizedBox::builder().height(4.0).build())
                                                        .child(
                                                            Text::builder()
                                                                .data("Following")
                                                                .size(14.0)
                                                                .color(Color::rgb(117, 117, 117))
                                                                .build(),
                                                        )
                                                        .build(),
                                                )
                                                .build(),
                                        )
                                        // Spacing
                                        .child(SizedBox::builder().height(20.0).build())
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
                .build(),
            )
            .build()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // run_app initializes tracing internally
    run_app(Box::new(ProfileCardApp))
}
