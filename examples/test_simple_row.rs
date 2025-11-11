//! Even simpler test - just Row with 3 Text widgets (no nested Column)

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::MainAxisAlignment;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Container, Padding, Row, Scaffold, Text};

#[derive(Debug, Clone)]
struct TestSimpleRowApp;

impl View for TestSimpleRowApp {
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
                                        Row::builder()
                                            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                            .child(
                                                Text::builder()
                                                    .data("Item 1")
                                                    .size(16.0)
                                                    .color(Color::rgb(255, 0, 0))
                                                    .build(),
                                            )
                                            .child(
                                                Text::builder()
                                                    .data("Item 2")
                                                    .size(16.0)
                                                    .color(Color::rgb(0, 255, 0))
                                                    .build(),
                                            )
                                            .child(
                                                Text::builder()
                                                    .data("Item 3")
                                                    .size(16.0)
                                                    .color(Color::rgb(0, 0, 255))
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
    println!("=== Test Simple Row (3 Text widgets) ===");
    run_app(Box::new(TestSimpleRowApp))
}
