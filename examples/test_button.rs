//! Test Button rendering

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::MainAxisSize;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Button, Center, Column, Container, Padding, Scaffold, SizedBox, Text};

#[derive(Debug, Clone)]
struct TestButtonApp;

impl View for TestButtonApp {
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
                                            .child(
                                                Text::builder()
                                                    .data("Button Test")
                                                    .size(20.0)
                                                    .color(Color::rgb(33, 33, 33))
                                                    .build(),
                                            )
                                            .child(SizedBox::builder().height(16.0).build())
                                            .child(
                                                Button::builder("Click Me!")
                                                    .color(Color::rgb(33, 150, 243))
                                                    .build(),
                                            )
                                            .child(SizedBox::builder().height(8.0).build())
                                            .child(
                                                Button::builder("Another Button")
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
            .build()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Test Button ===");
    run_app(Box::new(TestButtonApp))
}
