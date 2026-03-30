//! Test Background Color - Simple test to verify background works
//!
//! This example creates a window with NO background to test if
//! our WM_ERASEBKGND handler works correctly.

#![cfg(target_os = "windows")]

use std::sync::Arc;

use flui_platform::{Platform, WindowOptions, WindowsPlatform};
use flui_types::geometry::{Size, px};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing Background Handling");
    println!();

    let platform: Arc<dyn Platform> = Arc::new(WindowsPlatform::new()?);

    let options = WindowOptions {
        title: "Background Test - Should be BLACK not white".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: None,
        max_size: None,
    };

    let platform_clone = platform.clone();

    platform.run(Box::new(move || {
        match platform_clone.open_window(options) {
            Ok(_window) => {
                println!("Window created");
                println!();
                println!("If background is:");
                println!("  - WHITE = WM_ERASEBKGND not working (Windows default)");
                println!("  - BLACK = WM_ERASEBKGND working! (no background drawn)");
                println!();
                println!("Window will stay open for 10 seconds...");
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
            Err(e) => {
                eprintln!("Failed to create window: {}", e);
            }
        }
        platform_clone.quit();
    }));

    Ok(())
}
