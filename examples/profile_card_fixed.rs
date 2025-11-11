//! Profile Card Demo (Fixed with Expanded widgets)
//!
//! Demonstrates a modern profile card UI with:
//! - Avatar with custom background
//! - User name and title
//! - Stats row (using Expanded for equal spacing)
//! - Action buttons

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, FlexFit, MainAxisAlignment, MainAxisSize};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{
    Button, Card, Center, ClipOval, Column, Container, Divider, Expanded, Padding, Row, Scaffold,
    SizedBox, Text,
};

/// Profile card app
#[derive(Debug, Clone)]
struct ProfileCardApp;

impl View for ProfileCardApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Scaffold::builder()
            .background_color(Color::rgb(240, 240, 245))
            .body(
                Padding::builder()
                    .padding(EdgeInsets::all(40.0))
                    .child(
                        Center::builder()
                            .child(
                                Card::builder()
                                    .elevation(4.0)
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
                                                            .thickness(1.0)
                                                            .build(),
                                                    )
                                                    // Spacing
                                                    .child(SizedBox::builder().height(16.0).build())
                                                    // Stats row (FIXED: wrapped in Expanded)
                                                    .child(
                                                        Row::builder()
                                                            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                                            // Posts stat
                                                            .child(
                                                                Expanded::builder()
                                                                    .flex(1)
                                                                    .fit(FlexFit::Tight)
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
                                                                    .build(),
                                                            )
                                                            // Followers stat
                                                            .child(
                                                                Expanded::builder()
                                                                    .flex(1)
                                                                    .fit(FlexFit::Tight)
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
                                                                    .build(),
                                                            )
                                                            // Following stat
                                                            .child(
                                                                Expanded::builder()
                                                                    .flex(1)
                                                                    .fit(FlexFit::Tight)
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
                                                            .build(),
                                                    )
                                                    // Spacing
                                                    .child(SizedBox::builder().height(20.0).build())
                                                    // Action buttons
                                                    .child(
                                                        Row::builder()
                                                            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                                            .child(
                                                                Expanded::builder()
                                                                    .flex(1)
                                                                    .child(
                                                                        Padding::builder()
                                                                            .padding(EdgeInsets::symmetric(4.0, 0.0))
                                                                            .child(
                                                                                Button::builder()
                                                                                    .label("Follow")
                                                                                    .background_color(Color::rgb(33, 150, 243))
                                                                                    .text_color(Color::WHITE)
                                                                                    .build(),
                                                                            )
                                                                            .build(),
                                                                    )
                                                                    .build(),
                                                            )
                                                            .child(
                                                                Expanded::builder()
                                                                    .flex(1)
                                                                    .child(
                                                                        Padding::builder()
                                                                            .padding(EdgeInsets::symmetric(4.0, 0.0))
                                                                            .child(
                                                                                Button::builder()
                                                                                    .label("Message")
                                                                                    .background_color(Color::rgb(236, 64, 122))
                                                                                    .text_color(Color::WHITE)
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
                            .build(),
                    )
                    .build(),
            )
            .build()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Profile Card Demo (Fixed with Expanded) ===");
    run_app(Box::new(ProfileCardApp))
}
