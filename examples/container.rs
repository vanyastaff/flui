//! Container Widget Example
//!
//! This example demonstrates the Container widget, which is one of the most
//! versatile widgets in Flui. Container combines several simpler widgets:
//! - Padding (for padding and margin)
//! - Align (for alignment)
//! - DecoratedBox (for decoration and color)
//! - ConstrainedBox (for width/height/constraints)
//!
//! Run with: cargo run --example container

use flui_app::*;
use flui_widgets::prelude::*;
use flui_widgets::DynWidget;

/// Root application widget showing various Container examples
#[derive(Debug, Clone)]
struct ContainerApp;

impl StatelessWidget for ContainerApp {
    fn build(&self, _context: &BuildContext) -> Box<dyn DynWidget> {
        tracing::info!("📦 ContainerApp::build() called - building widget tree");
        tracing::debug!("  Creating Column with 4 Container children");
        tracing::debug!("  Building 4 containers...");

        // Create a column showing various container examples
        let result = Box::new(
            Column::builder()
                .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(vec![
                        // Example 1: Simple colored container with padding
                        {
                            tracing::debug!("    [1/4] Creating Blue container (300x80, padding=16, center aligned)");
                            Box::new(Container::builder()
                                .width(300.0)
                                .height(80.0)
                                .color(Color::rgb(100, 150, 255))
                                .padding(EdgeInsets::all(16.0))
                                .alignment(Alignment::CENTER)
                                .child(
                                    Text::builder()
                                        .data("Simple Container")
                                        .size(24.0)
                                        .color(Color::rgb(255, 255, 255))
                                        .build()
                                )
                                .build())
                        },

                        // Example 2: Container with different color
                        {
                            tracing::debug!("    [2/4] Creating Pink container (300x80, padding=16, center aligned)");
                            Box::new(Container::builder()
                                .width(300.0)
                                .height(80.0)
                                .color(Color::rgb(255, 100, 150))
                                .padding(EdgeInsets::all(16.0))
                                .alignment(Alignment::CENTER)
                                .child(
                                    Text::builder()
                                        .data("Styled Container")
                                        .size(24.0)
                                        .color(Color::rgb(255, 255, 255))
                                        .build()
                                )
                                .build())
                        },

                        // Example 3: Container with margin
                        {
                            tracing::debug!("    [3/4] Creating Green container (300x80, padding=16, margin=20h/10v)");
                            Box::new(Container::builder()
                                .width(300.0)
                                .height(80.0)
                                .color(Color::rgb(150, 255, 100))
                                .padding(EdgeInsets::all(16.0))
                                .margin(EdgeInsets::symmetric(20.0, 10.0))
                                .alignment(Alignment::CENTER)
                                .child(
                                    Text::builder()
                                        .data("With Margin")
                                        .size(24.0)
                                        .color(Color::rgb(50, 50, 50))
                                        .build()
                                )
                                .build())
                        },

                        // Example 4: Container with left alignment
                        {
                            tracing::debug!("    [4/4] Creating Orange container (300x80, padding=16, left aligned)");
                            Box::new(Container::builder()
                                .width(300.0)
                                .height(80.0)
                                .color(Color::rgb(255, 200, 100))
                                .padding(EdgeInsets::all(16.0))
                                .alignment(Alignment::CENTER_LEFT)
                                .child(
                                    Text::builder()
                                        .data("Left Aligned")
                                        .size(24.0)
                                        .color(Color::rgb(50, 50, 50))
                                        .build()
                                )
                                .build())
                        },
                    ])
                .build()
        );

        tracing::debug!("  Column widget created with 4 children");
        tracing::info!("✅ ContainerApp::build() completed - widget tree ready");

        result
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing for logging with DEBUG level
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tracing::info!("========================================");
    tracing::info!("Starting Container Debug Example");
    tracing::info!("========================================");
    tracing::info!("");
    tracing::info!("This example demonstrates Container widget with:");
    tracing::info!("  1. Simple colored container (Blue) - padding + center alignment");
    tracing::info!("  2. Styled container (Pink) - different color scheme");
    tracing::info!("  3. Container with margin (Green) - symmetric margin (20h, 10v)");
    tracing::info!("  4. Left-aligned container (Orange) - Alignment::CENTER_LEFT");
    tracing::info!("");
    tracing::info!("Container composition:");
    tracing::info!("  Container -> Padding -> Align -> DecoratedBox -> ConstrainedBox");
    tracing::info!("");
    tracing::info!("Watch for DEBUG logs showing:");
    tracing::info!("  - Element tree building");
    tracing::info!("  - Layout constraints and sizes");
    tracing::info!("  - Paint operations");
    tracing::info!("  - Frame performance metrics");
    tracing::info!("========================================");

    // Run the app
    run_app(Box::new(ContainerApp))
}
