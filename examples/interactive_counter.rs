//! Interactive counter example - demonstrates GestureDetector with tap callbacks
//!
//! Simple example showing tap detection with Text widgets.
//! Click on "Increment" or "Decrement" text to see tap events logged.

use flui_app::*;
use flui_widgets::prelude::*;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting Interactive Counter example...");
    tracing::info!("Click on 'Increment' or 'Decrement' text");
    run_app(Box::new(InteractiveCounterApp)).unwrap();
}

/// Root application widget
#[derive(Debug, Clone)]
struct InteractiveCounterApp;

impl StatelessWidget for InteractiveCounterApp {
    fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
        // Just a simple text with tap detection
        Box::new(
            GestureDetector::builder()
                .child(
                    Text::builder()
                        .data("Increment")
                        .size(32.0)
                        .color(Color::rgb(0, 255, 0)) // Green
                        .build()
                )
                .on_tap(|_| {
                    tracing::info!("✅ INCREMENT tapped!");
                })
                .build()
        )
    }
}
