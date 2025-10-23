//! Hello World Example
//!
//! The simplest possible Flui application that displays "Hello, World!".
//!
//! Run with: cargo run --example hello_world

use ::flui_app::*;
use ::flui_widgets::prelude::*;
use flui_widgets::DynWidget;

/// The root widget of our application
#[derive(Debug, Clone)]
struct HelloWorldApp;

impl StatelessWidget for HelloWorldApp {
    fn build(&self, _context: &BuildContext) -> Box<dyn DynWidget> {
        // Create a Text widget that displays "Hello, World!"
        Box::new(Text::builder()
            .data("Hello, World!")
            .size(32.0)
            .color(Color::rgb(255, 0, 0))
            .build())
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    tracing::info!("Starting Hello World example...");

    // Run the Flui app
    run_app(Box::new(HelloWorldApp))
}
