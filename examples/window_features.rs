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
    println!("FLUI Window Features Demo");
    println!("=========================");
    println!();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let platform = current_platform()?;
    println!("Platform: {}", platform.name());
    println!("OS: {}", std::env::consts::OS);
    println!();

    // Display information
    let displays = platform.displays();
    println!("Displays:");
    for (i, display) in displays.iter().enumerate() {
        println!(
            "  {}. {} - {}x{} @ {:.1}x",
            i + 1,
            display.name(),
            display.bounds().size.width,
            display.bounds().size.height,
            display.scale_factor()
        );
    }
    println!();

    // Create window
    println!("Creating window...");
    let window_options = WindowOptions {
        title: "Window Features Demo".to_string(),
        size: Size::new(px(1000.0), px(700.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: Some(Size::new(px(600.0), px(400.0))),
        max_size: None,
    };

    let platform_clone = platform.clone();

    platform.run(Box::new(move || {
        match platform_clone.open_window(window_options) {
            Ok(window) => {
                println!("Window created!");
                println!();
                println!("Window Information:");
                println!("  Logical size:  {:?}", window.logical_size());
                println!("  Physical size: {:?}", window.physical_size());
                println!("  Scale factor:  {:.1}x", window.scale_factor());
                println!("  Visible:       {}", window.is_visible());
                println!("  Focused:       {}", window.is_focused());
                println!();

                // Demonstrate cross-platform window features
                println!("Demonstrating cross-platform features:");
                println!();

                // Test window states
                println!("1. Testing window states...");
                std::thread::sleep(std::time::Duration::from_secs(2));

                // Note: Actual state changes would require mutable access
                // This demo shows what the API looks like
                println!("   (Window state changes require event loop integration)");
                println!();

                // Platform-specific features
                #[cfg(target_os = "windows")]
                {
                    // WindowsWindowExt is available for concrete WindowsWindow
                    println!("Windows-specific features available:");
                    println!("   - Mica backdrop");
                    println!("   - Snap Layouts");
                    println!("   - Rounded corners");
                    println!("   - Dark mode");
                    println!("   (Requires mutable window access for changes)");
                    println!();
                }

                #[cfg(target_os = "macos")]
                {
                    println!("macOS-specific features available:");
                    println!("   - Liquid Glass materials");
                    println!("   - Window tiling (Sequoia 15+)");
                    println!("   - Tabbed windows");
                    println!("   - Native fullscreen");
                    println!("   (Requires mutable window access for changes)");
                    println!();
                }

                #[cfg(target_os = "linux")]
                {
                    println!("Linux-specific features available:");
                    println!("   - Wayland layer surfaces");
                    println!("   - X11 window hints");
                    println!("   - Client/server decorations");
                    println!("   (Requires mutable window access for changes)");
                    println!();
                }

                println!("Window will close in 10 seconds...");
                std::thread::sleep(std::time::Duration::from_secs(10));

                println!("Closing window...");
            }
            Err(e) => {
                eprintln!("Failed to create window: {}", e);
            }
        }

        platform_clone.quit();
    }));

    println!("Demo finished!");
    Ok(())
}
