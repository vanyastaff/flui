//! Window Features Demo
//!
//! Demonstrates cross-platform window features and platform-specific
//! extensions.
//!
//! This example shows:
//! - Cross-platform Window trait API
//! - Window state management (minimize, maximize, fullscreen)
//! - Window properties (resizable, title, size)
//! - Platform-specific features (Mica on Windows, Liquid Glass on macOS)
//!
//! Usage:
//! ```bash
//! cargo run --example window_features
//! ```

use flui_platform::{WindowOptions, current_platform};
use flui_types::geometry::{Size, px};

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tracing::info!("FLUI Window Features Demo");
    tracing::info!("=========================");

    let platform = current_platform()?;
    tracing::info!("Platform: {}", platform.name());
    tracing::info!("OS: {}", std::env::consts::OS);

    // Display information
    let displays = platform.displays();
    tracing::info!("Displays:");
    for (i, disp) in displays.iter().enumerate() {
        tracing::info!(
            "  {}. {} - {}x{} @ {:.1}x",
            i + 1,
            disp.name(),
            disp.bounds().size.width,
            disp.bounds().size.height,
            disp.scale_factor()
        );
    }

    // Create window before running the event loop
    tracing::info!("Creating window...");
    let window_options = WindowOptions {
        title: "Window Features Demo".to_string(),
        size: Size::new(px(1000.0), px(700.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: Some(Size::new(px(600.0), px(400.0))),
        max_size: None,
    };

    let window = platform.open_window(window_options)?;
    tracing::info!("Window created!");
    tracing::info!("  Logical size:  {:?}", window.logical_size());
    tracing::info!("  Physical size: {:?}", window.physical_size());
    tracing::info!("  Scale factor:  {:.1}x", window.scale_factor());
    tracing::info!("  Visible:       {}", window.is_visible());
    tracing::info!("  Focused:       {}", window.is_focused());

    platform.run(Box::new(move |_platform| {
        tracing::info!("Platform ready, window is open");
        // Keep window alive via closure capture
        let _window = window;
    }));

    tracing::info!("Demo finished!");
    Ok(())
}
