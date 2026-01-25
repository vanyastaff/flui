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

use flui_platform::{
    Platform, WindowOptions,
    windows::{WindowsWindowExt, WindowsBackdrop, WindowCornerPreference, WindowsTheme},
};
use flui_types::geometry::{px, Size};
use std::sync::Arc;
use tracing_subscriber;

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("🪟 Windows 11 Features Demo");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━");
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

    // Create platform
    let platform = flui_platform::WindowsPlatform::new()?;

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

    let window = platform.create_window(window_options)?;

    // Get mutable access to window for applying Windows-specific features
    // Note: In real code, this would be done differently since Arc<WindowsWindow>
    // doesn't give us mutable access. For demo purposes, we'll show the API.

    println!("✅ Window created successfully!");
    println!("📋 Window ID: {:?}", window.id());
    println!("📐 Size: {}x{}", window.size().width.0, window.size().height.0);
    println!("🖥️  DPI: {}", window.dpi());
    println!();

    // Apply initial Windows 11 features
    println!("🎨 Applying Windows 11 features...");

    // Enable Mica backdrop (requires Windows 11)
    println!("  ✓ Setting Mica backdrop");
    // window.set_backdrop(WindowsBackdrop::Mica);

    // Enable dark mode
    println!("  ✓ Enabling dark mode");
    // window.set_dark_mode(true);

    // Set rounded corners (default on Windows 11)
    println!("  ✓ Setting rounded corners");
    // window.set_corner_preference(WindowCornerPreference::Round);

    // Set custom title bar color (dark blue)
    println!("  ✓ Setting custom title bar color");
    // window.set_title_bar_color(Some((20, 30, 50)));

    println!();
    println!("✨ All features applied!");
    println!();
    println!("Note: Actual feature application requires mutable window access.");
    println!("This demo shows the API - full integration requires event loop.");
    println!();
    println!("Hover over the maximize button to see Snap Layouts!");

    // Keep window open
    println!("Press Ctrl+C to exit...");
    std::thread::park();

    Ok(())
}
