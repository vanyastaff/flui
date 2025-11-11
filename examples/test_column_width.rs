//! Test Column width calculation with MainAxisSize::Min

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Column, Container, Padding, Row, Scaffold, SizedBox, Text};

#[derive(Debug, Clone)]
struct TestColumnWidthApp;

impl View for TestColumnWidthApp {
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
                                            .cross_axis_alignment(CrossAxisAlignment::Stretch)
                                            .child(
                                                Text::builder()
                                                    .data("Test: 3 Columns in Row")
                                                    .size(18.0)
                                                    .color(Color::rgb(33, 33, 33))
                                                    .build(),
                                            )
                                            .child(SizedBox::builder().height(16.0).build())
                                            .child(
                                                Row::builder()
                                                    .main_axis_alignment(
                                                        MainAxisAlignment::SpaceEvenly,
                                                    )
                                                    // Column 1 with red background
                                                    .child(
                                                        Container::builder()
                                                            .color(Color::rgb(255, 200, 200))
                                                            .padding(EdgeInsets::all(8.0))
                                                            .child(
                                                                Column::builder()
                                                                    .main_axis_size(
                                                                        MainAxisSize::Min,
                                                                    )
                                                                    .cross_axis_alignment(
                                                                        CrossAxisAlignment::Center,
                                                                    )
                                                                    .child(
                                                                        Text::builder()
                                                                            .data("128")
                                                                            .size(20.0)
                                                                            .color(Color::rgb(
                                                                                33, 33, 33,
                                                                            ))
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
                                                            .build(),
                                                    )
                                                    // Column 2 with green background
                                                    .child(
                                                        Container::builder()
                                                            .color(Color::rgb(200, 255, 200))
                                                            .padding(EdgeInsets::all(8.0))
                                                            .child(
                                                                Column::builder()
                                                                    .main_axis_size(
                                                                        MainAxisSize::Min,
                                                                    )
                                                                    .cross_axis_alignment(
                                                                        CrossAxisAlignment::Center,
                                                                    )
                                                                    .child(
                                                                        Text::builder()
                                                                            .data("2.5K")
                                                                            .size(20.0)
                                                                            .color(Color::rgb(
                                                                                33, 33, 33,
                                                                            ))
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
                                                            .build(),
                                                    )
                                                    // Column 3 with blue background
                                                    .child(
                                                        Container::builder()
                                                            .color(Color::rgb(200, 200, 255))
                                                            .padding(EdgeInsets::all(8.0))
                                                            .child(
                                                                Column::builder()
                                                                    .main_axis_size(
                                                                        MainAxisSize::Min,
                                                                    )
                                                                    .cross_axis_alignment(
                                                                        CrossAxisAlignment::Center,
                                                                    )
                                                                    .child(
                                                                        Text::builder()
                                                                            .data("312")
                                                                            .size(20.0)
                                                                            .color(Color::rgb(
                                                                                33, 33, 33,
                                                                            ))
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
    println!("=== Test Column Width (with Container backgrounds) ===");
    run_app(Box::new(TestColumnWidthApp))
}
