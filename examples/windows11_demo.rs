//! Windows 11 Features Demo - Simple Working Example
//!
//! This example demonstrates Windows 11-specific features by directly
//! calling DWM APIs after window creation.
//!
//! Features shown:
//! - Mica backdrop material
//! - Rounded corners
//! - Dark mode title bar
//! - Custom title bar color
//!
//! Requirements: Windows 11 Build 22000+

#![cfg(target_os = "windows")]
#![allow(unused)]

use flui_platform::traits::{Platform, PlatformWindow};
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

    // Create platform
    let platform = WindowsPlatform::new()?;

    // Create window
    let options = WindowOptions {
        title: "Windows 11 Features Demo".to_string(),
        size: Size::new(px(1000.0), px(700.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: Some(Size::new(px(600.0), px(400.0))),
        max_size: None,
    };

    let window = platform.open_window(options)?;

    // Get the HWND by downcasting to WindowsWindow and apply features
    #[cfg(target_os = "windows")]
    {
        use flui_platform::platforms::windows::win32::*;
        use flui_platform::platforms::windows::WindowsWindow;

        let windows_window = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Expected WindowsWindow");

        let hwnd = windows_window.hwnd();

        println!("✅ Window created successfully!");
        println!("📋 Window HWND: {:?}", hwnd);
        println!();

        // Apply Windows 11 features using DWM API directly
        unsafe {
            println!("🎨 Applying Windows 11 features...");

            // 1. Extend frame into client area (required for Mica)
            println!("  ✓ Extending frame into client area");
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            DwmExtendFrameIntoClientArea(hwnd, &margins).ok();

            // 2. Enable Mica backdrop (Windows 11+)
            println!("  ✓ Setting Mica backdrop");
            let mica_value: i32 = 2; // DWMSBT_MAINWINDOW (Mica) - стандартная прозрачность
            DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(38), // DWMWA_SYSTEMBACKDROP_TYPE
                &mica_value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
            .ok();

            // 3. Enable dark mode title bar
            println!("  ✓ Enabling dark mode");
            let dark_mode_value: i32 = 1;
            DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20), // DWMWA_USE_IMMERSIVE_DARK_MODE
                &dark_mode_value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
            .ok();

            // 4. Set rounded corners (default on Windows 11, but explicitly set)
            println!("  ✓ Setting rounded corners");
            let corner_value: i32 = 2; // DWMWCP_ROUND (2 = округленные углы)
            DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(33), // DWMWA_WINDOW_CORNER_PREFERENCE
                &corner_value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
            .ok();

            // Note: Title bar color is NOT set - Windows will use system theme colors
            // This allows the title bar to match the user's chosen Windows theme
        }

        println!();
        println!("✨ All features applied!");
        println!();
        println!("What you should see:");
        println!("  • Mica backdrop with subtle transparency and blur effect");
        println!("  • Dark title bar matching your Windows theme");
        println!("  • Rounded window corners");
        println!("  • Background tinted with system accent color");
        println!("  • Hover over maximize button to see Snap Layouts");
        println!();
        println!("Close the window to exit");
        println!();
        println!("NOTE: Mica backdrop may appear as solid color if:");
        println!("  - Transparency effects are disabled in Windows settings");
        println!("  - Windows theme doesn't support it");
        println!("  - Using Windows 10 instead of Windows 11");
    }

    // Run the platform message loop (this will block until all windows are closed)
    platform.run(Box::new(|| {
        println!("Platform ready!");
    }));

    Ok(())
}
