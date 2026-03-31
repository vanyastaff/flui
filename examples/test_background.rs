//! Test Background Color - Simple test to verify background works
//!
//! This example creates a window with NO background to test if
//! our WM_ERASEBKGND handler works correctly.

#![cfg(target_os = "windows")]

use flui_platform::{WindowOptions, WindowsPlatform};
use flui_types::geometry::{Size, px};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing Background Handling");
    println!();

    let platform: Box<dyn flui_platform::Platform> = Box::new(WindowsPlatform::new()?);

    let options = WindowOptions {
        title: "Background Test - Should be BLACK not white".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: None,
        max_size: None,
    };

    // Create window before running the event loop (run() takes ownership)
    let window = platform.open_window(options)?;

    println!("Window created");
    println!();
    println!("If background is:");
    println!("  - WHITE = WM_ERASEBKGND not working (Windows default)");
    println!("  - BLACK = WM_ERASEBKGND working! (no background drawn)");
    println!();

    platform.run(Box::new(move || {
        // Keep window alive via closure capture
        let _window = window;
    }));

    Ok(())
}
