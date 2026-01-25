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
//! ```

use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::px;

fn main() {
    println!("Starting FLUI simple window example...");

    // Get the platform
    let platform = current_platform().expect("Failed to create platform");
    println!("Platform: {}", platform.name());

    // Configure window
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
    platform.run(Box::new(move || {
        println!("Platform ready, creating window...");

        match platform_clone.open_window(window_options) {
            Ok(window) => {
                println!(
                    "✓ Window created successfully! Size: {:?}, Scale factor: {}",
                    window.logical_size(),
                    window.scale_factor()
                );
                println!("\nWindow will close automatically in 5 seconds...");

                // Keep window alive
                std::thread::sleep(std::time::Duration::from_secs(5));

                println!("Example finished, closing window...");
            }
            Err(e) => {
                eprintln!("✗ Failed to create window: {}", e);
            }
        }

        // Quit the platform
        platform_clone.quit();
    }));

    println!("Platform shut down successfully!");
}
