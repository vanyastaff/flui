//! Test nested Columns inside Row (like stats in profile card)

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Column, Container, Padding, Row, Scaffold, SizedBox, Text};

#[derive(Debug, Clone)]
struct TestNestedColumnsApp;

impl View for TestNestedColumnsApp {
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
                                    .width(400.0)
                                    .padding(EdgeInsets::all(24.0))
                                    .color(Color::WHITE)
                                    .child(
                                        Column::builder()
                                            .main_axis_size(MainAxisSize::Min)
                                            .cross_axis_alignment(CrossAxisAlignment::Center)
                                            .child(
                                                Text::builder()
                                                    .data("Stats Row (like profile card):")
                                                    .size(20.0)
                                                    .color(Color::rgb(33, 33, 33))
                                                    .build(),
                                            )
                                            .child(SizedBox::builder().height(16.0).build())
                                            // This Row with nested Columns is like profile card stats
                                            .child(
                                                Row::builder()
                                                    .main_axis_alignment(
                                                        MainAxisAlignment::SpaceEvenly,
                                                    )
                                                    // Posts stat
                                                    .child(
                                                        Column::builder()
                                                            .main_axis_size(MainAxisSize::Min)
                                                            .cross_axis_alignment(
                                                                CrossAxisAlignment::Center,
                                                            )
                                                            .child(
                                                                Text::builder()
                                                                    .data("128")
                                                                    .size(20.0)
                                                                    .color(Color::rgb(33, 33, 33))
                                                                    .build(),
                                                            )
                                                            .child(
                                                                SizedBox::builder()
                                                                    .height(4.0)
                                                                    .build(),
                                                            )
                                                            .child(
                                                                Text::builder()
                                                                    .data("Posts")
                                                                    .size(14.0)
                                                                    .color(Color::rgb(
                                                                        117, 117, 117,
                                                                    ))
                                                                    .build(),
                                                            )
                                                            .build(),
                                                    )
                                                    // Followers stat
                                                    .child(
                                                        Column::builder()
                                                            .main_axis_size(MainAxisSize::Min)
                                                            .cross_axis_alignment(
                                                                CrossAxisAlignment::Center,
                                                            )
                                                            .child(
                                                                Text::builder()
                                                                    .data("2.5K")
                                                                    .size(20.0)
                                                                    .color(Color::rgb(33, 33, 33))
                                                                    .build(),
                                                            )
                                                            .child(
                                                                SizedBox::builder()
                                                                    .height(4.0)
                                                                    .build(),
                                                            )
                                                            .child(
                                                                Text::builder()
                                                                    .data("Followers")
                                                                    .size(14.0)
                                                                    .color(Color::rgb(
                                                                        117, 117, 117,
                                                                    ))
                                                                    .build(),
                                                            )
                                                            .build(),
                                                    )
                                                    // Following stat
                                                    .child(
                                                        Column::builder()
                                                            .main_axis_size(MainAxisSize::Min)
                                                            .cross_axis_alignment(
                                                                CrossAxisAlignment::Center,
                                                            )
                                                            .child(
                                                                Text::builder()
                                                                    .data("312")
                                                                    .size(20.0)
                                                                    .color(Color::rgb(33, 33, 33))
                                                                    .build(),
                                                            )
                                                            .child(
                                                                SizedBox::builder()
                                                                    .height(4.0)
                                                                    .build(),
                                                            )
                                                            .child(
                                                                Text::builder()
                                                                    .data("Following")
                                                                    .size(14.0)
                                                                    .color(Color::rgb(
                                                                        117, 117, 117,
                                                                    ))
                                                                    .build(),
                                                            )
                                                            .build(),
                                                    )
                                                    .build(),
                                            )
                                            .child(SizedBox::builder().height(16.0).build())
                                            .child(
                                                Text::builder()
                                                    .data("After stats")
                                                    .size(20.0)
                                                    .color(Color::rgb(33, 33, 33))
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
    println!("=== Test Nested Columns in Row ===");
    run_app(Box::new(TestNestedColumnsApp))
}
