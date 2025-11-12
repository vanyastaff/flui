//! Hello World - Working Example with NEW View Architecture
//!
//! This demonstrates a minimal working app with the new View trait.
//!
//! Run with: cargo run --example hello_world_view --features="flui_app,flui_widgets"

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Container, Text};

/// Simple Hello World app using NEW View trait
#[derive(Debug, Clone)]
struct HelloWorldApp;

impl View for HelloWorldApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create a container with padding and background color
        let mut container = Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(100, 150, 200)) // Более яркий синий цвет для теста
            .build();

        // Add centered text
        container.child = Some(Box::new(HelloWorldContent));
        container
    }
}

/// Content widget
#[derive(Debug, Clone)]
struct HelloWorldContent;

impl View for HelloWorldContent {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut center = Center::builder().build();
        center.child = Some(Box::new(
            Text::builder()
                .data("Hello, World!")
                .size(32.0)
                .color(Color::rgb(255, 255, 255)) // Белый текст для контраста
                .build(),
        ));
        center
    }
}

fn main() -> ! {
    run_app(HelloWorldApp)
}
