//! Test Column with Row inside

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Column, Container, Padding, Row, Scaffold, SizedBox, Text};

#[derive(Debug, Clone)]
struct TestColumnRowApp;

impl View for TestColumnRowApp {
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
                                            .child(
                                                Text::builder()
                                                    .data("Before Row")
                                                    .size(24.0)
                                                    .color(Color::rgb(33, 33, 33))
                                                    .build(),
                                            )
                                            .child(SizedBox::builder().height(16.0).build())
                                            .child(
                                                Row::builder()
                                                    .main_axis_alignment(
                                                        MainAxisAlignment::SpaceEvenly,
                                                    )
                                                    .child(
                                                        Text::builder()
                                                            .data("Row 1")
                                                            .size(16.0)
                                                            .color(Color::rgb(255, 0, 0))
                                                            .build(),
                                                    )
                                                    .child(
                                                        Text::builder()
                                                            .data("Row 2")
                                                            .size(16.0)
                                                            .color(Color::rgb(0, 255, 0))
                                                            .build(),
                                                    )
                                                    .child(
                                                        Text::builder()
                                                            .data("Row 3")
                                                            .size(16.0)
                                                            .color(Color::rgb(0, 0, 255))
                                                            .build(),
                                                    )
                                                    .build(),
                                            )
                                            .child(SizedBox::builder().height(16.0).build())
                                            .child(
                                                Text::builder()
                                                    .data("After Row")
                                                    .size(24.0)
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
    println!("=== Test Column with Row ===");
    run_app(Box::new(TestColumnRowApp))
}
