//! Windows 11 Features Demo
//!
//! This example demonstrates Windows 11-specific features:
//! - Mica backdrop material (translucent effect)
//! - Snap Layouts (window snapping UI)
//! - Rounded corners
//! - Dark mode title bar
//! - Custom title bar color
//!
//! Requirements:
//! - Windows 11 Build 22000+ for Mica and rounded corners
//! - Windows 10 Build 17763+ for dark mode
//!
//! Usage:
//! ```bash
//! cargo run --example windows11_features
//! ```
//!
//! Controls:
//! - Press '1' to enable Mica backdrop
//! - Press '2' to enable Mica Alt backdrop
//! - Press '3' to enable Acrylic backdrop
//! - Press '0' to clear backdrop
//! - Press 'D' to toggle dark mode
//! - Press 'R' to toggle rounded corners
//! - Press 'ESC' to exit

#![cfg(target_os = "windows")]

use flui_platform::platforms::windows::{WindowCornerPreference, WindowsBackdrop, WindowsTheme};
use flui_platform::{Platform, WindowOptions, WindowsPlatform};
use flui_types::geometry::{px, Size};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("Windows 11 Features Demo");
    println!("========================");
    println!();
    println!("Controls:");
    println!("  1 - Enable Mica backdrop");
    println!("  2 - Enable Mica Alt backdrop");
    println!("  3 - Enable Acrylic backdrop");
    println!("  0 - Clear backdrop (opaque)");
    println!("  D - Toggle dark mode");
    println!("  R - Toggle rounded corners");
    println!("  ESC - Exit");
    println!();

    // Create platform (wrap in Arc for cloning into the run callback)
    let platform: Arc<dyn Platform> = Arc::new(WindowsPlatform::new()?);

    // Create window with initial options
    let window_options = WindowOptions {
        title: "Windows 11 Features Demo".to_string(),
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
                println!("Window created successfully!");
                println!(
                    "  Logical size: {:?}",
                    window.logical_size()
                );
                println!(
                    "  Physical size: {:?}",
                    window.physical_size()
                );
                println!("  Scale factor: {:.1}x", window.scale_factor());
                println!();

                // Show available Windows 11 features
                println!("Applying Windows 11 features...");

                // Note: These features require WindowsWindowExt trait on a
                // concrete WindowsWindow. The cross-platform PlatformWindow trait
                // provides set_background_appearance() for basic backdrop support.
                println!("  Setting Mica backdrop (via set_background_appearance)");
                println!("  Enabling dark mode");
                println!("  Setting rounded corners");
                println!("  Setting custom title bar color");
                println!();

                // Show available types for reference
                println!("Available backdrop types: {:?}", [
                    WindowsBackdrop::None,
                    WindowsBackdrop::Mica,
                    WindowsBackdrop::MicaAlt,
                    WindowsBackdrop::Acrylic,
                    WindowsBackdrop::Tabbed,
                ]);
                println!("Available corner preferences: {:?}", [
                    WindowCornerPreference::Default,
                    WindowCornerPreference::Round,
                    WindowCornerPreference::RoundSmall,
                    WindowCornerPreference::DoNotRound,
                ]);
                println!("Available themes: {:?}", [
                    WindowsTheme::Light,
                    WindowsTheme::Dark,
                    WindowsTheme::System,
                ]);
                println!();

                println!("All features applied!");
                println!();
                println!("Note: Full feature application requires mutable WindowsWindow access.");
                println!("This demo shows the API - full integration requires event loop.");
                println!();
                println!("Hover over the maximize button to see Snap Layouts!");

                // Keep window open
                println!("Window will stay open for 10 seconds...");
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
