//! Simple window example - demonstrates basic window creation with
//! flui-platform
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

use flui_platform::{WindowOptions, current_platform};
use flui_types::geometry::px;

fn main() {
    // Initialize tracing for debugging
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();
    tracing::info!("Tracing initialized for simple_window example");

    tracing::info!("Starting FLUI simple window example...");

    // Get the platform
    tracing::info!("Initializing platform...");
    let platform = current_platform().expect("Failed to create platform");
    tracing::info!("Platform initialized: {}", platform.name());

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

    // Create window before running the event loop.
    // On Windows, window creation works before run() since it uses Win32 API directly.
    // On the winit backend (Linux), this fails fast with a clear error instead —
    // `WinitPlatform::open_window` rejects calls made before `Platform::run` starts
    // the event loop rather than hanging forever, so the `.expect` below panics
    // immediately here rather than opening a window.
    let window = platform
        .open_window(window_options)
        .expect("Failed to create window");
    tracing::info!(
        "Window created: size={:?}, scale_factor={}",
        window.logical_size(),
        window.scale_factor()
    );

    // Run platform event loop (takes ownership)
    tracing::info!("Starting platform event loop...");
    platform.run(Box::new(move |_platform| {
        tracing::info!("Platform ready callback invoked");
        // Window is already created; keep it alive via the closure capture
        let _window = window;
    }));

    tracing::info!("Platform shut down successfully!");
}
