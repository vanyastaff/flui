//! Simple window example - demonstrates basic window creation with flui-platform
//!
//! This example shows:
//! - Platform initialization
//! - Window creation with custom options
//! - Basic event handling
//! - Clean shutdown
//!
//! Usage:
//! ```bash
//! cargo run --example simple_window -p flui-platform
//! # With debug tracing:
//! RUST_LOG=debug cargo run --example simple_window -p flui-platform
//! ```

use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::px;

fn main() {
    // Initialize tracing for debugging
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();
    tracing::info!("Tracing initialized for simple_window example");

    println!("Starting FLUI simple window example...");

    // Get the platform
    tracing::info!("Initializing platform...");
    let platform = current_platform().expect("Failed to create platform");
    tracing::info!("Platform initialized: {}", platform.name());
    println!("Platform: {}", platform.name());

    // Configure window
    tracing::debug!("Configuring window options...");
    let window_options = WindowOptions {
        title: "FLUI Platform Test Window".to_string(),
        size: flui_types::geometry::Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: Some(flui_types::geometry::Size::new(px(400.0), px(300.0))),
        max_size: None,
    };

    // Create window and run platform
    let platform_clone = platform.clone();
    tracing::info!("Starting platform event loop...");
    platform.run(Box::new(move || {
        tracing::info!("Platform ready callback invoked");
        println!("Platform ready, creating window...");

        tracing::debug!("Opening window with configured options");
        match platform_clone.open_window(window_options) {
            Ok(window) => {
                tracing::info!(
                    "Window created: size={:?}, scale_factor={}",
                    window.logical_size(),
                    window.scale_factor()
                );
                println!(
                    "✓ Window created successfully! Size: {:?}, Scale factor: {}",
                    window.logical_size(),
                    window.scale_factor()
                );
                println!("\nWindow will close automatically in 5 seconds...");

                // Keep window alive
                std::thread::sleep(std::time::Duration::from_secs(5));

                tracing::info!("Window lifecycle complete, shutting down");
                println!("Example finished, closing window...");
            }
            Err(e) => {
                tracing::error!("Failed to create window: {}", e);
                eprintln!("✗ Failed to create window: {}", e);
            }
        }

        // Quit the platform
        tracing::info!("Requesting platform quit");
        platform_clone.quit();
    }));

    println!("Platform shut down successfully!");
}
