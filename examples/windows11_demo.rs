//! Windows 11 Features Demo - Automatic Platform Integration
//!
//! This example demonstrates that Windows 11 features are applied
//! automatically by the platform - no manual API calls needed!
//!
//! Features automatically enabled:
//! - Mica backdrop material with translucent blur
//! - Dark mode title bar matching system theme
//! - Rounded window corners
//! - Snap Layouts support
//!
//! Requirements: Windows 11 Build 22000+

#![cfg(target_os = "windows")]
#![allow(unused)]

use flui_platform::traits::Platform;
use flui_platform::{WindowOptions, WindowsPlatform};
use flui_types::geometry::{px, Size};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🪟 Windows 11 Features Demo");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("This example shows Windows 11 features that are");
    println!("automatically applied by flui-platform!");
    println!();

    // Create platform
    let platform = WindowsPlatform::new()?;

    // Create window - Windows 11 features applied automatically!
    let options = WindowOptions {
        title: "Windows 11 Features Demo".to_string(),
        size: Size::new(px(1000.0), px(700.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: Some(Size::new(px(600.0), px(400.0))),
        max_size: None,
    };

    let _window = platform.open_window(options)?;

    println!("✅ Window created successfully!");
    println!();
    println!("🎨 Windows 11 features applied automatically:");
    println!("  ✓ Mica backdrop - translucent background with blur");
    println!("  ✓ Dark mode title bar - matches your Windows theme");
    println!("  ✓ Rounded corners - modern Windows 11 style");
    println!("  ✓ Snap Layouts - hover over maximize button");
    println!();
    println!("💡 These features are built into the platform!");
    println!("   No manual DWM API calls needed in your code.");
    println!();
    println!("Close the window to exit");
    println!();

    // Run the platform event loop
    platform.run(Box::new(|| {}));

    Ok(())
}
