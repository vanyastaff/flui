//! Card rendering test
//!
//! Simple test to verify Card widget renders text correctly

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Card, Center, Column, Container, Text};

#[derive(Debug, Clone)]
struct CardTestApp;

impl View for CardTestApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut main_column = Column::builder().build();

        // Simple card with white background
        let mut card = Card::builder().elevation(2.0).color(Color::WHITE).build();

        let mut card_content = Container::builder().padding(EdgeInsets::all(24.0)).build();

        card_content.child = Some(Box::new(
            Text::builder()
                .data("This text should be visible!")
                .size(24.0)
                .color(Color::rgb(0, 0, 0)) // BLACK text
                .build(),
        ));

        card.child = Some(Box::new(card_content));

        main_column
            .children
            .push(Box::new(Center::builder().child(card).build()));

        main_column
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    println!("=== Card Test ==");
    println!("You should see: BLACK text on WHITE card background");
    println!();

    run_app(Box::new(CardTestApp))
}
