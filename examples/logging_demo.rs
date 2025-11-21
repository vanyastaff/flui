//! Demonstration of the new hierarchical logging system
//!
//! Run with: cargo run --example logging_demo
//!
//! This example shows the improved logging output with:
//! - Hierarchical frame/build/layout/paint structure
//! - Clean, readable output via tracing-tree
//! - No noisy paint_child spam
//! - Configurable log levels

use flui_app::*;
use flui_core::logging::{init_logging, LogConfig, LogMode};
use flui_core::prelude::*;
use flui_rendering::objects::{FlexDirection, RenderFlex};
use flui_widgets::Text;

#[derive(Debug)]
struct LoggingDemo;

impl View for LoggingDemo {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);

        (
            RenderFlex::new(FlexDirection::Column),
            vec![
                Box::new(Text::new(format!("Clicks: {}", count.get()))) as Box<dyn AnyView>,
                Box::new(Text::new("Click anywhere to increment")) as Box<dyn AnyView>,
            ],
        )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize hierarchical logging in Development mode
    init_logging(LogConfig::new(LogMode::Development));

    println!("=== Hierarchical Logging Demo ===");
    println!("Watch the console for clean, tree-structured logs!");
    println!();

    // Run app
    run_app(Box::new(LoggingDemo))?;

    Ok(())
}
