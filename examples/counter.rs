//! Counter Example
//!
//! A simple counter application that demonstrates StatefulWidget.
//!
//! This example shows:
//! - StatefulWidget implementation
//! - State management with mutable state
//! - State lifecycle (init_state, build, dispose)
//!
//! Note: This is a static counter showing the initial value.
//! Interactive increment/decrement buttons will be added when
//! we have input event handling.
//!
//! Run with: cargo run --example counter

use flui_app::*;
use flui_widgets::prelude::*;
use flui_widgets::DynWidget;

/// Counter widget - a StatefulWidget that maintains a count
#[derive(Debug, Clone)]
struct Counter {
    /// Initial value for the counter
    initial_value: i32,
}

/// Counter state - holds the mutable count
#[derive(Debug)]
struct CounterState {
    /// Current count value
    count: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: self.initial_value,
        }
    }
}

impl State for CounterState {
    fn build(&mut self, _context: &BuildContext) -> Box<dyn DynWidget> {
        // Build the UI showing the current count
        Box::new(
            Text::builder()
                .data(format!("Counter: {}", self.count))
                .size(48.0)
                .color(Color::rgb(0, 150, 255))
                .build(),
        )
    }

    fn init_state(&mut self) {
        tracing::info!("Counter state initialized with count: {}", self.count);
    }

    fn dispose(&mut self) {
        tracing::info!("Counter state disposed at count: {}", self.count);
    }
}

// Manual Widget implementation for Counter (required until we have blanket impl)
impl Widget for Counter {
    type Element = StatefulElement<Self>;

    fn into_element(self) -> Self::Element {
        StatefulElement::new(self)
    }
}

/// Root application widget
#[derive(Debug, Clone)]
struct CounterApp;

impl StatelessWidget for CounterApp {
    fn build(&self, _context: &BuildContext) -> Box<dyn DynWidget> {
        // Create a counter starting at 42
        // This demonstrates that StatefulWidget maintains its own state
        // and can be initialized with different values
        Box::new(Counter { initial_value: 42 })
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    tracing::info!("Starting Counter example...");
    tracing::info!("Demonstrating StatefulWidget with a simple counter");

    // Run the Flui app
    run_app(Box::new(CounterApp))
}
