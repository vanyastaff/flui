//! Test Row widget

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::MainAxisAlignment;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Column, Container, Padding, Row, Scaffold, SizedBox, Text};

#[derive(Debug, Clone)]
struct TestRowApp;

impl View for TestRowApp {
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
                                            .child(
                                                Text::builder()
                                                    .data("Testing Row Widget")
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
                                                        Container::builder()
                                                            .width(80.0)
                                                            .height(80.0)
                                                            .color(Color::rgb(255, 0, 0))
                                                            .child(
                                                                Center::builder()
                                                                    .child(
                                                                        Text::builder()
                                                                            .data("Box 1")
                                                                            .size(16.0)
                                                                            .color(Color::WHITE)
                                                                            .build(),
                                                                    )
                                                                    .build(),
                                                            )
                                                            .build(),
                                                    )
                                                    .child(
                                                        Container::builder()
                                                            .width(80.0)
                                                            .height(80.0)
                                                            .color(Color::rgb(0, 255, 0))
                                                            .child(
                                                                Center::builder()
                                                                    .child(
                                                                        Text::builder()
                                                                            .data("Box 2")
                                                                            .size(16.0)
                                                                            .color(Color::WHITE)
                                                                            .build(),
                                                                    )
                                                                    .build(),
                                                            )
                                                            .build(),
                                                    )
                                                    .child(
                                                        Container::builder()
                                                            .width(80.0)
                                                            .height(80.0)
                                                            .color(Color::rgb(0, 0, 255))
                                                            .child(
                                                                Center::builder()
                                                                    .child(
                                                                        Text::builder()
                                                                            .data("Box 3")
                                                                            .size(16.0)
                                                                            .color(Color::WHITE)
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
    println!("=== Test Row Widget ===");
    run_app(Box::new(TestRowApp))
}
