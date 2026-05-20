//! Input Event Test - Test keyboard and mouse event handling
//!
//! This example demonstrates:
//! - Keyboard events (key down/up)
//! - Mouse events (move, click, scroll)
//! - W3C-compliant event types
//!
//! Run with: cargo run --example input_test

use flui_platform::{WindowOptions, current_platform};
use flui_types::geometry::{Size, px};

fn main() {
    // Initialize logging with detailed trace output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(true)
        .init();

    tracing::info!("FLUI Input Event Test");
    tracing::info!("Platform: {}", std::env::consts::OS);
    tracing::info!("");
    tracing::info!("Instructions:");
    tracing::info!("  - The window will open and stay responsive");
    tracing::info!("  - Move your mouse in the window");
    tracing::info!("  - Click the mouse buttons");
    tracing::info!("  - Press keyboard keys");
    tracing::info!("  - Scroll the mouse wheel");
    tracing::info!("  - Close the window to exit");
    tracing::info!("");

    let platform = current_platform().expect("Failed to initialize platform");
    tracing::info!("Platform initialized: {}", platform.name());

    // Display info
    let displays = platform.displays();
    tracing::info!("Found {} display(s)", displays.len());
    for (i, disp) in displays.iter().enumerate() {
        tracing::info!(
            "  Display {}: {} ({}x{} @ {:.1}x)",
            i + 1,
            disp.name(),
            disp.bounds().size.width,
            disp.bounds().size.height,
            disp.scale_factor()
        );
    }

    tracing::info!("");
    tracing::info!("Creating test window...");

    let window_options = WindowOptions {
        title: "Input Event Test - Move mouse & press keys!".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: Some(Size::new(px(400.0), px(300.0))),
        max_size: None,
    };

    // Create window before running the event loop
    let window = platform
        .open_window(window_options)
        .expect("Failed to create window");
    tracing::info!("Window created successfully!");
    tracing::info!("   Logical size: {:?}", window.logical_size());
    tracing::info!("   Physical size: {:?}", window.physical_size());
    tracing::info!("   Scale factor: {:.1}x", window.scale_factor());
    tracing::info!("");
    tracing::info!("Waiting for input events...");

    platform.run(Box::new(move || {
        tracing::info!("Platform ready, window is open");
        // Keep window alive via closure capture
        let _window = window;
    }));

    tracing::info!("Application finished!");
}
