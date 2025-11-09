//! Simple Card Demo - Test basic card rendering
//!
//! Demonstrates a simple Card with text content

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Card, Center, Container, Text};

#[derive(Debug, Clone)]
struct SimpleCardApp;

impl View for SimpleCardApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create a simple card
        let mut card = Card::builder().elevation(4.0).color(Color::WHITE).build();

        // Add content to card
        let mut content = Container::builder().padding(EdgeInsets::all(32.0)).build();

        content.child = Some(Box::new(
            Text::builder()
                .data("Hello from Card!")
                .size(24.0)
                .color(Color::rgb(33, 33, 33))
                .build(),
        ));

        card.child = Some(Box::new(content));

        // Center the card
        Center::builder().child(card).build()
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Simple Card Demo ===");
    println!("You should see: A white card with 'Hello from Card!' text");
    println!();

    run_app(Box::new(SimpleCardApp))
}
