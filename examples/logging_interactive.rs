//! Interactive logging demo showing hierarchical output during state changes
//!
//! Run with: RUST_LOG=flui_app=info,flui_core=debug cargo run --example logging_interactive
//!
//! This example demonstrates the beautiful hierarchical logging when:
//! - Building the initial UI
//! - Responding to state changes (signals)
//! - Rebuilding components
//! - Layout and paint phases
//!
//! Expected output:
//! ```
//! ┐flui_app::app::frame{num=0}
//! ├─ build_root
//! │  └─ Root built
//! ├─ layout size=Size(800, 600)
//! │  ├─ compute_layout: Processing N dirty
//! │  └─ Layout complete
//! ├─ paint
//! │  └─ Paint complete
//! ┘
//! ```

use flui_app::run_app;
use flui_core::prelude::*;
use flui_core::view::{AnyView, IntoElement, View};
use flui_rendering::objects::{FlexDirection, RenderFlex};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, Container, Text};

/// Main app with counter state
#[derive(Debug)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Signal for counter - triggers rebuilds
        let count = use_signal(ctx, 0);

        // Auto-increment every render to show continuous rebuilds
        let count_clone = count.clone();
        use_effect(ctx, move || {
            if count_clone.get() < 5 {
                // Request another frame
                std::thread::sleep(std::time::Duration::from_secs(1));
                count_clone.update(|c| *c += 1);
            }
            None
        });

        let mut container = Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(100, 150, 200))
            .build();

        container.child = Some(Box::new(CounterContent { count: count.get() }));
        container
    }
}

/// Content that changes based on count
#[derive(Debug)]
struct CounterContent {
    count: i32,
}

impl View for CounterContent {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        (
            RenderFlex::new(FlexDirection::Column),
            vec![
                Box::new(
                    Text::builder()
                        .data(format!("Frame count: {}", self.count))
                        .size(32.0)
                        .color(Color::rgb(255, 255, 255))
                        .build(),
                ) as Box<dyn AnyView>,
                Box::new(
                    Text::builder()
                        .data("Watch the hierarchical logs!")
                        .size(16.0)
                        .color(Color::rgb(200, 200, 200))
                        .build(),
                ) as Box<dyn AnyView>,
            ],
        )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Hierarchical Logging Demo ===");
    println!("Set RUST_LOG=flui_app=info,flui_core=debug to see beautiful tree output!");
    println!();
    println!("Expected output:");
    println!("┐flui_app::app::frame{{num=X}}");
    println!("├─ rebuild");
    println!("│  └─ Processing N pending rebuilds");
    println!("├─ layout size=...");
    println!("│  └─ Layout complete");
    println!("├─ paint");
    println!("│  └─ Paint complete");
    println!("┘");
    println!();

    run_app(Box::new(CounterApp))?;
    Ok(())
}
